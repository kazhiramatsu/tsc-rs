//! Dormant NodeBuilder module-specifier synthesis.
//!
//! This module is deliberately independent of the future
//! `NodeBuilderContext`: all checker, host, and enclosing-node facts enter
//! through explicit parameters. It has no production caller before the
//! declaration-serialization adapter lands.

#![allow(dead_code)]

use std::cmp::Ordering;
use std::collections::BTreeMap;

use indexmap::IndexMap;
use serde_json::Value;
use tsc_binder::{node_util, SymbolId};
use tsc_emitter::{EmitModuleSpecifierHost, EmitResolutionMode, EmitResolverNode};
use tsc_program::SourceFileId;
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{CompilerOptions, NodeFlags, ScriptTarget, SymbolFlags};

use crate::state::{CheckResult, CheckerState};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RelativePreference {
    Relative,
    NonRelative,
    Shortest,
    ExternalNonRelative,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ModuleSpecifierEnding {
    Minimal,
    Index,
    JsExtension,
    TsExtension,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ImportModuleSpecifierPreference {
    Relative,
    NonRelative,
    ProjectRelative,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ImportModuleSpecifierEnding {
    Minimal,
    Index,
    Js,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ModuleSpecifierUserPreferences {
    pub(crate) import_module_specifier_preference: Option<ImportModuleSpecifierPreference>,
    pub(crate) import_module_specifier_ending: Option<ImportModuleSpecifierEnding>,
    pub(crate) auto_import_specifier_exclude_regexes: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ModulePathMapping {
    pub(crate) key: String,
    pub(crate) patterns: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct SpecifierCompilerOptions {
    pub(crate) compiler_options: CompilerOptions,
    pub(crate) paths: Vec<ModulePathMapping>,
    pub(crate) paths_base_path: Option<String>,
    pub(crate) root_dirs: Vec<String>,
    pub(crate) config_file_path: Option<String>,
}

impl SpecifierCompilerOptions {
    /// tsrs-native: Rust constructor for the ported machinery.
    pub(crate) fn new(compiler_options: &CompilerOptions) -> Self {
        Self {
            compiler_options: compiler_options.clone(),
            paths: Vec::new(),
            paths_base_path: None,
            root_dirs: Vec::new(),
            config_file_path: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ModuleSpecifierOptions {
    pub(crate) override_import_mode: Option<EmitResolutionMode>,
}

impl ModuleSpecifierOptions {
    fn effective_override_import_mode(&self) -> Option<EmitResolutionMode> {
        self.override_import_mode
            .filter(|mode| *mode != EmitResolutionMode::None)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ModuleSpecifierPreferences {
    pub(crate) exclude_regexes: Vec<String>,
    pub(crate) relative_preference: RelativePreference,
    pub(crate) file_preferred_ending: ModuleSpecifierEnding,
    importing_file: NodeId,
    old_import_specifier: Option<String>,
    import_module_specifier_ending: Option<ImportModuleSpecifierEnding>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ModuleSpecifierKind {
    Ambient,
    Paths,
    Redirect,
    NodeModules,
    Relative,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ModuleSpecifiersWithCacheInfo {
    pub(crate) kind: Option<ModuleSpecifierKind>,
    pub(crate) module_specifiers: Vec<String>,
    pub(crate) computed_without_cache: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ModulePath {
    pub(crate) path: String,
    pub(crate) is_redirect: bool,
    pub(crate) is_in_node_modules: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ModuleSpecifierInfo {
    pub(crate) importing_source_file_name: String,
    pub(crate) source_directory: String,
    pub(crate) canonical_source_directory: String,
    pub(crate) case_sensitive: bool,
}

impl ModuleSpecifierInfo {
    fn canonical(&self, path: &str) -> String {
        canonical_file_name(path, self.case_sensitive)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ModuleSpecifierCacheProbe {
    pub(crate) kind: Option<ModuleSpecifierKind>,
    pub(crate) specifiers: Option<Vec<String>>,
    pub(crate) module_source_file: Option<usize>,
    pub(crate) module_paths: Option<Vec<ModulePath>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExportsKeyMode {
    Exact,
    Directory,
    Pattern,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ExportsOrImportsResult {
    pub(crate) module_file_to_try: String,
    pub(crate) package_root_path: Option<String>,
    pub(crate) blocked_by_exports: bool,
    pub(crate) verbatim_from_exports: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NodeModulePathParts {
    top_level_node_modules_index: usize,
    top_level_package_name_index: usize,
    package_root_index: usize,
    file_name_index: usize,
}

/// tsc-port: getSpecifierForModuleSymbol @6.0.3
/// tsc-hash: cc081ccc9162d99c71cfb5013a0786210de8d66472567a9ee1d6eab90f686463
/// tsc-span: _tsc.js:53060-53109
pub(crate) fn get_specifier_for_module_symbol(
    state: &mut CheckerState<'_>,
    module_symbol: SymbolId,
    host: Option<&dyn EmitModuleSpecifierHost>,
    enclosing_file: Option<NodeId>,
    enclosing_declaration: Option<NodeId>,
    bundled: bool,
    override_import_mode: Option<EmitResolutionMode>,
) -> CheckResult<String> {
    let override_import_mode =
        override_import_mode.filter(|mode| *mode != EmitResolutionMode::None);
    let mut source_file_declaration =
        state.get_declaration_of_kind(module_symbol, SyntaxKind::SourceFile);
    if source_file_declaration.is_none() {
        let declarations = state.binder.symbol(module_symbol).declarations.clone();
        for declaration in declarations {
            let Some(equivalent_file_symbol) =
                get_file_symbol_if_file_symbol_export_equals_container(
                    state,
                    declaration,
                    module_symbol,
                )?
            else {
                continue;
            };
            source_file_declaration =
                state.get_declaration_of_kind(equivalent_file_symbol, SyntaxKind::SourceFile);
            if source_file_declaration.is_some() {
                break;
            }
        }
    }

    if let Some(module_name) = source_file_declaration
        .and_then(|file| state.binder.source_of_node(file).module_name.as_ref())
    {
        return Ok(module_name.clone());
    }

    if source_file_declaration.is_none() && ambient_symbol_name(state, module_symbol).is_some() {
        return no_host_specifier_for_module_symbol(state, module_symbol);
    }

    let (Some(host), Some(enclosing_file)) = (host, enclosing_file) else {
        // Reuse disposition: the display slice is the decision authority for
        // this branch. Its method is module-private, so this dormant channel
        // transcribes the same predicate and path normalization explicitly.
        return no_host_specifier_for_module_symbol(state, module_symbol);
    };

    let importing_index = state.binder.file_index_of_node(enclosing_file);
    let importing_root = state.binder.source(importing_index).root;
    let importing_node = emit_resolver_node_for_file(state, importing_index, importing_root);

    let original_module_specifier = enclosing_declaration
        .filter(|&node| can_have_module_specifier(state, node))
        .and_then(|node| try_get_module_specifier_from_declaration(state, node));
    let original_mode = original_module_specifier
        .and_then(|literal| module_specifier_index(state, importing_index, literal))
        .map(|index| host.get_mode_for_resolution_at_index(importing_node, index))
        .filter(|mode| *mode != EmitResolutionMode::None);
    let resolution_mode = override_import_mode
        .filter(|mode| *mode != EmitResolutionMode::None)
        .or(original_mode)
        .unwrap_or_else(|| host.get_default_resolution_mode_for_file(importing_node));

    let context_path = canonical_host_path(&state.binder.source(importing_index).file_name, host);
    let cache_key = create_mode_aware_cache_key(&context_path, resolution_mode);
    if let Some(specifier) = state
        .links
        .symbol(module_symbol)
        .specifier_cache
        .as_ref()
        .and_then(|cache| cache.get(&cache_key))
        .filter(|specifier| !specifier.is_empty())
        .cloned()
    {
        return Ok(specifier);
    }

    let mut specifier_options = SpecifierCompilerOptions::new(state.options);
    if bundled {
        specifier_options.compiler_options.base_url = Some(host.get_common_source_directory());
    }
    let user_preferences = ModuleSpecifierUserPreferences {
        import_module_specifier_preference: Some(if bundled {
            ImportModuleSpecifierPreference::NonRelative
        } else {
            ImportModuleSpecifierPreference::ProjectRelative
        }),
        import_module_specifier_ending: if bundled {
            Some(ImportModuleSpecifierEnding::Minimal)
        } else if resolution_mode == EmitResolutionMode::EsNext {
            Some(ImportModuleSpecifierEnding::Js)
        } else {
            None
        },
        auto_import_specifier_exclude_regexes: Vec::new(),
    };
    let module_options = ModuleSpecifierOptions {
        override_import_mode,
    };
    let specifier = get_module_specifiers(
        state,
        module_symbol,
        &specifier_options,
        importing_root,
        importing_node,
        host,
        &user_preferences,
        &module_options,
    )?
    .into_iter()
    .next()
    .expect("a source-file module has at least one relative module specifier");

    state.links.set_symbol_specifier_cache_entry(
        state.speculation_depth,
        module_symbol,
        cache_key,
        specifier.clone(),
    );
    Ok(specifier)
}

/// Reused-anchor disposition: this is the exact checker display decision
/// already owned by `file_symbol_if_export_equals_container_slice`
/// (check.rs:8182), projected here because that anchor is module-private.
///
/// tsc-port: getFileSymbolIfFileSymbolExportEqualsContainer @6.0.3
/// tsc-hash: 664797354015a10df710b2c342bfa160aa42af4711c31bf017f98df27ae685ad
/// tsc-span: _tsc.js:50060-50064
fn get_file_symbol_if_file_symbol_export_equals_container(
    state: &mut CheckerState<'_>,
    declaration: NodeId,
    container: SymbolId,
) -> CheckResult<Option<SymbolId>> {
    let mut current = Some(declaration);
    let mut file_symbol = None;
    while let Some(node) = current {
        let is_external_module_container = match state.data_of(node) {
            NodeData::ModuleDeclaration(data) => data
                .name
                .is_some_and(|name| matches!(state.data_of(name), NodeData::StringLiteral(_))),
            NodeData::SourceFile(_) => state.binder.is_external_or_common_js_module_of_node(node),
            _ => false,
        };
        if is_external_module_container {
            file_symbol = state.binder.node_symbol(node);
            break;
        }
        current = state.parent_of(node);
    }
    let Some(file_symbol) = file_symbol else {
        return Ok(None);
    };
    let Some(exported) = state
        .binder
        .symbol(file_symbol)
        .exports
        .get(tsc_binder::InternalSymbolName::EXPORT_EQUALS)
        .copied()
    else {
        return Ok(None);
    };
    let left = state.get_merged_symbol(exported);
    let left = state
        .resolve_symbol_ex(Some(left), false)?
        .expect("resolveSymbol(Some) is Some");
    let right = state.get_merged_symbol(container);
    let right = state
        .resolve_symbol_ex(Some(right), false)?
        .expect("resolveSymbol(Some) is Some");
    Ok((state.get_merged_symbol(left) == state.get_merged_symbol(right)).then_some(file_symbol))
}

fn no_host_specifier_for_module_symbol(
    state: &CheckerState<'_>,
    module_symbol: SymbolId,
) -> CheckResult<String> {
    let symbol = state.binder.symbol(module_symbol);
    if let Some(name) = unquote_ambient_symbol_name(&symbol.escaped_name) {
        let source_file_module = symbol
            .declarations
            .iter()
            .any(|&declaration| state.kind_of(declaration) == SyntaxKind::SourceFile);
        if source_file_module {
            return Ok(CheckerState::normalize_program_path(
                name,
                &state.host_current_directory,
            ));
        }
        return Ok(name.to_owned());
    }

    let declaration = non_augmentation_declaration(state, module_symbol)
        .expect("module symbol without an ambient name has a non-augmentation declaration");
    Ok(CheckerState::normalize_program_path(
        &state.binder.source_of_node(declaration).file_name,
        &state.host_current_directory,
    ))
}

fn ambient_symbol_name<'a>(state: &'a CheckerState<'_>, symbol: SymbolId) -> Option<&'a str> {
    unquote_ambient_symbol_name(&state.binder.symbol(symbol).escaped_name)
}

fn unquote_ambient_symbol_name(name: &str) -> Option<&str> {
    (name.len() >= 3 && name.starts_with('"') && name.ends_with('"'))
        .then(|| &name[1..name.len() - 1])
}

fn non_augmentation_declaration(state: &CheckerState<'_>, symbol: SymbolId) -> Option<NodeId> {
    state
        .binder
        .symbol(symbol)
        .declarations
        .iter()
        .copied()
        .find(|&declaration| {
            let source = state.binder.source_of_node(declaration);
            let external_augmentation = node_util::is_ambient_module(source, declaration)
                && node_util::is_module_augmentation_external(source, declaration);
            let global_augmentation =
                matches!(state.data_of(declaration), NodeData::ModuleDeclaration(_))
                    && node_util::is_global_scope_augmentation(source, declaration);
            !external_augmentation && !global_augmentation
        })
}

/// tsc-port: canHaveModuleSpecifier @6.0.3
/// tsc-hash: f90399af489ec151a908b92101ded0463948dcc3704957b8705e5456992069da
/// tsc-span: _tsc.js:15203-15219
pub(crate) fn can_have_module_specifier(state: &CheckerState<'_>, node: NodeId) -> bool {
    matches!(
        state.kind_of(node),
        SyntaxKind::VariableDeclaration
            | SyntaxKind::BindingElement
            | SyntaxKind::ImportDeclaration
            | SyntaxKind::ExportDeclaration
            | SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::ImportClause
            | SyntaxKind::NamespaceExport
            | SyntaxKind::NamespaceImport
            | SyntaxKind::ExportSpecifier
            | SyntaxKind::ImportSpecifier
            | SyntaxKind::ImportType
    )
}

/// tsc-port: tryGetModuleSpecifierFromDeclaration @6.0.3
/// tsc-hash: 28f135dc9f2fabd8083a8f186bd5222aba931824e0e68e1a573b3605ae7cb401
/// tsc-span: _tsc.js:15220-15249
pub(crate) fn try_get_module_specifier_from_declaration(
    state: &CheckerState<'_>,
    node: NodeId,
) -> Option<NodeId> {
    match state.data_of(node) {
        NodeData::VariableDeclaration(data) => {
            find_require_call_argument(state, data.initializer, node)
        }
        NodeData::BindingElement(data) => find_require_call_argument(state, data.initializer, node),
        NodeData::ImportDeclaration(data) => string_literal_like(state, data.module_specifier),
        NodeData::ExportDeclaration(data) => string_literal_like(state, data.module_specifier),
        NodeData::JSDocImportTag(data) => string_literal_like(state, data.module_specifier),
        NodeData::ImportEqualsDeclaration(data) => data.module_reference.and_then(|reference| {
            let NodeData::ExternalModuleReference(reference) = state.data_of(reference) else {
                return None;
            };
            string_literal_like(state, reference.expression)
        }),
        NodeData::ImportClause(_) | NodeData::NamespaceExport(_) => state
            .parent_of(node)
            .and_then(|parent| direct_module_specifier(state, parent)),
        NodeData::NamespaceImport(_) | NodeData::ExportSpecifier(_) => state
            .parent_of(node)
            .and_then(|parent| state.parent_of(parent))
            .and_then(|parent| direct_module_specifier(state, parent)),
        NodeData::ImportSpecifier(_) => state
            .parent_of(node)
            .and_then(|parent| state.parent_of(parent))
            .and_then(|parent| state.parent_of(parent))
            .and_then(|parent| direct_module_specifier(state, parent)),
        NodeData::ImportType(data) => data.argument.and_then(|argument| {
            let NodeData::LiteralType(literal) = state.data_of(argument) else {
                return None;
            };
            string_literal_like(state, literal.literal)
        }),
        _ => None,
    }
}

fn find_require_call_argument(
    state: &CheckerState<'_>,
    initializer: Option<NodeId>,
    declaration: NodeId,
) -> Option<NodeId> {
    let mut current = initializer;
    while let Some(node) = current {
        if state.is_require_call(node, true) {
            if let NodeData::CallExpression(data) = state.data_of(node) {
                return state.nodes_of(data.arguments).first().copied();
            }
        }
        if node == declaration {
            break;
        }
        current = state.parent_of(node);
    }
    None
}

fn direct_module_specifier(state: &CheckerState<'_>, node: NodeId) -> Option<NodeId> {
    match state.data_of(node) {
        NodeData::ImportDeclaration(data) => string_literal_like(state, data.module_specifier),
        NodeData::ExportDeclaration(data) => string_literal_like(state, data.module_specifier),
        NodeData::JSDocImportTag(data) => string_literal_like(state, data.module_specifier),
        _ => None,
    }
}

fn string_literal_like(state: &CheckerState<'_>, node: Option<NodeId>) -> Option<NodeId> {
    node.filter(|&node| {
        matches!(
            state.kind_of(node),
            SyntaxKind::StringLiteral | SyntaxKind::NoSubstitutionTemplateLiteral
        )
    })
}

/// tsc-port: createModeAwareCacheKey @6.0.3
/// tsc-hash: ceb0fb588e02049b70512628a76eb9fe9271da5fe9dbf67b8d97b0b28080c305
/// tsc-span: _tsc.js:40469-40471
pub(crate) fn create_mode_aware_cache_key(specifier: &str, mode: EmitResolutionMode) -> String {
    match mode {
        EmitResolutionMode::None => specifier.to_owned(),
        EmitResolutionMode::CommonJs => format!("1|{specifier}"),
        EmitResolutionMode::EsNext => format!("99|{specifier}"),
    }
}

/// tsc-port: getModuleSpecifierPreferences @6.0.3
/// tsc-hash: be336c6d6d4e0813a9158ca43720fa4170c29889cd11a89d1094273192af3da2
/// tsc-span: _tsc.js:45391-45436
pub(crate) fn get_module_specifier_preferences(
    state: &CheckerState<'_>,
    user_preferences: &ModuleSpecifierUserPreferences,
    host: &dyn EmitModuleSpecifierHost,
    options: &SpecifierCompilerOptions,
    importing_file: NodeId,
    old_import_specifier: Option<&str>,
) -> ModuleSpecifierPreferences {
    let importing_index = state.binder.file_index_of_node(importing_file);
    let importing_node = emit_resolver_node_for_file(state, importing_index, importing_file);
    let default_mode = get_default_resolution_mode_for_file(importing_node, host, options);
    let file_preferred_ending = get_preferred_ending(
        state,
        user_preferences,
        host,
        options,
        importing_file,
        old_import_specifier,
        default_mode,
    );
    let relative_preference = if let Some(old) = old_import_specifier {
        if is_external_module_name_relative(old) {
            RelativePreference::Relative
        } else {
            RelativePreference::NonRelative
        }
    } else {
        match user_preferences.import_module_specifier_preference {
            Some(ImportModuleSpecifierPreference::Relative) => RelativePreference::Relative,
            Some(ImportModuleSpecifierPreference::NonRelative) => RelativePreference::NonRelative,
            Some(ImportModuleSpecifierPreference::ProjectRelative) => {
                RelativePreference::ExternalNonRelative
            }
            None => RelativePreference::Shortest,
        }
    };
    ModuleSpecifierPreferences {
        exclude_regexes: user_preferences
            .auto_import_specifier_exclude_regexes
            .clone(),
        relative_preference,
        file_preferred_ending,
        importing_file,
        old_import_specifier: old_import_specifier.map(str::to_owned),
        import_module_specifier_ending: user_preferences.import_module_specifier_ending,
    }
}

/// tsc-port: getPreferredEnding @6.0.3 (nested in getModuleSpecifierPreferences)
/// tsc-hash: 8056bce639d758ed3cc7929f9591e2b015ceb2a96bdfb4f856b93e3cbf3762c7
/// tsc-span: _tsc.js:45424-45435
pub(crate) fn get_preferred_ending(
    state: &CheckerState<'_>,
    user_preferences: &ModuleSpecifierUserPreferences,
    host: &dyn EmitModuleSpecifierHost,
    options: &SpecifierCompilerOptions,
    importing_file: NodeId,
    old_import_specifier: Option<&str>,
    resolution_mode: EmitResolutionMode,
) -> ModuleSpecifierEnding {
    if let Some(old) = old_import_specifier {
        if has_js_file_extension(old) {
            return ModuleSpecifierEnding::JsExtension;
        }
        if old.ends_with("/index") {
            return ModuleSpecifierEnding::Index;
        }
    }
    get_module_specifier_ending_preference(
        state,
        user_preferences.import_module_specifier_ending,
        resolution_mode,
        host,
        options,
        importing_file,
    )
}

impl ModuleSpecifierPreferences {
    /// tsc-port: getAllowedEndingsInPreferredOrder @6.0.3 (closure)
    /// tsc-hash: f674e0d02c2e0420e74ac4a7ea6b888cd156c7f43cea60b124134018bf34951e
    /// tsc-span: _tsc.js:45396-45422
    pub(crate) fn get_allowed_endings_in_preferred_order(
        &self,
        state: &CheckerState<'_>,
        host: &dyn EmitModuleSpecifierHost,
        options: &SpecifierCompilerOptions,
        syntax_implied_node_format: EmitResolutionMode,
    ) -> Vec<ModuleSpecifierEnding> {
        let importing_index = state.binder.file_index_of_node(self.importing_file);
        let importing_node =
            emit_resolver_node_for_file(state, importing_index, self.importing_file);
        let implied_node_format =
            get_default_resolution_mode_for_file(importing_node, host, options);
        let preferred_ending = if syntax_implied_node_format != EmitResolutionMode::None
            && syntax_implied_node_format != implied_node_format
        {
            get_preferred_ending(
                state,
                &ModuleSpecifierUserPreferences {
                    import_module_specifier_preference: None,
                    import_module_specifier_ending: self.import_module_specifier_ending,
                    auto_import_specifier_exclude_regexes: Vec::new(),
                },
                host,
                options,
                self.importing_file,
                self.old_import_specifier.as_deref(),
                syntax_implied_node_format,
            )
        } else {
            self.file_preferred_ending
        };
        let module_resolution = options.compiler_options.emit_module_resolution_kind();
        let effective_mode = if syntax_implied_node_format == EmitResolutionMode::None {
            implied_node_format
        } else {
            syntax_implied_node_format
        };
        let allow_ts = should_allow_importing_ts_extension(state, self.importing_file, options);
        if effective_mode == EmitResolutionMode::EsNext && (3..=99).contains(&module_resolution) {
            return if allow_ts {
                vec![
                    ModuleSpecifierEnding::TsExtension,
                    ModuleSpecifierEnding::JsExtension,
                ]
            } else {
                vec![ModuleSpecifierEnding::JsExtension]
            };
        }
        if module_resolution == 1 {
            return if preferred_ending == ModuleSpecifierEnding::JsExtension {
                vec![
                    ModuleSpecifierEnding::JsExtension,
                    ModuleSpecifierEnding::Index,
                ]
            } else {
                vec![
                    ModuleSpecifierEnding::Index,
                    ModuleSpecifierEnding::JsExtension,
                ]
            };
        }
        match preferred_ending {
            ModuleSpecifierEnding::JsExtension if allow_ts => vec![
                ModuleSpecifierEnding::JsExtension,
                ModuleSpecifierEnding::TsExtension,
                ModuleSpecifierEnding::Minimal,
                ModuleSpecifierEnding::Index,
            ],
            ModuleSpecifierEnding::JsExtension => vec![
                ModuleSpecifierEnding::JsExtension,
                ModuleSpecifierEnding::Minimal,
                ModuleSpecifierEnding::Index,
            ],
            ModuleSpecifierEnding::TsExtension => vec![
                ModuleSpecifierEnding::TsExtension,
                ModuleSpecifierEnding::Minimal,
                ModuleSpecifierEnding::JsExtension,
                ModuleSpecifierEnding::Index,
            ],
            ModuleSpecifierEnding::Index if allow_ts => vec![
                ModuleSpecifierEnding::Index,
                ModuleSpecifierEnding::Minimal,
                ModuleSpecifierEnding::TsExtension,
                ModuleSpecifierEnding::JsExtension,
            ],
            ModuleSpecifierEnding::Index => vec![
                ModuleSpecifierEnding::Index,
                ModuleSpecifierEnding::Minimal,
                ModuleSpecifierEnding::JsExtension,
            ],
            ModuleSpecifierEnding::Minimal if allow_ts => vec![
                ModuleSpecifierEnding::Minimal,
                ModuleSpecifierEnding::Index,
                ModuleSpecifierEnding::TsExtension,
                ModuleSpecifierEnding::JsExtension,
            ],
            ModuleSpecifierEnding::Minimal => vec![
                ModuleSpecifierEnding::Minimal,
                ModuleSpecifierEnding::Index,
                ModuleSpecifierEnding::JsExtension,
            ],
        }
    }
}

fn get_module_specifier_ending_preference(
    state: &CheckerState<'_>,
    preference: Option<ImportModuleSpecifierEnding>,
    resolution_mode: EmitResolutionMode,
    host: &dyn EmitModuleSpecifierHost,
    options: &SpecifierCompilerOptions,
    importing_file: NodeId,
) -> ModuleSpecifierEnding {
    let module_resolution = options.compiler_options.emit_module_resolution_kind();
    let node_next = (3..=99).contains(&module_resolution);
    let allow_ts = compiler_allows_importing_ts_extensions(options);
    let infer = || infer_ending_preference(state, importing_file, resolution_mode, node_next, host);
    if preference == Some(ImportModuleSpecifierEnding::Js)
        || resolution_mode == EmitResolutionMode::EsNext && node_next
    {
        if !allow_ts {
            return ModuleSpecifierEnding::JsExtension;
        }
        return if infer() == ModuleSpecifierEnding::JsExtension {
            ModuleSpecifierEnding::JsExtension
        } else {
            ModuleSpecifierEnding::TsExtension
        };
    }
    match preference {
        Some(ImportModuleSpecifierEnding::Minimal) => ModuleSpecifierEnding::Minimal,
        Some(ImportModuleSpecifierEnding::Index) => ModuleSpecifierEnding::Index,
        Some(ImportModuleSpecifierEnding::Js) => unreachable!(),
        None if !allow_ts => {
            if source_uses_extensions_on_imports(state, importing_file) {
                ModuleSpecifierEnding::JsExtension
            } else {
                ModuleSpecifierEnding::Minimal
            }
        }
        None => infer(),
    }
}

fn infer_ending_preference(
    state: &CheckerState<'_>,
    importing_file: NodeId,
    resolution_mode: EmitResolutionMode,
    node_next: bool,
    host: &dyn EmitModuleSpecifierHost,
) -> ModuleSpecifierEnding {
    let index = state.binder.file_index_of_node(importing_file);
    let importing_node = emit_resolver_node_for_file(state, index, importing_file);
    let mut uses_js = false;
    for (specifier_index, literal) in module_name_literals(state, index).0.into_iter().enumerate() {
        let Some(text) = literal_text(state, literal) else {
            continue;
        };
        if !path_is_relative(text) || extension_does_not_support_extensionless_resolution(text) {
            continue;
        }
        if node_next
            && resolution_mode == EmitResolutionMode::CommonJs
            && u32::try_from(specifier_index).is_ok_and(|specifier_index| {
                host.get_mode_for_resolution_at_index(importing_node, specifier_index)
                    == EmitResolutionMode::EsNext
            })
        {
            continue;
        }
        if has_ts_file_extension(text) {
            return ModuleSpecifierEnding::TsExtension;
        }
        uses_js |= has_js_file_extension(text);
    }
    if uses_js {
        ModuleSpecifierEnding::JsExtension
    } else {
        ModuleSpecifierEnding::Minimal
    }
}

fn source_uses_extensions_on_imports(state: &CheckerState<'_>, importing_file: NodeId) -> bool {
    let index = state.binder.file_index_of_node(importing_file);
    module_name_literals(state, index)
        .0
        .into_iter()
        .filter_map(|literal| literal_text(state, literal))
        .find_map(|text| {
            (path_is_relative(text) && !extension_does_not_support_extensionless_resolution(text))
                .then(|| has_js_file_extension(text) || has_ts_file_extension(text))
        })
        .unwrap_or(false)
}

fn should_allow_importing_ts_extension(
    state: &CheckerState<'_>,
    importing_file: NodeId,
    options: &SpecifierCompilerOptions,
) -> bool {
    compiler_allows_importing_ts_extensions(options)
        || state
            .binder
            .source_of_node(importing_file)
            .is_declaration_file
}

fn compiler_allows_importing_ts_extensions(options: &SpecifierCompilerOptions) -> bool {
    options.compiler_options.allow_importing_ts_extensions == Some(true)
        || options.compiler_options.rewrite_relative_import_extensions == Some(true)
}

/// tsc-port: tryGetModuleSpecifiersFromCacheWorker @6.0.3
/// tsc-hash: 10648e220ed92dbaddcdf3346de02d23e656c0778da806e306ac9c6645a62a61
/// tsc-span: _tsc.js:45437-45446
pub(crate) fn try_get_module_specifiers_from_cache_worker(
    state: &CheckerState<'_>,
    module_symbol: SymbolId,
) -> ModuleSpecifierCacheProbe {
    let module_source_file = source_file_index_of_module(state, module_symbol);
    // h2-7a-m-3 §3a residue: getModuleSpecifierCache is absent from the
    // pinned CLI host and from EmitModuleSpecifierHost. Its optional lookup
    // therefore has the upstream typed-absent result (no cached specifiers,
    // paths, cache handle, or kind), while the source-file projection remains.
    ModuleSpecifierCacheProbe {
        module_source_file,
        ..ModuleSpecifierCacheProbe::default()
    }
}

/// tsc-port: getModuleSpecifiers @6.0.3
/// tsc-hash: e5ecc2f7960d98bef2b0cb27402157333d2535845136f779b0aee130a3253f09
/// tsc-span: _tsc.js:45447-45459
#[allow(clippy::too_many_arguments)]
pub(crate) fn get_module_specifiers(
    state: &mut CheckerState<'_>,
    module_symbol: SymbolId,
    compiler_options: &SpecifierCompilerOptions,
    importing_file: NodeId,
    importing_node: EmitResolverNode,
    host: &dyn EmitModuleSpecifierHost,
    user_preferences: &ModuleSpecifierUserPreferences,
    options: &ModuleSpecifierOptions,
) -> CheckResult<Vec<String>> {
    Ok(get_module_specifiers_with_cache_info(
        state,
        module_symbol,
        compiler_options,
        importing_file,
        importing_node,
        host,
        user_preferences,
        options,
        false,
    )?
    .module_specifiers)
}

/// tsc-port: getModuleSpecifiersWithCacheInfo @6.0.3
/// tsc-hash: 35ef7120889a0b9161f440010f8c801ca4715f4f982e9e16317bf991c83e022c
/// tsc-span: _tsc.js:45460-45492
#[allow(clippy::too_many_arguments)]
pub(crate) fn get_module_specifiers_with_cache_info(
    state: &mut CheckerState<'_>,
    module_symbol: SymbolId,
    compiler_options: &SpecifierCompilerOptions,
    importing_file: NodeId,
    importing_node: EmitResolverNode,
    host: &dyn EmitModuleSpecifierHost,
    user_preferences: &ModuleSpecifierUserPreferences,
    options: &ModuleSpecifierOptions,
    for_auto_import: bool,
) -> CheckResult<ModuleSpecifiersWithCacheInfo> {
    if let Some(ambient) = try_get_module_name_from_ambient_module(state, module_symbol)?
        .filter(|name| !name.is_empty())
    {
        let module_specifiers = if for_auto_import
            && is_excluded_by_regex(
                &ambient,
                &user_preferences.auto_import_specifier_exclude_regexes,
            ) {
            Vec::new()
        } else {
            vec![ambient]
        };
        return Ok(ModuleSpecifiersWithCacheInfo {
            kind: Some(ModuleSpecifierKind::Ambient),
            module_specifiers,
            computed_without_cache: false,
        });
    }

    let cache_probe = try_get_module_specifiers_from_cache_worker(state, module_symbol);
    if let Some(specifiers) = cache_probe.specifiers {
        return Ok(ModuleSpecifiersWithCacheInfo {
            kind: cache_probe.kind,
            module_specifiers: specifiers,
            computed_without_cache: false,
        });
    }
    let Some(module_source_file) = cache_probe.module_source_file else {
        return Ok(ModuleSpecifiersWithCacheInfo {
            kind: None,
            module_specifiers: Vec::new(),
            computed_without_cache: false,
        });
    };

    let info = get_info(&state.binder.source_of_node(importing_file).file_name, host);
    let module_paths = cache_probe.module_paths.unwrap_or_else(|| {
        get_all_module_paths_worker(
            &info,
            &state.binder.source(module_source_file).file_name,
            host,
            compiler_options,
            options,
        )
    });
    compute_module_specifiers(
        state,
        &module_paths,
        compiler_options,
        importing_file,
        importing_node,
        host,
        user_preferences,
        options,
        for_auto_import,
    )
}

/// tsc-port: computeModuleSpecifiers @6.0.3
/// tsc-hash: 3b65f2d4af82611bfbf4a32c2099a5fe215cbfff1b4daa729393650ac0a7238d
/// tsc-span: _tsc.js:45493-45561
#[allow(clippy::too_many_arguments)]
pub(crate) fn compute_module_specifiers(
    state: &mut CheckerState<'_>,
    module_paths: &[ModulePath],
    compiler_options: &SpecifierCompilerOptions,
    importing_file: NodeId,
    importing_node: EmitResolverNode,
    host: &dyn EmitModuleSpecifierHost,
    user_preferences: &ModuleSpecifierUserPreferences,
    options: &ModuleSpecifierOptions,
    for_auto_import: bool,
) -> CheckResult<ModuleSpecifiersWithCacheInfo> {
    let info = get_info(&state.binder.source_of_node(importing_file).file_name, host);
    let preferences = get_module_specifier_preferences(
        state,
        user_preferences,
        host,
        compiler_options,
        importing_file,
        None,
    );

    if state.kind_of(importing_file) == SyntaxKind::SourceFile {
        for module_path in module_paths {
            let imported_path = canonical_host_path(&module_path.path, host);
            for reason in host.import_include_reasons(&imported_path) {
                if reason.importing_file != importing_node.source() {
                    continue;
                }
                let existing_mode =
                    host.get_mode_for_resolution_at_index(importing_node, reason.index);
                let target_mode = options
                    .effective_override_import_mode()
                    .unwrap_or_else(|| host.get_default_resolution_mode_for_file(importing_node));
                if existing_mode != target_mode
                    && existing_mode != EmitResolutionMode::None
                    && target_mode != EmitResolutionMode::None
                {
                    continue;
                }
                let importing_index = state.binder.file_index_of_node(importing_file);
                let Some(specifier) =
                    get_module_name_string_literal_at(state, importing_index, reason.index)
                        .filter(|specifier| !specifier.is_empty())
                else {
                    continue;
                };
                if preferences.relative_preference != RelativePreference::NonRelative
                    || !path_is_relative(&specifier)
                {
                    return Ok(ModuleSpecifiersWithCacheInfo {
                        kind: None,
                        module_specifiers: vec![specifier],
                        computed_without_cache: true,
                    });
                }
            }
        }
    }

    let imported_file_is_in_node_modules = module_paths.iter().any(|path| path.is_in_node_modules);
    let mut node_modules_specifiers = Vec::new();
    let mut paths_specifiers = Vec::new();
    let mut redirect_paths_specifiers = Vec::new();
    let mut relative_specifiers = Vec::new();

    for module_path in module_paths {
        let specifier = if module_path.is_in_node_modules {
            try_get_module_name_as_node_module(
                state,
                module_path,
                &info,
                importing_file,
                host,
                compiler_options,
                user_preferences,
                false,
                options.effective_override_import_mode(),
            )
        } else {
            None
        };
        if let Some(specifier) = specifier.as_ref().filter(|specifier| {
            !(specifier.is_empty()
                || for_auto_import && is_excluded_by_regex(specifier, &preferences.exclude_regexes))
        }) {
            node_modules_specifiers.push(specifier.clone());
            if module_path.is_redirect {
                return Ok(ModuleSpecifiersWithCacheInfo {
                    kind: Some(ModuleSpecifierKind::NodeModules),
                    module_specifiers: node_modules_specifiers,
                    computed_without_cache: true,
                });
            }
        }

        let local = get_local_module_specifier(
            state,
            &module_path.path,
            &info,
            compiler_options,
            host,
            options
                .effective_override_import_mode()
                .unwrap_or(EmitResolutionMode::None),
            &preferences,
            module_path.is_redirect || specifier.is_some(),
        );
        let Some(local) = local.filter(|local| !local.is_empty()) else {
            continue;
        };
        if for_auto_import && is_excluded_by_regex(&local, &preferences.exclude_regexes) {
            continue;
        }
        if module_path.is_redirect {
            redirect_paths_specifiers.push(local);
        } else if path_is_bare_specifier(&local) {
            if path_contains_node_modules(&local) {
                relative_specifiers.push(local);
            } else {
                paths_specifiers.push(local);
            }
        } else if for_auto_import
            || !imported_file_is_in_node_modules
            || module_path.is_in_node_modules
        {
            relative_specifiers.push(local);
        }
    }

    let (kind, module_specifiers) = if !paths_specifiers.is_empty() {
        (ModuleSpecifierKind::Paths, paths_specifiers)
    } else if !redirect_paths_specifiers.is_empty() {
        (ModuleSpecifierKind::Redirect, redirect_paths_specifiers)
    } else if !node_modules_specifiers.is_empty() {
        (ModuleSpecifierKind::NodeModules, node_modules_specifiers)
    } else {
        (ModuleSpecifierKind::Relative, relative_specifiers)
    };
    Ok(ModuleSpecifiersWithCacheInfo {
        kind: Some(kind),
        module_specifiers,
        computed_without_cache: true,
    })
}

/// tsc-port: getInfo @6.0.3
/// tsc-hash: e5480f7a1312e0a56c08d8179e53dab7b8f8c6e4c7b0c0339728b06348c24732
/// tsc-span: _tsc.js:45568-45578
pub(crate) fn get_info(
    importing_source_file_name: &str,
    host: &dyn EmitModuleSpecifierHost,
) -> ModuleSpecifierInfo {
    let importing_source_file_name =
        normalized_absolute_path(importing_source_file_name, &host.get_current_directory());
    let source_directory = directory_path(&importing_source_file_name);
    let case_sensitive = host.use_case_sensitive_file_names();
    let canonical_source_directory = canonical_file_name(&source_directory, case_sensitive);
    ModuleSpecifierInfo {
        importing_source_file_name,
        source_directory,
        canonical_source_directory,
        case_sensitive,
    }
}

/// tsc-port: getLocalModuleSpecifier @6.0.3
/// tsc-hash: e0c4acfe0d045ce6dd2b39f1c843069c3be2b76c871ee1ab36caeb22bc9aaa02
/// tsc-span: _tsc.js:45579-45639
#[allow(clippy::too_many_arguments)]
pub(crate) fn get_local_module_specifier(
    state: &CheckerState<'_>,
    module_file_name: &str,
    info: &ModuleSpecifierInfo,
    options: &SpecifierCompilerOptions,
    host: &dyn EmitModuleSpecifierHost,
    import_mode: EmitResolutionMode,
    preferences: &ModuleSpecifierPreferences,
    paths_only: bool,
) -> Option<String> {
    if paths_only && options.paths.is_empty() {
        return None;
    }
    let allowed_endings =
        preferences.get_allowed_endings_in_preferred_order(state, host, options, import_mode);
    let relative_path = if let Some(relative_path) = try_get_module_name_from_root_dirs(
        &options.root_dirs,
        module_file_name,
        &info.source_directory,
        info.case_sensitive,
        &allowed_endings,
        options,
    ) {
        relative_path
    } else {
        let relative = get_relative_path_from_directory(
            &info.source_directory,
            module_file_name,
            info.case_sensitive,
        );
        process_ending(
            &ensure_path_is_non_module_name(&relative),
            &allowed_endings,
            options,
            Some(host),
        )?
    };

    let base_url = options.compiler_options.base_url.as_deref();
    let resolve_package_json_imports = get_resolve_package_json_imports(&options.compiler_options);
    if (base_url.is_none() && options.paths.is_empty() && !resolve_package_json_imports)
        || preferences.relative_preference == RelativePreference::Relative
    {
        return (!paths_only).then_some(relative_path);
    }

    let current_directory = host.get_current_directory();
    let paths_base_path = if options.paths.is_empty() {
        None
    } else {
        options.paths_base_path.as_deref()
    };
    let base_directory_source = base_url.or(paths_base_path).unwrap_or(&current_directory);
    let base_directory = normalized_absolute_path(base_directory_source, &current_directory);
    let relative_to_base_url =
        get_relative_path_if_in_same_volume(module_file_name, &base_directory, info.case_sensitive);
    let Some(relative_to_base_url) = relative_to_base_url.filter(|path| !path.is_empty()) else {
        return (!paths_only).then_some(relative_path);
    };

    let from_package_json_imports = if paths_only {
        None
    } else {
        try_get_module_name_from_package_json_imports(
            module_file_name,
            &info.source_directory,
            options,
            host,
            import_mode,
            prefers_ts_extension(&allowed_endings),
        )
    };
    let from_paths = if paths_only || from_package_json_imports.is_none() {
        try_get_module_name_from_paths(
            &relative_to_base_url,
            &options.paths,
            &allowed_endings,
            &base_directory,
            info.case_sensitive,
            host,
            options,
        )
    } else {
        None
    };
    if paths_only {
        return from_paths;
    }

    let maybe_non_relative = from_package_json_imports.or_else(|| {
        if from_paths.is_none() && base_url.is_some() {
            process_ending(&relative_to_base_url, &allowed_endings, options, None)
        } else {
            from_paths
        }
    });
    let Some(maybe_non_relative) = maybe_non_relative.filter(|specifier| !specifier.is_empty())
    else {
        return Some(relative_path);
    };

    let relative_is_excluded = is_excluded_by_regex(&relative_path, &preferences.exclude_regexes);
    let non_relative_is_excluded =
        is_excluded_by_regex(&maybe_non_relative, &preferences.exclude_regexes);
    if !relative_is_excluded && non_relative_is_excluded {
        return Some(relative_path);
    }
    if relative_is_excluded && !non_relative_is_excluded {
        return Some(maybe_non_relative);
    }
    if preferences.relative_preference == RelativePreference::NonRelative
        && !path_is_relative(&maybe_non_relative)
    {
        return Some(maybe_non_relative);
    }
    if preferences.relative_preference == RelativePreference::ExternalNonRelative
        && !path_is_relative(&maybe_non_relative)
    {
        let project_directory = options
            .config_file_path
            .as_deref()
            .map(directory_path)
            .map(|path| normalized_absolute_path(&path, &host.get_current_directory()))
            .unwrap_or_else(|| {
                normalized_absolute_path(
                    &host.get_current_directory(),
                    &host.get_current_directory(),
                )
            });
        let canonical_project_directory = info.canonical(&project_directory);
        let module_path = info.canonical(&normalized_absolute_path(
            module_file_name,
            &project_directory,
        ));
        let source_is_internal = info
            .canonical_source_directory
            .starts_with(&canonical_project_directory);
        let target_is_internal = module_path.starts_with(&canonical_project_directory);
        if source_is_internal != target_is_internal {
            return Some(maybe_non_relative);
        }
        let nearest_target_package_json =
            get_nearest_ancestor_directory_with_package_json(host, &directory_path(&module_path));
        let nearest_source_package_json =
            get_nearest_ancestor_directory_with_package_json(host, &info.source_directory);
        if !package_json_paths_are_equal(
            nearest_target_package_json.as_deref(),
            nearest_source_package_json.as_deref(),
            !host.use_case_sensitive_file_names(),
        ) {
            return Some(maybe_non_relative);
        }
        return Some(relative_path);
    }
    if is_path_relative_to_parent(&maybe_non_relative)
        || count_path_components(&relative_path) < count_path_components(&maybe_non_relative)
    {
        Some(relative_path)
    } else {
        Some(maybe_non_relative)
    }
}

/// tsc-port: packageJsonPathsAreEqual @6.0.3
/// tsc-hash: e0bcfb4183d5867103bc9539bdf95052c6680b5a393b612e7dcbbfee4ee2d6fb
/// tsc-span: _tsc.js:45640-45644
pub(crate) fn package_json_paths_are_equal(
    a: Option<&str>,
    b: Option<&str>,
    ignore_case: bool,
) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            let a = normalize_path_text(a);
            let b = normalize_path_text(b);
            paths_equal(&a, &b, ignore_case)
        }
        _ => false,
    }
}

/// tsc-port: countPathComponents @6.0.3
/// tsc-hash: b4ea80436ccf1d215c2561c8842f6cce0e1986a6fcb8afa561a314649a061026
/// tsc-span: _tsc.js:45645-45651
pub(crate) fn count_path_components(path: &str) -> usize {
    path.as_bytes()
        .iter()
        .skip(if path.starts_with("./") { 2 } else { 0 })
        .filter(|&&byte| byte == b'/')
        .count()
}

/// tsc-port: comparePathsByRedirectAndNumberOfDirectorySeparators @6.0.3
/// tsc-hash: b50457fb89d1c0a196bec15eb60415805944a12ba415b7baa9f23f7e51b2be82
/// tsc-span: _tsc.js:45652-45654
pub(crate) fn compare_paths_by_redirect_and_number_of_directory_separators(
    a: &ModulePath,
    b: &ModulePath,
) -> Ordering {
    b.is_redirect
        .cmp(&a.is_redirect)
        .then_with(|| count_directory_separators(&a.path).cmp(&count_directory_separators(&b.path)))
}

/// tsc-port: getNearestAncestorDirectoryWithPackageJson @6.0.3
/// tsc-hash: 62c270fac72dd01d0e0d116219e773d8b7e4e85830f229bc80ed7c28931c18e5
/// tsc-span: _tsc.js:45655-45664
pub(crate) fn get_nearest_ancestor_directory_with_package_json(
    host: &dyn EmitModuleSpecifierHost,
    file_name: &str,
) -> Option<String> {
    if let Some(directory) = host.get_nearest_ancestor_directory_with_package_json(file_name) {
        return Some(directory);
    }
    let global_cache = host.get_global_typings_cache_location();
    for directory in ancestor_directories(file_name) {
        if host.file_exists(&combine_paths(&directory, "package.json")) {
            return Some(directory);
        }
        if global_cache.as_deref() == Some(directory.as_str()) {
            break;
        }
    }
    None
}

/// tsc-port: forEachFileNameOfModule @6.0.3
/// tsc-hash: 78f43caaf0e47b60e268854811e359132a263fcd8ef34a48190cbf1e76a6afb9
/// tsc-span: _tsc.js:45665-45705
pub(crate) fn for_each_file_name_of_module<T>(
    importing_file_name: &str,
    imported_file_name: &str,
    host: &dyn EmitModuleSpecifierHost,
    prefer_symlinks: bool,
    mut callback: impl FnMut(&str, bool) -> Option<T>,
) -> Option<T> {
    let cwd = host.get_current_directory();
    let case_sensitive = host.use_case_sensitive_file_names();
    let reference_redirect = host
        .is_source_of_project_reference_redirect(imported_file_name)
        .then(|| host.get_redirect_from_source_file(imported_file_name))
        .flatten();
    let imported_path = canonical_host_path(imported_file_name, host);
    let mut imported_file_names = Vec::new();
    if let Some(reference_redirect) = &reference_redirect {
        imported_file_names.push(reference_redirect.clone());
    }
    imported_file_names.push(imported_file_name.to_owned());
    imported_file_names.extend(host.redirect_targets(&imported_path));
    let targets: Vec<String> = imported_file_names
        .iter()
        .map(|file| normalized_absolute_path(file, &cwd))
        .collect();
    let mut should_filter_ignored_paths = !targets.iter().all(|path| contains_ignored_path(path));

    if !prefer_symlinks {
        for target in &targets {
            if !(should_filter_ignored_paths && contains_ignored_path(target)) {
                let is_redirect = reference_redirect.as_ref().is_some_and(|redirect| {
                    paths_equal(
                        target,
                        &normalized_absolute_path(redirect, &cwd),
                        !case_sensitive,
                    )
                });
                if let Some(result) = callback(target, is_redirect) {
                    return Some(result);
                }
            }
        }
    }

    let importing_file_name = normalized_absolute_path(importing_file_name, &cwd);
    let mut symlink_candidates = Vec::new();
    // EmitModuleSpecifierHost represents each symlink fact as
    // `(real_path, symlink_path)`, matching the upstream cache's realpath key.
    for (real_file, symlink_file) in host.symlinked_files() {
        let real_file = normalized_absolute_path(&real_file, &cwd);
        for target in &targets {
            if paths_equal(target, &real_file, !case_sensitive) {
                symlink_candidates.push((
                    normalized_absolute_path(&symlink_file, &cwd),
                    reference_redirect
                        .as_ref()
                        .is_some_and(|redirect| paths_equal(target, redirect, !case_sensitive)),
                ));
            }
        }
    }

    let symlinked_directories = host.symlinked_directories();
    let global_typings_cache_location = host
        .get_global_typings_cache_location()
        .map(|path| normalized_absolute_path(&path, &cwd));
    let full_imported_file_name = normalized_absolute_path(imported_file_name, &cwd);
    for real_directory in ancestor_directories(&directory_path(&full_imported_file_name)) {
        if path_starts_with_directory(&importing_file_name, &real_directory, case_sensitive) {
            break;
        }
        for (real, symlink) in &symlinked_directories {
            let real = normalized_absolute_path(real, &cwd);
            if !paths_equal(&real, &real_directory, !case_sensitive) {
                continue;
            }
            for target in &targets {
                if !path_starts_with_directory(target, &real_directory, case_sensitive) {
                    continue;
                }
                let relative =
                    get_relative_path_from_directory(&real_directory, target, case_sensitive);
                symlink_candidates.push((
                    normalized_absolute_path(&combine_paths(symlink, &relative), &cwd),
                    reference_redirect
                        .as_ref()
                        .is_some_and(|redirect| paths_equal(target, redirect, !case_sensitive)),
                ));
            }
        }
        if global_typings_cache_location.as_deref() == Some(real_directory.as_str()) {
            break;
        }
    }
    for (candidate, is_redirect) in symlink_candidates {
        should_filter_ignored_paths = true;
        if let Some(result) = callback(&candidate, is_redirect) {
            return Some(result);
        }
    }

    if prefer_symlinks {
        for target in &targets {
            if should_filter_ignored_paths && contains_ignored_path(target) {
                continue;
            }
            let is_redirect = reference_redirect.as_ref().is_some_and(|redirect| {
                paths_equal(
                    target,
                    &normalized_absolute_path(redirect, &cwd),
                    !case_sensitive,
                )
            });
            if let Some(result) = callback(target, is_redirect) {
                return Some(result);
            }
        }
    }
    None
}

/// tsc-port: getAllRuntimeDependencies @6.0.3
/// tsc-hash: 62d9e01fb8c9f3f49fcd53deafb8cb72ff3d1b91d8b9f86ad573d1f29900a484
/// tsc-span: _tsc.js:45707-45716
pub(crate) fn get_all_runtime_dependencies(package_json: &Value) -> Vec<String> {
    let mut result = Vec::new();
    for field in ["dependencies", "peerDependencies", "optionalDependencies"] {
        if let Some(object) = package_json.get(field).and_then(Value::as_object) {
            result.extend(object.keys().cloned());
        }
    }
    result
}

/// tsc-port: getAllModulePathsWorker @6.0.3
/// tsc-hash: 9ba4e434497c9b1d29f5e0d15f59c6680591c4743b44f506cc97e6fbce3fc437
/// tsc-span: _tsc.js:45717-45785
pub(crate) fn get_all_module_paths_worker(
    info: &ModuleSpecifierInfo,
    imported_file_name: &str,
    host: &dyn EmitModuleSpecifierHost,
    _compiler_options: &SpecifierCompilerOptions,
    _options: &ModuleSpecifierOptions,
) -> Vec<ModulePath> {
    if host.module_resolution_cache_available()
        && !path_contains_node_modules(&info.importing_source_file_name)
    {
        // h2-7a-m-3 §3a residue: the package-json info cache nested inside
        // getModuleResolutionCache is not exposed by the pinned CLI host.
        // Dependency-resolution prewarming therefore observes typed absence;
        // authoritative symlink facts still flow through the ordinary walk.
    }

    let mut all_file_names = IndexMap::<String, ModulePath>::new();
    let mut imported_file_from_node_modules = false;
    let _: Option<()> = for_each_file_name_of_module(
        &info.importing_source_file_name,
        imported_file_name,
        host,
        true,
        |path, is_redirect| {
            let is_in_node_modules = path_contains_node_modules(path);
            all_file_names.insert(
                path.to_owned(),
                ModulePath {
                    path: info.canonical(path),
                    is_redirect,
                    is_in_node_modules,
                },
            );
            imported_file_from_node_modules |= is_in_node_modules;
            None
        },
    );
    let _ = imported_file_from_node_modules;

    let mut sorted_paths = Vec::new();
    let mut directory = info.canonical_source_directory.clone();
    while !all_file_names.is_empty() {
        let directory_start = ensure_trailing_directory_separator(&directory);
        let keys: Vec<String> = all_file_names
            .iter()
            .filter(|(_, value)| value.path.starts_with(&directory_start))
            .map(|(file_name, _)| file_name.clone())
            .collect();
        if !keys.is_empty() {
            let mut paths_in_directory: Vec<ModulePath> = keys
                .into_iter()
                .filter_map(|file_name| {
                    let value = all_file_names.shift_remove(&file_name)?;
                    Some(ModulePath {
                        path: file_name,
                        ..value
                    })
                })
                .collect();
            if paths_in_directory.len() > 1 {
                paths_in_directory
                    .sort_by(compare_paths_by_redirect_and_number_of_directory_separators);
            }
            sorted_paths.extend(paths_in_directory);
        }
        let new_directory = directory_path(&directory);
        if new_directory == directory {
            break;
        }
        directory = new_directory;
    }
    if !all_file_names.is_empty() {
        let mut remaining_paths: Vec<ModulePath> = all_file_names
            .into_iter()
            .map(|(file_name, value)| ModulePath {
                path: file_name,
                ..value
            })
            .collect();
        if remaining_paths.len() > 1 {
            remaining_paths.sort_by(compare_paths_by_redirect_and_number_of_directory_separators);
        }
        sorted_paths.extend(remaining_paths);
    }
    sorted_paths
}

/// tsc-port: tryGetModuleNameFromAmbientModule @6.0.3
/// tsc-hash: 510a64d026bfd561565d8bd9bdacb31a14f922854f88a76ff2892c08c657f057
/// tsc-span: _tsc.js:45786-45816
pub(crate) fn try_get_module_name_from_ambient_module(
    state: &mut CheckerState<'_>,
    module_symbol: SymbolId,
) -> CheckResult<Option<String>> {
    let declarations = state.binder.symbol(module_symbol).declarations.clone();
    for declaration in &declarations {
        let source = state.binder.source_of_node(*declaration);
        if !node_util::is_ambient_module(source, *declaration)
            || node_util::is_global_scope_augmentation(source, *declaration)
        {
            continue;
        }
        let Some(name) = module_declaration_name_text(state, *declaration) else {
            continue;
        };
        if !node_util::is_module_augmentation_external(source, *declaration)
            || !is_external_module_name_relative(&name)
        {
            return Ok(Some(name));
        }
    }

    for declaration in declarations {
        if state.kind_of(declaration) != SyntaxKind::ModuleDeclaration {
            continue;
        }
        let top_namespace = get_top_namespace(state, declaration);
        let Some(module_block) = state.parent_of(top_namespace) else {
            continue;
        };
        let Some(ambient_declaration) = state.parent_of(module_block) else {
            continue;
        };
        let Some(source_file) = state.parent_of(ambient_declaration) else {
            continue;
        };
        if state.kind_of(module_block) != SyntaxKind::ModuleBlock
            || !node_util::is_ambient_module(
                state.binder.source_of_node(ambient_declaration),
                ambient_declaration,
            )
            || state.kind_of(source_file) != SyntaxKind::SourceFile
        {
            continue;
        }
        let Some(ambient_symbol) = state.get_symbol_of_declaration_opt(ambient_declaration) else {
            continue;
        };
        let export_assignment = state
            .binder
            .symbol(ambient_symbol)
            .exports
            .get(tsc_binder::InternalSymbolName::EXPORT_EQUALS)
            .and_then(|&symbol| state.binder.symbol(symbol).value_declaration)
            .and_then(|value_declaration| match state.data_of(value_declaration) {
                NodeData::ExportAssignment(data) => data.expression,
                _ => None,
            });
        let Some(export_assignment) = export_assignment else {
            continue;
        };
        let Some(mut export_symbol) = state.get_resolved_symbol(export_assignment)? else {
            continue;
        };
        if state
            .binder
            .symbol(export_symbol)
            .flags
            .intersects(SymbolFlags::ALIAS)
        {
            export_symbol = state.resolve_alias(export_symbol)?;
        }
        if state.get_symbol_of_declaration_opt(declaration) == Some(export_symbol) {
            return Ok(module_declaration_name_text(state, ambient_declaration));
        }
    }
    Ok(None)
}

/// tsc-port: getTopNamespace @6.0.3 (nested in tryGetModuleNameFromAmbientModule)
/// tsc-hash: bff6b5d2375509a2e8872123bd66d5977817d898e962a0632d088cc468cb4e9c
/// tsc-span: _tsc.js:45805-45810
pub(crate) fn get_top_namespace(state: &CheckerState<'_>, mut declaration: NodeId) -> NodeId {
    while NodeFlags::from_bits(
        state
            .binder
            .source_of_node(declaration)
            .arena
            .node(declaration)
            .flags,
    )
    .intersects(NodeFlags::NESTED_NAMESPACE)
    {
        let Some(parent) = state.parent_of(declaration) else {
            break;
        };
        declaration = parent;
    }
    declaration
}

/// tsc-port: tryGetModuleNameFromPaths @6.0.3
/// tsc-hash: 4ec24fe788e16d7a27d09495ad090e154d73f0a1ff126e65bbc0b9d8fb58e417
/// tsc-span: _tsc.js:45817-45849
#[allow(clippy::too_many_arguments)]
pub(crate) fn try_get_module_name_from_paths(
    relative_to_base_url: &str,
    paths: &[ModulePathMapping],
    allowed_endings: &[ModuleSpecifierEnding],
    base_directory: &str,
    case_sensitive: bool,
    host: &dyn EmitModuleSpecifierHost,
    compiler_options: &SpecifierCompilerOptions,
) -> Option<String> {
    for mapping in paths {
        for pattern_text in &mapping.patterns {
            let normalized = normalize_path_text(pattern_text);
            let pattern = if path_is_absolute(&normalized) {
                get_relative_path_if_in_same_volume(&normalized, base_directory, case_sensitive)
                    .unwrap_or(normalized)
            } else {
                normalized
            };
            let mut candidates: Vec<(Option<ModuleSpecifierEnding>, String)> = allowed_endings
                .iter()
                .filter_map(|&ending| {
                    process_ending(relative_to_base_url, &[ending], compiler_options, None)
                        .map(|value| (Some(ending), value))
                })
                .collect();
            if extension_from_path(&pattern).is_some() {
                candidates.push((None, relative_to_base_url.to_owned()));
            }
            if let Some(star) = pattern.find('*') {
                let prefix = &pattern[..star];
                let suffix = &pattern[star + 1..];
                for &(ending, ref value) in &candidates {
                    if value.len() >= prefix.len() + suffix.len()
                        && value.starts_with(prefix)
                        && value.ends_with(suffix)
                        && validate_ending(
                            relative_to_base_url,
                            ending,
                            value,
                            compiler_options,
                            host,
                        )
                    {
                        let matched_star = &value[prefix.len()..value.len() - suffix.len()];
                        if !path_is_relative(matched_star) {
                            return Some(mapping.key.replacen('*', matched_star, 1));
                        }
                    }
                }
            } else if candidates.iter().any(|(ending, value)| {
                *ending != Some(ModuleSpecifierEnding::Minimal) && pattern == *value
            }) || candidates.iter().any(|(ending, value)| {
                *ending == Some(ModuleSpecifierEnding::Minimal)
                    && pattern == *value
                    && validate_ending(relative_to_base_url, *ending, value, compiler_options, host)
            }) {
                return Some(mapping.key.clone());
            }
        }
    }
    None
}

/// tsc-port: validateEnding @6.0.3 (nested in tryGetModuleNameFromPaths)
/// tsc-hash: 65f452bfe82f22cab223b2d242cf22c1120604dec911b98c4e35c352305b19a1
/// tsc-span: _tsc.js:45846-45848
pub(crate) fn validate_ending(
    relative_to_base_url: &str,
    ending: Option<ModuleSpecifierEnding>,
    value: &str,
    compiler_options: &SpecifierCompilerOptions,
    host: &dyn EmitModuleSpecifierHost,
) -> bool {
    ending != Some(ModuleSpecifierEnding::Minimal)
        || process_ending(
            relative_to_base_url,
            &[ModuleSpecifierEnding::Minimal],
            compiler_options,
            Some(host),
        )
        .as_deref()
            == Some(value)
}

/// tsc-port: tryGetModuleNameFromExportsOrImports @6.0.3
/// tsc-hash: e72371a7e2feae069b2dc51b43d8d3540490274f2bd4f3d67df58698e77cb0e8
/// tsc-span: _tsc.js:45850-45970
#[allow(clippy::too_many_arguments)]
pub(crate) fn try_get_module_name_from_exports_or_imports(
    options: &SpecifierCompilerOptions,
    host: &dyn EmitModuleSpecifierHost,
    target_file_path: &str,
    package_directory: &str,
    package_name: &str,
    exports: &Value,
    conditions: &[String],
    mode: ExportsKeyMode,
    is_imports: bool,
    prefer_ts_extension: bool,
) -> Option<ExportsOrImportsResult> {
    if let Some(target) = exports.as_str() {
        let ignore_case = !host.use_case_sensitive_file_names();
        let output_file = is_imports
            .then(|| output_js_file_name(target_file_path, options, host))
            .flatten();
        let declaration_file = is_imports
            .then(|| output_declaration_file_name(target_file_path, options, host))
            .flatten();
        let path_or_pattern = normalized_absolute_path(
            &combine_paths(package_directory, target),
            &host.get_current_directory(),
        );
        let extension_swapped_target = has_ts_file_extension(target_file_path)
            .then(|| {
                try_get_js_extension_for_file(target_file_path, options).map(|extension| {
                    format!("{}{}", remove_file_extension(target_file_path), extension)
                })
            })
            .flatten();
        let can_try_ts_extension =
            prefer_ts_extension && has_implementation_ts_file_extension(target_file_path);
        match mode {
            ExportsKeyMode::Exact => {
                let matches = extension_swapped_target
                    .as_deref()
                    .is_some_and(|path| paths_equal(path, &path_or_pattern, ignore_case))
                    || paths_equal(target_file_path, &path_or_pattern, ignore_case)
                    || output_file
                        .as_deref()
                        .is_some_and(|path| paths_equal(path, &path_or_pattern, ignore_case))
                    || declaration_file
                        .as_deref()
                        .is_some_and(|path| paths_equal(path, &path_or_pattern, ignore_case));
                if matches {
                    return Some(exports_result(package_name));
                }
            }
            ExportsKeyMode::Directory => {
                if can_try_ts_extension
                    && path_starts_with_directory(&path_or_pattern, target_file_path, !ignore_case)
                {
                    let fragment = get_relative_path_from_directory(
                        &path_or_pattern,
                        target_file_path,
                        !ignore_case,
                    );
                    return Some(exports_result(&normalize_path_text(&combine_paths(
                        &combine_paths(package_name, target),
                        &fragment,
                    ))));
                }
                if let Some(extension_swapped_target) = &extension_swapped_target {
                    if path_starts_with_directory(
                        extension_swapped_target,
                        &path_or_pattern,
                        !ignore_case,
                    ) {
                        let fragment = get_relative_path_from_directory(
                            &path_or_pattern,
                            extension_swapped_target,
                            !ignore_case,
                        );
                        return Some(exports_result(&normalize_path_text(&combine_paths(
                            &combine_paths(package_name, target),
                            &fragment,
                        ))));
                    }
                }
                if !can_try_ts_extension
                    && path_starts_with_directory(target_file_path, &path_or_pattern, !ignore_case)
                {
                    let fragment = get_relative_path_from_directory(
                        &path_or_pattern,
                        target_file_path,
                        !ignore_case,
                    );
                    return Some(exports_result(&normalize_path_text(&combine_paths(
                        &combine_paths(package_name, target),
                        &fragment,
                    ))));
                }
                if let Some(output_file) = &output_file {
                    if path_starts_with_directory(output_file, &path_or_pattern, !ignore_case) {
                        let fragment = get_relative_path_from_directory(
                            &path_or_pattern,
                            output_file,
                            !ignore_case,
                        );
                        return Some(exports_result(&combine_paths(package_name, &fragment)));
                    }
                }
                if let Some(declaration_file) = &declaration_file {
                    if path_starts_with_directory(declaration_file, &path_or_pattern, !ignore_case)
                    {
                        let fragment = get_relative_path_from_directory(
                            &path_or_pattern,
                            declaration_file,
                            !ignore_case,
                        );
                        let extension = get_js_extension_for_file(declaration_file, options);
                        return Some(exports_result(&change_full_extension(
                            &combine_paths(package_name, &fragment),
                            extension,
                        )));
                    }
                }
            }
            ExportsKeyMode::Pattern => {
                let (leading, trailing) = if let Some(star) = path_or_pattern.find('*') {
                    (&path_or_pattern[..star], &path_or_pattern[star + 1..])
                } else {
                    // Preserve JS `slice(0, -1)` / `slice(0)` behavior for a
                    // wildcard package key whose target contains no `*`.
                    let last = path_or_pattern
                        .char_indices()
                        .next_back()
                        .map_or(0, |(index, _)| index);
                    (&path_or_pattern[..last], path_or_pattern.as_str())
                };
                if can_try_ts_extension {
                    if let Some(star_replacement) =
                        match_path_pattern(target_file_path, leading, trailing, ignore_case)
                    {
                        return Some(exports_result(&package_name.replacen(
                            '*',
                            star_replacement,
                            1,
                        )));
                    }
                }
                if let Some(extension_swapped_target) = &extension_swapped_target {
                    if let Some(star_replacement) =
                        match_path_pattern(extension_swapped_target, leading, trailing, ignore_case)
                    {
                        return Some(exports_result(&package_name.replacen(
                            '*',
                            star_replacement,
                            1,
                        )));
                    }
                }
                if !can_try_ts_extension {
                    if let Some(star_replacement) =
                        match_path_pattern(target_file_path, leading, trailing, ignore_case)
                    {
                        return Some(exports_result(&package_name.replacen(
                            '*',
                            star_replacement,
                            1,
                        )));
                    }
                }
                if let Some(output_file) = &output_file {
                    if let Some(star_replacement) =
                        match_path_pattern(output_file, leading, trailing, ignore_case)
                    {
                        return Some(exports_result(&package_name.replacen(
                            '*',
                            star_replacement,
                            1,
                        )));
                    }
                }
                if let Some(declaration_file) = &declaration_file {
                    if let Some(star_replacement) =
                        match_path_pattern(declaration_file, leading, trailing, ignore_case)
                    {
                        let substituted = package_name.replacen('*', star_replacement, 1);
                        return try_get_js_extension_for_file(declaration_file, options).map(
                            |extension| {
                                exports_result(&change_full_extension(&substituted, extension))
                            },
                        );
                    }
                }
            }
        }
    } else if let Some(array) = exports.as_array() {
        for entry in array {
            if let Some(result) = try_get_module_name_from_exports_or_imports(
                options,
                host,
                target_file_path,
                package_directory,
                package_name,
                entry,
                conditions,
                mode,
                is_imports,
                prefer_ts_extension,
            ) {
                return Some(result);
            }
        }
    } else if let Some(object) = exports.as_object() {
        for (key, sub_target) in object {
            if key == "default"
                || conditions.iter().any(|condition| condition == key)
                || is_applicable_versioned_types_key(conditions, key)
            {
                if let Some(result) = try_get_module_name_from_exports_or_imports(
                    options,
                    host,
                    target_file_path,
                    package_directory,
                    package_name,
                    sub_target,
                    conditions,
                    mode,
                    is_imports,
                    prefer_ts_extension,
                ) {
                    return Some(result);
                }
            }
        }
    }
    None
}

fn exports_result(module_file_to_try: &str) -> ExportsOrImportsResult {
    ExportsOrImportsResult {
        module_file_to_try: module_file_to_try.to_owned(),
        ..ExportsOrImportsResult::default()
    }
}

/// tsc-port: tryGetModuleNameFromExports @6.0.3
/// tsc-hash: 3252395c4d8796e30f35179766c05dbac63a21fdb614737bceeb95a253e1681f
/// tsc-span: _tsc.js:45971-46010
#[allow(clippy::too_many_arguments)]
pub(crate) fn try_get_module_name_from_exports(
    options: &SpecifierCompilerOptions,
    host: &dyn EmitModuleSpecifierHost,
    target_file_path: &str,
    package_directory: &str,
    package_name: &str,
    exports: &Value,
    conditions: &[String],
) -> Option<ExportsOrImportsResult> {
    if let Some(object) = exports
        .as_object()
        .filter(|object| !object.is_empty() && object.keys().all(|key| key.starts_with('.')))
    {
        for (key, target) in object {
            let sub_package_name = normalize_path_text(&combine_paths(package_name, key));
            let mode = if key.ends_with('/') {
                ExportsKeyMode::Directory
            } else if key.contains('*') {
                ExportsKeyMode::Pattern
            } else {
                ExportsKeyMode::Exact
            };
            if let Some(result) = try_get_module_name_from_exports_or_imports(
                options,
                host,
                target_file_path,
                package_directory,
                &sub_package_name,
                target,
                conditions,
                mode,
                false,
                false,
            ) {
                return Some(result);
            }
        }
        return None;
    }
    try_get_module_name_from_exports_or_imports(
        options,
        host,
        target_file_path,
        package_directory,
        package_name,
        exports,
        conditions,
        ExportsKeyMode::Exact,
        false,
        false,
    )
}

/// tsc-port: tryGetModuleNameFromPackageJsonImports @6.0.3
/// tsc-hash: 1091088171043c527e0ce5ee5aa678b10d0209f896215f0700ec1186751614cd
/// tsc-span: _tsc.js:46011-46048
pub(crate) fn try_get_module_name_from_package_json_imports(
    module_file_name: &str,
    source_directory: &str,
    options: &SpecifierCompilerOptions,
    host: &dyn EmitModuleSpecifierHost,
    import_mode: EmitResolutionMode,
    prefer_ts_extension: bool,
) -> Option<String> {
    if !get_resolve_package_json_imports(&options.compiler_options) {
        return None;
    }
    let ancestor = get_nearest_ancestor_directory_with_package_json(host, source_directory)?;
    let package_json_path = combine_paths(&ancestor, "package.json");
    if !host.file_exists(&package_json_path) {
        return None;
    }
    // The optional package-json info cache is absent; upstream immediately
    // falls back to readFile in this arm, so no behavior is guessed here.
    let package_json: Value = serde_json::from_str(&host.read_file(&package_json_path)?).ok()?;
    let imports = package_json.get("imports")?.as_object()?;
    let conditions = get_conditions(&options.compiler_options, import_mode);
    for (key, target) in imports {
        if !key.starts_with('#') || key == "#" || key.starts_with("#/") {
            continue;
        }
        let mode = if key.ends_with('/') {
            ExportsKeyMode::Directory
        } else if key.contains('*') {
            ExportsKeyMode::Pattern
        } else {
            ExportsKeyMode::Exact
        };
        if let Some(result) = try_get_module_name_from_exports_or_imports(
            options,
            host,
            module_file_name,
            &ancestor,
            key,
            target,
            &conditions,
            mode,
            true,
            prefer_ts_extension,
        ) {
            return Some(result.module_file_to_try);
        }
    }
    None
}

/// tsc-port: tryGetModuleNameFromRootDirs @6.0.3
/// tsc-hash: 53d7945047cecac6bc1e65d999f856fec0383c6b829493bd307736d02897a89a
/// tsc-span: _tsc.js:46049-46063
pub(crate) fn try_get_module_name_from_root_dirs(
    root_dirs: &[String],
    module_file_name: &str,
    source_directory: &str,
    case_sensitive: bool,
    allowed_endings: &[ModuleSpecifierEnding],
    compiler_options: &SpecifierCompilerOptions,
) -> Option<String> {
    if root_dirs.is_empty() {
        return None;
    }
    let normalized_target_paths =
        get_paths_relative_to_root_dirs(module_file_name, root_dirs, case_sensitive);
    if normalized_target_paths.is_empty() {
        return None;
    }
    let normalized_source_paths =
        get_paths_relative_to_root_dirs(source_directory, root_dirs, case_sensitive);
    let mut relative_paths = Vec::new();
    for source_path in &normalized_source_paths {
        for target_path in &normalized_target_paths {
            relative_paths.push(ensure_path_is_non_module_name(
                &get_relative_path_from_directory(source_path, target_path, case_sensitive),
            ));
        }
    }
    let shortest = relative_paths
        .into_iter()
        .reduce(|left, right| {
            if count_directory_separators(&left) < count_directory_separators(&right) {
                left
            } else {
                right
            }
        })
        .filter(|path| !path.is_empty())?;
    process_ending(&shortest, allowed_endings, compiler_options, None)
}

/// tsc-port: tryGetModuleNameAsNodeModule @6.0.3
/// tsc-hash: fc15518f28df3dc93f72029b597d9208658ed4dc7a224a4d4f5d2d3ff0382d52
/// tsc-span: _tsc.js:46064-46179
#[allow(clippy::too_many_arguments)]
pub(crate) fn try_get_module_name_as_node_module(
    state: &CheckerState<'_>,
    module_path: &ModulePath,
    info: &ModuleSpecifierInfo,
    importing_file: NodeId,
    host: &dyn EmitModuleSpecifierHost,
    options: &SpecifierCompilerOptions,
    user_preferences: &ModuleSpecifierUserPreferences,
    package_name_only: bool,
    override_mode: Option<EmitResolutionMode>,
) -> Option<String> {
    let parts = get_node_module_path_parts(&module_path.path)?;
    let preferences = get_module_specifier_preferences(
        state,
        user_preferences,
        host,
        options,
        importing_file,
        None,
    );
    let allowed_endings = preferences.get_allowed_endings_in_preferred_order(
        state,
        host,
        options,
        EmitResolutionMode::None,
    );
    let mut module_specifier = module_path.path.clone();
    let mut is_package_root_path = false;
    if !package_name_only {
        let mut package_root_index = parts.package_root_index;
        let mut module_file_name = None;
        loop {
            let result = try_directory_with_package_json(
                &module_path.path,
                parts,
                package_root_index,
                emit_resolver_node_for_file(
                    state,
                    state.binder.file_index_of_node(importing_file),
                    importing_file,
                ),
                host,
                options,
                &allowed_endings,
                override_mode,
            );
            if options.compiler_options.emit_module_resolution_kind() != 1 {
                if result.blocked_by_exports {
                    return None;
                }
                if result.verbatim_from_exports {
                    return Some(result.module_file_to_try);
                }
            }
            if let Some(package_root_path) = result.package_root_path {
                module_specifier = package_root_path;
                is_package_root_path = true;
                break;
            }
            module_file_name.get_or_insert(result.module_file_to_try);
            // `path.indexOf(directorySeparator, packageRootIndex + 1)` returns -1
            // when the start index is at or beyond the end of the string — a
            // file directly under node_modules (`/node_modules/umd.d.ts`) ends
            // at its package-root index (h2-7b-m-2 fence amendment #4b).
            let Some(next_slash) = module_path
                .path
                .get(package_root_index + 1..)
                .and_then(|rest| rest.find('/'))
            else {
                module_specifier = process_ending(
                    module_file_name.as_deref().unwrap_or(&module_path.path),
                    &allowed_endings,
                    options,
                    Some(host),
                )?;
                break;
            };
            package_root_index += next_slash + 1;
        }
    }
    if module_path.is_redirect && !is_package_root_path {
        return None;
    }

    let global_typings_cache_location = host.get_global_typings_cache_location();
    // h2-7a-m-3 §3a residue: a missing global typings location is the
    // typed absence of the second containment root, never a fabricated root.
    let path_to_top_level_node_modules = info.canonical(
        module_specifier
            .get(..parts.top_level_node_modules_index)
            .unwrap_or_default(),
    );
    let source_contains = info
        .canonical_source_directory
        .starts_with(&path_to_top_level_node_modules);
    let global_contains = global_typings_cache_location
        .as_deref()
        .is_some_and(|location| {
            info.canonical(location)
                .starts_with(&path_to_top_level_node_modules)
        });
    if !source_contains && !global_contains {
        return None;
    }

    let node_modules_directory_name =
        module_specifier.get(parts.top_level_package_name_index + 1..)?;
    let package_name = get_package_name_from_types_package_name(node_modules_directory_name);
    if options.compiler_options.emit_module_resolution_kind() == 1
        && package_name == node_modules_directory_name
    {
        None
    } else {
        Some(package_name)
    }
}

/// tsc-port: tryDirectoryWithPackageJson @6.0.3 (nested in tryGetModuleNameAsNodeModule)
/// tsc-hash: c8d4efd504f506135eb3cb422366a3441f4ed875c3e863de0821bf4902d0d22c
/// tsc-span: _tsc.js:46113-46178
#[allow(clippy::too_many_arguments)]
pub(crate) fn try_directory_with_package_json(
    path: &str,
    parts: NodeModulePathParts,
    package_root_index: usize,
    importing_file: EmitResolverNode,
    host: &dyn EmitModuleSpecifierHost,
    options: &SpecifierCompilerOptions,
    allowed_endings: &[ModuleSpecifierEnding],
    override_mode: Option<EmitResolutionMode>,
) -> ExportsOrImportsResult {
    let package_root_path = &path[..package_root_index];
    let package_json_path = combine_paths(package_root_path, "package.json");
    let mut module_file_to_try = path.to_owned();
    let mut maybe_blocked_by_types_versions = false;
    let package_json_exists = host.file_exists(&package_json_path);
    let package_json_content = package_json_exists
        .then(|| host.read_file(&package_json_path))
        .flatten()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    if package_json_exists {
        let import_mode = override_mode
            .unwrap_or_else(|| host.get_default_resolution_mode_for_file(importing_file));
        if get_resolve_package_json_exports(&options.compiler_options) {
            let node_modules_directory_name =
                &package_root_path[parts.top_level_package_name_index + 1..];
            let package_name =
                get_package_name_from_types_package_name(node_modules_directory_name);
            let conditions = get_conditions(&options.compiler_options, import_mode);
            if let Some(exports) = package_json_content
                .as_ref()
                .and_then(|content| content.get("exports"))
                .filter(|exports| json_value_is_truthy(exports))
            {
                if let Some(mut from_exports) = try_get_module_name_from_exports(
                    options,
                    host,
                    path,
                    package_root_path,
                    &package_name,
                    exports,
                    &conditions,
                ) {
                    from_exports.verbatim_from_exports = true;
                    return from_exports;
                }
                return ExportsOrImportsResult {
                    module_file_to_try: path.to_owned(),
                    blocked_by_exports: true,
                    ..ExportsOrImportsResult::default()
                };
            }
        }

        if let Some(version_paths) = package_json_content
            .as_ref()
            .and_then(selected_types_versions_paths)
        {
            let submodule_name = path.get(package_root_path.len() + 1..).unwrap_or_default();
            if let Some(from_paths) = try_get_module_name_from_paths(
                submodule_name,
                &version_paths,
                allowed_endings,
                package_root_path,
                host.use_case_sensitive_file_names(),
                host,
                options,
            ) {
                module_file_to_try = combine_paths(package_root_path, &from_paths);
            } else {
                maybe_blocked_by_types_versions = true;
            }
        }

        let main_file_value = ["typings", "types", "main"]
            .iter()
            .find_map(|key| {
                package_json_content
                    .as_ref()
                    .and_then(|content| content.get(key))
                    .filter(|value| json_value_is_truthy(value))
            })
            .cloned()
            .unwrap_or_else(|| Value::String("index.js".to_owned()));
        let main_file_relative = main_file_value.as_str();
        let main_is_blocked_by_types_versions = main_file_relative.is_some_and(|main_file| {
            maybe_blocked_by_types_versions
                && package_json_content
                    .as_ref()
                    .and_then(selected_types_versions_paths)
                    .is_some_and(|paths| {
                        paths
                            .iter()
                            .any(|mapping| path_mapping_key_matches(&mapping.key, main_file))
                    })
        });
        if let Some(main_file_relative) =
            main_file_relative.filter(|_| !main_is_blocked_by_types_versions)
        {
            let main_export_file = canonical_file_name(
                &normalized_absolute_path(main_file_relative, package_root_path),
                host.use_case_sensitive_file_names(),
            );
            let canonical_module_file_to_try =
                canonical_file_name(&module_file_to_try, host.use_case_sensitive_file_names());
            if remove_file_extension(&main_export_file)
                == remove_file_extension(&canonical_module_file_to_try)
            {
                return ExportsOrImportsResult {
                    package_root_path: Some(package_root_path.to_owned()),
                    module_file_to_try,
                    ..ExportsOrImportsResult::default()
                };
            }
            let package_is_module = package_json_content
                .as_ref()
                .and_then(|content| content.get("type"))
                .and_then(Value::as_str)
                == Some("module");
            if !package_is_module
                && !extension_does_not_support_extensionless_resolution(
                    &canonical_module_file_to_try,
                )
                && canonical_module_file_to_try.starts_with(&main_export_file)
                && directory_path(&canonical_module_file_to_try)
                    == main_export_file.trim_end_matches('/')
                && remove_file_extension(base_file_name(&canonical_module_file_to_try)) == "index"
            {
                return ExportsOrImportsResult {
                    package_root_path: Some(package_root_path.to_owned()),
                    module_file_to_try,
                    ..ExportsOrImportsResult::default()
                };
            }
        }
    } else {
        let file_name = canonical_file_name(
            path.get(parts.package_root_index + 1..).unwrap_or_default(),
            host.use_case_sensitive_file_names(),
        );
        if matches!(
            file_name.as_str(),
            "index.d.ts" | "index.js" | "index.ts" | "index.tsx"
        ) {
            return ExportsOrImportsResult {
                package_root_path: Some(package_root_path.to_owned()),
                module_file_to_try,
                ..ExportsOrImportsResult::default()
            };
        }
    }
    ExportsOrImportsResult {
        module_file_to_try,
        ..ExportsOrImportsResult::default()
    }
}

/// tsc-port: tryGetAnyFileFromPath @6.0.3
/// tsc-hash: e93b7fcddf01449570a10abb891b3c0d45d970608688491f1108fc9346549a71
/// tsc-span: _tsc.js:46180-46189
pub(crate) fn try_get_any_file_from_path(
    host: &dyn EmitModuleSpecifierHost,
    path: &str,
) -> Option<String> {
    for extension in [
        ".ts", ".tsx", ".d.ts", ".js", ".jsx", ".cts", ".d.cts", ".cjs", ".mts", ".d.mts", ".mjs",
        ".node", ".json",
    ] {
        let full_path = format!("{path}{extension}");
        if host.file_exists(&full_path) {
            return Some(full_path);
        }
    }
    None
}

/// tsc-port: getPathsRelativeToRootDirs @6.0.3
/// tsc-hash: 0999acb417e2c9858cff3cc8bb693d31cd968e39a364d4db952c7e140d423636
/// tsc-span: _tsc.js:46190-46195
pub(crate) fn get_paths_relative_to_root_dirs(
    path: &str,
    root_dirs: &[String],
    case_sensitive: bool,
) -> Vec<String> {
    root_dirs
        .iter()
        .filter_map(|root_dir| {
            let relative = get_relative_path_if_in_same_volume(path, root_dir, case_sensitive)?;
            (!is_path_relative_to_parent(&relative)).then_some(relative)
        })
        .collect()
}

/// tsc-port: processEnding @6.0.3
/// tsc-hash: 64c345fbec1324434ec6982b33c84acd20e6fb25de15ae741eb0ebffe77e8f4b
/// tsc-span: _tsc.js:46196-46233
pub(crate) fn process_ending(
    file_name: &str,
    allowed_endings: &[ModuleSpecifierEnding],
    options: &SpecifierCompilerOptions,
    host: Option<&dyn EmitModuleSpecifierHost>,
) -> Option<String> {
    if file_extension_is_one_of(file_name, &[".json", ".mjs", ".cjs"]) {
        return Some(file_name.to_owned());
    }
    let no_extension = remove_file_extension(file_name);
    if file_name == no_extension {
        return Some(file_name.to_owned());
    }
    let js_priority = allowed_endings
        .iter()
        .position(|ending| *ending == ModuleSpecifierEnding::JsExtension);
    let ts_priority = allowed_endings
        .iter()
        .position(|ending| *ending == ModuleSpecifierEnding::TsExtension);
    if file_extension_is_one_of(file_name, &[".mts", ".cts"])
        && ts_priority.is_some_and(|ts_priority| {
            js_priority.is_some_and(|js_priority| ts_priority < js_priority)
        })
    {
        return Some(file_name.to_owned());
    }
    if file_extension_is_one_of(file_name, &[".d.mts", ".mts", ".d.cts", ".cts"]) {
        return Some(format!(
            "{}{}",
            no_extension,
            get_js_extension_for_file(file_name, options)
        ));
    }
    if !file_extension_is_one_of(file_name, &[".d.ts"])
        && file_extension_is_one_of(file_name, &[".ts"])
        && file_name.contains(".d.")
    {
        return try_get_real_file_name_for_non_js_declaration_file_name(file_name);
    }
    Some(match allowed_endings.first().copied()? {
        ModuleSpecifierEnding::Minimal => {
            let without_index = no_extension.strip_suffix("/index").unwrap_or(&no_extension);
            if host.is_some_and(|host| {
                without_index != no_extension
                    && try_get_any_file_from_path(host, without_index).is_some()
            }) {
                no_extension
            } else {
                without_index.to_owned()
            }
        }
        ModuleSpecifierEnding::Index => no_extension,
        ModuleSpecifierEnding::JsExtension => format!(
            "{}{}",
            no_extension,
            get_js_extension_for_file(file_name, options)
        ),
        ModuleSpecifierEnding::TsExtension => {
            if is_declaration_file_name(file_name) {
                let extensionless_priority = allowed_endings.iter().position(|ending| {
                    matches!(
                        ending,
                        ModuleSpecifierEnding::Minimal | ModuleSpecifierEnding::Index
                    )
                });
                if extensionless_priority.is_some_and(|extensionless_priority| {
                    js_priority.is_some_and(|js_priority| extensionless_priority < js_priority)
                }) {
                    no_extension
                } else {
                    format!(
                        "{}{}",
                        no_extension,
                        get_js_extension_for_file(file_name, options)
                    )
                }
            } else {
                file_name.to_owned()
            }
        }
    })
}

/// tsc-port: tryGetRealFileNameForNonJsDeclarationFileName @6.0.3
/// tsc-hash: 708ef279f3a044763a0274c327f23e6adc91e8541dcbd2612fa796d67527db7a
/// tsc-span: _tsc.js:46234-46240
pub(crate) fn try_get_real_file_name_for_non_js_declaration_file_name(
    file_name: &str,
) -> Option<String> {
    let base_name = base_file_name(file_name);
    if !file_name.ends_with(".ts") || !base_name.contains(".d.") || base_name.ends_with(".d.ts") {
        return None;
    }
    let no_extension = file_name.strip_suffix(".ts")?;
    let extension = &no_extension[no_extension.rfind('.')?..];
    let declaration_marker = file_name.find(".d.")?;
    Some(format!(
        "{}{}",
        &no_extension[..declaration_marker],
        extension
    ))
}

/// tsc-port: getJSExtensionForFile @6.0.3
/// tsc-hash: 0cab3d0888593f4265b5f7582423e5f52c3fafed97c2843f73f4d5c63e099763
/// tsc-span: _tsc.js:46241-46243
pub(crate) fn get_js_extension_for_file(
    file_name: &str,
    options: &SpecifierCompilerOptions,
) -> &'static str {
    try_get_js_extension_for_file(file_name, options)
        .unwrap_or_else(|| panic!("unsupported module-specifier extension: {file_name}"))
}

/// tsc-port: tryGetJSExtensionForFile @6.0.3
/// tsc-hash: 48e572843ba74a29b29855c877760bc71dbea427eeb4f6576e66b1e212d31a1f
/// tsc-span: _tsc.js:46244-46267
pub(crate) fn try_get_js_extension_for_file(
    file_name: &str,
    options: &SpecifierCompilerOptions,
) -> Option<&'static str> {
    match extension_from_path(file_name)? {
        ".ts" | ".d.ts" => Some(".js"),
        ".tsx" => Some(if options.compiler_options.jsx == Some(1) {
            ".jsx"
        } else {
            ".js"
        }),
        ".js" => Some(".js"),
        ".jsx" => Some(".jsx"),
        ".json" => Some(".json"),
        ".d.mts" | ".mts" | ".mjs" => Some(".mjs"),
        ".d.cts" | ".cts" | ".cjs" => Some(".cjs"),
        _ => None,
    }
}

/// tsc-port: getRelativePathIfInSameVolume @6.0.3
/// tsc-hash: 92f50dd116901f21d73e66701319358380809eacabd880157e0451c823dd52f4
/// tsc-span: _tsc.js:46268-46278
pub(crate) fn get_relative_path_if_in_same_volume(
    path: &str,
    directory_path: &str,
    case_sensitive: bool,
) -> Option<String> {
    let path = if path_is_absolute(path) {
        normalize_path_text(path)
    } else {
        normalized_absolute_path(path, directory_path)
    };
    let directory_path = if path_is_absolute(directory_path) {
        normalize_path_text(directory_path)
    } else {
        normalized_absolute_path(directory_path, "/")
    };
    if !roots_equal(&path, &directory_path) {
        return None;
    }
    let relative = get_relative_path_from_directory(&directory_path, &path, case_sensitive);
    (!path_is_absolute(&relative)).then_some(relative)
}

/// tsc-port: isPathRelativeToParent @6.0.3
/// tsc-hash: efe08b88ffa50eb44d7a8de90ce3506f9d7f50a1d1400bb55a716b3f5b297ff0
/// tsc-span: _tsc.js:46279-46281
pub(crate) fn is_path_relative_to_parent(path: &str) -> bool {
    path.starts_with("..")
}

/// tsc-port: getDefaultResolutionModeForFile @6.0.3
/// tsc-hash: 2e8aa0716865c45fb13dfcb54b024ae5449dff0d38d0a81d7b9f82f4cb30cafa
/// tsc-span: _tsc.js:46282-46284
pub(crate) fn get_default_resolution_mode_for_file(
    file: EmitResolverNode,
    host: &dyn EmitModuleSpecifierHost,
    _compiler_options: &SpecifierCompilerOptions,
) -> EmitResolutionMode {
    host.get_default_resolution_mode_for_file(file)
}

/// tsc-port: prefersTsExtension @6.0.3
/// tsc-hash: 7939652b473b2d6db3983a1db2c93ca510e565f6616ff31a9f8bfcd6c01f24ec
/// tsc-span: _tsc.js:46285-46288
pub(crate) fn prefers_ts_extension(allowed_endings: &[ModuleSpecifierEnding]) -> bool {
    let ts_priority = allowed_endings
        .iter()
        .position(|ending| *ending == ModuleSpecifierEnding::TsExtension);
    let js_priority = allowed_endings
        .iter()
        .position(|ending| *ending == ModuleSpecifierEnding::JsExtension);
    ts_priority
        .is_some_and(|ts_priority| js_priority.is_some_and(|js_priority| ts_priority < js_priority))
}

/// tsc-port: isExcludedByRegex @6.0.3
/// tsc-hash: c6787571d1157d0ee4c7e797349b370267dc000c04df5567d3075b501ed4eb69
/// tsc-span: _tsc.js:45562-45567
pub(crate) fn is_excluded_by_regex(module_specifier: &str, exclude_regexes: &[String]) -> bool {
    exclude_regexes.iter().any(|pattern| {
        match string_to_regex(pattern) {
            Some(CompiledExcludeRegex::Supported(regex)) => regex.is_match(module_specifier),
            // A valid JS regexp outside the small no-dependency matcher is
            // exclusion-safe: never admit a possibly excluded specifier.
            Some(CompiledExcludeRegex::FailClosed) => true,
            None => false,
        }
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CompiledExcludeRegex {
    Supported(SimpleRegex),
    FailClosed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SimpleRegex {
    alternatives: Vec<Vec<RegexPiece>>,
    anchored_start: bool,
    anchored_end: bool,
    ignore_case: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RegexPiece {
    atom: RegexAtom,
    repetition: RegexRepetition,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RegexAtom {
    Literal(char),
    Any,
    Digit,
    Word,
    Whitespace,
    Class {
        negated: bool,
        ranges: Vec<(char, char)>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RegexRepetition {
    One,
    ZeroOrOne,
    ZeroOrMore,
    OneOrMore,
}

/// tsc-port: stringToRegex @6.0.3 (memoized closure)
/// tsc-hash: fd26ebd0f4f096eacee27f3b2e49eb34d0767855685dd67ddb47d96a856460e1
/// tsc-span: _tsc.js:45369-45390
fn string_to_regex(pattern: &str) -> Option<CompiledExcludeRegex> {
    let (body, flags) = split_regex_pattern(pattern);
    // The upstream delimiter form discards every flag except `i` and `u`
    // before constructing the RegExp; duplicate retained flags still throw.
    let flags: String = flags
        .chars()
        .filter(|flag| matches!(flag, 'i' | 'u'))
        .collect();
    if flags.matches('i').count() > 1 || flags.matches('u').count() > 1 {
        return None;
    }
    match SimpleRegex::parse(body, flags.contains('i')) {
        Ok(regex) => Some(CompiledExcludeRegex::Supported(regex)),
        Err(RegexParseError::Unsupported) if regex_syntax_is_valid(body, &flags) => {
            Some(CompiledExcludeRegex::FailClosed)
        }
        Err(RegexParseError::Unsupported) => None,
        Err(RegexParseError::Invalid) => None,
    }
}

fn regex_syntax_is_valid(body: &str, flags: &str) -> bool {
    let mut literal = String::with_capacity(body.len() + flags.len() + 2);
    literal.push('/');
    let mut escaped = false;
    for character in body.chars() {
        match character {
            '/' if !escaped => literal.push_str("\\/"),
            '\n' => literal.push_str("\\n"),
            '\r' => literal.push_str("\\r"),
            '\u{2028}' => literal.push_str("\\u2028"),
            '\u{2029}' => literal.push_str("\\u2029"),
            _ => literal.push(character),
        }
        escaped = character == '\\' && !escaped;
        if character != '\\' {
            escaped = false;
        }
    }
    literal.push('/');
    literal.push_str(flags);
    tsc_syntax::regex::validate_regular_expression_literal(&literal, ScriptTarget::ES_NEXT)
        .is_empty()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RegexParseError {
    Invalid,
    Unsupported,
}

fn split_regex_pattern(pattern: &str) -> (&str, &str) {
    if !pattern.starts_with('/') {
        return (pattern, "");
    }
    let Some(last_slash) = pattern.rfind('/') else {
        return (pattern, "");
    };
    if last_slash == 0 {
        return (pattern, "");
    }
    let bytes = pattern.as_bytes();
    for index in 1..last_slash {
        if bytes[index] == b'/' && bytes[index.saturating_sub(1)] != b'\\' {
            return (pattern, "");
        }
    }
    (&pattern[1..last_slash], &pattern[last_slash + 1..])
}

impl SimpleRegex {
    fn parse(pattern: &str, ignore_case: bool) -> Result<Self, RegexParseError> {
        let mut anchored_start = false;
        let mut anchored_end = false;
        let mut body = pattern;
        if body.starts_with('^') {
            anchored_start = true;
            body = &body[1..];
        }
        if body.ends_with('$') && !body.ends_with("\\$") {
            anchored_end = true;
            body = &body[..body.len() - 1];
        }
        if body.contains(['(', ')', '{', '}']) {
            return Err(RegexParseError::Unsupported);
        }
        let mut alternatives = Vec::new();
        for alternative in split_unescaped(body, '|') {
            alternatives.push(parse_regex_pieces(alternative)?);
        }
        Ok(Self {
            alternatives,
            anchored_start,
            anchored_end,
            ignore_case,
        })
    }

    fn is_match(&self, text: &str) -> bool {
        let text: Vec<char> = text.chars().collect();
        let starts: Box<dyn Iterator<Item = usize>> = if self.anchored_start {
            Box::new(std::iter::once(0))
        } else {
            Box::new(0..=text.len())
        };
        for start in starts {
            for alternative in &self.alternatives {
                let mut memo = BTreeMap::new();
                if regex_match_pieces(
                    alternative,
                    &text,
                    0,
                    start,
                    self.ignore_case,
                    self.anchored_end,
                    &mut memo,
                ) {
                    return true;
                }
            }
        }
        false
    }
}

fn split_unescaped(text: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut escaped = false;
    for (index, ch) in text.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == separator {
            parts.push(&text[start..index]);
            start = index + ch.len_utf8();
        }
    }
    parts.push(&text[start..]);
    parts
}

fn parse_regex_pieces(text: &str) -> Result<Vec<RegexPiece>, RegexParseError> {
    let chars: Vec<char> = text.chars().collect();
    let mut pieces: Vec<RegexPiece> = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let atom = match chars[index] {
            '\\' => {
                index += 1;
                let Some(&escaped) = chars.get(index) else {
                    return Err(RegexParseError::Invalid);
                };
                match escaped {
                    'd' => RegexAtom::Digit,
                    'w' => RegexAtom::Word,
                    's' => RegexAtom::Whitespace,
                    'D' | 'W' | 'S' | 'b' | 'B' => return Err(RegexParseError::Unsupported),
                    other => RegexAtom::Literal(other),
                }
            }
            '.' => RegexAtom::Any,
            '[' => {
                let (atom, end) = parse_regex_class(&chars, index)?;
                index = end;
                atom
            }
            '*' | '+' | '?' => return Err(RegexParseError::Invalid),
            other => RegexAtom::Literal(other),
        };
        index += 1;
        let repetition = match chars.get(index) {
            Some('*') => RegexRepetition::ZeroOrMore,
            Some('+') => RegexRepetition::OneOrMore,
            Some('?') => RegexRepetition::ZeroOrOne,
            _ => RegexRepetition::One,
        };
        if repetition != RegexRepetition::One {
            index += 1;
        }
        pieces.push(RegexPiece { atom, repetition });
    }
    Ok(pieces)
}

fn parse_regex_class(chars: &[char], start: usize) -> Result<(RegexAtom, usize), RegexParseError> {
    let mut index = start + 1;
    let negated = chars.get(index) == Some(&'^');
    index += usize::from(negated);
    let mut ranges = Vec::new();
    while let Some(&ch) = chars.get(index) {
        if ch == ']' {
            if ranges.is_empty() {
                return Err(RegexParseError::Invalid);
            }
            return Ok((RegexAtom::Class { negated, ranges }, index));
        }
        let first = if ch == '\\' {
            index += 1;
            *chars.get(index).ok_or(RegexParseError::Invalid)?
        } else {
            ch
        };
        if chars.get(index + 1) == Some(&'-') && chars.get(index + 2) != Some(&']') {
            let last = *chars.get(index + 2).ok_or(RegexParseError::Invalid)?;
            ranges.push((first, last));
            index += 3;
        } else {
            ranges.push((first, first));
            index += 1;
        }
    }
    Err(RegexParseError::Invalid)
}

#[allow(clippy::too_many_arguments)]
fn regex_match_pieces(
    pieces: &[RegexPiece],
    text: &[char],
    piece_index: usize,
    text_index: usize,
    ignore_case: bool,
    anchored_end: bool,
    memo: &mut BTreeMap<(usize, usize), bool>,
) -> bool {
    if let Some(result) = memo.get(&(piece_index, text_index)) {
        return *result;
    }
    let result = if piece_index == pieces.len() {
        !anchored_end || text_index == text.len()
    } else {
        let piece = &pieces[piece_index];
        let atom_matches = |index: usize| {
            text.get(index)
                .is_some_and(|&ch| regex_atom_matches(&piece.atom, ch, ignore_case))
        };
        match piece.repetition {
            RegexRepetition::One => {
                atom_matches(text_index)
                    && regex_match_pieces(
                        pieces,
                        text,
                        piece_index + 1,
                        text_index + 1,
                        ignore_case,
                        anchored_end,
                        memo,
                    )
            }
            RegexRepetition::ZeroOrOne => {
                regex_match_pieces(
                    pieces,
                    text,
                    piece_index + 1,
                    text_index,
                    ignore_case,
                    anchored_end,
                    memo,
                ) || atom_matches(text_index)
                    && regex_match_pieces(
                        pieces,
                        text,
                        piece_index + 1,
                        text_index + 1,
                        ignore_case,
                        anchored_end,
                        memo,
                    )
            }
            RegexRepetition::ZeroOrMore | RegexRepetition::OneOrMore => {
                let minimum = usize::from(piece.repetition == RegexRepetition::OneOrMore);
                let mut end = text_index;
                while atom_matches(end) {
                    end += 1;
                }
                (text_index + minimum..=end).rev().any(|next| {
                    regex_match_pieces(
                        pieces,
                        text,
                        piece_index + 1,
                        next,
                        ignore_case,
                        anchored_end,
                        memo,
                    )
                })
            }
        }
    };
    memo.insert((piece_index, text_index), result);
    result
}

fn regex_atom_matches(atom: &RegexAtom, ch: char, ignore_case: bool) -> bool {
    let normalize = |value: char| {
        if ignore_case {
            value.to_ascii_lowercase()
        } else {
            value
        }
    };
    match atom {
        RegexAtom::Literal(expected) => normalize(*expected) == normalize(ch),
        RegexAtom::Any => ch != '\n' && ch != '\r',
        RegexAtom::Digit => ch.is_ascii_digit(),
        RegexAtom::Word => ch.is_ascii_alphanumeric() || ch == '_',
        RegexAtom::Whitespace => ch.is_whitespace(),
        RegexAtom::Class { negated, ranges } => {
            let ch = normalize(ch);
            let contains = ranges
                .iter()
                .any(|&(first, last)| normalize(first) <= ch && ch <= normalize(last));
            contains != *negated
        }
    }
}

const EXTENSIONS_TO_REMOVE: [&str; 12] = [
    ".d.ts", ".d.mts", ".d.cts", ".mjs", ".mts", ".cjs", ".cts", ".ts", ".js", ".tsx", ".jsx",
    ".json",
];

fn extension_from_path(path: &str) -> Option<&'static str> {
    EXTENSIONS_TO_REMOVE
        .iter()
        .copied()
        .find(|extension| file_extension_is(path, extension))
}

fn remove_file_extension(path: &str) -> String {
    extension_from_path(path)
        .and_then(|extension| path.strip_suffix(extension))
        .unwrap_or(path)
        .to_owned()
}

fn change_full_extension(path: &str, extension: &str) -> String {
    format!("{}{}", remove_file_extension(path), extension)
}

fn file_extension_is(path: &str, extension: &str) -> bool {
    path.len() > extension.len() && path.ends_with(extension)
}

fn file_extension_is_one_of(path: &str, extensions: &[&str]) -> bool {
    extensions
        .iter()
        .any(|extension| file_extension_is(path, extension))
}

fn has_js_file_extension(path: &str) -> bool {
    file_extension_is_one_of(path, &[".js", ".jsx", ".mjs", ".cjs"])
}

fn has_ts_file_extension(path: &str) -> bool {
    extension_from_path(path).is_some_and(|extension| {
        matches!(
            extension,
            ".ts" | ".tsx" | ".d.ts" | ".mts" | ".d.mts" | ".cts" | ".d.cts"
        )
    })
}

fn has_implementation_ts_file_extension(path: &str) -> bool {
    has_ts_file_extension(path) && !is_declaration_file_name(path)
}

fn is_declaration_file_name(path: &str) -> bool {
    path.ends_with(".d.ts")
        || path.ends_with(".d.mts")
        || path.ends_with(".d.cts")
        || path
            .rsplit(['/', '\\'])
            .next()
            .is_some_and(|base| base.ends_with(".ts") && base.contains(".d."))
}

fn extension_does_not_support_extensionless_resolution(path: &str) -> bool {
    file_extension_is_one_of(path, &[".mts", ".d.mts", ".mjs", ".cts", ".d.cts", ".cjs"])
}

fn normalized_absolute_path(path: &str, current_directory: &str) -> String {
    CheckerState::normalize_program_path(path, current_directory)
}

fn canonical_host_path(path: &str, host: &dyn EmitModuleSpecifierHost) -> String {
    canonical_file_name(
        &normalized_absolute_path(path, &host.get_current_directory()),
        host.use_case_sensitive_file_names(),
    )
}

fn canonical_file_name(path: &str, case_sensitive: bool) -> String {
    if case_sensitive {
        path.to_owned()
    } else {
        // Exact `tsc_host::to_file_name_lower_case` posture. The checker
        // crate intentionally has no direct host-crate dependency, so keep
        // this tiny canonicalization projection local while all lexical path
        // normalization continues through the program utility above.
        let mut folded = String::with_capacity(path.len());
        let mut run = String::new();
        for character in path.chars() {
            let protected = character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(
                    character,
                    '\u{0130}' | '\u{0131}' | '\u{00df}' | '/' | '\\' | ':' | '-' | '_' | '.' | ' '
                );
            if protected {
                if !run.is_empty() {
                    folded.push_str(&run.to_lowercase());
                    run.clear();
                }
                folded.push(character);
            } else {
                run.push(character);
            }
        }
        if !run.is_empty() {
            folded.push_str(&run.to_lowercase());
        }
        folded
    }
}

fn normalize_path_text(path: &str) -> String {
    let path = path.replace('\\', "/");
    let (root, remainder) = split_path_root(&path);
    let mut components: Vec<&str> = Vec::new();
    for component in remainder.split('/') {
        match component {
            "" | "." => {}
            ".." if components.last().is_some_and(|last| *last != "..") => {
                components.pop();
            }
            ".." if root.is_empty() => components.push(component),
            ".." => {}
            _ => components.push(component),
        }
    }
    let joined = components.join("/");
    if root.is_empty() {
        joined
    } else if joined.is_empty() {
        root.to_owned()
    } else if root.ends_with('/') {
        format!("{root}{joined}")
    } else {
        format!("{root}/{joined}")
    }
}

fn split_path_root(path: &str) -> (&str, &str) {
    if let Some(scheme) = path.find("://") {
        let after_scheme = scheme + 3;
        let host_end = path[after_scheme..]
            .find('/')
            .map(|index| after_scheme + index + 1)
            .unwrap_or(path.len());
        return (&path[..host_end], &path[host_end..]);
    }
    if path.starts_with("//") {
        let mut separators = path.match_indices('/');
        let _ = separators.next();
        let _ = separators.next();
        if let Some((server_end, _)) = separators.next() {
            if let Some((share_end, _)) = path[server_end + 1..].match_indices('/').next() {
                let end = server_end + 1 + share_end + 1;
                return (&path[..end], &path[end..]);
            }
        }
        return (path, "");
    }
    if let Some(remainder) = path.strip_prefix('/') {
        return ("/", remainder);
    }
    if path.as_bytes().get(1) == Some(&b':') {
        if path.as_bytes().get(2) == Some(&b'/') {
            return (&path[..3], &path[3..]);
        }
        return (&path[..2], &path[2..]);
    }
    ("", path)
}

fn path_is_absolute(path: &str) -> bool {
    !split_path_root(&path.replace('\\', "/")).0.is_empty()
}

fn path_is_relative(path: &str) -> bool {
    matches!(path, "." | "..")
        || path.starts_with("./")
        || path.starts_with("../")
        || path.starts_with(".\\")
        || path.starts_with("..\\")
}

fn is_external_module_name_relative(path: &str) -> bool {
    path_is_relative(path) || path_is_absolute(path)
}

fn path_is_bare_specifier(path: &str) -> bool {
    !path_is_absolute(path) && !path_is_relative(path)
}

fn path_contains_node_modules(path: &str) -> bool {
    path.replace('\\', "/").contains("/node_modules/")
}

fn combine_paths(parent: &str, child: &str) -> String {
    if child.is_empty() {
        return normalize_path_text(parent);
    }
    if path_is_absolute(child) {
        return normalize_path_text(child);
    }
    if parent.is_empty() {
        return normalize_path_text(child);
    }
    normalize_path_text(&format!(
        "{}/{}",
        parent.trim_end_matches(['/', '\\']),
        child.trim_start_matches(['/', '\\'])
    ))
}

fn directory_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let trimmed = if normalized == "/" {
        normalized.as_str()
    } else {
        normalized.trim_end_matches('/')
    };
    let Some(index) = trimmed.rfind('/') else {
        return String::new();
    };
    if index == 0 {
        "/".to_owned()
    } else if index == 2 && trimmed.as_bytes().get(1) == Some(&b':') {
        trimmed[..=index].to_owned()
    } else {
        trimmed[..index].to_owned()
    }
}

fn base_file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn ensure_trailing_directory_separator(path: &str) -> String {
    if path.ends_with('/') {
        path.to_owned()
    } else {
        format!("{path}/")
    }
}

fn ensure_path_is_non_module_name(path: &str) -> String {
    if path_is_relative(path) || path_is_absolute(path) {
        path.to_owned()
    } else {
        format!("./{path}")
    }
}

fn roots_equal(left: &str, right: &str) -> bool {
    let left_root = split_path_root(left).0;
    let right_root = split_path_root(right).0;
    left_root.eq_ignore_ascii_case(right_root)
}

fn get_relative_path_from_directory(directory: &str, target: &str, case_sensitive: bool) -> String {
    let directory = normalize_path_text(directory);
    let target = normalize_path_text(target);
    if !roots_equal(&directory, &target) {
        return target;
    }
    let (_, directory_remainder) = split_path_root(&directory);
    let (_, target_remainder) = split_path_root(&target);
    let from: Vec<&str> = directory_remainder
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let to: Vec<&str> = target_remainder
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let shared = from
        .iter()
        .zip(&to)
        .take_while(|(left, right)| {
            if case_sensitive {
                left == right
            } else {
                left.eq_ignore_ascii_case(right)
            }
        })
        .count();
    let mut parts = vec![".."; from.len().saturating_sub(shared)];
    parts.extend(to[shared..].iter().copied());
    parts.join("/")
}

fn path_starts_with_directory(path: &str, directory: &str, case_sensitive: bool) -> bool {
    if paths_equal(path, directory, !case_sensitive) {
        return true;
    }
    let prefix = ensure_trailing_directory_separator(directory);
    starts_with(path, &prefix, !case_sensitive)
}

fn paths_equal(left: &str, right: &str, ignore_case: bool) -> bool {
    if ignore_case {
        left.eq_ignore_ascii_case(right)
    } else {
        left == right
    }
}

fn starts_with(value: &str, prefix: &str, ignore_case: bool) -> bool {
    if ignore_case {
        value
            .get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
    } else {
        value.starts_with(prefix)
    }
}

fn ends_with(value: &str, suffix: &str, ignore_case: bool) -> bool {
    if ignore_case {
        value
            .get(value.len().saturating_sub(suffix.len())..)
            .is_some_and(|tail| tail.eq_ignore_ascii_case(suffix))
    } else {
        value.ends_with(suffix)
    }
}

fn ancestor_directories(path: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = normalize_path_text(path);
    loop {
        result.push(current.clone());
        let parent = directory_path(&current);
        if parent == current || parent.is_empty() {
            if parent.is_empty() && current != parent {
                result.push(parent);
            }
            break;
        }
        current = parent;
    }
    result
}

fn count_directory_separators(path: &str) -> usize {
    path.as_bytes()
        .iter()
        .filter(|&&byte| matches!(byte, b'/' | b'\\'))
        .count()
}

fn contains_ignored_path(path: &str) -> bool {
    ["/node_modules/.", "/.git", "/.#"]
        .iter()
        .any(|ignored| path.contains(ignored))
}

fn get_node_module_path_parts(path: &str) -> Option<NodeModulePathParts> {
    let normalized = path.replace('\\', "/");
    let marker = "/node_modules/";
    let top_level_node_modules_index = normalized.find(marker)?;
    let top_level_package_name_index = top_level_node_modules_index + marker.len() - 1;
    let mut package_root_index = package_root_end(&normalized, top_level_package_name_index + 1)?;
    let mut search = package_root_index;
    while let Some(relative) = normalized.get(search..)?.find(marker) {
        let marker_index = search + relative;
        package_root_index = package_root_end(&normalized, marker_index + marker.len())?;
        search = package_root_index;
    }
    let file_name_index = normalized.rfind('/').unwrap_or(0);
    Some(NodeModulePathParts {
        top_level_node_modules_index,
        top_level_package_name_index,
        package_root_index,
        file_name_index,
    })
}

fn package_root_end(path: &str, package_start: usize) -> Option<usize> {
    let mut end = package_start;
    if path.get(package_start..)?.starts_with('@') {
        end += path.get(end..)?.find('/')? + 1;
    }
    Some(
        path.get(end..)?
            .find('/')
            .map(|relative| end + relative)
            .unwrap_or(path.len()),
    )
}

fn get_package_name_from_types_package_name(name: &str) -> String {
    let Some(without_prefix) = name.strip_prefix("@types/") else {
        return name.to_owned();
    };
    if let Some(separator) = without_prefix.find("__") {
        format!(
            "@{}/{}",
            &without_prefix[..separator],
            &without_prefix[separator + 2..]
        )
    } else {
        without_prefix.to_owned()
    }
}

fn get_resolve_package_json_exports(options: &CompilerOptions) -> bool {
    options
        .resolve_package_json_exports
        .unwrap_or_else(|| matches!(options.emit_module_resolution_kind(), 3 | 99 | 100))
}

fn get_resolve_package_json_imports(options: &CompilerOptions) -> bool {
    options
        .resolve_package_json_imports
        .unwrap_or_else(|| matches!(options.emit_module_resolution_kind(), 3 | 99 | 100))
}

fn get_conditions(options: &CompilerOptions, resolution_mode: EmitResolutionMode) -> Vec<String> {
    let module_resolution = options.emit_module_resolution_kind();
    if resolution_mode == EmitResolutionMode::None && module_resolution == 2 {
        return Vec::new();
    }
    let resolution_mode = if resolution_mode == EmitResolutionMode::None && module_resolution == 100
    {
        EmitResolutionMode::EsNext
    } else {
        resolution_mode
    };
    let mut conditions = vec![if resolution_mode == EmitResolutionMode::EsNext {
        "import".to_owned()
    } else {
        "require".to_owned()
    }];
    if options.no_dts_resolution != Some(true) {
        conditions.push("types".to_owned());
    }
    if module_resolution != 100 {
        conditions.push("node".to_owned());
    }
    conditions.extend(options.custom_conditions.iter().flatten().cloned());
    conditions
}

fn is_applicable_versioned_types_key(conditions: &[String], key: &str) -> bool {
    conditions.iter().any(|condition| condition == "types")
        && key
            .strip_prefix("types@")
            .is_some_and(version_range_matches_current)
}

fn selected_types_versions_paths(package_json: &Value) -> Option<Vec<ModulePathMapping>> {
    let versions = package_json.get("typesVersions")?.as_object()?;
    for (range, entry) in versions {
        if !version_range_matches_current(range) {
            continue;
        }
        let paths = entry.as_object()?;
        let mappings = paths
            .iter()
            .filter_map(|(key, value)| {
                let patterns = value
                    .as_array()?
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect();
                Some(ModulePathMapping {
                    key: key.clone(),
                    patterns,
                })
            })
            .collect();
        return Some(mappings);
    }
    None
}

fn path_mapping_key_matches(key: &str, candidate: &str) -> bool {
    let Some(star) = key.find('*') else {
        return key == candidate;
    };
    if key[star + 1..].contains('*') {
        return false;
    }
    let prefix = &key[..star];
    let suffix = &key[star + 1..];
    candidate.len() >= prefix.len() + suffix.len()
        && candidate.starts_with(prefix)
        && candidate.ends_with(suffix)
}

fn json_value_is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().is_some_and(|value| value != 0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

fn version_range_matches_current(range: &str) -> bool {
    const CURRENT: (u32, u32, u32) = (6, 0, 3);
    range.split("||").any(|alternative| {
        let alternative = alternative.trim();
        if alternative.is_empty() || alternative == "*" {
            return true;
        }
        alternative
            .split_whitespace()
            .all(|term| version_term_matches(term, CURRENT))
    })
}

fn version_term_matches(term: &str, current: (u32, u32, u32)) -> bool {
    let (operator, version) = if let Some(version) = term.strip_prefix(">=") {
        (">=", version)
    } else if let Some(version) = term.strip_prefix("<=") {
        ("<=", version)
    } else if let Some(version) = term.strip_prefix('>') {
        (">", version)
    } else if let Some(version) = term.strip_prefix('<') {
        ("<", version)
    } else if let Some(version) = term.strip_prefix('^') {
        ("^", version)
    } else if let Some(version) = term.strip_prefix('~') {
        ("~", version)
    } else {
        ("=", term)
    };
    let Some(parsed) = parse_version(version) else {
        return false;
    };
    match operator {
        ">=" => current >= parsed,
        "<=" => current <= parsed,
        ">" => current > parsed,
        "<" => current < parsed,
        "^" => current >= parsed && current.0 == parsed.0,
        "~" => current >= parsed && current.0 == parsed.0 && current.1 == parsed.1,
        _ => {
            current == parsed
                || version.matches('.').count() == 0 && current.0 == parsed.0
                || version.matches('.').count() == 1
                    && current.0 == parsed.0
                    && current.1 == parsed.1
        }
    }
}

fn parse_version(version: &str) -> Option<(u32, u32, u32)> {
    let mut parts = version.trim_start_matches('=').split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts
        .next()
        .unwrap_or("0")
        .split('-')
        .next()?
        .parse()
        .ok()?;
    Some((major, minor, patch))
}

fn output_js_file_name(
    target_file_path: &str,
    options: &SpecifierCompilerOptions,
    host: &dyn EmitModuleSpecifierHost,
) -> Option<String> {
    let extension = try_get_js_extension_for_file(target_file_path, options)?;
    Some(output_path_with_extension(
        target_file_path,
        extension,
        options.compiler_options.out_dir.as_deref(),
        options.compiler_options.root_dir.as_deref(),
        host,
    ))
}

fn output_declaration_file_name(
    target_file_path: &str,
    options: &SpecifierCompilerOptions,
    host: &dyn EmitModuleSpecifierHost,
) -> Option<String> {
    let extension = match extension_from_path(target_file_path)? {
        ".mts" | ".mjs" => ".d.mts",
        ".cts" | ".cjs" => ".d.cts",
        ".json" => ".d.json.ts",
        _ => ".d.ts",
    };
    Some(output_path_with_extension(
        target_file_path,
        extension,
        options
            .compiler_options
            .declaration_dir
            .as_deref()
            .or(options.compiler_options.out_dir.as_deref()),
        options.compiler_options.root_dir.as_deref(),
        host,
    ))
}

fn output_path_with_extension(
    target_file_path: &str,
    extension: &str,
    output_directory: Option<&str>,
    root_directory: Option<&str>,
    host: &dyn EmitModuleSpecifierHost,
) -> String {
    let target = normalized_absolute_path(target_file_path, &host.get_current_directory());
    let path = if let Some(output_directory) = output_directory {
        let root = root_directory
            .map(|root| normalized_absolute_path(root, &host.get_current_directory()))
            .unwrap_or_else(|| host.get_common_source_directory());
        let relative =
            get_relative_path_from_directory(&root, &target, host.use_case_sensitive_file_names());
        normalized_absolute_path(
            &combine_paths(output_directory, &relative),
            &host.get_current_directory(),
        )
    } else {
        target
    };
    change_full_extension(&path, extension)
}

fn source_file_index_of_module(state: &CheckerState<'_>, module_symbol: SymbolId) -> Option<usize> {
    let symbol = state.binder.symbol(module_symbol);
    let declaration = symbol
        .value_declaration
        .or_else(|| non_augmentation_declaration(state, module_symbol))?;
    Some(state.binder.file_index_of_node(declaration))
}

fn emit_resolver_node_for_file(
    state: &CheckerState<'_>,
    file_index: usize,
    node: NodeId,
) -> EmitResolverNode {
    let source = if state.authoritative_source_tokens.is_empty() {
        u32::try_from(file_index).expect("checker file index exceeds SourceFileId")
    } else {
        state
            .authoritative_source_tokens
            .get(file_index)
            .expect("authoritative metadata covers every checker file")
            .0
    };
    EmitResolverNode::new(SourceFileId::from_raw(source), node)
}

fn module_specifier_index(
    state: &CheckerState<'_>,
    file_index: usize,
    literal: NodeId,
) -> Option<u32> {
    let (imports, augmentations) = module_name_literals(state, file_index);
    imports
        .into_iter()
        .chain(augmentations)
        .position(|candidate| candidate == literal)
        .and_then(|index| u32::try_from(index).ok())
}

pub(super) fn module_name_literals(
    state: &CheckerState<'_>,
    file_index: usize,
) -> (Vec<NodeId>, Vec<NodeId>) {
    let source = state.binder.source(file_index);
    let mut static_imports = Vec::<NodeId>::new();
    let mut dynamic_imports = Vec::<NodeId>::new();
    let mut augmentations = Vec::<NodeId>::new();
    for node in source.arena.node_ids() {
        match &source.arena.node(node).data {
            NodeData::ImportDeclaration(data) => {
                if let Some(literal) = string_literal_like(state, data.module_specifier)
                    .filter(|&literal| static_module_reference_is_collected(state, node, literal))
                {
                    static_imports.push(literal);
                }
            }
            NodeData::ExportDeclaration(data) => {
                if let Some(literal) = string_literal_like(state, data.module_specifier)
                    .filter(|&literal| static_module_reference_is_collected(state, node, literal))
                {
                    static_imports.push(literal);
                }
            }
            NodeData::ImportEqualsDeclaration(data) => {
                if let Some(literal) = data
                    .module_reference
                    .and_then(|reference| {
                        let NodeData::ExternalModuleReference(reference) = state.data_of(reference)
                        else {
                            return None;
                        };
                        string_literal_like(state, reference.expression)
                    })
                    .filter(|&literal| static_module_reference_is_collected(state, node, literal))
                {
                    static_imports.push(literal);
                }
            }
            NodeData::ImportType(data) => {
                if let Some(literal) = data.argument.and_then(|argument| {
                    let NodeData::LiteralType(literal) = state.data_of(argument) else {
                        return None;
                    };
                    string_literal_like(state, literal.literal)
                }) {
                    dynamic_imports.push(literal);
                }
            }
            NodeData::JSDocImportTag(data) => {
                if let Some(literal) = data.module_specifier.filter(|&literal| {
                    state.kind_of(literal) == SyntaxKind::StringLiteral
                        && literal_text(state, literal).is_some_and(|text| !text.is_empty())
                }) {
                    dynamic_imports.push(literal);
                }
            }
            NodeData::CallExpression(data) => {
                let is_javascript_file = NodeFlags::from_bits(source.arena.node(source.root).flags)
                    .intersects(NodeFlags::JAVA_SCRIPT_FILE);
                let import_like = data.expression.is_some_and(|expression| {
                    state.kind_of(expression) == SyntaxKind::ImportKeyword
                        || is_javascript_file && state.is_require_call(node, true)
                });
                if import_like {
                    if let Some(literal) = state
                        .nodes_of(data.arguments)
                        .first()
                        .copied()
                        .and_then(|literal| string_literal_like(state, Some(literal)))
                    {
                        dynamic_imports.push(literal);
                    }
                }
            }
            NodeData::ModuleDeclaration(data) => {
                if module_declaration_is_collected_augmentation(state, node) {
                    if let Some(literal) = string_literal_like(state, data.name) {
                        augmentations.push(literal);
                    }
                }
            }
            _ => {}
        }
    }
    let by_position = |left: &NodeId, right: &NodeId| {
        source
            .arena
            .node(*left)
            .pos
            .cmp(&source.arena.node(*right).pos)
    };
    static_imports.sort_by(by_position);
    dynamic_imports.sort_by(by_position);
    augmentations.sort_by(by_position);
    static_imports.extend(dynamic_imports);
    (static_imports, augmentations)
}

fn static_module_reference_is_collected(
    state: &CheckerState<'_>,
    node: NodeId,
    literal: NodeId,
) -> bool {
    let Some(text) = literal_text(state, literal).filter(|text| !text.is_empty()) else {
        return false;
    };
    let source = state.binder.source_of_node(node);
    let source_is_external = state
        .binder
        .is_external_or_common_js_module_of_node(source.root);
    let Some(parent) = state.parent_of(node) else {
        return false;
    };
    if parent == source.root {
        return true;
    }
    if source_is_external || state.kind_of(parent) != SyntaxKind::ModuleBlock {
        return false;
    }
    let Some(ambient_module) = state.parent_of(parent) else {
        return false;
    };
    state.parent_of(ambient_module) == Some(source.root)
        && node_util::is_ambient_module(source, ambient_module)
        && !is_external_module_name_relative(text)
}

fn module_declaration_is_collected_augmentation(
    state: &CheckerState<'_>,
    declaration: NodeId,
) -> bool {
    let source = state.binder.source_of_node(declaration);
    if !node_util::is_ambient_module(source, declaration) {
        return false;
    }
    let Some(name) = module_declaration_name_text(state, declaration) else {
        return false;
    };
    let Some(parent) = state.parent_of(declaration) else {
        return false;
    };
    if state
        .binder
        .is_external_or_common_js_module_of_node(source.root)
    {
        return parent == source.root;
    }
    if state.kind_of(parent) == SyntaxKind::ModuleBlock {
        let Some(outer_ambient_module) = state.parent_of(parent) else {
            return false;
        };
        return state.parent_of(outer_ambient_module) == Some(source.root)
            && node_util::is_ambient_module(source, outer_ambient_module)
            && !is_external_module_name_relative(&name);
    }
    false
}

/// tsc-port: getModuleNameStringLiteralAt @6.0.3
/// tsc-hash: 22f156a0759135f31aa7768a466e27b2fe7324183a1202e474754d5548392653
/// tsc-span: _tsc.js:125731-125741
pub(crate) fn get_module_name_string_literal_at(
    state: &CheckerState<'_>,
    file_index: usize,
    index: u32,
) -> Option<String> {
    let (imports, augmentations) = module_name_literals(state, file_index);
    let literal = imports
        .into_iter()
        .chain(augmentations)
        .nth(index as usize)?;
    literal_text(state, literal).map(str::to_owned)
}

fn literal_text<'a>(state: &'a CheckerState<'_>, node: NodeId) -> Option<&'a str> {
    match state.data_of(node) {
        NodeData::StringLiteral(data) => Some(&data.text),
        NodeData::NoSubstitutionTemplateLiteral(data) => Some(&data.text),
        _ => None,
    }
}

fn match_path_pattern<'a>(
    candidate: &'a str,
    leading: &str,
    trailing: &str,
    ignore_case: bool,
) -> Option<&'a str> {
    if starts_with(candidate, leading, ignore_case) && ends_with(candidate, trailing, ignore_case) {
        let end = candidate.len().saturating_sub(trailing.len());
        if leading.len() <= end {
            Some(&candidate[leading.len()..end])
        } else {
            Some("")
        }
    } else {
        None
    }
}

fn module_declaration_name_text(state: &CheckerState<'_>, declaration: NodeId) -> Option<String> {
    let NodeData::ModuleDeclaration(data) = state.data_of(declaration) else {
        return None;
    };
    data.name
        .and_then(|name| literal_text(state, name))
        .map(str::to_owned)
}

#[cfg(test)]
#[path = "../../tests/unit/node_builder_specifier/tests.rs"]
mod tests;

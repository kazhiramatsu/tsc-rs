use std::collections::{HashMap, HashSet};

use tsc_binder::{node_util, SymbolId};
use tsc_emitter::{
    EmitFunctionProperty, EmitImportIncludeReason, EmitInternalNodeBuilderFlags,
    EmitModuleSpecifierHost, EmitNodeBuilderFlags, EmitResolutionMode, EmitResolverError,
    EmitResolverMethod, EmitResolverNode, EmitSymbolAccessibility, EmitSymbolAccessibilityResult,
    EmitSymbolMeaning, EmitTrackerAccess, EmitTrackerNode, EmitTrackerNodeDescription,
    EmitTrackerSymbol, EmitTrackerSymbolDescription, SourceFileId, SourceRange, TransformArena,
    TransformNode, TransformSourceId,
};
use tsc_syntax::nodes::{
    ComputedPropertyNameData, ElementAccessExpressionData, ImportAttributeData,
    ImportAttributesData, ImportTypeData, IndexedAccessTypeData, LiteralTypeData,
    NumericLiteralData, ParenthesizedTypeData, PrefixUnaryExpressionData,
    PropertyAccessExpressionData, QualifiedNameData, StringLiteralData, TypeQueryData,
    TypeReferenceData,
};
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{
    CheckFlags, LiteralValue, NodeFlags, ObjectFlags, SymbolFlags, TypeData, TypeFlags, TypeId,
};

use crate::check::can_use_property_access_slice;
use crate::modules::ModuleResolutionMode;
use crate::state::{CheckAbort, CheckerState, PackageJsonModuleType};

use super::signatures::type_parameter_to_declaration;
use super::specifier::{get_specifier_for_module_symbol, module_name_literals};
use super::type_nodes::{
    add_approximate_length, checker_abort_error, create_identifier, create_node, create_node_array,
    factory_error, set_no_ascii_escaping, type_to_type_node_helper, BuildResult,
};
use super::NodeBuilderContext;

const USE_FULLY_QUALIFIED_TYPE: u32 = 64;
const USE_ONLY_EXTERNAL_ALIASING: u32 = 128;
const WRITE_TYPE_PARAMETERS_IN_QUALIFIED_NAME: u32 = 512;
const USE_ALIAS_DEFINED_OUTSIDE_CURRENT_SCOPE: u32 = 16_384;
const ALLOW_QUALIFIED_NAME_IN_PLACE_OF_IDENTIFIER: u32 = 65_536;
const IN_INITIAL_ENTITY_NAME: u32 = 16_777_216;
const ALLOW_NODE_MODULES_RELATIVE_PATHS: u32 = 67_108_864;
const FORBID_INDEXED_ACCESS_SYMBOL_REFERENCES: u32 = 16;
const DO_NOT_INCLUDE_SYMBOL_CHAIN: u32 = 4;
const ALLOW_UNIQUE_ES_SYMBOL_TYPE: u32 = 1_048_576;
const IGNORE_ERRORS: EmitNodeBuilderFlags = EmitNodeBuilderFlags(70_221_824);

fn has_flag(context: &NodeBuilderContext<'_>, flag: u32) -> bool {
    context.flags.0 & flag != 0
}

fn js_len(text: &str) -> usize {
    text.encode_utf16().count()
}

fn value_meaning(meaning: EmitSymbolMeaning) -> bool {
    meaning.0 == SymbolFlags::VALUE.bits() as u32
        || meaning == EmitSymbolMeaning::VALUE_EXPORT_VALUE
}

fn symbol_flags_for_meaning(meaning: EmitSymbolMeaning) -> SymbolFlags {
    if value_meaning(meaning) {
        SymbolFlags::VALUE
    } else {
        SymbolFlags::from_bits(meaning.0 as i32)
    }
}

fn program_source_id(checker: &CheckerState<'_>, file_index: usize) -> SourceFileId {
    let raw = checker
        .authoritative_source_tokens
        .get(file_index)
        .map_or_else(
            || u32::try_from(file_index).unwrap_or_default(),
            |token| token.0,
        );
    SourceFileId::from_raw(raw)
}

fn enclosing_resolver_node(checker: &CheckerState<'_>, node: NodeId) -> EmitResolverNode {
    EmitResolverNode::new(
        program_source_id(checker, checker.binder.file_index_of_node(node)),
        node,
    )
}

fn project_parse_node(
    checker: &CheckerState<'_>,
    arena: &TransformArena,
    node: NodeId,
) -> BuildResult<Option<TransformNode>> {
    let source = program_source_id(checker, checker.binder.file_index_of_node(node));
    arena
        .parse_tree_transform_node(EmitResolverNode::new(source, node))
        .map_err(factory_error)
}

fn clone_parse_node(
    checker: &CheckerState<'_>,
    arena: &mut TransformArena,
    node: NodeId,
) -> BuildResult<Option<TransformNode>> {
    let Some(original) = project_parse_node(checker, arena, node)? else {
        return Ok(None);
    };
    let clone = arena
        .factory()
        .clone_node(original)
        .map_err(factory_error)?;
    arena
        .factory()
        .set_text_range(clone, original)
        .map_err(factory_error)
        .map(Some)
}

fn tracker_error(
    checker: &CheckerState<'_>,
    node: Option<NodeId>,
    abort: CheckAbort,
) -> EmitResolverError {
    let node = node.unwrap_or_else(|| checker.binder.source(0).root);
    EmitResolverError::CheckerAborted {
        method: EmitResolverMethod::CreateTypeOfDeclaration,
        node: enclosing_resolver_node(checker, node),
        reason: abort.description(),
    }
}

struct CheckerTrackerAccess<'state, 'program> {
    checker: &'state mut CheckerState<'program>,
    arena: Option<&'state mut TransformArena>,
    target: Option<TransformSourceId>,
    statement_tracking: bool,
}

impl CheckerTrackerAccess<'_, '_> {
    fn symbol(&self, symbol: EmitTrackerSymbol) -> Option<SymbolId> {
        u32::try_from(symbol.0)
            .ok()
            .map(SymbolId)
            .filter(|&symbol| self.checker.binder.try_symbol(symbol).is_some())
    }

    fn node(&self, node: EmitTrackerNode) -> Option<NodeId> {
        super::tracker::tracker_node_id(node)
            .filter(|&node| self.checker.binder.try_file_index_of_node(node).is_some())
    }

    fn unavailable(&self, node: Option<NodeId>) -> EmitResolverError {
        let node = node.unwrap_or_else(|| self.checker.binder.source(0).root);
        EmitResolverError::CheckerAborted {
            method: EmitResolverMethod::CreateTypeOfDeclaration,
            node: enclosing_resolver_node(self.checker, node),
            reason: "tracker callback carried an invalid h2-7a-m-3 token",
        }
    }

    fn build_accessibility_error_name(
        &mut self,
        symbol: SymbolId,
        enclosing: NodeId,
        enclosing_is_synthetic: bool,
        meaning: EmitSymbolMeaning,
    ) -> Result<(), EmitResolverError> {
        let unavailable = self.unavailable(Some(enclosing));
        let Some(arena) = self.arena.as_deref_mut() else {
            return Ok(());
        };
        let target = project_parse_node(self.checker, arena, enclosing)?
            .map(TransformNode::source)
            .or(self.target)
            .ok_or(unavailable)?;
        super::context::with_context(
            self.checker,
            arena,
            target,
            (!enclosing_is_synthetic).then_some(enclosing),
            Some(IGNORE_ERRORS),
            None,
            None,
            None,
            None,
            |checker, arena, target, context| {
                symbol_to_node(checker, arena, target, context, symbol, meaning)
            },
            None,
        )?;
        Ok(())
    }

    fn accessibility_error_module_symbol(
        &mut self,
        symbol: SymbolId,
        error_module_name: &str,
    ) -> BuildResult<Option<SymbolId>> {
        let mut parent = self.checker.binder.symbol(symbol).parent;
        while let Some(candidate) = parent {
            if self.checker.symbol_display_name(candidate) == error_module_name {
                return Ok(Some(candidate));
            }
            parent = self.checker.binder.symbol(candidate).parent;
        }
        for declaration in self.checker.binder.symbol(symbol).declarations.clone() {
            if let Some(candidate) = self
                .checker
                .get_external_module_container(declaration)
                .map_err(|abort| tracker_error(self.checker, Some(declaration), abort))?
            {
                if self.checker.symbol_display_name(candidate) == error_module_name {
                    return Ok(Some(candidate));
                }
            }
        }
        Ok(None)
    }
}

impl EmitTrackerAccess for CheckerTrackerAccess<'_, '_> {
    fn is_symbol_accessible(
        &mut self,
        symbol: EmitTrackerSymbol,
        enclosing_declaration: Option<EmitTrackerNode>,
        meaning: EmitSymbolMeaning,
        should_compute_aliases: bool,
    ) -> Result<EmitSymbolAccessibilityResult, EmitResolverError> {
        let enclosing_is_synthetic =
            enclosing_declaration.is_some_and(super::tracker::tracker_node_is_synthetic);
        let enclosing = enclosing_declaration.and_then(|node| self.node(node));
        let symbol = self
            .symbol(symbol)
            .ok_or_else(|| self.unavailable(enclosing))?;
        let enclosing = enclosing.ok_or_else(|| self.unavailable(None))?;
        // declarations.ts records the transform trackSymbol callback and
        // then returns immediately for a type parameter.  The scoped
        // symbol-table tracker is different: it still asks accessibility so
        // `isDeclarationVisible` can paint referenced generic declarations.
        if !self.statement_tracking
            && self
                .checker
                .symbol_flags(symbol)
                .intersects(SymbolFlags::TYPE_PARAMETER)
        {
            return Ok(self.checker.emit_accessible_symbol_observation(
                symbol,
                enclosing,
                enclosing_is_synthetic,
                meaning,
                should_compute_aliases,
            ));
        }
        let result = self
            .checker
            .emit_is_symbol_accessible_with_enclosing_kind(
                symbol,
                enclosing,
                enclosing_is_synthetic,
                meaning,
                should_compute_aliases,
            )
            .map_err(|abort| tracker_error(self.checker, Some(enclosing), abort))?;
        // The statement wrapper performs the name-building call before it
        // forwards an inaccessible symbol to the declaration-transform
        // tracker. Avoid formatting the same error a second time when that
        // tracker rechecks with alias painting enabled.
        if self.statement_tracking && !should_compute_aliases {
            if result.error_symbol_name.is_some() {
                self.build_accessibility_error_name(
                    symbol,
                    enclosing,
                    enclosing_is_synthetic,
                    meaning,
                )?;
            }
            if let Some(error_module_name) = result.error_module_name.as_deref() {
                if let Some(module_symbol) =
                    self.accessibility_error_module_symbol(symbol, error_module_name)?
                {
                    let module_meaning =
                        if result.accessibility == EmitSymbolAccessibility::NotAccessible {
                            EmitSymbolMeaning::NAMESPACE
                        } else {
                            EmitSymbolMeaning(0)
                        };
                    self.build_accessibility_error_name(
                        module_symbol,
                        enclosing,
                        enclosing_is_synthetic,
                        module_meaning,
                    )?;
                }
            }
        }
        Ok(result)
    }

    fn is_expando_function_declaration(
        &mut self,
        node: EmitTrackerNode,
    ) -> Result<bool, EmitResolverError> {
        let node = self.node(node).ok_or_else(|| self.unavailable(None))?;
        self.checker
            .emit_is_expando_function_declaration(node)
            .map_err(|abort| tracker_error(self.checker, Some(node), abort))
    }

    fn get_properties_of_container_function(
        &mut self,
        node: EmitTrackerNode,
    ) -> Result<Vec<EmitFunctionProperty>, EmitResolverError> {
        let node = self.node(node).ok_or_else(|| self.unavailable(None))?;
        self.checker
            .emit_get_properties_of_container_function(node, 0)
            .map_err(|abort| tracker_error(self.checker, Some(node), abort))
    }

    fn requires_adding_implicit_undefined(
        &mut self,
        parameter: EmitTrackerNode,
        enclosing_declaration: Option<EmitTrackerNode>,
    ) -> Result<bool, EmitResolverError> {
        let parameter = self.node(parameter).ok_or_else(|| self.unavailable(None))?;
        let enclosing = enclosing_declaration.and_then(|node| self.node(node));
        self.checker
            .emit_requires_adding_implicit_undefined(parameter, enclosing)
            .map_err(|abort| tracker_error(self.checker, Some(parameter), abort))
    }

    fn describe_symbol(&mut self, symbol: EmitTrackerSymbol) -> EmitTrackerSymbolDescription {
        let Some(symbol) = self.symbol(symbol) else {
            return EmitTrackerSymbolDescription::default();
        };
        let data = self.checker.binder.symbol(symbol);
        let declarations = data
            .declarations
            .iter()
            .take(8)
            .map(|&node| EmitTrackerNodeDescription {
                parse: Some(enclosing_resolver_node(self.checker, node)),
                original: None,
            })
            .collect();
        EmitTrackerSymbolDescription {
            escaped_name: data.escaped_name.clone(),
            declaration_count: u32::try_from(data.declarations.len()).unwrap_or(u32::MAX),
            declarations,
        }
    }

    fn describe_node(&mut self, node: EmitTrackerNode) -> EmitTrackerNodeDescription {
        if super::tracker::tracker_node_is_synthetic(node) {
            return EmitTrackerNodeDescription::default();
        }
        self.node(node)
            .map(|node| EmitTrackerNodeDescription {
                parse: Some(enclosing_resolver_node(self.checker, node)),
                original: None,
            })
            .unwrap_or_default()
    }
}

#[derive(Clone)]
struct BasicModuleSpecifierHost {
    current_directory: String,
    files: HashMap<String, Option<String>>,
    modes: HashMap<u32, EmitResolutionMode>,
}

impl BasicModuleSpecifierHost {
    fn new(checker: &CheckerState<'_>) -> Self {
        let current_directory = checker.host_current_directory.clone();
        let mut files = HashMap::new();
        let mut modes = HashMap::with_capacity(checker.binder.file_count());
        for index in 0..checker.binder.file_count() {
            let source = checker.binder.source(index);
            let normalized =
                CheckerState::normalize_program_path(&source.file_name, &current_directory);
            files.insert(normalized, Some(source.text().to_owned()));
            modes.insert(
                program_source_id(checker, index).raw(),
                default_resolution_mode_for_checker_file(checker, source.root),
            );
        }
        let mut host_file_paths = checker.host_file_paths.iter().collect::<Vec<_>>();
        host_file_paths.sort_unstable();
        for path in host_file_paths {
            let normalized = CheckerState::normalize_program_path(path, &current_directory);
            files.entry(normalized).or_insert(None);
        }
        // Prepared package manifests are checker host-only inputs. Their
        // parsed values deliberately override an equal-path Program JSON
        // source for host reads, while the binder and authoritative token
        // table continue to own the Program source itself.
        let mut host_package_json_values =
            checker.host_package_json_values.iter().collect::<Vec<_>>();
        host_package_json_values.sort_unstable_by_key(|(path, _)| *path);
        for (path, value) in host_package_json_values {
            let normalized = CheckerState::normalize_program_path(path, &current_directory);
            files.insert(normalized, Some(value.to_string()));
        }
        Self {
            current_directory,
            files,
            modes,
        }
    }

    fn normalized(&self, path: &str) -> String {
        CheckerState::normalize_program_path(path, &self.current_directory)
    }
}

/// tsc-port: getDefaultResolutionModeForFileWorker @6.0.3
/// tsc-hash: d8d78ed1732a7ecd9966c4b77346fa7c5622e33dc59ccfe78780e9efd612f0f0
/// tsc-span: _tsc.js:125510-125512
fn default_resolution_mode_for_checker_file(
    checker: &CheckerState<'_>,
    file: NodeId,
) -> EmitResolutionMode {
    // `importSyntaxAffectsModuleResolution` (_tsc.js:17994-17997) gates the
    // implied format. In particular, an ordinary `.ts` file compiled with
    // `module=esnext` does not acquire an ESNext syntax-implied mode merely
    // from the output module kind.
    let module_resolution = checker.options.emit_module_resolution_kind();
    let default_package_maps = matches!(module_resolution, 3 | 99 | 100);
    let import_syntax_affects_resolution = (3..=99).contains(&module_resolution)
        || checker
            .options
            .resolve_package_json_exports
            .unwrap_or(default_package_maps)
        || checker
            .options
            .resolve_package_json_imports
            .unwrap_or(default_package_maps);
    if !import_syntax_affects_resolution {
        return EmitResolutionMode::None;
    }

    let source = checker.binder.source_of_node(file);
    let implied = checker.implied_node_format_for_emit(file);
    if (100..=199).contains(&checker.options.emit_module_kind()) {
        return emit_resolution_mode(implied);
    }

    let file_name = &source.file_name;
    let extension_implies_common_js = file_name.ends_with(".cts") || file_name.ends_with(".cjs");
    let extension_implies_es_next = file_name.ends_with(".mts") || file_name.ends_with(".mjs");
    let package_type = checker_package_scope_module_type(checker, file_name);
    match implied {
        Some(ModuleResolutionMode::CommonJs)
            if extension_implies_common_js
                || package_type == Some(PackageJsonModuleType::CommonJs) =>
        {
            EmitResolutionMode::CommonJs
        }
        Some(ModuleResolutionMode::EsNext)
            if extension_implies_es_next || package_type == Some(PackageJsonModuleType::Module) =>
        {
            EmitResolutionMode::EsNext
        }
        Some(ModuleResolutionMode::CommonJs)
        | Some(ModuleResolutionMode::EsNext)
        | Some(ModuleResolutionMode::Unknown)
        | None => EmitResolutionMode::None,
    }
}

fn checker_package_scope_module_type(
    checker: &CheckerState<'_>,
    file_name: &str,
) -> Option<PackageJsonModuleType> {
    let normalized = CheckerState::normalize_program_path(file_name, "");
    let mut directory = normalized
        .rsplit_once('/')
        .map(|(directory, _)| directory)
        .unwrap_or("");
    loop {
        let package_json = if directory.is_empty() {
            "/package.json".to_owned()
        } else {
            format!("{directory}/package.json")
        };
        if let Some(&module_type) = checker.host_package_json_module_types.get(&package_json) {
            return Some(module_type);
        }
        let (parent, _) = directory.rsplit_once('/')?;
        directory = parent;
    }
}

impl EmitModuleSpecifierHost for BasicModuleSpecifierHost {
    fn get_current_directory(&self) -> String {
        self.current_directory.clone()
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }

    fn file_exists(&self, file_name: &str) -> bool {
        self.files.contains_key(&self.normalized(file_name))
    }

    fn read_file(&self, file_name: &str) -> Option<String> {
        self.files
            .get(&self.normalized(file_name))
            .and_then(Clone::clone)
    }

    fn get_common_source_directory(&self) -> String {
        self.current_directory.clone()
    }

    fn get_default_resolution_mode_for_file(&self, file: EmitResolverNode) -> EmitResolutionMode {
        self.modes
            .get(&file.source().raw())
            .copied()
            .unwrap_or(EmitResolutionMode::None)
    }

    fn get_mode_for_resolution_at_index(
        &self,
        file: EmitResolverNode,
        _index: u32,
    ) -> EmitResolutionMode {
        self.get_default_resolution_mode_for_file(file)
    }

    fn module_resolution_cache_available(&self) -> bool {
        true
    }
}

/// Preserve every caller-host capability while extending its file view with
/// the checker's complete input set. Prepared package manifests are host
/// files, not emit sources, so the declaration tracker's syntax-only adapter
/// deliberately needs this checker-owned fallback for `fileExists` and
/// `readFile`.
struct ModuleSpecifierHostWithFallback<'a> {
    primary: &'a dyn EmitModuleSpecifierHost,
    fallback: &'a dyn EmitModuleSpecifierHost,
}

impl EmitModuleSpecifierHost for ModuleSpecifierHostWithFallback<'_> {
    fn get_current_directory(&self) -> String {
        self.primary.get_current_directory()
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.primary.use_case_sensitive_file_names()
    }

    fn file_exists(&self, file_name: &str) -> bool {
        self.primary.file_exists(file_name) || self.fallback.file_exists(file_name)
    }

    fn read_file(&self, file_name: &str) -> Option<String> {
        self.primary
            .read_file(file_name)
            .or_else(|| self.fallback.read_file(file_name))
    }

    fn get_common_source_directory(&self) -> String {
        self.primary.get_common_source_directory()
    }

    fn get_default_resolution_mode_for_file(&self, file: EmitResolverNode) -> EmitResolutionMode {
        self.fallback.get_default_resolution_mode_for_file(file)
    }

    fn get_mode_for_resolution_at_index(
        &self,
        file: EmitResolverNode,
        index: u32,
    ) -> EmitResolutionMode {
        self.primary.get_mode_for_resolution_at_index(file, index)
    }

    fn symlinked_directories(&self) -> Vec<(String, String)> {
        self.primary.symlinked_directories()
    }

    fn symlinked_files(&self) -> Vec<(String, String)> {
        self.primary.symlinked_files()
    }

    fn get_nearest_ancestor_directory_with_package_json(&self, file_name: &str) -> Option<String> {
        self.primary
            .get_nearest_ancestor_directory_with_package_json(file_name)
    }

    fn get_global_typings_cache_location(&self) -> Option<String> {
        self.primary.get_global_typings_cache_location()
    }

    fn redirect_targets(&self, file_path: &str) -> Vec<String> {
        self.primary.redirect_targets(file_path)
    }

    fn get_redirect_from_source_file(&self, file_name: &str) -> Option<String> {
        self.primary.get_redirect_from_source_file(file_name)
    }

    fn is_source_of_project_reference_redirect(&self, file_name: &str) -> bool {
        self.primary
            .is_source_of_project_reference_redirect(file_name)
    }

    fn import_include_reasons(&self, imported_path: &str) -> Vec<EmitImportIncludeReason> {
        self.primary.import_include_reasons(imported_path)
    }

    fn module_resolution_cache_available(&self) -> bool {
        self.primary.module_resolution_cache_available()
    }
}

/// tsc-port: lookupSymbolChain @6.0.3
/// tsc-hash: 5c2dedc6ecdf455ed0945fd4d0da73e87a6ad323f14a02e80433c988609c9826
/// tsc-span: _tsc.js:52939-52942
pub(crate) fn chains_lookup_symbol_chain(
    checker: &mut CheckerState<'_>,
    context: &mut NodeBuilderContext<'_>,
    symbol: SymbolId,
    meaning: EmitSymbolMeaning,
) -> BuildResult<Vec<SymbolId>> {
    lookup_symbol_chain(checker, None, None, context, symbol, meaning, false)
}

fn lookup_symbol_chain(
    checker: &mut CheckerState<'_>,
    arena: Option<&mut TransformArena>,
    target: Option<TransformSourceId>,
    context: &mut NodeBuilderContext<'_>,
    symbol: SymbolId,
    meaning: EmitSymbolMeaning,
    yield_module_symbol: bool,
) -> BuildResult<Vec<SymbolId>> {
    track_symbol_in_context(checker, arena, target, context, symbol, meaning)?;
    lookup_symbol_chain_worker(checker, context, symbol, meaning, yield_module_symbol)
}

pub(super) fn track_symbol_in_context(
    checker: &mut CheckerState<'_>,
    arena: Option<&mut TransformArena>,
    target: Option<TransformSourceId>,
    context: &mut NodeBuilderContext<'_>,
    symbol: SymbolId,
    meaning: EmitSymbolMeaning,
) -> BuildResult<()> {
    let enclosing = context.enclosing_declaration;
    track_symbol_in_context_at(checker, arena, target, context, symbol, enclosing, meaning)
}

/// `context.tracker.trackSymbol(symbol, enclosingDeclaration, meaning)` with
/// an explicit enclosing declaration — the cache-hit replay of
/// visitAndTransformType (51811) re-tracks the symbols recorded when the
/// node was first built, under the enclosing declaration recorded then.
pub(super) fn track_symbol_in_context_at(
    checker: &mut CheckerState<'_>,
    arena: Option<&mut TransformArena>,
    target: Option<TransformSourceId>,
    context: &mut NodeBuilderContext<'_>,
    symbol: SymbolId,
    enclosing_declaration_override: Option<NodeId>,
    meaning: EmitSymbolMeaning,
) -> BuildResult<()> {
    let symbol_flags = checker.symbol_flags(symbol);
    let statement_tracking = context.tracker.is_statement_tracking();
    let symbol_is_remapped = super::is_statement_symbol_remapped(checker, context, symbol);
    {
        let mut access = CheckerTrackerAccess {
            checker,
            arena,
            target,
            statement_tracking,
        };
        let NodeBuilderContext {
            tracker,
            reported_diagnostic,
            tracked_symbols,
            recovery_tracked_symbols,
            enclosing_declaration,
            enclosing_declaration_is_synthetic,
            ..
        } = context;
        let _ = enclosing_declaration;
        tracker.track_symbol(
            reported_diagnostic,
            tracked_symbols,
            recovery_tracked_symbols,
            &mut access,
            symbol,
            symbol_flags,
            enclosing_declaration_override,
            *enclosing_declaration_is_synthetic,
            meaning,
            symbol_is_remapped,
        )?;
    }
    Ok(())
}

/// Decision reuse: `CheckerState::symbol_chain_slice` is the already-exact
/// Rust owner of the nested `getSymbolChain` and `sortByBestName` decisions.
/// This emit-channel wrapper preserves upstream's TypeParameter/context/
/// DoNotIncludeSymbolChain front gate and never consumes its string result.
///
/// tsc-port: lookupSymbolChainWorker @6.0.3
/// tsc-hash: 49cd0d3c42543be1ed55eca2f0c7cf9484cb25939c4505cd892611d54621d2b6
/// tsc-span: _tsc.js:52943-53017
/// tsc-port: getSymbolChain @6.0.3 (decision reuse)
/// tsc-hash: 8ccb0f4b99b34c677210c369edfdf15d1f0cc32eed7f57b6b153783b4808d291
/// tsc-span: _tsc.js:52958-53016
/// tsc-port: sortByBestName @6.0.3 (decision reuse)
/// tsc-hash: 5254873e77fc56b5bacdcd29064b22dbc40c38236f549a6c0af509851523b662
/// tsc-span: _tsc.js:53001-53015
pub(super) fn lookup_symbol_chain_worker(
    checker: &mut CheckerState<'_>,
    context: &NodeBuilderContext<'_>,
    symbol: SymbolId,
    meaning: EmitSymbolMeaning,
    yield_module_symbol: bool,
) -> BuildResult<Vec<SymbolId>> {
    if yield_module_symbol && value_meaning(meaning) && context.enclosing_declaration_is_synthetic {
        if let Some(parent) = checker.binder.symbol(symbol).parent {
            if checker
                .symbol_flags(symbol)
                .intersects(SymbolFlags::FUNCTION)
                && checker.symbol_has_external_module_declaration(parent)
            {
                return Ok(vec![parent, symbol]);
            }
        }
    }
    if !checker
        .symbol_flags(symbol)
        .intersects(SymbolFlags::TYPE_PARAMETER)
        && (context.enclosing_declaration.is_some() || has_flag(context, USE_FULLY_QUALIFIED_TYPE))
        && context.internal_flags.0 & DO_NOT_INCLUDE_SYMBOL_CHAIN == 0
    {
        // Rust represents enterNewScope's synthesized Block as an overlay
        // rather than a binder node. Give that overlay the same first-scope
        // precedence as upstream before delegating the parse-tree walk.
        let escaped_name = checker.binder.symbol(symbol).escaped_name.as_str();
        let meaning_flags = symbol_flags_for_meaning(meaning);
        let shadowed_by_synthetic_local = context
            .synthetic_scope_locals
            .as_ref()
            .and_then(|locals| locals.get(escaped_name))
            .copied()
            .is_some_and(|local| {
                checker.get_merged_symbol(local) != checker.get_merged_symbol(symbol)
                    && checker.symbol_flags(local).intersects(meaning_flags)
            });
        let is_shadowed_global = shadowed_by_synthetic_local
            && checker
                .globals
                .get(escaped_name)
                .copied()
                .is_some_and(|global| {
                    checker.get_merged_symbol(global) == checker.get_merged_symbol(symbol)
                });
        let global_this_is_shadowed = context
            .synthetic_scope_locals
            .as_ref()
            .and_then(|locals| locals.get("globalThis"))
            .copied()
            .is_some_and(|local| {
                checker.get_merged_symbol(local)
                    != checker.get_merged_symbol(checker.global_this_symbol)
                    && checker.symbol_flags(local).intersects(
                        if meaning_flags == SymbolFlags::VALUE {
                            SymbolFlags::VALUE
                        } else {
                            SymbolFlags::NAMESPACE
                        },
                    )
            });
        if is_shadowed_global && !global_this_is_shadowed {
            return Ok(vec![checker.global_this_symbol, symbol]);
        }
        let chain = checker
            .symbol_chain_slice(
                symbol,
                symbol_flags_for_meaning(meaning),
                true,
                yield_module_symbol,
                context.enclosing_declaration,
            )
            .map(|chain| chain.expect("endOfChain always yields a symbol chain"))
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        return prefer_alternative_containing_module_chain(
            checker, context, symbol, meaning, chain,
        );
    }
    Ok(vec![symbol])
}

/// tsc-port: getAlternativeContainingModules @6.0.3
/// tsc-hash: 2993e3c865bd02307b93e7eb47d2eaea892fd2e3b321ba2bb78612cb66c60724
/// tsc-span: _tsc.js:49949-49988
///
/// The shared display slice deliberately omits the enclosing-file import
/// candidates because its non-emitting callers do not have the declaration
/// tracker's module-specifier host. The NodeBuilder caller does: reconstruct
/// `SourceFile.imports`, resolve each module, and retain the modules whose
/// export table contains the symbol by reference.
fn alternative_containing_module_chains(
    checker: &mut CheckerState<'_>,
    context: &NodeBuilderContext<'_>,
    symbol: SymbolId,
) -> BuildResult<Vec<Vec<SymbolId>>> {
    let Some(enclosing) = context.enclosing_declaration else {
        return Ok(Vec::new());
    };
    let file_index = checker.binder.file_index_of_node(enclosing);
    let (imports, _) = module_name_literals(checker, file_index);
    let mut results = Vec::new();
    for import_ref in imports {
        let Some(module) = checker
            .resolve_external_module_name(enclosing, import_ref, true)
            .map_err(|abort| checker_abort_error(checker, context, abort))?
        else {
            continue;
        };
        let alias = alias_for_symbol_in_module(checker, context, module, symbol)?;
        let Some(alias) = alias else {
            continue;
        };
        let mut chain = vec![module];
        if alias != module {
            chain.push(alias);
        }
        results.push(chain);
    }
    if !results.is_empty() {
        return Ok(results);
    }

    // Once the per-containing-file import cache misses, upstream computes
    // the symbol-wide fallback by scanning every external source file.
    let modules = (0..checker.binder.file_count())
        .filter_map(|index| {
            let root = checker.binder.source(index).root;
            checker
                .binder
                .is_external_module_of_node(root)
                .then(|| checker.binder.node_symbol(root))
                .flatten()
        })
        .map(|module| checker.get_merged_symbol(module))
        .collect::<Vec<_>>();
    for module in modules {
        let Some(alias) = alias_for_symbol_in_module(checker, context, module, symbol)? else {
            continue;
        };
        let mut chain = vec![module];
        if alias != module {
            chain.push(alias);
        }
        results.push(chain);
    }
    Ok(results)
}

/// tsc-port: getAliasForSymbolInContainer @6.0.3
/// tsc-hash: 33333377bf20d625fbd2b1ed3577e8e1ff93b9385d89c1fd0818cf487e348c63
/// tsc-span: _tsc.js:50065-50083
fn alias_for_symbol_in_module(
    checker: &mut CheckerState<'_>,
    context: &NodeBuilderContext<'_>,
    module: SymbolId,
    symbol: SymbolId,
) -> BuildResult<Option<SymbolId>> {
    if checker.get_parent_of_symbol(symbol) == Some(module) {
        return Ok(Some(symbol));
    }
    let export_equals = checker
        .binder
        .symbol(module)
        .exports
        .get(tsc_types::InternalSymbolName::EXPORT_EQUALS)
        .copied();
    if let Some(export_equals) = export_equals {
        if checker
            .get_symbol_if_same_reference(export_equals, symbol)
            .map_err(|abort| checker_abort_error(checker, context, abort))?
            .is_some()
        {
            return Ok(Some(module));
        }
    }
    let exports = checker
        .get_exports_of_symbol(module)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    let escaped_name = checker.binder.symbol(symbol).escaped_name.clone();
    if let Some(candidate) = exports.get(&escaped_name).copied() {
        if checker
            .get_symbol_if_same_reference(candidate, symbol)
            .map_err(|abort| checker_abort_error(checker, context, abort))?
            .is_some()
        {
            return Ok(Some(candidate));
        }
    }
    let candidates = exports.values().copied().collect::<Vec<_>>();
    for candidate in candidates {
        if checker
            .get_symbol_if_same_reference(candidate, symbol)
            .map_err(|abort| checker_abort_error(checker, context, abort))?
            .is_some()
        {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

fn module_specifier_is_relative(specifier: &str) -> bool {
    specifier == "."
        || specifier == ".."
        || specifier.starts_with("./")
        || specifier.starts_with("../")
}

/// Apply getSymbolChain's `sortByBestName` to the ordinary container chain
/// and the enclosing file's re-export containers. A bare package candidate
/// therefore wins over a relative `/node_modules/` spelling, while equal
/// shapes preserve discovery order.
fn prefer_alternative_containing_module_chain(
    checker: &mut CheckerState<'_>,
    context: &NodeBuilderContext<'_>,
    symbol: SymbolId,
    _meaning: EmitSymbolMeaning,
    chain: Vec<SymbolId>,
) -> BuildResult<Vec<SymbolId>> {
    if checker.symbol_has_external_module_declaration(symbol)
        || !chain
            .first()
            .is_some_and(|&root| checker.symbol_has_external_module_declaration(root))
    {
        return Ok(chain);
    }
    let alternatives = alternative_containing_module_chains(checker, context, symbol)?;
    if alternatives.is_empty() {
        return Ok(chain);
    }
    let mut candidates = Vec::with_capacity(alternatives.len() + 1);
    candidates.push(chain);
    candidates.extend(alternatives);
    let mut ranked = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let specifier = specifier_for_module_symbol(checker, context, candidate[0], None)?;
        ranked.push((candidate, specifier));
    }
    ranked.sort_by(|(_, specifier_a), (_, specifier_b)| {
        let relative_a = module_specifier_is_relative(specifier_a);
        let relative_b = module_specifier_is_relative(specifier_b);
        if relative_a == relative_b {
            let components = |specifier: &str| {
                specifier
                    .as_bytes()
                    .iter()
                    .filter(|&&byte| byte == b'/')
                    .count()
            };
            components(specifier_a).cmp(&components(specifier_b))
        } else if relative_b {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        }
    });
    Ok(ranked
        .into_iter()
        .next()
        .expect("ordinary module container is always ranked")
        .0)
}

/// tsc-port: typeParametersToTypeParameterDeclarations @6.0.3
/// tsc-hash: 786175bc645d5b5f91a1562f899b9b8448fcd88f0493fedca42d371ddafd0987
/// tsc-span: _tsc.js:53018-53025
fn type_parameters_to_type_parameter_declarations(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    symbol: SymbolId,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<Option<Vec<TransformNode>>> {
    let target_symbol = checker.get_target_symbol(symbol);
    if !checker
        .symbol_flags(target_symbol)
        .intersects(SymbolFlags::CLASS | SymbolFlags::INTERFACE | SymbolFlags::TYPE_ALIAS)
    {
        return Ok(None);
    }
    let parameters = checker.get_local_type_parameters_of_class_or_interface_or_type_alias(symbol);
    let mut declarations = Vec::with_capacity(parameters.len());
    for parameter in parameters {
        declarations.push(type_parameter_to_declaration(
            checker, arena, target, parameter, context, None,
        )?);
    }
    Ok(Some(declarations))
}

fn class_or_interface_type_parameters(
    checker: &mut CheckerState<'_>,
    symbol: SymbolId,
    context: &NodeBuilderContext<'_>,
) -> BuildResult<Vec<TypeId>> {
    let declared = checker
        .get_declared_type_of_symbol_slice(symbol)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    Ok(match &checker.tables.type_of(declared).data {
        TypeData::GenericType {
            type_parameters, ..
        } => type_parameters.to_vec(),
        _ => checker.get_local_type_parameters_of_class_or_interface_or_type_alias(symbol),
    })
}

/// tsc-port: lookupTypeParameterNodes @6.0.3
/// tsc-hash: e6b9d7c2a9c995b35507303c582cd892219f72e7e3add4143d39587c0d6f4f29
/// tsc-span: _tsc.js:53026-53053
fn lookup_type_parameter_nodes(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    chain: &[SymbolId],
    index: usize,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<Option<Vec<TransformNode>>> {
    debug_assert!(index < chain.len());
    let symbol = chain[index];
    if context
        .type_parameter_symbol_list
        .as_ref()
        .is_some_and(|symbols| symbols.contains(&symbol))
    {
        return Ok(None);
    }
    if context.must_create_type_parameter_symbol_list {
        context.must_create_type_parameter_symbol_list = false;
        context.type_parameter_symbol_list = Some(
            context
                .type_parameter_symbol_list
                .clone()
                .unwrap_or_default(),
        );
    }
    context
        .type_parameter_symbol_list
        .get_or_insert_with(Default::default)
        .insert(symbol);

    if !has_flag(context, WRITE_TYPE_PARAMETERS_IN_QUALIFIED_NAME) || index >= chain.len() - 1 {
        return Ok(None);
    }
    let next = chain[index + 1];
    if checker
        .get_check_flags(next)
        .intersects(CheckFlags::INSTANTIATED)
    {
        let parent = if checker.symbol_flags(symbol).intersects(SymbolFlags::ALIAS) {
            checker
                .resolve_alias(symbol)
                .map_err(|abort| checker_abort_error(checker, context, abort))?
        } else {
            symbol
        };
        let parameters = class_or_interface_type_parameters(checker, parent, context)?;
        let Some(mapper) = checker.links.symbol(next).mapper else {
            return Err(EmitResolverError::CheckerAborted {
                method: EmitResolverMethod::CreateTypeOfDeclaration,
                node: enclosing_resolver_node(
                    checker,
                    context
                        .enclosing_declaration
                        .unwrap_or_else(|| checker.binder.source(0).root),
                ),
                reason: "instantiated symbol is missing its type mapper",
            });
        };
        let mut mapped = Vec::with_capacity(parameters.len());
        for parameter in parameters {
            mapped.push(
                checker
                    .get_mapped_type(parameter, mapper)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?,
            );
        }
        return super::map_to_type_nodes(checker, arena, target, &mapped, context, false);
    }
    type_parameters_to_type_parameter_declarations(checker, arena, target, symbol, context)
}

/// tsc-port: getTopmostIndexedAccessType @6.0.3
/// tsc-hash: f53b3efbb38f1c4c60a4920a41f44cd8525db5bdbaa41a6143e46609d4130fcf
/// tsc-span: _tsc.js:53054-53059
fn get_topmost_indexed_access_type(
    arena: &TransformArena,
    mut top: TransformNode,
) -> BuildResult<TransformNode> {
    loop {
        let NodeData::IndexedAccessType(data) = &arena.node(top).map_err(factory_error)?.data
        else {
            return Ok(top);
        };
        let Some(object_type) = data
            .object_type
            .and_then(|node| arena.node_ref(top.source(), node))
        else {
            return Ok(top);
        };
        if arena.node(object_type).map_err(factory_error)?.kind != SyntaxKind::IndexedAccessType {
            return Ok(top);
        }
        top = object_type;
    }
}

/// Delegates the complete mode/cache/computation closure to lane H.
///
/// tsc-port: getSpecifierForModuleSymbol @6.0.3 (lane-H call)
/// tsc-hash: cc081ccc9162d99c71cfb5013a0786210de8d66472567a9ee1d6eab90f686463
/// tsc-span: _tsc.js:53060-53109
/// (h2-7a-m-3 widening: statements alias/module-specifier synthesis.)
pub(crate) fn specifier_for_module_symbol(
    checker: &mut CheckerState<'_>,
    context: &NodeBuilderContext<'_>,
    symbol: SymbolId,
    override_import_mode: Option<EmitResolutionMode>,
) -> BuildResult<String> {
    let enclosing_file = context.enclosing_file;
    let enclosing_declaration = context.enclosing_declaration;
    let bundled = context.bundled;
    if enclosing_file.is_none() {
        return get_specifier_for_module_symbol(
            checker,
            symbol,
            None,
            enclosing_file,
            enclosing_declaration,
            bundled,
            override_import_mode,
        )
        .map_err(|abort| checker_abort_error(checker, context, abort));
    }
    let fallback = BasicModuleSpecifierHost::new(checker);
    if let Some(primary) = context.tracker.caller_module_resolver_host() {
        let host = ModuleSpecifierHostWithFallback {
            primary,
            fallback: &fallback,
        };
        return get_specifier_for_module_symbol(
            checker,
            symbol,
            Some(&host),
            enclosing_file,
            enclosing_declaration,
            bundled,
            override_import_mode,
        )
        .map_err(|abort| checker_abort_error(checker, context, abort));
    }
    get_specifier_for_module_symbol(
        checker,
        symbol,
        Some(&fallback),
        enclosing_file,
        enclosing_declaration,
        bundled,
        override_import_mode,
    )
    .map_err(|abort| checker_abort_error(checker, context, abort))
}

fn create_qualified_name(
    arena: &mut TransformArena,
    target: TransformSourceId,
    left: TransformNode,
    right: TransformNode,
) -> BuildResult<TransformNode> {
    create_node(
        arena,
        target,
        NodeData::QualifiedName(QualifiedNameData {
            left: Some(left.node()),
            right: Some(right.node()),
        }),
    )
}

/// tsc-port: symbolToEntityNameNode @6.0.3
/// tsc-hash: c19388e4a4f65093f960c575873468d2e30c9b860998cd32ae9016a2705b93b2
/// tsc-span: _tsc.js:53110-53113
pub(crate) fn chains_symbol_to_entity_name_node(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    _context: &mut NodeBuilderContext<'_>,
    symbol: SymbolId,
) -> BuildResult<TransformNode> {
    fn build(
        checker: &CheckerState<'_>,
        arena: &mut TransformArena,
        target: TransformSourceId,
        symbol: SymbolId,
    ) -> BuildResult<TransformNode> {
        let identifier = create_identifier(
            arena,
            target,
            tsc_binder::unescape_leading_underscores(&checker.binder.symbol(symbol).escaped_name),
        )?;
        match checker.binder.symbol(symbol).parent {
            Some(parent) => {
                let left = build(checker, arena, target, parent)?;
                create_qualified_name(arena, target, left, identifier)
            }
            None => Ok(identifier),
        }
    }
    build(checker, arena, target, symbol)
}

fn create_literal_type(
    arena: &mut TransformArena,
    target: TransformSourceId,
    literal: TransformNode,
) -> BuildResult<TransformNode> {
    create_node(
        arena,
        target,
        NodeData::LiteralType(LiteralTypeData {
            literal: Some(literal.node()),
        }),
    )
}

fn create_string_literal(
    arena: &mut TransformArena,
    target: TransformSourceId,
    text: &str,
    single_quote: bool,
) -> BuildResult<TransformNode> {
    let literal = create_node(
        arena,
        target,
        NodeData::StringLiteral(StringLiteralData {
            text: text.to_owned(),
            has_extended_unicode_escape: None,
        }),
    )?;
    arena
        .metadata_mut(literal)
        .set_string_literal_single_quote(single_quote);
    Ok(literal)
}

fn create_numeric_literal(
    arena: &mut TransformArena,
    target: TransformSourceId,
    value: f64,
) -> BuildResult<TransformNode> {
    let magnitude = if value < 0.0 { -value } else { value };
    let literal = arena
        .factory()
        .create_numeric_literal(target, tsc_types::js_number_to_string(magnitude))
        .map_err(factory_error)?;
    if value < 0.0 {
        arena
            .factory()
            .create_prefix_unary_expression(target, SyntaxKind::MinusToken, literal)
            .map_err(factory_error)
    } else {
        Ok(literal)
    }
}

fn create_type_reference(
    arena: &mut TransformArena,
    target: TransformSourceId,
    name: TransformNode,
    arguments: Option<Vec<TransformNode>>,
) -> BuildResult<TransformNode> {
    let type_arguments = arguments
        .map(|arguments| create_node_array(arena, target, arguments))
        .transpose()?;
    create_node(
        arena,
        target,
        NodeData::TypeReference(TypeReferenceData {
            type_arguments,
            type_name: Some(name.node()),
        }),
    )
}

fn create_type_query(
    arena: &mut TransformArena,
    target: TransformSourceId,
    name: TransformNode,
) -> BuildResult<TransformNode> {
    create_node(
        arena,
        target,
        NodeData::TypeQuery(TypeQueryData {
            type_arguments: None,
            expr_name: Some(name.node()),
        }),
    )
}

fn create_indexed_access(
    arena: &mut TransformArena,
    target: TransformSourceId,
    object_type: TransformNode,
    index_type: TransformNode,
) -> BuildResult<TransformNode> {
    create_node(
        arena,
        target,
        NodeData::IndexedAccessType(IndexedAccessTypeData {
            object_type: Some(object_type.node()),
            index_type: Some(index_type.node()),
        }),
    )
}

fn create_parenthesized_type(
    arena: &mut TransformArena,
    target: TransformSourceId,
    r#type: TransformNode,
) -> BuildResult<TransformNode> {
    create_node(
        arena,
        target,
        NodeData::ParenthesizedType(ParenthesizedTypeData {
            r#type: Some(r#type.node()),
        }),
    )
}

fn create_import_attributes(
    arena: &mut TransformArena,
    target: TransformSourceId,
    mode: EmitResolutionMode,
) -> BuildResult<TransformNode> {
    let name = create_string_literal(arena, target, "resolution-mode", false)?;
    let value = create_string_literal(
        arena,
        target,
        if mode == EmitResolutionMode::EsNext {
            "import"
        } else {
            "require"
        },
        false,
    )?;
    let attribute = create_node(
        arena,
        target,
        NodeData::ImportAttribute(ImportAttributeData {
            name: Some(name.node()),
            value: Some(value.node()),
        }),
    )?;
    let elements = create_node_array(arena, target, vec![attribute])?;
    create_node(
        arena,
        target,
        NodeData::ImportAttributes(ImportAttributesData {
            token: SyntaxKind::WithKeyword,
            elements: Some(elements),
            multi_line: None,
        }),
    )
}

fn create_import_type(
    arena: &mut TransformArena,
    target: TransformSourceId,
    argument: TransformNode,
    attributes: Option<TransformNode>,
    qualifier: Option<TransformNode>,
    type_arguments: Option<Vec<TransformNode>>,
    is_type_of: bool,
) -> BuildResult<TransformNode> {
    let type_arguments = type_arguments
        .map(|arguments| create_node_array(arena, target, arguments))
        .transpose()?;
    create_node(
        arena,
        target,
        NodeData::ImportType(ImportTypeData {
            type_arguments,
            is_type_of,
            argument: Some(argument.node()),
            attributes: attributes.map(TransformNode::node),
            qualifier: qualifier.map(TransformNode::node),
        }),
    )
}

fn is_entity_name(arena: &TransformArena, node: TransformNode) -> BuildResult<bool> {
    match &arena.node(node).map_err(factory_error)?.data {
        NodeData::Identifier(_) => Ok(true),
        NodeData::QualifiedName(data) => {
            let Some(left) = data
                .left
                .and_then(|left| arena.node_ref(node.source(), left))
            else {
                return Ok(false);
            };
            let right_is_identifier = data
                .right
                .and_then(|right| arena.node_ref(node.source(), right))
                .is_some_and(|right| {
                    arena
                        .node(right)
                        .is_ok_and(|right| right.kind == SyntaxKind::Identifier)
                });
            Ok(right_is_identifier && is_entity_name(arena, left)?)
        }
        _ => Ok(false),
    }
}

#[derive(Clone)]
struct AccessBuild {
    node: TransformNode,
    final_type_arguments: Option<Vec<TransformNode>>,
}

fn parse_node_is_entity_name(checker: &CheckerState<'_>, node: NodeId) -> bool {
    match checker.data_of(node) {
        NodeData::Identifier(_) => true,
        NodeData::QualifiedName(data) => {
            data.left
                .is_some_and(|left| parse_node_is_entity_name(checker, left))
                && data
                    .right
                    .is_some_and(|right| checker.kind_of(right) == SyntaxKind::Identifier)
        }
        _ => false,
    }
}

fn create_entity_name_from_parse_node(
    checker: &CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    node: NodeId,
) -> BuildResult<TransformNode> {
    match checker.data_of(node) {
        NodeData::Identifier(data) => {
            let identifier = create_identifier(arena, target, &data.text)?;
            Ok(set_no_ascii_escaping(arena, identifier))
        }
        NodeData::QualifiedName(data) => {
            let left = data.left.ok_or_else(|| EmitResolverError::CheckerAborted {
                method: EmitResolverMethod::CreateTypeOfDeclaration,
                node: enclosing_resolver_node(checker, node),
                reason: "qualified computed entity name has no left operand",
            })?;
            let right = data
                .right
                .ok_or_else(|| EmitResolverError::CheckerAborted {
                    method: EmitResolverMethod::CreateTypeOfDeclaration,
                    node: enclosing_resolver_node(checker, node),
                    reason: "qualified computed entity name has no right identifier",
                })?;
            let left = create_entity_name_from_parse_node(checker, arena, target, left)?;
            let right = create_entity_name_from_parse_node(checker, arena, target, right)?;
            create_qualified_name(arena, target, left, right)
        }
        _ => Err(EmitResolverError::CheckerAborted {
            method: EmitResolverMethod::CreateTypeOfDeclaration,
            node: enclosing_resolver_node(checker, node),
            reason: "computed symbol fallback requires an entity name",
        }),
    }
}

fn exported_name_for_chain_link(
    checker: &mut CheckerState<'_>,
    parent: SymbolId,
    symbol: SymbolId,
    context: &NodeBuilderContext<'_>,
) -> BuildResult<Option<String>> {
    let exports = checker
        .get_exports_of_symbol(parent)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    for (name, &exported) in exports.iter() {
        let same_reference = checker
            .get_symbol_if_same_reference(exported, symbol)
            .map_err(|abort| checker_abort_error(checker, context, abort))?
            .is_some();
        if same_reference
            && !name.starts_with("__@")
            && name != tsc_types::InternalSymbolName::EXPORT_EQUALS
        {
            return Ok(Some(
                tsc_binder::unescape_leading_underscores(name).to_owned(),
            ));
        }
    }
    Ok(None)
}

/// tsc-port: createAccessFromSymbolChain @6.0.3
/// tsc-hash: 702a651dcc1e3cb163bfbcd065fcb88ceb8714e0dd9cb8bb6b81b452f1f3e757
/// tsc-span: _tsc.js:53199-53251
#[allow(clippy::too_many_arguments)]
fn create_access_from_symbol_chain(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    chain: &[SymbolId],
    index: usize,
    stopper: usize,
    context: &mut NodeBuilderContext<'_>,
    override_type_arguments: Option<&[TransformNode]>,
) -> BuildResult<AccessBuild> {
    let type_parameter_nodes = if index == chain.len() - 1 {
        override_type_arguments.map(<[TransformNode]>::to_vec)
    } else {
        lookup_type_parameter_nodes(checker, arena, target, chain, index, context)?
    };
    let symbol = super::remapped_statement_symbol_reference(context, chain[index]);
    let parent = index.checked_sub(1).map(|index| chain[index]);
    let mut symbol_name = if index == 0 {
        context.flags.0 |= IN_INITIAL_ENTITY_NAME;
        let name = checker.entity_symbol_name_as_written_slice(
            symbol,
            true,
            has_flag(context, USE_ALIAS_DEFINED_OUTSIDE_CURRENT_SCOPE),
            context.enclosing_declaration,
        );
        add_approximate_length(context, js_len(&name) + 1);
        context.flags.0 ^= IN_INITIAL_ENTITY_NAME;
        Some(name)
    } else if let Some(parent) = parent {
        exported_name_for_chain_link(checker, parent, symbol, context)?
    } else {
        None
    };

    if symbol_name.is_none() {
        let computed_expression =
            checker
                .binder
                .symbol(symbol)
                .declarations
                .iter()
                .find_map(|&declaration| {
                    let name = node_util::get_name_of_declaration(
                        checker.binder.source_of_node(declaration),
                        declaration,
                    )?;
                    let NodeData::ComputedPropertyName(data) = checker.data_of(name) else {
                        return None;
                    };
                    data.expression
                        .filter(|&expression| parse_node_is_entity_name(checker, expression))
                });
        if let (Some(expression), Some(previous)) = (computed_expression, index.checked_sub(1)) {
            let lhs = create_access_from_symbol_chain(
                checker,
                arena,
                target,
                chain,
                previous,
                stopper,
                context,
                override_type_arguments,
            )?;
            if is_entity_name(arena, lhs.node)? {
                let object_query = create_type_query(arena, target, lhs.node)?;
                let object = create_parenthesized_type(arena, target, object_query)?;
                let expression =
                    create_entity_name_from_parse_node(checker, arena, target, expression)?;
                let index_query = create_type_query(arena, target, expression)?;
                return Ok(AccessBuild {
                    node: create_indexed_access(arena, target, object, index_query)?,
                    final_type_arguments: None,
                });
            }
            return Ok(lhs);
        }
        symbol_name = Some(checker.entity_symbol_name_as_written_slice(
            symbol,
            false,
            has_flag(context, USE_ALIAS_DEFINED_OUTSIDE_CURRENT_SCOPE),
            context.enclosing_declaration,
        ));
    }
    let symbol_name = symbol_name.expect("symbol name fallback is total");
    add_approximate_length(context, js_len(&symbol_name) + 1);

    if !has_flag(context, FORBID_INDEXED_ACCESS_SYMBOL_REFERENCES) {
        if let Some(parent) = parent {
            let members = checker
                .get_members_of_symbol(parent)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            let matching_member = match members.get(&checker.binder.symbol(symbol).escaped_name) {
                Some(&member) => checker
                    .get_symbol_if_same_reference(member, symbol)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?
                    .is_some(),
                None => false,
            };
            if matching_member {
                let lhs = create_access_from_symbol_chain(
                    checker,
                    arena,
                    target,
                    chain,
                    index - 1,
                    stopper,
                    context,
                    override_type_arguments,
                )?;
                let object = if arena.node(lhs.node).map_err(factory_error)?.kind
                    == SyntaxKind::IndexedAccessType
                {
                    lhs.node
                } else {
                    create_type_reference(arena, target, lhs.node, type_parameter_nodes.clone())?
                };
                let literal = create_string_literal(arena, target, &symbol_name, false)?;
                let index_type = create_literal_type(arena, target, literal)?;
                return Ok(AccessBuild {
                    node: create_indexed_access(arena, target, object, index_type)?,
                    final_type_arguments: None,
                });
            }
        }
    }

    let identifier = create_identifier(arena, target, &symbol_name)?;
    let identifier = set_no_ascii_escaping(arena, identifier);
    if index > stopper {
        let lhs = create_access_from_symbol_chain(
            checker,
            arena,
            target,
            chain,
            index - 1,
            stopper,
            context,
            override_type_arguments,
        )?;
        if !is_entity_name(arena, lhs.node)? {
            return Err(EmitResolverError::CheckerAborted {
                method: EmitResolverMethod::CreateTypeOfDeclaration,
                node: enclosing_resolver_node(
                    checker,
                    context
                        .enclosing_declaration
                        .unwrap_or_else(|| checker.binder.source(0).root),
                ),
                reason: "indexed-access export cannot be qualified as an entity name",
            });
        }
        return Ok(AccessBuild {
            node: create_qualified_name(arena, target, lhs.node, identifier)?,
            final_type_arguments: type_parameter_nodes,
        });
    }
    Ok(AccessBuild {
        node: identifier,
        final_type_arguments: type_parameter_nodes,
    })
}

fn module_source_root(checker: &CheckerState<'_>, symbol: SymbolId) -> Option<NodeId> {
    checker
        .binder
        .symbol(symbol)
        .declarations
        .iter()
        .copied()
        .find(|&declaration| checker.kind_of(declaration) == SyntaxKind::SourceFile)
        .or_else(|| {
            checker
                .binder
                .symbol(symbol)
                .declarations
                .first()
                .map(|&declaration| checker.binder.source_of_node(declaration).root)
        })
}

fn emit_resolution_mode(mode: Option<ModuleResolutionMode>) -> EmitResolutionMode {
    match mode {
        Some(ModuleResolutionMode::CommonJs) => EmitResolutionMode::CommonJs,
        Some(ModuleResolutionMode::EsNext) => EmitResolutionMode::EsNext,
        Some(ModuleResolutionMode::Unknown) | None => EmitResolutionMode::None,
    }
}

/// tsc-port: symbolToTypeNode @6.0.3
/// tsc-hash: 689e9be6e575f9f7c4ac745e1a5de28e6ba56b84b35330b8c7d8b0c350fe8b36
/// tsc-span: _tsc.js:53114-53252
pub(crate) fn chains_symbol_to_type_node(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    symbol: SymbolId,
    meaning: EmitSymbolMeaning,
    type_arguments: Option<Vec<TransformNode>>,
) -> BuildResult<TransformNode> {
    let chain = lookup_symbol_chain(
        checker,
        Some(arena),
        Some(target),
        context,
        symbol,
        meaning,
        !has_flag(context, USE_ALIAS_DEFINED_OUTSIDE_CURRENT_SCOPE),
    )?;
    let is_type_of = value_meaning(meaning);
    if checker.symbol_has_external_module_declaration(chain[0]) {
        let non_root = if chain.len() > 1 {
            Some(
                create_access_from_symbol_chain(
                    checker,
                    arena,
                    target,
                    &chain,
                    chain.len() - 1,
                    1,
                    context,
                    type_arguments.as_deref(),
                )?
                .node,
            )
        } else {
            None
        };
        let root_type_arguments = match type_arguments {
            Some(arguments) => Some(arguments),
            None => lookup_type_parameter_nodes(checker, arena, target, &chain, 0, context)?,
        };
        let context_file = context.enclosing_file;
        let target_file = module_source_root(checker, chain[0]);
        let module_resolution = checker.options.emit_module_resolution_kind();
        let context_mode = emit_resolution_mode(
            context_file.and_then(|file| checker.implied_node_format_for_emit(file)),
        );
        let target_mode = emit_resolution_mode(
            target_file.and_then(|file| checker.implied_node_format_for_emit(file)),
        );
        let mut attributes = None;
        let mut specifier = None;
        if matches!(module_resolution, 3 | 99)
            && target_mode == EmitResolutionMode::EsNext
            && target_mode != context_mode
        {
            specifier = Some(specifier_for_module_symbol(
                checker,
                context,
                chain[0],
                Some(EmitResolutionMode::EsNext),
            )?);
            attributes = Some(create_import_attributes(
                arena,
                target,
                EmitResolutionMode::EsNext,
            )?);
        }
        let mut specifier = match specifier {
            Some(specifier) => specifier,
            None => specifier_for_module_symbol(checker, context, chain[0], None)?,
        };
        if !has_flag(context, ALLOW_NODE_MODULES_RELATIVE_PATHS)
            && module_resolution != 1
            && specifier.contains("/node_modules/")
        {
            let old_specifier = specifier.clone();
            if matches!(module_resolution, 3 | 99) {
                let swapped_mode = if context_mode == EmitResolutionMode::EsNext {
                    EmitResolutionMode::CommonJs
                } else {
                    EmitResolutionMode::EsNext
                };
                let swapped =
                    specifier_for_module_symbol(checker, context, chain[0], Some(swapped_mode))?;
                if !swapped.contains("/node_modules/") {
                    specifier = swapped;
                    attributes = Some(create_import_attributes(arena, target, swapped_mode)?);
                }
            }
            if attributes.is_none() {
                context.encountered_error = true;
                context.tracker.report_likely_unsafe_import_required_error(
                    &mut context.reported_diagnostic,
                    &old_specifier,
                    Some(tsc_binder::unescape_leading_underscores(
                        &checker.binder.symbol(symbol).escaped_name,
                    )),
                );
            }
        }
        let literal = create_string_literal(arena, target, &specifier, false)?;
        let literal_type = create_literal_type(arena, target, literal)?;
        add_approximate_length(context, js_len(&specifier) + 10);
        if non_root
            .map(|node| is_entity_name(arena, node))
            .transpose()?
            .unwrap_or(true)
        {
            return create_import_type(
                arena,
                target,
                literal_type,
                attributes,
                non_root,
                root_type_arguments,
                is_type_of,
            );
        }
        let split = get_topmost_indexed_access_type(arena, non_root.expect("non-entity access"))?;
        let NodeData::IndexedAccessType(split_data) =
            arena.node(split).map_err(factory_error)?.data.clone()
        else {
            unreachable!("non-entity access from a symbol chain is indexed access")
        };
        let object = split_data
            .object_type
            .and_then(|node| arena.node_ref(split.source(), node))
            .expect("indexed access object");
        let qualifier = match &arena.node(object).map_err(factory_error)?.data {
            NodeData::TypeReference(data) => Some(
                data.type_name
                    .and_then(|node| arena.node_ref(object.source(), node))
                    .expect("indexed access object type name"),
            ),
            _ => None,
        };
        let import = create_import_type(
            arena,
            target,
            literal_type,
            attributes,
            qualifier,
            root_type_arguments,
            is_type_of,
        )?;
        let index_type = split_data
            .index_type
            .and_then(|node| arena.node_ref(split.source(), node))
            .expect("indexed access index");
        return create_indexed_access(arena, target, import, index_type);
    }

    let access = create_access_from_symbol_chain(
        checker,
        arena,
        target,
        &chain,
        chain.len() - 1,
        0,
        context,
        type_arguments.as_deref(),
    )?;
    if arena.node(access.node).map_err(factory_error)?.kind == SyntaxKind::IndexedAccessType {
        return Ok(access.node);
    }
    if is_type_of {
        create_type_query(arena, target, access.node)
    } else {
        create_type_reference(arena, target, access.node, access.final_type_arguments)
    }
}

/// tsc-port: typeParameterShadowsOtherTypeParameterInScope @6.0.3
/// tsc-hash: de490c564fbc92d3c74c1148d84aa6288cf0d44e7ad0f5b816592851f91a0f62
/// tsc-span: _tsc.js:53253-53267
fn type_parameter_shadows_other_type_parameter_in_scope(
    checker: &mut CheckerState<'_>,
    escaped_name: &str,
    context: &NodeBuilderContext<'_>,
    r#type: TypeId,
) -> BuildResult<bool> {
    if let Some(resolved) = context
        .synthetic_scope_locals
        .as_ref()
        .and_then(|locals| locals.get(escaped_name))
        .copied()
    {
        return Ok(checker
            .symbol_flags(resolved)
            .intersects(SymbolFlags::TYPE_PARAMETER)
            && checker.tables.type_of(r#type).symbol != Some(resolved));
    }
    let resolved = checker
        .resolve_name(
            context.enclosing_declaration,
            escaped_name,
            SymbolFlags::TYPE,
            None,
            false,
            false,
        )
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    Ok(resolved.is_some_and(|resolved| {
        checker
            .symbol_flags(resolved)
            .intersects(SymbolFlags::TYPE_PARAMETER)
            && checker.tables.type_of(r#type).symbol != Some(resolved)
    }))
}

fn identifier_text(arena: &TransformArena, node: TransformNode) -> BuildResult<Option<String>> {
    Ok(match &arena.node(node).map_err(factory_error)?.data {
        NodeData::Identifier(data) => Some(data.text.clone()),
        _ => None,
    })
}

/// tsc-port: typeParameterToName @6.0.3
/// tsc-hash: 2af39579c4d49dc989a641e9e4afb16c3d552d062a61fe050e7e81b8bdc709f2
/// tsc-span: _tsc.js:53268-53314
pub(crate) fn type_parameter_to_name(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    r#type: TypeId,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<TransformNode> {
    if has_flag(
        context,
        tsc_emitter::EmitNodeBuilderFlags::GENERATE_NAMES_FOR_SHADOWED_TYPE_PARAMS.0,
    ) {
        if let Some(cached) = context
            .type_parameter_names
            .as_ref()
            .and_then(|names| names.get(&r#type))
        {
            return Ok(*cached);
        }
    }
    let Some(symbol) = checker.tables.type_of(r#type).symbol else {
        return create_identifier(arena, target, "(Missing type parameter)");
    };
    let mut result = symbol_to_name(
        checker,
        arena,
        target,
        symbol,
        context,
        EmitSymbolMeaning::TYPE,
        true,
    )?;
    if arena.node(result).map_err(factory_error)?.kind != SyntaxKind::Identifier {
        return create_identifier(arena, target, "(Missing type parameter)");
    }
    let declaration = checker.binder.symbol(symbol).declarations.first().copied();
    if let Some(NodeData::TypeParameter(data)) = declaration.map(|node| checker.data_of(node)) {
        if let Some(name) = data.name {
            if let Some(location) = project_parse_node(checker, arena, name)? {
                result = set_text_range2(checker, arena, context, result, Some(location))?;
            }
        }
    }
    if has_flag(
        context,
        tsc_emitter::EmitNodeBuilderFlags::GENERATE_NAMES_FOR_SHADOWED_TYPE_PARAMS.0,
    ) {
        let raw_text = identifier_text(arena, result)?.expect("identifier checked above");
        let mut index = context
            .type_parameter_names_by_text_next_name_count
            .as_ref()
            .and_then(|counts| counts.get(&raw_text))
            .copied()
            .unwrap_or_default();
        let mut text = raw_text.clone();
        while context
            .type_parameter_names_by_text
            .as_ref()
            .is_some_and(|names| names.contains(&text))
            || type_parameter_shadows_other_type_parameter_in_scope(
                checker, &text, context, r#type,
            )?
        {
            index += 1;
            text = format!("{raw_text}_{index}");
        }
        if text != raw_text {
            result = create_identifier(arena, target, &text)?;
        }
        if context.must_create_type_parameters_names_lookups {
            context.must_create_type_parameters_names_lookups = false;
            context.type_parameter_names =
                Some(context.type_parameter_names.clone().unwrap_or_default());
            context.type_parameter_names_by_text_next_name_count = Some(
                context
                    .type_parameter_names_by_text_next_name_count
                    .clone()
                    .unwrap_or_default(),
            );
            context.type_parameter_names_by_text = Some(
                context
                    .type_parameter_names_by_text
                    .clone()
                    .unwrap_or_default(),
            );
        }
        context
            .type_parameter_names_by_text_next_name_count
            .get_or_insert_with(Default::default)
            .insert(raw_text, index);
        context
            .type_parameter_names
            .get_or_insert_with(Default::default)
            .insert(r#type, result);
        context
            .type_parameter_names_by_text
            .get_or_insert_with(Default::default)
            .insert(text);
    }
    Ok(result)
}

/// tsc-port: symbolToName @6.0.3
/// tsc-hash: 8000600326491063f035e6aea718ffc812fadb6de27ef8661ac40f43f7f91d26
/// tsc-span: _tsc.js:53315-53336
fn symbol_to_name(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    symbol: SymbolId,
    context: &mut NodeBuilderContext<'_>,
    meaning: EmitSymbolMeaning,
    expects_identifier: bool,
) -> BuildResult<TransformNode> {
    let chain = lookup_symbol_chain(
        checker,
        Some(arena),
        Some(target),
        context,
        symbol,
        meaning,
        false,
    )?;
    if expects_identifier
        && chain.len() != 1
        && !context.encountered_error
        && !has_flag(context, ALLOW_QUALIFIED_NAME_IN_PLACE_OF_IDENTIFIER)
    {
        context.encountered_error = true;
    }
    create_entity_name_from_symbol_chain(checker, arena, target, &chain, chain.len() - 1, context)
}

/// tsc-port: createEntityNameFromSymbolChain @6.0.3
/// tsc-hash: 91c5127fa4caccb57b1ba9ab1b58ce250b0c87cd67ceea5732bc4bb289b2945b
/// tsc-span: _tsc.js:53321-53335
fn create_entity_name_from_symbol_chain(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    chain: &[SymbolId],
    index: usize,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<TransformNode> {
    let _type_parameter_nodes =
        lookup_type_parameter_nodes(checker, arena, target, chain, index, context)?;
    let symbol = super::remapped_statement_symbol_reference(context, chain[index]);
    if index == 0 {
        context.flags.0 |= IN_INITIAL_ENTITY_NAME;
    }
    let name = checker.entity_symbol_name_as_written_slice(
        symbol,
        index == 0,
        has_flag(context, USE_ALIAS_DEFINED_OUTSIDE_CURRENT_SCOPE),
        context.enclosing_declaration,
    );
    if index == 0 {
        context.flags.0 ^= IN_INITIAL_ENTITY_NAME;
    }
    let identifier = create_identifier(arena, target, &name)?;
    let identifier = set_no_ascii_escaping(arena, identifier);
    if index == 0 {
        Ok(identifier)
    } else {
        let left = create_entity_name_from_symbol_chain(
            checker,
            arena,
            target,
            chain,
            index - 1,
            context,
        )?;
        create_qualified_name(arena, target, left, identifier)
    }
}

fn strip_symbol_name_quotes(name: &str) -> String {
    let body = name
        .strip_prefix(['\'', '"'])
        .and_then(|body| body.strip_suffix(['\'', '"']))
        .unwrap_or(name);
    let mut output = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(escaped) = chars.next() {
                output.push(escaped);
            }
        } else {
            output.push(ch);
        }
    }
    output
}

fn first_utf16(text: &str) -> Option<u16> {
    text.encode_utf16().next()
}

/// tsc-port: symbolToExpression @6.0.3
/// tsc-hash: f1c7de91b82f1b2f5a3b4a2e7c1b82bd8504e06172492e073464b298e0938e03
/// tsc-span: _tsc.js:53337-53387
pub(crate) fn chains_symbol_to_expression(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    symbol: SymbolId,
    meaning: EmitSymbolMeaning,
) -> BuildResult<TransformNode> {
    let chain = lookup_symbol_chain(
        checker,
        Some(arena),
        Some(target),
        context,
        symbol,
        meaning,
        false,
    )?;
    create_expression_from_symbol_chain(checker, arena, target, &chain, chain.len() - 1, context)
}

/// tsc-port: createExpressionFromSymbolChain @6.0.3
/// tsc-hash: ae319967a0dd4f380b2afc00a70a7767b8dc2b0e698691bd8abe27c673f52193
/// tsc-span: _tsc.js:53340-53386
fn create_expression_from_symbol_chain(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    chain: &[SymbolId],
    index: usize,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<TransformNode> {
    let _type_parameter_nodes =
        lookup_type_parameter_nodes(checker, arena, target, chain, index, context)?;
    let symbol = super::remapped_statement_symbol_reference(context, chain[index]);
    if index == 0 {
        context.flags.0 |= IN_INITIAL_ENTITY_NAME;
    }
    let mut name = checker.entity_symbol_name_as_written_slice(
        symbol,
        index == 0,
        has_flag(context, USE_ALIAS_DEFINED_OUTSIDE_CURRENT_SCOPE),
        context.enclosing_declaration,
    );
    if index == 0 {
        context.flags.0 ^= IN_INITIAL_ENTITY_NAME;
    }
    let mut first = first_utf16(&name);
    if matches!(first, Some(value) if value == u16::from(b'\'') || value == u16::from(b'"'))
        && checker.symbol_has_external_module_declaration(symbol)
    {
        let specifier = specifier_for_module_symbol(checker, context, symbol, None)?;
        add_approximate_length(context, js_len(&specifier) + 2);
        return create_string_literal(arena, target, &specifier, false);
    }
    if index == 0 || can_use_property_access_slice(&name, checker.options.emit_script_target()) {
        let identifier = create_identifier(arena, target, &name)?;
        let identifier = set_no_ascii_escaping(arena, identifier);
        add_approximate_length(context, js_len(&name) + 1);
        if index == 0 {
            return Ok(identifier);
        }
        let expression =
            create_expression_from_symbol_chain(checker, arena, target, chain, index - 1, context)?;
        return create_node(
            arena,
            target,
            NodeData::PropertyAccessExpression(PropertyAccessExpressionData {
                name: Some(identifier.node()),
                expression: Some(expression.node()),
                question_dot_token: None,
            }),
        );
    }
    if name.starts_with('[') && name.ends_with(']') && name.len() >= 2 {
        name = name[1..name.len() - 1].to_owned();
        first = first_utf16(&name);
    }
    let argument = if matches!(first, Some(value) if value == u16::from(b'\'') || value == u16::from(b'"'))
        && !checker
            .symbol_flags(symbol)
            .intersects(SymbolFlags::ENUM_MEMBER)
    {
        let text = strip_symbol_name_quotes(&name);
        add_approximate_length(context, js_len(&text) + 2);
        create_string_literal(arena, target, &text, first == Some(u16::from(b'\'')))?
    } else {
        let numeric = crate::evaluate::js_string_to_number(&name);
        if tsc_types::js_number_to_string(numeric) == name {
            add_approximate_length(context, js_len(&name));
            create_numeric_literal(arena, target, numeric)?
        } else {
            add_approximate_length(context, js_len(&name));
            let identifier = create_identifier(arena, target, &name)?;
            set_no_ascii_escaping(arena, identifier)
        }
    };
    add_approximate_length(context, 2);
    let expression =
        create_expression_from_symbol_chain(checker, arena, target, chain, index - 1, context)?;
    create_node(
        arena,
        target,
        NodeData::ElementAccessExpression(ElementAccessExpressionData {
            expression: Some(expression.node()),
            question_dot_token: None,
            argument_expression: Some(argument.node()),
        }),
    )
}

/// tsc-port: isStringNamed @6.0.3
/// tsc-hash: c000f08977999a9f153126ccfb4e5b4c8721c5e160a361bd941308799c3c657d
/// tsc-span: _tsc.js:53388-53402
fn is_string_named(
    checker: &CheckerState<'_>,
    declaration: NodeId,
    flags: Option<TypeFlags>,
) -> bool {
    checker.declaration_is_string_named(declaration, flags)
}

/// tsc-port: isSingleQuotedStringNamed @6.0.3
/// tsc-hash: a1cfaf3bb4dfc1e20d532883c41dc2ed9d730618cb43b9184a022875a3013093
/// tsc-span: _tsc.js:53403-53410
fn is_single_quoted_string_named(checker: &CheckerState<'_>, declaration: NodeId) -> bool {
    checker.declaration_is_single_quoted_string_named(declaration)
}

fn cloned_hash_private_name(
    checker: &CheckerState<'_>,
    arena: &mut TransformArena,
    symbol: SymbolId,
) -> BuildResult<Option<TransformNode>> {
    let Some(value_declaration) = checker.binder.symbol(symbol).value_declaration else {
        return Ok(None);
    };
    let Some(name) = node_util::get_name_of_declaration(
        checker.binder.source_of_node(value_declaration),
        value_declaration,
    ) else {
        return Ok(None);
    };
    if checker.kind_of(name) != SyntaxKind::PrivateIdentifier {
        return Ok(None);
    }
    clone_parse_node(checker, arena, name)
}

fn create_property_name_for_identifier_or_literal(
    arena: &mut TransformArena,
    target: TransformSourceId,
    name: &str,
    script_target: tsc_types::ScriptTarget,
    single_quote: bool,
    string_named: bool,
    is_method: bool,
) -> BuildResult<TransformNode> {
    let method_named_new = is_method && name == "new";
    if !method_named_new && tsc_syntax::is_identifier_text_for_target(name, script_target) {
        return create_identifier(arena, target, name);
    }
    if !string_named && !method_named_new && crate::evaluate::is_numeric_literal_name(name) {
        let value = crate::evaluate::js_string_to_number(name);
        if value >= 0.0 {
            return create_numeric_literal(arena, target, value);
        }
    }
    create_string_literal(arena, target, name, single_quote)
}

/// tsc-port: getPropertyNameNodeForSymbol @6.0.3
/// tsc-hash: a64ea322c766c6a89edc618ddf69f81752dfc6283c09d92c1f4cfe969de2aa10
/// tsc-span: _tsc.js:53411-53425
pub(crate) fn chains_get_property_name_node_for_symbol(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    symbol: SymbolId,
) -> BuildResult<TransformNode> {
    if let Some(name) = cloned_hash_private_name(checker, arena, symbol)? {
        return Ok(name);
    }
    let declarations = checker.binder.symbol(symbol).declarations.clone();
    let name_type = checker.links.symbol(symbol).name_type;
    let name_type_flags = name_type.map(|name_type| checker.tables.flags_of(name_type));
    let string_named = !declarations.is_empty()
        && declarations
            .iter()
            .all(|&declaration| is_string_named(checker, declaration, name_type_flags));
    let single_quote = !declarations.is_empty()
        && declarations
            .iter()
            .all(|&declaration| is_single_quoted_string_named(checker, declaration));
    let is_method = checker.symbol_flags(symbol).intersects(SymbolFlags::METHOD);
    if let Some(name) = get_property_name_node_for_symbol_from_name_type(
        checker,
        arena,
        target,
        context,
        symbol,
        single_quote,
        string_named,
        is_method,
    )? {
        return Ok(name);
    }
    let raw_name =
        tsc_binder::unescape_leading_underscores(&checker.binder.symbol(symbol).escaped_name);
    create_property_name_for_identifier_or_literal(
        arena,
        target,
        raw_name,
        checker.options.emit_script_target(),
        single_quote,
        string_named,
        is_method,
    )
}

/// tsc-port: getPropertyNameNodeForSymbolFromNameType @6.0.3
/// tsc-hash: 06ac4eff5825901e06db1944b8f2232d9eaf4190791754bd7210495787a98049
/// tsc-span: _tsc.js:53426-53443
#[allow(clippy::too_many_arguments)]
fn get_property_name_node_for_symbol_from_name_type(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    symbol: SymbolId,
    single_quote: bool,
    string_named: bool,
    is_method: bool,
) -> BuildResult<Option<TransformNode>> {
    let Some(name_type) = checker.links.symbol(symbol).name_type else {
        return Ok(None);
    };
    let flags = checker.tables.flags_of(name_type);
    if flags.intersects(TypeFlags::STRING_LITERAL | TypeFlags::NUMBER_LITERAL) {
        let literal_type = checker.tables.type_of(name_type).data.clone();
        if let TypeData::Literal {
            value: LiteralValue::String(value),
        } = &literal_type
        {
            if value.to_utf8().is_none() {
                return arena
                    .factory()
                    .create_string_literal_from_code_units(target, value.units(), single_quote)
                    .map(Some)
                    .map_err(factory_error);
            }
        }
        let name = match &literal_type {
            TypeData::Literal {
                value: LiteralValue::String(value),
            } => value.to_utf8().expect("lossless branch handled above"),
            TypeData::Literal {
                value: LiteralValue::Number(value),
            } => tsc_types::js_number_to_string(*value),
            _ => unreachable!("string/number literal flags carry literal data"),
        };
        if !tsc_syntax::is_identifier_text_for_target(&name, checker.options.emit_script_target())
            && (string_named || !crate::evaluate::is_numeric_literal_name(&name))
        {
            return create_string_literal(arena, target, &name, single_quote).map(Some);
        }
        if crate::evaluate::is_numeric_literal_name(&name) && name.starts_with('-') {
            let value = -crate::evaluate::js_string_to_number(&name);
            let literal = create_numeric_literal(arena, target, value)?;
            let prefix = create_node(
                arena,
                target,
                NodeData::PrefixUnaryExpression(PrefixUnaryExpressionData {
                    operator: SyntaxKind::MinusToken,
                    operand: Some(literal.node()),
                }),
            )?;
            return create_node(
                arena,
                target,
                NodeData::ComputedPropertyName(ComputedPropertyNameData {
                    expression: Some(prefix.node()),
                }),
            )
            .map(Some);
        }
        return create_property_name_for_identifier_or_literal(
            arena,
            target,
            &name,
            checker.options.emit_script_target(),
            single_quote,
            string_named,
            is_method,
        )
        .map(Some);
    }
    if flags.intersects(TypeFlags::UNIQUE_ES_SYMBOL) {
        let name_symbol = checker
            .tables
            .type_of(name_type)
            .symbol
            .expect("unique symbol type carries a symbol");
        let expression = chains_symbol_to_expression(
            checker,
            arena,
            target,
            context,
            name_symbol,
            EmitSymbolMeaning(SymbolFlags::VALUE.bits() as u32),
        )?;
        return create_node(
            arena,
            target,
            NodeData::ComputedPropertyName(ComputedPropertyNameData {
                expression: Some(expression.node()),
            }),
        )
        .map(Some);
    }
    Ok(None)
}

#[derive(Clone)]
pub(crate) struct ClonedNodeBuilderContextRestore {
    must_create_type_parameter_symbol_list: bool,
    must_create_type_parameters_names_lookups: bool,
    type_parameter_names: Option<HashMap<TypeId, TransformNode>>,
    type_parameter_names_by_text: Option<std::collections::HashSet<String>>,
    type_parameter_names_by_text_next_name_count: Option<HashMap<String, u32>>,
    type_parameter_symbol_list: Option<std::collections::HashSet<SymbolId>>,
}

/// tsc-port: cloneNodeBuilderContext @6.0.3
/// tsc-hash: 18ef4bc35335d2ff447f64ee1bffe8b29c89101b0b41a25825482c6ed2c1c24d
/// tsc-span: _tsc.js:53444-53461
pub(crate) fn clone_node_builder_context(
    context: &mut NodeBuilderContext<'_>,
) -> ClonedNodeBuilderContextRestore {
    let restore = ClonedNodeBuilderContextRestore {
        must_create_type_parameter_symbol_list: context.must_create_type_parameter_symbol_list,
        must_create_type_parameters_names_lookups: context
            .must_create_type_parameters_names_lookups,
        type_parameter_names: context.type_parameter_names.clone(),
        type_parameter_names_by_text: context.type_parameter_names_by_text.clone(),
        type_parameter_names_by_text_next_name_count: context
            .type_parameter_names_by_text_next_name_count
            .clone(),
        type_parameter_symbol_list: context.type_parameter_symbol_list.clone(),
    };
    context.must_create_type_parameter_symbol_list = true;
    context.must_create_type_parameters_names_lookups = true;
    restore
}

/// tsrs-native: cloned-context restore (Rust borrow shape of upstream context mutation).
pub(crate) fn restore_cloned_node_builder_context(
    context: &mut NodeBuilderContext<'_>,
    restore: ClonedNodeBuilderContextRestore,
) {
    context.type_parameter_names = restore.type_parameter_names;
    context.type_parameter_names_by_text = restore.type_parameter_names_by_text;
    context.type_parameter_names_by_text_next_name_count =
        restore.type_parameter_names_by_text_next_name_count;
    context.type_parameter_symbol_list = restore.type_parameter_symbol_list;
    context.must_create_type_parameter_symbol_list = restore.must_create_type_parameter_symbol_list;
    context.must_create_type_parameters_names_lookups =
        restore.must_create_type_parameters_names_lookups;
}

fn is_descendant_of(checker: &CheckerState<'_>, node: NodeId, ancestor: NodeId) -> bool {
    let mut current = Some(node);
    while let Some(node) = current {
        if node == ancestor {
            return true;
        }
        current = checker.parent_of(node);
    }
    false
}

/// tsc-port: getDeclarationWithTypeAnnotation @6.0.3
/// tsc-hash: 42754a319d127dbebf6a1b56e5ab5c06654aa5cae761236f85b61c860ec02690
/// tsc-span: _tsc.js:53462-53464
pub(crate) fn get_declaration_with_type_annotation(
    checker: &mut CheckerState<'_>,
    symbol: SymbolId,
    enclosing_declaration: Option<NodeId>,
    context: &NodeBuilderContext<'_>,
) -> BuildResult<Option<NodeId>> {
    let declarations = checker.binder.symbol(symbol).declarations.clone();
    for declaration in declarations {
        let annotation = checker
            .emit_nonlocal_effective_type_annotation_node(declaration)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        if annotation.is_some()
            && enclosing_declaration
                .map(|enclosing| is_descendant_of(checker, declaration, enclosing))
                .unwrap_or(true)
        {
            return Ok(Some(declaration));
        }
    }
    Ok(None)
}

/// tsc-port: existingTypeNodeIsNotReferenceOrIsReferenceWithCompatibleTypeArgumentCount @6.0.3
/// tsc-hash: e6e9d95bc3cc72529b38bf0e3e6823ba285163f447e4ec1d515f76718799e1b0
/// tsc-span: _tsc.js:53465-53473
pub(crate) fn existing_type_node_is_not_reference_or_is_reference_with_compatible_type_argument_count(
    checker: &mut CheckerState<'_>,
    existing: NodeId,
    r#type: TypeId,
    context: &NodeBuilderContext<'_>,
) -> BuildResult<bool> {
    if !checker
        .tables
        .object_flags_of(r#type)
        .intersects(ObjectFlags::REFERENCE)
        || checker.kind_of(existing) != SyntaxKind::TypeReference
    {
        return Ok(true);
    }
    checker
        .get_type_from_type_node(existing)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    let symbol = checker.links.node(existing).resolved_symbol.resolved();
    let Some(symbol) = symbol else {
        return Ok(true);
    };
    let existing_target = checker
        .get_declared_type_of_symbol_slice(symbol)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    let target = checker.tables.reference_target(r#type);
    if existing_target != target {
        return Ok(true);
    }
    let parameters = match &checker.tables.type_of(target).data {
        TypeData::GenericType {
            type_parameters, ..
        } => Some(type_parameters.as_ref()),
        _ => None,
    };
    let existing_count = match checker.data_of(existing) {
        NodeData::TypeReference(data) => checker.nodes_of(data.type_arguments).len(),
        _ => 0,
    };
    Ok(existing_count >= checker.get_min_type_argument_count(parameters))
}

/// tsc-port: getEnclosingDeclarationIgnoringFakeScope @6.0.3
/// tsc-hash: eb482aa2f143915e72b4bd2e78eb3b2561479fc0def67c5f1f6c641e18d45dc9
/// tsc-span: _tsc.js:53474-53479
pub(crate) const fn get_enclosing_declaration_ignoring_fake_scope(
    enclosing_declaration: NodeId,
) -> NodeId {
    // Rust's checker accepts only parse-tree NodeIds here. Upstream's
    // fakeScopeForSignatureDeclaration exists solely on synthesized service
    // nodes, so the parse-only representation makes the loop an identity.
    enclosing_declaration
}

/// tsc-port: serializeInferredTypeForDeclaration @6.0.3
/// tsc-hash: e3ed2326a64d3e4ec629c0017c1b0d48314418fb49326203f4cbdea1d9ed9f69
/// tsc-span: _tsc.js:53480-53486
pub(crate) fn serialize_inferred_type_for_declaration(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    symbol: SymbolId,
    context: &mut NodeBuilderContext<'_>,
    r#type: TypeId,
) -> BuildResult<Option<TransformNode>> {
    let ty = checker.tables.type_of(r#type);
    if ty.flags.intersects(TypeFlags::UNIQUE_ES_SYMBOL)
        && ty.symbol == Some(symbol)
        && (context.enclosing_declaration.is_none()
            || checker
                .binder
                .symbol(symbol)
                .declarations
                .iter()
                .any(|&declaration| {
                    Some(checker.binder.source_of_node(declaration).root) == context.enclosing_file
                }))
    {
        context.flags.0 |= ALLOW_UNIQUE_ES_SYMBOL_TYPE;
    }
    type_to_type_node_helper(checker, arena, target, r#type, context)
}

/// tsc-port: getTypeFromTypeNode2 @6.0.3
/// tsc-hash: f9c9056ae0674444ccf84bfde2d55862444d1927b02a1344a328e44ed99e2e5f
/// tsc-span: _tsc.js:51096-51101
pub(crate) fn get_type_from_type_node2(
    checker: &mut CheckerState<'_>,
    context: &NodeBuilderContext<'_>,
    node: NodeId,
    no_mapped_types: bool,
) -> BuildResult<Option<TypeId>> {
    let r#type = checker
        .get_type_from_type_node(node)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    let Some(mapper) = context.mapper else {
        return Ok(Some(r#type));
    };
    let mapped = checker
        .instantiate_type(r#type, Some(mapper))
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    Ok((!no_mapped_types || mapped == r#type).then_some(mapped))
}

fn source_matches_enclosing_file(
    checker: &CheckerState<'_>,
    arena: &TransformArena,
    context: &NodeBuilderContext<'_>,
    node: TransformNode,
) -> BuildResult<bool> {
    let Some(enclosing_file) = context.enclosing_file else {
        return Ok(false);
    };
    let enclosing_index = checker.binder.file_index_of_node(enclosing_file);
    let enclosing_source = program_source_id(checker, enclosing_index);
    let source = arena.source(node.source()).map_err(factory_error)?;
    Ok(source.program_source() == Some(enclosing_source))
}

fn node_is_synthesized(arena: &TransformArena, node: TransformNode) -> BuildResult<bool> {
    let source = arena.source(node.source()).map_err(factory_error)?;
    let record = arena.node(node).map_err(factory_error)?;
    let range = SourceRange::from_raw(record.pos, record.end, source.syntax().positions())
        .map_err(|error| {
            factory_error(tsc_emitter::TransformError::InvalidSourceRange { node, error })
        })?;
    Ok(matches!(range, SourceRange::Synthesized))
}

/// The original link is written before the text range. `set_original_node`
/// merges source metadata, so reversing these operations can change the
/// provenance ultimately observed by `parse_tree_resolver_node`.
///
/// tsc-port: setTextRange2 @6.0.3
/// tsc-hash: 07b8fa38d2f39bb231e53741f7b348d56af10b2d2a73fe778b415eb2622c5c00
/// tsc-span: _tsc.js:51102-51121
pub(crate) fn set_text_range2(
    checker: &CheckerState<'_>,
    arena: &mut TransformArena,
    context: &NodeBuilderContext<'_>,
    mut range: TransformNode,
    location: Option<TransformNode>,
) -> BuildResult<TransformNode> {
    let original = arena.get_original_node(range);
    let flags = NodeFlags::from_bits(arena.node(range).map_err(factory_error)?.flags);
    if !node_is_synthesized(arena, range)?
        || !flags.contains(NodeFlags::SYNTHESIZED)
        || !source_matches_enclosing_file(checker, arena, context, original)?
    {
        range = arena.factory().clone_node(range).map_err(factory_error)?;
    }
    if location == Some(range) {
        return Ok(range);
    }
    let Some(location) = location else {
        return Ok(range);
    };
    let mut original = arena
        .metadata(range)
        .and_then(|metadata| metadata.original());
    let mut contains_location = false;
    let mut seen = HashSet::new();
    while let Some(node) = original {
        if node == location {
            contains_location = true;
            break;
        }
        if !seen.insert(node) {
            break;
        }
        original = arena
            .metadata(node)
            .and_then(|metadata| metadata.original());
    }
    if !contains_location {
        arena
            .set_original_node(range, Some(location))
            .map_err(factory_error)?;
    }
    let location_original = arena.get_original_node(location);
    if source_matches_enclosing_file(checker, arena, context, location_original)? {
        return arena
            .factory()
            .set_text_range(range, location)
            .map_err(factory_error);
    }
    Ok(range)
}

/// tsc-port: symbolToNode @6.0.3
/// tsc-hash: ba015cf97ede8e4493cf851a6464d86bfd06e225e4fed66cae72ea6a2d91ff41
/// tsc-span: _tsc.js:51122-51135
pub(crate) fn symbol_to_node(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    symbol: SymbolId,
    meaning: EmitSymbolMeaning,
) -> BuildResult<TransformNode> {
    if context
        .internal_flags
        .contains(EmitInternalNodeBuilderFlags::WRITE_COMPUTED_PROPS)
    {
        if let Some(value_declaration) = checker.binder.symbol(symbol).value_declaration {
            if let Some(name) = node_util::get_name_of_declaration(
                checker.binder.source_of_node(value_declaration),
                value_declaration,
            ) {
                if checker.kind_of(name) == SyntaxKind::ComputedPropertyName {
                    if let Some(name) = project_parse_node(checker, arena, name)? {
                        return Ok(name);
                    }
                }
            }
        }
        if let Some(name_type) = checker.links.symbol(symbol).name_type {
            if checker
                .tables
                .flags_of(name_type)
                .intersects(TypeFlags::ENUM_LITERAL | TypeFlags::UNIQUE_ES_SYMBOL)
            {
                let name_symbol = checker
                    .tables
                    .type_of(name_type)
                    .symbol
                    .expect("enum/unique literal name type carries a symbol");
                context.enclosing_declaration =
                    checker.binder.symbol(name_symbol).value_declaration;
                let expression = chains_symbol_to_expression(
                    checker,
                    arena,
                    target,
                    context,
                    name_symbol,
                    meaning,
                )?;
                return create_node(
                    arena,
                    target,
                    NodeData::ComputedPropertyName(ComputedPropertyNameData {
                        expression: Some(expression.node()),
                    }),
                );
            }
        }
    }
    chains_symbol_to_expression(checker, arena, target, context, symbol, meaning)
}

/// Isolated syntactic-builder consult route. It deliberately delegates any
/// replacement to lane H and returns `None` when upstream keeps the literal.
///
/// tsc-port: syntacticBuilderResolver.getModuleSpecifierOverride @6.0.3
/// tsc-hash: c89bdb40ce2b87864f72841c93665476d6772410d3d0b2b3671d0563412ae1f7
/// tsc-span: _tsc.js:50890-50928
pub(crate) fn get_module_specifier_override(
    checker: &mut CheckerState<'_>,
    arena: &TransformArena,
    context: &mut NodeBuilderContext<'_>,
    parent: TransformNode,
    literal: TransformNode,
) -> BuildResult<Option<String>> {
    let original_name = match &arena.node(literal).map_err(factory_error)?.data {
        NodeData::StringLiteral(data) => data.text.clone(),
        _ => return Ok(None),
    };
    let literal_file_differs = match arena
        .parse_tree_resolver_node(literal)
        .map_err(factory_error)?
    {
        Some(node) => context.enclosing_file.is_none_or(|file| {
            program_source_id(checker, checker.binder.file_index_of_node(file)) != node.source()
        }),
        None => true,
    };
    if !context.bundled && !literal_file_differs {
        return Ok(None);
    }
    let parent_parse = arena
        .parse_tree_resolver_node(parent)
        .map_err(factory_error)?;
    let node_symbol = match parent_parse {
        Some(parent) => checker
            .get_resolved_symbol(parent.node())
            .map_err(|abort| checker_abort_error(checker, context, abort))?,
        None => None,
    };
    let is_type_of = matches!(
        &arena.node(parent).map_err(factory_error)?.data,
        NodeData::ImportType(data) if data.is_type_of
    );
    let meaning = if is_type_of {
        EmitSymbolMeaning::VALUE_EXPORT_VALUE
    } else {
        EmitSymbolMeaning::TYPE
    };
    let mut parent_symbol = None;
    if let Some(symbol) = node_symbol {
        if let Some(enclosing) = context.enclosing_declaration {
            let accessible = checker
                .emit_is_symbol_accessible_with_enclosing_kind(
                    symbol,
                    enclosing,
                    context.enclosing_declaration_is_synthetic,
                    meaning,
                    false,
                )
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            if accessible.accessibility == tsc_emitter::EmitSymbolAccessibility::Accessible {
                parent_symbol =
                    lookup_symbol_chain(checker, None, None, context, symbol, meaning, true)?
                        .first()
                        .copied();
            }
        }
    }
    let fallback_module_symbol = match parent_parse {
        Some(parent) => checker
            .get_external_module_file_from_declaration(parent.node())
            .map_err(|abort| checker_abort_error(checker, context, abort))?
            .and_then(|file| checker.binder.node_symbol(file)),
        None => None,
    };
    let module_symbol = parent_symbol
        .filter(|&symbol| checker.symbol_has_external_module_declaration(symbol))
        .or(fallback_module_symbol);
    let name = match module_symbol {
        Some(module_symbol) => specifier_for_module_symbol(checker, context, module_symbol, None)?,
        None => original_name.clone(),
    };
    if name.contains("/node_modules/") {
        context.encountered_error = true;
        context.tracker.report_likely_unsafe_import_required_error(
            &mut context.reported_diagnostic,
            &name,
            node_symbol.map(|symbol| {
                tsc_binder::unescape_leading_underscores(
                    &checker.binder.symbol(symbol).escaped_name,
                )
            }),
        );
    }
    Ok((name != original_name).then_some(name))
}

#[cfg(test)]
#[path = "../../tests/unit/node_builder_chains/tests.rs"]
mod tests;

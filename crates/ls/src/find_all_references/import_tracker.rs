//! tsgo `ls/importTracker.go`: the imports of an exported symbol (direct
//! imports, the files that use the module through a namespace or a default
//! import, the names an import gives it) and the module references of a
//! module (imports and `<reference>` directives).

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use tsc_binder::node_util;
use tsc_checker::state::CheckResult;
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{InternalSymbolName, ModifierFlags, SymbolFlags, SymbolId};

use crate::references::{collect_external_module_references, statements};
use crate::request::Request;
use crate::utilities::Touching;

/// tsgo `ImpExpKind`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ImpExpKind {
    Unknown,
    Import,
    Export,
}

/// tsgo `ExportKind`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ExportKind {
    Named,
    Default,
    ExportEquals,
}

/// tsgo `ExportInfo`.
#[derive(Clone, Copy, Debug)]
pub(super) struct ExportInfo {
    pub(super) exporting_module_symbol: SymbolId,
    pub(super) export_kind: ExportKind,
}

/// tsgo `ImportExportSymbol`.
pub(super) struct ImportExportSymbol {
    pub(super) kind: ImpExpKind,
    pub(super) symbol: SymbolId,
    pub(super) export_info: Option<ExportInfo>,
}

/// An import's name for an export, with its symbol (tsgo `importSearch`).
pub(super) type ImportSearch = (NodeId, SymbolId);

/// tsgo `ImportsResult`.
#[derive(Default)]
pub(super) struct ImportsResult {
    /// The names an import gives the export, with their symbols.
    pub(super) import_searches: Vec<ImportSearch>,
    pub(super) single_references: Vec<NodeId>,
    /// The files that may use the export without importing its name.
    pub(super) indirect_users: Vec<usize>,
}

/// tsgo `ModuleReference`.
pub(super) enum ModuleReference {
    /// A module specifier.
    Import { literal: NodeId },
    /// A program's synthesized import of the JSX runtime or the import
    /// helpers into a file.
    Implicit { file: usize, specifier: String },
    /// A `<reference>` directive's range in the referencing file.
    Reference { file: usize, range: Range<usize> },
}

/// tsgo `createImportTracker`: the map of each module symbol to the import
/// statements that reference it, made on first use.
pub(super) struct ImportTracker {
    all_direct_imports: HashMap<SymbolId, Vec<NodeId>>,
}

impl ImportTracker {
    /// tsgo `getDirectImportsMap`.
    pub(super) fn new(request: &mut Request<'_, '_>, files: &[usize]) -> CheckResult<Self> {
        let mut all_direct_imports: HashMap<SymbolId, Vec<NodeId>> = HashMap::new();
        for &file in files {
            for (import, module_specifier) in for_each_import(request, file) {
                if let Some(module_symbol) =
                    request.state.get_symbol_at_location(module_specifier)?
                {
                    all_direct_imports
                        .entry(module_symbol)
                        .or_default()
                        .push(import);
                }
            }
        }
        Ok(Self { all_direct_imports })
    }

    /// The tracker's answer for an export: tsgo `getImportersForExport`
    /// and `getSearchesFromDirectImports`.
    pub(super) fn imports(
        &self,
        request: &mut Request<'_, '_>,
        files: &[usize],
        file_set: &HashSet<usize>,
        export_symbol: SymbolId,
        export_info: ExportInfo,
        is_for_rename: bool,
    ) -> CheckResult<ImportsResult> {
        let (direct_imports, indirect_users) =
            Importers::find(self, request, files, file_set, export_info)?;
        let (import_searches, single_references) = get_searches_from_direct_imports(
            request,
            &direct_imports,
            export_symbol,
            export_info.export_kind,
            is_for_rename,
        )?;
        Ok(ImportsResult {
            import_searches,
            single_references,
            indirect_users,
        })
    }
}

/// tsgo `forEachImport`: each import, re-export or `require()` of a file
/// with its module specifier.
pub(super) fn for_each_import(request: &Request<'_, '_>, file: usize) -> Vec<(NodeId, NodeId)> {
    let source = request.state.binder.source(file);
    let imports = collect_external_module_references(source).imports;
    // The implicit imports follow (`implicit_imports`); they import nothing
    // a search reads.
    if source.external_module_indicator.is_some()
        || !imports.is_empty()
        || !implicit_imports(request, file).is_empty()
    {
        return imports
            .into_iter()
            .filter_map(|literal| {
                try_get_import_from_module_specifier(request, literal)
                    .map(|import| (import, literal))
            })
            .collect();
    }
    let mut result = Vec::new();
    for_each_possible_import_or_export_statement(request, source.root, &mut |statement| {
        match &request.state.binder.node_record(statement).data {
            NodeData::ExportDeclaration(data) => {
                if let Some(specifier) = data.module_specifier.filter(|&specifier| {
                    request.state.binder.node_record(specifier).kind == SyntaxKind::StringLiteral
                }) {
                    result.push((statement, specifier));
                }
            }
            NodeData::ImportDeclaration(data) => {
                if let Some(specifier) = data.module_specifier.filter(|&specifier| {
                    request.state.binder.node_record(specifier).kind == SyntaxKind::StringLiteral
                }) {
                    result.push((statement, specifier));
                }
            }
            NodeData::ImportEqualsDeclaration(_) => {
                if let Some(expression) =
                    external_module_import_equals_expression(request, statement)
                {
                    result.push((statement, expression));
                }
            }
            _ => {}
        }
        false
    });
    result
}

/// The specifiers of the imports the program synthesizes into a file, in
/// tsgo `forEachImport`'s order: the JSX runtime's, then the import
/// helpers' (`tslib`).
pub(super) fn implicit_imports(request: &Request<'_, '_>, file: usize) -> Vec<String> {
    let source = request.state.binder.source(file);
    let synthetic = tsc_program::synthetic_imports(
        source,
        matches!(
            source.script_kind,
            tsc_syntax::ScriptKind::Js | tsc_syntax::ScriptKind::Jsx
        ),
        source.script_kind == tsc_syntax::ScriptKind::Tsx,
        request.state.options,
    );
    let mut specifiers = Vec::new();
    if let Some(jsx_runtime) = synthetic.jsx_runtime {
        specifiers.push(jsx_runtime.to_string_lossy().into_owned());
    }
    if synthetic.import_helpers {
        specifiers.push("tslib".to_owned());
    }
    specifiers
}

/// tsgo `ast.TryGetImportFromModuleSpecifier` (a JSDoc `@import` tag is
/// tsgo's reparsed import declaration).
pub(super) fn try_get_import_from_module_specifier(
    request: &Request<'_, '_>,
    literal: NodeId,
) -> Option<NodeId> {
    let parent = request.state.binder.node_record(literal).parent?;
    match request.state.binder.node_record(parent).kind {
        SyntaxKind::ImportDeclaration
        | SyntaxKind::ExportDeclaration
        | SyntaxKind::JSDocImportTag => Some(parent),
        SyntaxKind::ExternalModuleReference => request.state.binder.node_record(parent).parent,
        SyntaxKind::CallExpression => {
            let source = request.state.binder.source_of_node(parent);
            (request.state.is_import_call(parent)
                || crate::utilities::is_require_call(source, parent, false))
            .then_some(parent)
        }
        SyntaxKind::LiteralType => {
            if request.state.binder.node_record(literal).kind != SyntaxKind::StringLiteral {
                return None;
            }
            let grandparent = request.state.binder.node_record(parent).parent?;
            (request.state.binder.node_record(grandparent).kind == SyntaxKind::ImportType)
                .then_some(grandparent)
        }
        _ => None,
    }
}

/// tsgo `forEachPossibleImportOrExportStatement`: the statements of a file
/// or an ambient module, into its ambient modules.
fn for_each_possible_import_or_export_statement(
    request: &Request<'_, '_>,
    source_file_like: NodeId,
    action: &mut dyn FnMut(NodeId) -> bool,
) -> bool {
    for statement in statements_of_source_file_like(request, source_file_like) {
        if action(statement)
            || is_ambient_module_declaration(request, statement)
                && for_each_possible_import_or_export_statement(request, statement, action)
        {
            return true;
        }
    }
    false
}

/// tsgo `getStatementsOfSourceFileLike`.
fn statements_of_source_file_like(request: &Request<'_, '_>, node: NodeId) -> Vec<NodeId> {
    let source = request.state.binder.source_of_node(node);
    match &source.arena.node(node).data {
        NodeData::SourceFile(_) => statements(source, node).to_vec(),
        NodeData::ModuleDeclaration(data) => data
            .body
            .filter(|&body| source.arena.node(body).kind == SyntaxKind::ModuleBlock)
            .map(|body| statements(source, body).to_vec())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// tsgo `isAmbientModuleDeclaration`: a module declaration named by a
/// string.
pub(super) fn is_ambient_module_declaration(request: &Request<'_, '_>, node: NodeId) -> bool {
    match &request.state.binder.node_record(node).data {
        NodeData::ModuleDeclaration(data) => data.name.is_some_and(|name| {
            request.state.binder.node_record(name).kind == SyntaxKind::StringLiteral
        }),
        _ => false,
    }
}

/// tsgo `getSourceFileLikeForImportDeclaration`: the file or ambient module
/// an import is in.
fn get_source_file_like_for_import_declaration(request: &Request<'_, '_>, node: NodeId) -> NodeId {
    let source = request.state.binder.source_of_node(node);
    let kind = request.state.binder.node_record(node).kind;
    if matches!(
        kind,
        SyntaxKind::CallExpression | SyntaxKind::JSDocImportTag
    ) {
        return source.root;
    }
    let parent = request
        .state
        .binder
        .node_record(node)
        .parent
        .expect("an import declaration's parent");
    if request.state.binder.node_record(parent).kind == SyntaxKind::SourceFile {
        return parent;
    }
    request
        .state
        .binder
        .node_record(parent)
        .parent
        .expect("an ambient module's block")
}

/// tsgo `isExternalModuleImportEquals`: `import x = require("m")`'s module
/// specifier.
pub(super) fn external_module_import_equals_expression(
    request: &Request<'_, '_>,
    node: NodeId,
) -> Option<NodeId> {
    let NodeData::ImportEqualsDeclaration(data) = &request.state.binder.node_record(node).data
    else {
        return None;
    };
    let reference = data.module_reference?;
    let NodeData::ExternalModuleReference(reference) =
        &request.state.binder.node_record(reference).data
    else {
        return None;
    };
    reference.expression.filter(|&expression| {
        request.state.binder.node_record(expression).kind == SyntaxKind::StringLiteral
    })
}

/// tsgo `isSourceFileWithGlobalExports`: a file with `export as namespace`.
pub(super) fn is_source_file_with_global_exports(
    request: &Request<'_, '_>,
    node: Option<NodeId>,
) -> bool {
    let Some(node) = node else {
        return false;
    };
    if request.state.binder.node_record(node).kind != SyntaxKind::SourceFile {
        return false;
    }
    request
        .state
        .binder
        .node_symbol(node)
        .is_some_and(|symbol| {
            !request
                .state
                .binder
                .symbol(symbol)
                .extras()
                .global_exports
                .is_empty()
        })
}

/// tsgo `getImportersForExport`: the direct imports of the exporting module
/// and the files that use it indirectly.
struct Importers<'t> {
    tracker: &'t ImportTracker,
    export_info: ExportInfo,
    is_available_through_global: bool,
    direct_imports: Vec<NodeId>,
    indirect_user_declarations: Vec<NodeId>,
    seen_direct_imports: HashSet<NodeId>,
    seen_indirect_users: HashSet<NodeId>,
}

impl<'t> Importers<'t> {
    fn find(
        tracker: &'t ImportTracker,
        request: &mut Request<'_, '_>,
        files: &[usize],
        file_set: &HashSet<usize>,
        export_info: ExportInfo,
    ) -> CheckResult<(Vec<NodeId>, Vec<usize>)> {
        let value_declaration = request
            .state
            .binder
            .symbol(export_info.exporting_module_symbol)
            .value_declaration;
        let mut importers = Self {
            tracker,
            export_info,
            is_available_through_global: is_source_file_with_global_exports(
                request,
                value_declaration,
            ),
            direct_imports: Vec::new(),
            indirect_user_declarations: Vec::new(),
            seen_direct_imports: HashSet::new(),
            seen_indirect_users: HashSet::new(),
        };
        importers.handle_direct_imports(request, export_info.exporting_module_symbol)?;
        let indirect_users = importers.get_indirect_users(request, files, file_set)?;
        Ok((importers.direct_imports, indirect_users))
    }

    fn get_direct_imports(&self, module_symbol: SymbolId) -> Vec<NodeId> {
        self.tracker
            .all_direct_imports
            .get(&module_symbol)
            .cloned()
            .unwrap_or_default()
    }

    /// Adds a module and all of its transitive dependencies as possible indirect users
    fn add_indirect_user(
        &mut self,
        request: &mut Request<'_, '_>,
        source_file_like: NodeId,
        add_transitive_dependencies: bool,
    ) -> CheckResult<()> {
        // When isAvailableThroughGlobal, getIndirectUsers already returns all source files,
        // so indirectUserDeclarations is never consulted. Nothing to do here.
        if self.is_available_through_global || !self.seen_indirect_users.insert(source_file_like) {
            return Ok(());
        }
        self.indirect_user_declarations.push(source_file_like);
        if !add_transitive_dependencies {
            return Ok(());
        }
        let Some(symbol) = request.state.binder.node_symbol(source_file_like) else {
            return Ok(());
        };
        let module_symbol = request.state.get_merged_symbol(symbol);
        for direct_import in self.get_direct_imports(module_symbol) {
            if request.state.binder.node_record(direct_import).kind != SyntaxKind::ImportType {
                let source_file_like =
                    get_source_file_like_for_import_declaration(request, direct_import);
                self.add_indirect_user(request, source_file_like, true)?;
            }
        }
        Ok(())
    }

    fn is_exported(request: &Request<'_, '_>, node: NodeId, stop_at_ambient_module: bool) -> bool {
        let mut current = Some(node);
        while let Some(node) = current {
            if stop_at_ambient_module && is_ambient_module_declaration(request, node) {
                return false;
            }
            if node_util::has_syntactic_modifier(
                request.state.binder.source_of_node(node),
                node,
                ModifierFlags::EXPORT,
            ) {
                return true;
            }
            current = request.state.binder.node_record(node).parent;
        }
        false
    }

    fn handle_import_call(
        &mut self,
        request: &mut Request<'_, '_>,
        import_call: NodeId,
    ) -> CheckResult<()> {
        let mut top = Some(import_call);
        while let Some(node) = top {
            if is_ambient_module_declaration(request, node) {
                break;
            }
            top = request.state.binder.node_record(node).parent;
        }
        let top = top.unwrap_or(request.state.binder.source_of_node(import_call).root);
        let exported = Self::is_exported(request, import_call, true);
        self.add_indirect_user(request, top, exported)
    }

    fn handle_namespace_import(
        &mut self,
        request: &mut Request<'_, '_>,
        import_declaration: NodeId,
        name: Option<NodeId>,
        is_re_export: bool,
        already_added_direct: bool,
    ) -> CheckResult<()> {
        if self.export_info.export_kind == ExportKind::ExportEquals {
            // This is a direct import, not import-as-namespace.
            if !already_added_direct {
                self.direct_imports.push(import_declaration);
            }
        } else if !self.is_available_through_global {
            let source_file_like =
                get_source_file_like_for_import_declaration(request, import_declaration);
            let re_exported = is_re_export
                || match name {
                    Some(name) => find_namespace_re_exports(request, source_file_like, name)?,
                    None => false,
                };
            self.add_indirect_user(request, source_file_like, re_exported)?;
        }
        Ok(())
    }

    fn handle_direct_imports(
        &mut self,
        request: &mut Request<'_, '_>,
        exporting_module_symbol: SymbolId,
    ) -> CheckResult<()> {
        for direct in self.get_direct_imports(exporting_module_symbol) {
            if !self.seen_direct_imports.insert(direct) {
                continue;
            }
            let source = request.state.binder.source_of_node(direct);
            match request.state.binder.node_record(direct).data.clone() {
                NodeData::CallExpression(_) => {
                    if request.state.is_import_call(direct) {
                        self.handle_import_call(request, direct)?;
                    } else if !self.is_available_through_global {
                        let parent = request.state.binder.node_record(direct).parent;
                        if self.export_info.export_kind == ExportKind::ExportEquals {
                            if let Some(NodeData::VariableDeclaration(variable)) =
                                parent.map(|parent| &request.state.binder.node_record(parent).data)
                            {
                                if let Some(name) = variable.name.filter(|&name| {
                                    request.state.binder.node_record(name).kind
                                        == SyntaxKind::Identifier
                                }) {
                                    self.direct_imports.push(name);
                                }
                            }
                        }
                    }
                }
                NodeData::Identifier(_) => {}
                NodeData::ImportEqualsDeclaration(data) => {
                    let exported =
                        node_util::has_syntactic_modifier(source, direct, ModifierFlags::EXPORT);
                    self.handle_namespace_import(request, direct, data.name, exported, false)?;
                }
                data @ (NodeData::ImportDeclaration(_) | NodeData::JSDocImportTag(_)) => {
                    let import_clause = match data {
                        NodeData::ImportDeclaration(data) => data.import_clause,
                        NodeData::JSDocImportTag(data) => data.import_clause,
                        _ => None,
                    };
                    self.direct_imports.push(direct);
                    if let Some(NodeData::ImportClause(clause)) = import_clause
                        .map(|clause| request.state.binder.node_record(clause).data.clone())
                    {
                        if let Some(NodeData::NamespaceImport(namespace)) = clause
                            .named_bindings
                            .map(|bindings| request.state.binder.node_record(bindings).data.clone())
                        {
                            self.handle_namespace_import(
                                request,
                                direct,
                                namespace.name,
                                false,
                                true,
                            )?;
                            continue;
                        }
                        if !self.is_available_through_global && clause.name.is_some() {
                            let source_file_like =
                                get_source_file_like_for_import_declaration(request, direct);
                            self.add_indirect_user(request, source_file_like, false)?;
                            // Add a check for indirect uses to handle synthetic default imports
                        }
                    }
                }
                NodeData::ExportDeclaration(data) => match data.export_clause {
                    None => {
                        // This is `export * from "foo"`, so imports of this module may import the export too.
                        let source_file_like =
                            get_source_file_like_for_import_declaration(request, direct);
                        if let Some(symbol) = request.state.binder.node_symbol(source_file_like) {
                            let module_symbol = request.state.get_merged_symbol(symbol);
                            self.handle_direct_imports(request, module_symbol)?;
                        }
                    }
                    Some(clause)
                        if request.state.binder.node_record(clause).kind
                            == SyntaxKind::NamespaceExport =>
                    {
                        // `export * as foo from "foo"` add to indirect uses
                        let source_file_like =
                            get_source_file_like_for_import_declaration(request, direct);
                        self.add_indirect_user(request, source_file_like, true)?;
                    }
                    Some(_) => {
                        // This is `export { foo } from "foo"` and creates an alias symbol, so recursive search will get handle re-exports.
                        self.direct_imports.push(direct);
                    }
                },
                NodeData::ImportType(data) => {
                    // Only check for typeof import('xyz')
                    if !self.is_available_through_global
                        && data.is_type_of
                        && data.qualifier.is_none()
                        && Self::is_exported(request, direct, false)
                    {
                        let root = request.state.binder.source_of_node(direct).root;
                        self.add_indirect_user(request, root, true)?;
                    }
                    self.direct_imports.push(direct);
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn get_indirect_users(
        &mut self,
        request: &mut Request<'_, '_>,
        files: &[usize],
        file_set: &HashSet<usize>,
    ) -> CheckResult<Vec<usize>> {
        if self.is_available_through_global {
            // It has `export as namespace`, so anything could potentially use it.
            return Ok(files.to_vec());
        }
        // Module augmentations may use this module's exports without importing it.
        for declaration in request.declarations_of(self.export_info.exporting_module_symbol) {
            let source = request.state.binder.source_of_node(declaration);
            if node_util::is_ambient_module(source, declaration)
                && node_util::is_module_augmentation_external(source, declaration)
                && file_set.contains(&request.state.binder.file_index_of_node(declaration))
            {
                self.add_indirect_user(request, declaration, false)?;
            }
        }
        // This may return duplicates (if there are multiple module declarations in a single source file, all importing the same thing as a namespace), but `State.markSearchedSymbol` will handle that.
        Ok(self
            .indirect_user_declarations
            .iter()
            .map(|&declaration| request.state.binder.file_index_of_node(declaration))
            .collect())
    }
}

/// tsgo `findNamespaceReExports`: whether the namespace import `name` is
/// re-exported from the file (or module).
fn find_namespace_re_exports(
    request: &mut Request<'_, '_>,
    source_file_like: NodeId,
    name: NodeId,
) -> CheckResult<bool> {
    let namespace_import_symbol = request.state.get_symbol_at_location(name)?;
    let mut exports = Vec::new();
    for_each_possible_import_or_export_statement(request, source_file_like, &mut |statement| {
        if let NodeData::ExportDeclaration(data) = &request.state.binder.node_record(statement).data
        {
            if data.module_specifier.is_none() {
                if let Some(NodeData::NamedExports(named)) = data
                    .export_clause
                    .map(|clause| &request.state.binder.node_record(clause).data)
                {
                    if let Some(elements) = named.elements {
                        exports.extend(
                            request
                                .state
                                .binder
                                .node_array(elements)
                                .nodes
                                .iter()
                                .copied(),
                        );
                    }
                }
            }
        }
        false
    });
    for element in exports {
        if request
            .state
            .get_export_specifier_local_target_symbol(element)?
            == namespace_import_symbol
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// tsgo `getSearchesFromDirectImports`.
fn get_searches_from_direct_imports(
    request: &mut Request<'_, '_>,
    direct_imports: &[NodeId],
    export_symbol: SymbolId,
    export_kind: ExportKind,
    is_for_rename: bool,
) -> CheckResult<(Vec<ImportSearch>, Vec<NodeId>)> {
    let mut import_searches = Vec::new();
    let mut single_references = Vec::new();
    let export_name = symbol_name(request, export_symbol);
    // Use name of "default" even in `export =` case because we may have allowSyntheticDefaultImports
    let is_name_match = |name: &str| {
        name == export_name
            || export_kind != ExportKind::Named && name == InternalSymbolName::DEFAULT
    };
    for &declaration in direct_imports {
        let record = request.state.binder.node_record(declaration).clone();
        match &record.data {
            NodeData::ImportEqualsDeclaration(data) => {
                if external_module_import_equals_expression(request, declaration).is_some() {
                    if let Some(name) = data.name {
                        handle_namespace_import_like(
                            request,
                            name,
                            export_kind,
                            is_for_rename,
                            &is_name_match,
                            &mut import_searches,
                        )?;
                    }
                }
                continue;
            }
            NodeData::Identifier(_) => {
                handle_namespace_import_like(
                    request,
                    declaration,
                    export_kind,
                    is_for_rename,
                    &is_name_match,
                    &mut import_searches,
                )?;
                continue;
            }
            NodeData::ImportType(data) => {
                if let Some(qualifier) = data.qualifier {
                    let first_identifier = first_identifier(request, qualifier);
                    if identifier_text(request, first_identifier)
                        == symbol_name(request, export_symbol)
                    {
                        single_references.push(first_identifier);
                    }
                } else if export_kind == ExportKind::ExportEquals {
                    if let Some(NodeData::LiteralType(literal)) = data
                        .argument
                        .map(|argument| &request.state.binder.node_record(argument).data)
                    {
                        single_references.extend(literal.literal);
                    }
                }
                continue;
            }
            _ => {}
        }
        // Ignore if there's a grammar error
        let (module_specifier, import_clause, export_clause) = match &record.data {
            NodeData::ExportDeclaration(data) => (data.module_specifier, None, data.export_clause),
            NodeData::ImportDeclaration(data) => (data.module_specifier, data.import_clause, None),
            NodeData::JSDocImportTag(data) => (data.module_specifier, data.import_clause, None),
            _ => continue,
        };
        if !module_specifier.is_some_and(|specifier| {
            request.state.binder.node_record(specifier).kind == SyntaxKind::StringLiteral
        }) {
            continue;
        }
        if record.kind == SyntaxKind::ExportDeclaration {
            if let Some(clause) = export_clause.filter(|&clause| {
                request.state.binder.node_record(clause).kind == SyntaxKind::NamedExports
            }) {
                search_for_named_import(
                    request,
                    clause,
                    export_symbol,
                    is_for_rename,
                    &is_name_match,
                    &mut import_searches,
                    &mut single_references,
                )?;
            }
            continue;
        }
        let Some(NodeData::ImportClause(clause)) =
            import_clause.map(|clause| request.state.binder.node_record(clause).data.clone())
        else {
            continue;
        };
        if let Some(named_bindings) = clause.named_bindings {
            match request
                .state
                .binder
                .node_record(named_bindings)
                .data
                .clone()
            {
                NodeData::NamespaceImport(namespace) => {
                    if let Some(name) = namespace.name {
                        handle_namespace_import_like(
                            request,
                            name,
                            export_kind,
                            is_for_rename,
                            &is_name_match,
                            &mut import_searches,
                        )?;
                    }
                }
                NodeData::NamedImports(_) => {
                    // 'default' might be accessed as a named import `{ default as foo }`.
                    if matches!(export_kind, ExportKind::Named | ExportKind::Default) {
                        search_for_named_import(
                            request,
                            named_bindings,
                            export_symbol,
                            is_for_rename,
                            &is_name_match,
                            &mut import_searches,
                            &mut single_references,
                        )?;
                    }
                }
                _ => {}
            }
        }
        // `export =` might be imported by a default import if `--allowSyntheticDefaultImports` is on, so this handles both ExportKind.Default and ExportKind.ExportEquals.
        // If a default import has the same name as the default export, allow to rename it.
        // Given `import f` and `export default function f`, we will rename both, but for `import g` we will rename just that.
        if let Some(name) = clause.name {
            if matches!(export_kind, ExportKind::Default | ExportKind::ExportEquals)
                && (!is_for_rename
                    || identifier_text(request, name)
                        == symbol_name_no_default(request, export_symbol))
            {
                if let Some(default_import_alias) = request.state.get_symbol_at_location(name)? {
                    import_searches.push((name, default_import_alias));
                }
            }
        }
    }
    Ok((import_searches, single_references))
}

/// `import x = require("./x")` or `import * as x from "./x"`.
/// An `export =` may be imported by this syntax, so it may be a direct import.
/// If it's not a direct import, it will be in `indirectUsers`, so we don't have to do anything here.
fn handle_namespace_import_like(
    request: &mut Request<'_, '_>,
    import_name: NodeId,
    export_kind: ExportKind,
    is_for_rename: bool,
    is_name_match: &dyn Fn(&str) -> bool,
    import_searches: &mut Vec<(NodeId, SymbolId)>,
) -> CheckResult<()> {
    // Don't rename an import that already has a different name than the export.
    if export_kind == ExportKind::ExportEquals
        && (!is_for_rename || is_name_match(&identifier_text(request, import_name)))
    {
        if let Some(symbol) = request.state.get_symbol_at_location(import_name)? {
            import_searches.push((import_name, symbol));
        }
    }
    Ok(())
}

fn search_for_named_import(
    request: &mut Request<'_, '_>,
    named_bindings: NodeId,
    export_symbol: SymbolId,
    is_for_rename: bool,
    is_name_match: &dyn Fn(&str) -> bool,
    import_searches: &mut Vec<(NodeId, SymbolId)>,
    single_references: &mut Vec<NodeId>,
) -> CheckResult<()> {
    let elements = match &request.state.binder.node_record(named_bindings).data {
        NodeData::NamedImports(data) => data.elements,
        NodeData::NamedExports(data) => data.elements,
        _ => None,
    };
    let elements = elements
        .map(|elements| request.state.binder.node_array(elements).nodes.to_vec())
        .unwrap_or_default();
    for element in elements {
        let (name, property_name, is_export_specifier) =
            match &request.state.binder.node_record(element).data {
                NodeData::ImportSpecifier(data) => (data.name, data.property_name, false),
                NodeData::ExportSpecifier(data) => (data.name, data.property_name, true),
                _ => continue,
            };
        let Some(name) = name else {
            continue;
        };
        if !is_name_match(&module_export_name_text(
            request,
            property_name.unwrap_or(name),
        )) {
            continue;
        }
        if let Some(property_name) = property_name {
            // This is `import { foo as bar } from "./a"` or `export { foo as bar } from "./a"`. `foo` isn't a local in the file, so just add it as a single reference.
            single_references.push(property_name);
            // If renaming `{ foo as bar }`, don't touch `bar`, just `foo`.
            // But do rename `foo` in ` { default as foo }` if that's the original export name.
            if !is_for_rename
                || module_export_name_text(request, name) == symbol_name(request, export_symbol)
            {
                // Search locally for `bar`.
                if let Some(symbol) = request.state.get_symbol_at_location(name)? {
                    import_searches.push((name, symbol));
                }
            }
        } else {
            let local_symbol = if is_export_specifier && property_name.is_some() {
                request
                    .state
                    .get_export_specifier_local_target_symbol(element)?
            } else {
                request.state.get_symbol_at_location(name)?
            };
            if let Some(local_symbol) = local_symbol {
                import_searches.push((name, local_symbol));
            }
        }
    }
    Ok(())
}

/// tsgo `getImportOrExportSymbol`: the import or export a reference is at.
pub(super) fn get_import_or_export_symbol(
    request: &mut Request<'_, '_>,
    node: Touching,
    symbol: SymbolId,
    coming_from_export: bool,
) -> CheckResult<Option<ImportExportSymbol>> {
    if let Some(result) = get_export(request, node, symbol, coming_from_export)? {
        return Ok(Some(result));
    }
    if coming_from_export {
        return Ok(None);
    }
    get_import(request, node, symbol)
}

fn export_info_of(
    request: &mut Request<'_, '_>,
    symbol: SymbolId,
    kind: ExportKind,
) -> Option<ImportExportSymbol> {
    get_export_info(request, symbol, kind).map(|export_info| ImportExportSymbol {
        kind: ImpExpKind::Export,
        symbol,
        export_info: Some(export_info),
    })
}

fn get_export(
    request: &mut Request<'_, '_>,
    node: Touching,
    symbol: SymbolId,
    coming_from_export: bool,
) -> CheckResult<Option<ImportExportSymbol>> {
    let Some(parent) = request.parent(node) else {
        return Ok(None);
    };
    let grandparent = request.state.binder.node_record(parent).parent;
    // Not meant for use with export specifiers or export assignment.
    let export_kind_for_declaration = |request: &Request<'_, '_>, node: NodeId| {
        if node_util::has_syntactic_modifier(
            request.state.binder.source_of_node(node),
            node,
            ModifierFlags::DEFAULT,
        ) {
            ExportKind::Default
        } else {
            ExportKind::Named
        }
    };
    if let Some(export_symbol) = request.state.binder.symbol(symbol).export_symbol {
        if request.state.binder.node_record(parent).kind == SyntaxKind::PropertyAccessExpression {
            // When accessing an export of a JS module, there's no alias. The symbol will still be flagged as an export even though we're at the use.
            // So check that we are at the declaration.
            if let Some(grandparent) = grandparent.filter(|&grandparent| {
                request.state.binder.node_record(grandparent).kind == SyntaxKind::BinaryExpression
            }) {
                if request.declarations_of(symbol).contains(&parent) {
                    return Ok(get_special_property_export(
                        request,
                        grandparent,
                        symbol,
                        false,
                    ));
                }
            }
            return Ok(None);
        }
        let kind = export_kind_for_declaration(request, parent);
        return Ok(export_info_of(request, export_symbol, kind));
    }
    let export_node = get_export_node(request, parent, node);
    // tsgo's `IsImplicitlyExportedJSDocDeclaration` holds only for its
    // reparsed `@typedef` declarations; tsc-rs keeps the JSDoc tags, which
    // the `JSDocTypedefTag` case below exports alike.
    if let Some(export_node) = export_node.filter(|&export_node| {
        node_util::has_syntactic_modifier(
            request.state.binder.source_of_node(export_node),
            export_node,
            ModifierFlags::EXPORT,
        )
    }) {
        if let NodeData::ImportEqualsDeclaration(data) =
            &request.state.binder.node_record(export_node).data
        {
            if data.module_reference.map(Touching::Node) == Some(node) {
                // We're at `Y` in `export import X = Y`. This is not the exported symbol, the left-hand-side is. So treat this as an import statement.
                if coming_from_export {
                    return Ok(None);
                }
                let Some(name) = data.name else {
                    return Ok(None);
                };
                return Ok(request
                    .state
                    .get_symbol_at_location(name)?
                    .map(|lhs_symbol| ImportExportSymbol {
                        kind: ImpExpKind::Import,
                        symbol: lhs_symbol,
                        export_info: None,
                    }));
            }
        }
        let kind = export_kind_for_declaration(request, export_node);
        return Ok(export_info_of(request, symbol, kind));
    }
    let parent_kind = request.state.binder.node_record(parent).kind;
    let grandparent_kind =
        grandparent.map(|grandparent| request.state.binder.node_record(grandparent).kind);
    if parent_kind == SyntaxKind::NamespaceExport {
        return Ok(export_info_of(request, symbol, ExportKind::Named));
    }
    if parent_kind == SyntaxKind::ExportAssignment {
        return Ok(get_export_assignment_export(request, parent, symbol));
    }
    if grandparent_kind == Some(SyntaxKind::ExportAssignment) {
        return Ok(get_export_assignment_export(
            request,
            grandparent.expect("a grandparent"),
            symbol,
        ));
    }
    if parent_kind == SyntaxKind::BinaryExpression {
        return Ok(get_special_property_export(request, parent, symbol, true));
    }
    if grandparent_kind == Some(SyntaxKind::BinaryExpression) {
        return Ok(get_special_property_export(
            request,
            grandparent.expect("a grandparent"),
            symbol,
            true,
        ));
    }
    if matches!(
        parent_kind,
        SyntaxKind::JSDocTypedefTag | SyntaxKind::JSDocCallbackTag
    ) {
        return Ok(export_info_of(request, symbol, ExportKind::Named));
    }
    Ok(None)
}

/// tsgo's `getExportAssignmentExport`: the symbol for the `export =` node;
/// its parent is the module it's the export of.
fn get_export_assignment_export(
    request: &Request<'_, '_>,
    export_assignment: NodeId,
    symbol: SymbolId,
) -> Option<ImportExportSymbol> {
    let assignment_symbol = request.state.binder.node_symbol(export_assignment)?;
    let exporting_module_symbol = request.state.binder.symbol(assignment_symbol).parent?;
    let NodeData::ExportAssignment(data) =
        &request.state.binder.node_record(export_assignment).data
    else {
        return None;
    };
    Some(ImportExportSymbol {
        kind: ImpExpKind::Export,
        symbol,
        export_info: Some(ExportInfo {
            exporting_module_symbol,
            export_kind: if data.is_export_equals == Some(true) {
                ExportKind::ExportEquals
            } else {
                ExportKind::Default
            },
        }),
    })
}

/// tsgo's `getSpecialPropertyExport`: `exports.x =` (named) or
/// `module.exports =` (export equals).
fn get_special_property_export(
    request: &mut Request<'_, '_>,
    node: NodeId,
    symbol: SymbolId,
    use_lhs_symbol: bool,
) -> Option<ImportExportSymbol> {
    let source = request.state.binder.source_of_node(node);
    let kind = match tsc_binder::assignment::get_assignment_declaration_kind(source, node) {
        tsc_binder::assignment::AssignmentDeclarationKind::ExportsProperty => ExportKind::Named,
        tsc_binder::assignment::AssignmentDeclarationKind::ModuleExports => {
            ExportKind::ExportEquals
        }
        _ => return None,
    };
    let symbol = if use_lhs_symbol {
        request.state.binder.node_symbol(node)?
    } else {
        symbol
    };
    export_info_of(request, symbol, kind)
}

fn get_import(
    request: &mut Request<'_, '_>,
    node: Touching,
    symbol: SymbolId,
) -> CheckResult<Option<ImportExportSymbol>> {
    if !is_node_import(request, node) {
        return Ok(None);
    }
    // JS destructuring from `require(...)` is import-like for references, but the binding element
    // itself is still a local variable symbol rather than an alias.
    let imported_symbol = if request
        .state
        .binder
        .symbol(symbol)
        .flags
        .intersects(SymbolFlags::ALIAS)
    {
        request.state.get_immediate_aliased_symbol(symbol)?
    } else {
        super::get_property_symbol_of_object_binding_pattern_without_property_name(request, symbol)?
    };
    let Some(imported_symbol) = imported_symbol else {
        return Ok(None);
    };
    // Search on the local symbol in the exporting module, not the exported symbol.
    let Some(mut imported_symbol) = skip_export_specifier_symbol(request, imported_symbol)? else {
        return Ok(None);
    };
    // Similarly, skip past the symbol for 'export ='
    if request.state.binder.symbol(imported_symbol).escaped_name
        == InternalSymbolName::EXPORT_EQUALS
    {
        let Some(local) = get_export_equals_local_symbol(request, imported_symbol)? else {
            return Ok(None);
        };
        imported_symbol = local;
    }
    // If the import has a different name than the export, do not continue searching.
    // If `importedName` is undefined, do continue searching as the export is anonymous.
    // (All imports returned from this function will be ignored anyway if we are in rename and this is a not a named export.)
    let imported_name = symbol_name_no_default(request, imported_symbol);
    if imported_name.is_empty()
        || imported_name == InternalSymbolName::DEFAULT
        || imported_name == symbol_name(request, symbol)
    {
        return Ok(Some(ImportExportSymbol {
            kind: ImpExpKind::Import,
            symbol: imported_symbol,
            export_info: None,
        }));
    }
    Ok(None)
}

/// tsgo `getExportInfo`: the exporting module of an export at the top of an
/// external module.
pub(super) fn get_export_info(
    request: &mut Request<'_, '_>,
    export_symbol: SymbolId,
    export_kind: ExportKind,
) -> Option<ExportInfo> {
    // Parent can be nil if an `export` is not at the top-level (which is a compile error).
    let parent = request.state.binder.symbol(export_symbol).parent?;
    let exporting_module_symbol = request.state.get_merged_symbol(parent);
    // `export` may appear in a namespace. In that case, just rely on global search.
    is_external_module_symbol(request, exporting_module_symbol).then_some(ExportInfo {
        exporting_module_symbol,
        export_kind,
    })
}

/// tsgo `checker.IsExternalModuleSymbol`: a module named as an ambient
/// module (`ast.IsAmbientModuleSymbolName`).
pub(super) fn is_external_module_symbol(request: &Request<'_, '_>, symbol: SymbolId) -> bool {
    let data = request.state.binder.symbol(symbol);
    data.flags.intersects(SymbolFlags::MODULE)
        && data
            .escaped_name
            .as_str()
            .is_some_and(|name| try_get_ambient_module_name_from_symbol_name(name).is_some())
}

/// tsgo `ast.TryGetAmbientModuleNameFromSymbolName`: a quoted module name,
/// or a pattern module's (`__"name"pattern@N` in tsc-rs, whose internal
/// names start with `__`).
pub(super) fn try_get_ambient_module_name_from_symbol_name(name: &str) -> Option<String> {
    if name.len() >= 2 && name.starts_with('"') && name.ends_with('"') {
        return Some(name[1..name.len() - 1].to_owned());
    }
    let rest = name.strip_prefix("__\"")?;
    let marker_index = rest.rfind("\"pattern@")?;
    if marker_index < 1 {
        return None;
    }
    Some(rest[..marker_index].to_owned())
}

/// tsgo `getExportNode`: for a variable's name, its variable statement; for
/// another reference, its parent.
fn get_export_node(request: &Request<'_, '_>, parent: NodeId, node: Touching) -> Option<NodeId> {
    let source = request.state.binder.source_of_node(parent);
    let declaration = match request.state.binder.node_record(parent).kind {
        SyntaxKind::VariableDeclaration => Some(parent),
        SyntaxKind::BindingElement => {
            node_util::walk_up_binding_elements_and_patterns(source, parent)
        }
        _ => None,
    };
    let Some(declaration) = declaration else {
        return Some(parent);
    };
    let list = request.state.binder.node_record(declaration).parent?;
    let statement = request.state.binder.node_record(list).parent?;
    (request.name_of(parent).map(Touching::Node) == Some(node)
        && request.state.binder.node_record(list).kind != SyntaxKind::CatchClause
        && request.state.binder.node_record(statement).kind == SyntaxKind::VariableStatement)
        .then_some(statement)
}

/// tsgo `isNodeImport`.
fn is_node_import(request: &Request<'_, '_>, node: Touching) -> bool {
    let Some(parent) = request.parent(node) else {
        return false;
    };
    match &request.state.binder.node_record(parent).data {
        NodeData::ImportEqualsDeclaration(data) => {
            data.name.map(Touching::Node) == Some(node)
                && external_module_import_equals_expression(request, parent).is_some()
        }
        // For a rename import `{ foo as bar }`, don't search for the imported symbol. Just find local uses of `bar`.
        NodeData::ImportSpecifier(data) => data.property_name.is_none(),
        NodeData::ImportClause(_) | NodeData::NamespaceImport(_) => true,
        NodeData::BindingElement(_) => {
            let source = request.state.binder.source_of_node(parent);
            request.state.is_in_js_file(parent)
                && request
                    .state
                    .binder
                    .node_record(parent)
                    .parent
                    .and_then(|pattern| request.state.binder.node_record(pattern).parent)
                    .is_some_and(|declaration| {
                        crate::utilities::is_variable_declaration_initialized_to_bare_or_accessed_require(
                            source,
                            declaration,
                        )
                    })
        }
        _ => false,
    }
}

/// tsgo `skipExportSpecifierSymbol`: at an export specifier, the symbol it
/// refers to.
fn skip_export_specifier_symbol(
    request: &mut Request<'_, '_>,
    symbol: SymbolId,
) -> CheckResult<Option<SymbolId>> {
    // For `export { foo } from './bar", there's nothing to skip, because it does not create a new alias. But `export { foo } does.
    for declaration in request.declarations_of(symbol) {
        let source = request.state.binder.source_of_node(declaration);
        match &request.state.binder.node_record(declaration).data {
            NodeData::ExportSpecifier(data) => {
                let module_specifier = request
                    .state
                    .binder
                    .node_record(declaration)
                    .parent
                    .and_then(|exports| request.state.binder.node_record(exports).parent)
                    .and_then(
                        |export| match &request.state.binder.node_record(export).data {
                            NodeData::ExportDeclaration(export) => export.module_specifier,
                            _ => None,
                        },
                    );
                if data.property_name.is_none() && module_specifier.is_none() {
                    return Ok(Some(
                        request
                            .state
                            .get_export_specifier_local_target_symbol(declaration)?
                            .unwrap_or(symbol),
                    ));
                }
            }
            NodeData::PropertyAccessExpression(data) => {
                // Export of form 'module.exports.propName = expr';
                if data.expression.is_some_and(|expression| {
                    tsc_binder::assignment::is_module_exports_access_expression(source, expression)
                }) && data.name.is_some_and(|name| {
                    request.state.binder.node_record(name).kind != SyntaxKind::PrivateIdentifier
                }) {
                    return request.state.get_symbol_at_location(declaration);
                }
            }
            NodeData::ShorthandPropertyAssignment(data) => {
                let object = request.state.binder.node_record(declaration).parent;
                let binary =
                    object.and_then(|object| request.state.binder.node_record(object).parent);
                if let Some(binary) = binary.filter(|&binary| {
                    request.state.binder.node_record(binary).kind == SyntaxKind::BinaryExpression
                        && tsc_binder::assignment::get_assignment_declaration_kind(source, binary)
                            == tsc_binder::assignment::AssignmentDeclarationKind::ModuleExports
                }) {
                    let _ = binary;
                    if let Some(name) = data.name {
                        return request.state.get_export_specifier_local_target_symbol(name);
                    }
                }
            }
            _ => {}
        }
    }
    Ok(Some(symbol))
}

/// tsgo `getExportEqualsLocalSymbol`.
fn get_export_equals_local_symbol(
    request: &mut Request<'_, '_>,
    imported_symbol: SymbolId,
) -> CheckResult<Option<SymbolId>> {
    if request
        .state
        .binder
        .symbol(imported_symbol)
        .flags
        .intersects(SymbolFlags::ALIAS)
    {
        return request.state.get_immediate_aliased_symbol(imported_symbol);
    }
    let Some(declaration) = request
        .state
        .binder
        .symbol(imported_symbol)
        .value_declaration
    else {
        return Ok(None);
    };
    Ok(match &request.state.binder.node_record(declaration).data {
        NodeData::ExportAssignment(data) => data
            .expression
            .and_then(|expression| request.state.binder.node_symbol(expression)),
        NodeData::BinaryExpression(data) => data
            .right
            .and_then(|right| request.state.binder.node_symbol(right)),
        NodeData::SourceFile(_) => request.state.binder.node_symbol(declaration),
        _ => None,
    })
}

/// tsgo `symbolNameNoDefault`: a symbol's name, a default export's
/// declared name (empty without one).
pub(super) fn symbol_name_no_default(request: &Request<'_, '_>, symbol: SymbolId) -> String {
    if request.state.binder.symbol(symbol).escaped_name != InternalSymbolName::DEFAULT {
        return symbol_name(request, symbol);
    }
    for declaration in request.declarations_of(symbol) {
        let source = request.state.binder.source_of_node(declaration);
        if let Some(name) = node_util::get_name_of_declaration(source, declaration) {
            if request.state.binder.node_record(name).kind == SyntaxKind::Identifier {
                return identifier_text(request, name);
            }
        }
    }
    String::new()
}

/// tsgo `findModuleReferences`: the `<reference path>` and `<reference
/// types>` directives that resolve to the module's file, and the imports
/// of it.
pub(super) fn find_module_references(
    request: &mut Request<'_, '_>,
    files: &[usize],
    search_module_symbol: SymbolId,
) -> CheckResult<Vec<ModuleReference>> {
    let mut references = Vec::new();
    let search_source_file = request
        .state
        .binder
        .symbol(search_module_symbol)
        .value_declaration
        .filter(|&declaration| {
            request.state.binder.node_record(declaration).kind == SyntaxKind::SourceFile
        })
        .map(|declaration| request.state.binder.file_index_of_node(declaration));
    for &referencing_file in files {
        if let Some(search_file) = search_source_file {
            // Check <reference path> and <reference types> directives
            for (range, referenced) in crate::utilities::file_reference_targets(
                request.state,
                request.prepared,
                referencing_file,
            ) {
                if referenced == Some(search_file) {
                    references.push(ModuleReference::Reference {
                        file: referencing_file,
                        range,
                    });
                }
            }
        }
        // Check all imports (including require() calls)
        for (_, module_specifier) in for_each_import(request, referencing_file) {
            if request.state.get_symbol_at_location(module_specifier)? == Some(search_module_symbol)
            {
                references.push(ModuleReference::Import {
                    literal: module_specifier,
                });
            }
        }
        for specifier in implicit_imports(request, referencing_file) {
            if request
                .state
                .get_synthetic_import_module_symbol(referencing_file, &specifier)
                == Some(search_module_symbol)
            {
                references.push(ModuleReference::Implicit {
                    file: referencing_file,
                    specifier,
                });
            }
        }
    }
    Ok(references)
}

/// tsgo `ast.GetFirstIdentifier`.
fn first_identifier(request: &Request<'_, '_>, mut node: NodeId) -> NodeId {
    loop {
        match &request.state.binder.node_record(node).data {
            NodeData::QualifiedName(data) => match data.left {
                Some(left) => node = left,
                None => return node,
            },
            NodeData::PropertyAccessExpression(data) => match data.expression {
                Some(expression) => node = expression,
                None => return node,
            },
            _ => return node,
        }
    }
}

/// An identifier's text.
pub(super) fn identifier_text(request: &Request<'_, '_>, node: NodeId) -> String {
    match &request.state.binder.node_record(node).data {
        NodeData::Identifier(identifier) => identifier.text().to_owned(),
        NodeData::PrivateIdentifier(identifier) => identifier.text().to_owned(),
        _ => String::new(),
    }
}

/// A module export name's text (an identifier or a string literal).
pub(super) fn module_export_name_text(request: &Request<'_, '_>, node: NodeId) -> String {
    match &request.state.binder.node_record(node).data {
        NodeData::StringLiteral(literal) => literal.text.to_string_lossy().into_owned(),
        _ => identifier_text(request, node),
    }
}

/// tsgo `symbol.Name()` (the escaped name, unescaped).
pub(super) fn symbol_name(request: &Request<'_, '_>, symbol: SymbolId) -> String {
    request
        .state
        .binder
        .symbol(symbol)
        .escaped_name
        .unescape()
        .to_string_lossy()
        .into_owned()
}

//! The per-file facts an incremental program's build info records (tsgo
//! execute/incremental programtosnapshot.go): the files a file references
//! through its module symbols (`getReferencedFiles`), whether it affects the
//! global scope (`fileAffectsGlobalScope`), and the semantic rows the
//! program caches for it (`GetSemanticDiagnosticsForIncremental`).

use std::collections::BTreeSet;

use tsc_binder::node_util;
use tsc_diagnostics::{Diagnostic, DiagnosticList};
use tsc_syntax::{NodeData, NodeId, SourceFile, SyntaxKind};
use tsc_types::{JsString, ModifierFlags, NodeFlags, SymbolFlags};

use crate::emit::CheckerSession;
use crate::program::{ProgramFileId, ProgramSnapshot};
use crate::state::CheckerState;

/// What the build info records for one Program file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IncrementalFileFacts {
    /// The bind and check rows the program caches for the file (sorted and
    /// deduplicated, before the `noEmit` filter and without the include
    /// processor's rows); `None` when the file was not checked, empty for a
    /// file whose check is skipped.
    pub semantic_rows: Option<DiagnosticList>,
    /// The Program files declaring the symbols of the file's imports, its
    /// module augmentations and every ambient module (as Program file
    /// indices, excluding the file itself).
    pub referenced_files: Vec<u32>,
    /// The file's `/// <reference path="...">` names as written; the
    /// program resolves them against the file's directory.
    pub path_references: Vec<JsString>,
    /// tsgo `fileAffectsGlobalScope`.
    pub affects_global_scope: bool,
    /// tsgo `SkipTypeChecking(file, false)`.
    pub skipped: bool,
    /// The include-processor rows located in the file (the program's
    /// file-located diagnostics after its comment directives), which the
    /// program appends to a file's semantic rows (tsgo
    /// `GetIncludeProcessorDiagnostics`; empty for a skipped file).
    pub program_rows: Vec<Diagnostic>,
    /// The file's module symbol exports a const enum, directly or through an
    /// alias declared in the file (tsgo affectedfileshandler.go's
    /// `invalidateJsFiles`).
    pub exports_const_enum: bool,
}

/// The plan an incremental program's planner returns before the check: the
/// files the check covers, per snapshot document (the library prefix
/// included); empty means every file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IncrementalPlan {
    pub check: Vec<bool>,
}

/// The incremental program's planner (tsgo `programToSnapshot` and
/// `collectAllAffectedFiles`): runs over the initialized checker session
/// before any source is checked, with every file's module facts and whether
/// tsgo's semantic getter runs (the syntactic, options and global gates are
/// open), and restricts the checked files.
pub type IncrementalPlanner<'p> = dyn FnMut(&ProgramSnapshot, &CheckerSession<'_>, &[IncrementalFileFacts], bool) -> IncrementalPlan
    + 'p;

/// What an incremental program asks of the check.
pub struct IncrementalRequest<'p> {
    /// Collect every file's [`IncrementalFileFacts`].
    pub facts: bool,
    pub planner: Option<&'p mut IncrementalPlanner<'p>>,
}

impl<'p> IncrementalRequest<'p> {
    /// Nothing: not an incremental program.
    pub const NONE: IncrementalRequest<'static> = IncrementalRequest {
        facts: false,
        planner: None,
    };

    pub fn facts_only(facts: bool) -> Self {
        Self {
            facts,
            planner: None,
        }
    }
}

/// The facts of every Program file, in Program order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IncrementalCheckFacts {
    pub files: Vec<IncrementalFileFacts>,
    /// The planner's declaration signatures reported file-less rows: the
    /// check of the files then defers none of them, but tsgo's
    /// `GetGlobalDiagnostics` sees them when it writes the build info.
    pub planner_global_rows: bool,
    /// The check of the files reported file-less rows (deferred into the
    /// rows of the file being checked, even when an unchecked JavaScript
    /// file's filter then drops them).
    pub check_global_rows: bool,
}

/// tsgo parser/references.go `collectExternalModuleReferences`: the module
/// specifier literals of the file's imports (static, dynamic, `require` in
/// JavaScript and `import()` types) and its module augmentations.
#[derive(Default)]
struct ModuleReferences {
    imports: Vec<NodeId>,
    /// The imports whose literal tsgo's GetSymbolAtLocation resolves as a
    /// module (checker.go getSymbolAtLocation, the string-literal case):
    /// the module names of import/export declarations and of
    /// `import x = require()`, `import()` calls, import types, and a
    /// `require()` that initializes a variable declaration. A `require()`
    /// elsewhere (`module.exports = require("x")`) resolves no symbol, so
    /// the build info's referencedMap does not record it.
    symbol_imports: Vec<NodeId>,
    /// The `declare module "x"` / `declare global` declarations of an
    /// external module file (and the non-relative ones nested in an
    /// ambient module).
    module_augmentations: Vec<NodeId>,
}

fn root_statements(source: &SourceFile) -> &[NodeId] {
    match &source.arena.node(source.root).data {
        NodeData::SourceFile(data) => data
            .statements
            .map(|statements| source.arena.node_array(statements).nodes)
            .unwrap_or_default(),
        _ => &[],
    }
}

fn is_string_literal_like(source: &SourceFile, node: NodeId) -> bool {
    matches!(
        source.arena.node(node).kind,
        SyntaxKind::StringLiteral | SyntaxKind::NoSubstitutionTemplateLiteral
    )
}

/// tsgo `IsImportCall`: `import(...)` or `import.defer(...)`.
fn is_import_callee(source: &SourceFile, callee: NodeId) -> bool {
    let node = source.arena.node(callee);
    match &node.data {
        NodeData::MetaProperty(meta) => {
            meta.keyword_token == SyntaxKind::ImportKeyword
                && meta.name.is_some_and(|name| {
                    matches!(
                        &source.arena.node(name).data,
                        NodeData::Identifier(identifier) if identifier.escaped_text == "defer"
                    )
                })
        }
        _ => node.kind == SyntaxKind::ImportKeyword,
    }
}

fn collect_module_references(source: &SourceFile, javascript: bool) -> ModuleReferences {
    let mut references = ModuleReferences::default();
    for &statement in root_statements(source) {
        collect_module_reference(source, statement, false, &mut references);
    }
    // ForEachDynamicImportOrRequireCall: the whole file, when the parser saw
    // a dynamic import or import type, or the file is JavaScript (require
    // calls); a node inside JSDoc counts only in a JavaScript file.
    let root_flags = NodeFlags::from_bits(source.arena.node(source.root).flags);
    if root_flags.contains(NodeFlags::POSSIBLY_CONTAINS_DYNAMIC_IMPORT) || javascript {
        let base = source.arena.node_base();
        for (offset, node) in source.arena.nodes().iter().enumerate() {
            if !javascript && NodeFlags::from_bits(node.flags).contains(NodeFlags::JS_DOC) {
                continue;
            }
            match &node.data {
                NodeData::CallExpression(call) => {
                    let Some(callee) = call.expression else {
                        continue;
                    };
                    let arguments = call
                        .arguments
                        .map(|arguments| source.arena.node_array(arguments).nodes)
                        .unwrap_or_default();
                    let Some(&argument) = arguments.first() else {
                        continue;
                    };
                    let require_call = javascript
                        && arguments.len() == 1
                        && matches!(
                            &source.arena.node(callee).data,
                            NodeData::Identifier(identifier) if identifier.escaped_text == "require"
                        );
                    if (require_call || is_import_callee(source, callee))
                        && is_string_literal_like(source, argument)
                    {
                        references.imports.push(argument);
                        // tsgo IsVariableDeclarationInitializedToRequire: the
                        // call initializes the variable declaration above it.
                        let initializes_variable = node.parent.is_some_and(|parent| match &source
                            .arena
                            .node(parent)
                            .data
                        {
                            NodeData::VariableDeclaration(declaration) => {
                                declaration.initializer.is_some_and(|initializer| {
                                    match &source.arena.node(initializer).data {
                                        NodeData::CallExpression(initializer_call) => {
                                            initializer_call.arguments.is_some_and(|arguments| {
                                                source.arena.node_array(arguments).nodes.first()
                                                    == Some(&argument)
                                            })
                                        }
                                        _ => false,
                                    }
                                })
                            }
                            _ => false,
                        });
                        if !require_call || initializes_variable {
                            references.symbol_imports.push(argument);
                        }
                    }
                }
                NodeData::ImportType(import_type) => {
                    let Some(argument) = import_type.argument else {
                        continue;
                    };
                    if let NodeData::LiteralType(literal_type) = &source.arena.node(argument).data {
                        if let Some(literal) = literal_type.literal {
                            if source.arena.node(literal).kind == SyntaxKind::StringLiteral {
                                references.imports.push(literal);
                                references.symbol_imports.push(literal);
                            }
                        }
                    }
                }
                _ => {}
            }
            let _ = (base, offset);
        }
    }
    references
}

/// tsgo `collectModuleReferences` for one statement.
fn collect_module_reference(
    source: &SourceFile,
    node: NodeId,
    in_ambient_module: bool,
    references: &mut ModuleReferences,
) {
    let data = &source.arena.node(node).data;
    let module_name = match data {
        NodeData::ImportDeclaration(import) => Some(import.module_specifier),
        NodeData::ExportDeclaration(export) => Some(export.module_specifier),
        NodeData::ImportEqualsDeclaration(import) => {
            Some(import.module_reference.and_then(|reference| {
                match &source.arena.node(reference).data {
                    NodeData::ExternalModuleReference(external) => external.expression,
                    _ => None,
                }
            }))
        }
        _ => None,
    };
    if let Some(module_name) = module_name {
        if let Some(name) = module_name {
            if let NodeData::StringLiteral(literal) = &source.arena.node(name).data {
                if !literal.text.is_empty()
                    && (!in_ambient_module
                        || !CheckerState::is_external_module_name_relative(literal.text.as_js()))
                {
                    references.imports.push(name);
                    references.symbol_imports.push(name);
                }
            }
        }
        return;
    }
    let NodeData::ModuleDeclaration(module) = data else {
        return;
    };
    if !node_util::is_ambient_module(source, node)
        || !(in_ambient_module
            || node_util::has_syntactic_modifier(source, node, ModifierFlags::AMBIENT)
            || source.is_declaration_file)
    {
        return;
    }
    let name_text = module
        .name
        .and_then(|name| node_util::get_text_of_identifier_or_literal(source, name))
        .unwrap_or_default();
    if source.external_module_indicator.is_some()
        || (in_ambient_module && !CheckerState::is_external_module_name_relative(name_text.as_js()))
    {
        references.module_augmentations.push(node);
    } else if !in_ambient_module {
        if let Some(statements) = module
            .body
            .and_then(|body| node_util::statements_of(source, body))
        {
            for &statement in source.arena.node_array(statements).nodes {
                collect_module_reference(source, statement, true, references);
            }
        }
    }
}

/// tsgo `IsModuleWithStringLiteralName`.
fn is_module_with_string_literal_name(source: &SourceFile, node: NodeId) -> bool {
    match &source.arena.node(node).data {
        NodeData::ModuleDeclaration(module) => module
            .name
            .is_some_and(|name| source.arena.node(name).kind == SyntaxKind::StringLiteral),
        _ => false,
    }
}

/// tsgo `fileAffectsGlobalScope`.
fn affects_global_scope(
    state: &CheckerState<'_>,
    source: &SourceFile,
    references: &ModuleReferences,
) -> bool {
    if references
        .module_augmentations
        .iter()
        .any(|&augmentation| node_util::is_global_scope_augmentation(source, augmentation))
    {
        return true;
    }
    let json =
        NodeFlags::from_bits(source.arena.node(source.root).flags).contains(NodeFlags::JSON_FILE);
    if state
        .binder
        .is_external_or_common_js_module_of_node(source.root)
        || json
    {
        return false;
    }
    root_statements(source)
        .iter()
        .any(|&statement| !is_module_with_string_literal_name(source, statement))
}

/// tsgo `getReferencedFiles` without the path references and type
/// directives (the program adds those): the declaring files of the import
/// symbols, the augmented modules and every ambient module.
fn referenced_files(
    state: &mut CheckerState<'_>,
    file: usize,
    references: &ModuleReferences,
) -> Vec<u32> {
    let mut referenced = BTreeSet::new();
    let mut add_symbol = |state: &CheckerState<'_>, symbol| {
        for &declaration in state.binder.symbol(symbol).declarations.iter() {
            let declaring_file = state.binder.file_index_of_node(declaration);
            if declaring_file != file {
                referenced.insert(u32::try_from(declaring_file).expect("program file index"));
            }
        }
    };
    for &import in &references.symbol_imports {
        // GetSymbolAtLocation of a module specifier: resolveExternalModuleName
        // with errors ignored.
        if let Ok(Some(symbol)) = state.resolve_external_module_name(import, import, true) {
            add_symbol(state, symbol);
        }
    }
    for &augmentation in &references.module_augmentations {
        let source = state.binder.source(file);
        let string_name = match &source.arena.node(augmentation).data {
            NodeData::ModuleDeclaration(module) => module
                .name
                .is_some_and(|name| source.arena.node(name).kind == SyntaxKind::StringLiteral),
            _ => false,
        };
        if !string_name {
            continue;
        }
        // GetSymbolAtLocation of the declaration's name: the merged symbol
        // of the module it augments.
        if let Some(symbol) = state.binder.node_symbol(augmentation) {
            let merged = state.get_merged_symbol(symbol);
            add_symbol(state, merged);
        }
    }
    // GetAmbientModules: the ambient modules of the global scope and the
    // pattern ambient modules.
    let mut ambient = Vec::new();
    for (name, &symbol) in state.globals.iter() {
        if name.starts_with("\"") {
            ambient.push(symbol);
        }
    }
    for (_, _, symbol) in &state.pattern_ambient_modules {
        ambient.push(state.get_merged_symbol(*symbol));
    }
    ambient.sort_unstable();
    ambient.dedup();
    for symbol in ambient {
        add_symbol(state, symbol);
    }
    referenced.into_iter().collect()
}

/// tsgo affectedfileshandler.go `handleDtsMayChangeOfAffectedFile`'s const
/// enum test: the file's module symbol exports a const enum, or an alias
/// (`SkipAlias`) of one declared in the file.
fn exports_const_enum(state: &mut CheckerState<'_>, file: usize) -> bool {
    let root = state.binder.source(file).root;
    let Some(symbol) = state.binder.node_symbol(root) else {
        return false;
    };
    let exported = state
        .binder
        .symbol(symbol)
        .exports()
        .values()
        .copied()
        .collect::<Vec<_>>();
    for export in exported {
        if state
            .binder
            .symbol(export)
            .flags
            .contains(SymbolFlags::CONST_ENUM)
        {
            return true;
        }
        // tsgo SkipAlias: only an alias is resolved.
        if !state
            .binder
            .symbol(export)
            .flags
            .contains(SymbolFlags::ALIAS)
        {
            continue;
        }
        let Ok(aliased) = state.resolve_alias(export) else {
            continue;
        };
        if aliased == export {
            continue;
        }
        let aliased_symbol = state.binder.symbol(aliased);
        if aliased_symbol.flags.contains(SymbolFlags::CONST_ENUM)
            && aliased_symbol
                .declarations
                .iter()
                .any(|&declaration| state.binder.file_index_of_node(declaration) == file)
        {
            return true;
        }
    }
    false
}

/// The facts of one file except its semantic rows and its program rows.
pub(crate) fn file_facts_without_rows(
    state: &mut CheckerState<'_>,
    file: usize,
) -> IncrementalFileFacts {
    let source = state.binder.source(file);
    let javascript = crate::is_js_file_name(&source.file_name);
    let references = collect_module_references(source, javascript);
    let affects_global_scope = affects_global_scope(state, source, &references);
    let path_references = source
        .referenced_files
        .iter()
        .map(|reference| reference.file_name.clone())
        .collect();
    let skipped = state.skip_type_checking_file(ProgramFileId::from_raw(
        u32::try_from(file).expect("program file index"),
    ));
    let referenced_files = referenced_files(state, file, &references);
    let exports_const_enum = exports_const_enum(state, file);
    IncrementalFileFacts {
        semantic_rows: None,
        referenced_files,
        path_references,
        affects_global_scope,
        skipped,
        program_rows: Vec::new(),
        exports_const_enum,
    }
}

/// The facts the planner receives: every file's module facts and program
/// rows, before any source is checked.
pub(crate) fn planner_facts(
    state: &mut CheckerState<'_>,
    program_diagnostics: &[Diagnostic],
) -> Vec<IncrementalFileFacts> {
    (0..state.binder.file_count())
        .map(|file| {
            let mut facts = file_facts_without_rows(state, file);
            facts.program_rows =
                crate::program_rows_for_file(state.binder.source(file), program_diagnostics);
            facts
        })
        .collect()
}

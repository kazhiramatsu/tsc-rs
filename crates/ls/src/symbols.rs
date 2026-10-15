//! tsgo `ls/symbols.go`: the workspace's symbols matching a query, and the
//! protocol kind of a declaration (the document symbols come with L6).

use std::cmp::Ordering;
use std::collections::BTreeMap;

use tsc_binder::assignment::{get_assignment_declaration_kind, AssignmentDeclarationKind};
use tsc_binder::node_util;
use tsc_checker::program::ProgramFileId;
use tsc_checker::state::CheckerState;
use tsc_lsproto::{
    DocumentUri, Location, SymbolInformation, SymbolInformationsOrWorkspaceSymbolsOrNull,
    SymbolKind, WorkspaceSymbolResponse,
};
use tsc_program::uri::file_name_to_document_uri;
use tsc_syntax::{NodeData, NodeId, SourceFile, SyntaxKind};
use tsc_types::ModifierFlags;

use crate::language_service::{LanguageService, LsError};
use crate::request::Request;
use crate::utilities::{path_key, script_of, token_start};

/// A declaration matching a workspace symbol query (tsgo
/// `DeclarationInfo`), with its answer.
pub struct WorkspaceSymbolMatch {
    /// The file's path (tsgo `PathKey`).
    path: String,
    name: String,
    score: usize,
    position: u32,
    symbol: SymbolInformation,
}

impl LanguageService<'_> {
    /// tsgo `ProvideWorkspaceSymbols`' part of one program: the matches of
    /// its files (the declaration files only of a program with TypeScript
    /// files; no library or `node_modules` file when the preferences leave
    /// them out), by file path.
    pub fn workspace_symbol_matches(
        &mut self,
        query: &str,
    ) -> Result<Vec<(String, Vec<WorkspaceSymbolMatch>)>, LsError> {
        let exclude_library_symbols =
            self.user_preferences().exclude_library_symbols_in_nav_to == Some(true);
        let matches = self.with_request_at(0, 0, false, |request, _| {
            let file_count = request.state.binder.file_count();
            let has_ts_file = (0..file_count).any(|file| {
                has_implementation_ts_file_extension(
                    &request
                        .state
                        .binder
                        .source(file)
                        .file_name
                        .to_string_lossy(),
                )
            });
            let mut files = Vec::new();
            for file in 0..file_count {
                let source = request.state.binder.source(file);
                let file_name = source.file_name.to_string_lossy().into_owned();
                let library = request
                    .state
                    .binder
                    .file_facts(ProgramFileId::from_raw(file as u32))
                    .is_default_library();
                let excluded = exclude_library_symbols
                    && (file_name.to_lowercase().contains("/node_modules/") || library);
                // A program without TypeScript files leaves out declaration files.
                let included = has_ts_file || !source.is_declaration_file;
                if !included || excluded {
                    continue;
                }
                let mut matches = Vec::new();
                for (name, declarations) in declaration_map(request.state, source) {
                    let Some(score) = match_score(&name, query) else {
                        continue;
                    };
                    for declaration in declarations {
                        matches.push(WorkspaceSymbolMatch {
                            path: path_key(request.prepared, &file_name),
                            name: name.clone(),
                            score,
                            position: source.arena.node(declaration).pos,
                            symbol: request.workspace_symbol_information(
                                source,
                                &name,
                                declaration,
                            ),
                        });
                    }
                }
                files.push((path_key(request.prepared, &file_name), matches));
            }
            Ok(files)
        })?;
        Ok(matches.unwrap_or_default())
    }
}

/// tsgo `ProvideWorkspaceSymbols`: the matches of every program's files (a
/// file's last program's), by score, name, file and position; the first
/// 256.
pub fn workspace_symbols(
    programs: Vec<Vec<(String, Vec<WorkspaceSymbolMatch>)>>,
) -> WorkspaceSymbolResponse {
    let mut files: BTreeMap<String, Vec<WorkspaceSymbolMatch>> = BTreeMap::new();
    for program in programs {
        for (path, matches) in program {
            files.insert(path, matches);
        }
    }
    let mut matches: Vec<WorkspaceSymbolMatch> = files.into_values().flatten().collect();
    matches.sort_by(|left, right| {
        left.score
            .cmp(&right.score)
            .then_with(|| compare_strings_case_insensitive(&left.name, &right.name))
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.position.cmp(&right.position))
    });
    matches.truncate(256);
    SymbolInformationsOrWorkspaceSymbolsOrNull::SymbolInformations(
        matches.into_iter().map(|found| found.symbol).collect(),
    )
}

impl Request<'_, '_> {
    /// A workspace symbol's answer: its name's location, kind and
    /// container.
    fn workspace_symbol_information(
        &self,
        source: &SourceFile,
        name: &str,
        declaration: NodeId,
    ) -> SymbolInformation {
        let container_name = node_util::get_container_node(source, declaration)
            .map(|container| get_declaration_name(source, container))
            .filter(|name| !name.is_empty());
        // Use the name node's span so that VS selects just the symbol name (matching
        // the TS5 navto behaviour). GetNameOfDeclaration is always non-nil here because
        // computeDeclarationMap only adds declarations whose GetDeclarationName (string
        // form) is non-empty, which implies a name node exists.
        let name_node = node_util::get_name_of_declaration(source, declaration)
            .expect("a declaration with a name");
        let range = token_start(source, name_node)..source.arena.node(name_node).end as usize;
        SymbolInformation {
            name: name.to_owned(),
            kind: symbol_kind_from_node(self, declaration),
            tags: None,
            container_name,
            deprecated: None,
            location: Location {
                uri: DocumentUri(file_name_to_document_uri(
                    &source.file_name.to_string_lossy(),
                )),
                range: self.converters.to_lsp_range(&script_of(source), range),
            },
        }
    }
}

/// tsgo `SourceFile.computeDeclarationMap`: a file's declarations by name
/// (an overload group once, by its implementation when it has one).
fn declaration_map(state: &CheckerState<'_>, source: &SourceFile) -> Vec<(String, Vec<NodeId>)> {
    let mut map = DeclarationMap {
        state,
        source,
        names: Vec::new(),
    };
    for child in children(source, source.root) {
        map.visit(child);
    }
    map.names
}

struct DeclarationMap<'s, 'c> {
    state: &'s CheckerState<'c>,
    source: &'s SourceFile,
    names: Vec<(String, Vec<NodeId>)>,
}

impl DeclarationMap<'_, '_> {
    fn entry(&mut self, name: String) -> &mut Vec<NodeId> {
        let index = match self.names.iter().position(|(known, _)| *known == name) {
            Some(index) => index,
            None => {
                self.names.push((name, Vec::new()));
                self.names.len() - 1
            }
        };
        &mut self.names[index].1
    }

    fn add(&mut self, declaration: NodeId) {
        let name = get_declaration_name(self.source, declaration);
        if !name.is_empty() {
            self.entry(name).push(declaration);
        }
    }

    fn visit_children(&mut self, node: NodeId) {
        for child in children(self.source, node) {
            self.visit(child);
        }
    }

    fn visit(&mut self, node: NodeId) {
        let source = self.source;
        let data = &source.arena.node(node).data;
        match source.arena.node(node).kind {
            SyntaxKind::FunctionDeclaration
            | SyntaxKind::FunctionExpression
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::MethodSignature => {
                let name = get_declaration_name(source, node);
                if !name.is_empty() {
                    let state = self.state;
                    let declarations = self.entry(name);
                    match declarations.last().copied() {
                        // Check whether this declaration belongs to an "overload group".
                        Some(last)
                            if source.arena.node(node).parent == source.arena.node(last).parent
                                && state.binder.node_symbol(node)
                                    == state.binder.node_symbol(last) =>
                        {
                            // Overwrite the last declaration if it was an overload and this one is an implementation.
                            if node_util::body_of(source, node).is_some()
                                && node_util::body_of(source, last).is_none()
                            {
                                *declarations.last_mut().expect("a last declaration") = node;
                            }
                        }
                        _ => declarations.push(node),
                    }
                }
                self.visit_children(node);
            }
            SyntaxKind::ClassDeclaration
            | SyntaxKind::ClassExpression
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::EnumDeclaration
            | SyntaxKind::ModuleDeclaration
            | SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::ImportClause
            | SyntaxKind::NamespaceImport
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::TypeLiteral => {
                self.add(node);
                self.visit_children(node);
            }
            SyntaxKind::ImportSpecifier | SyntaxKind::ExportSpecifier => {
                let property_name = match data {
                    NodeData::ImportSpecifier(data) => data.property_name,
                    NodeData::ExportSpecifier(data) => data.property_name,
                    _ => None,
                };
                if property_name.is_some() {
                    self.add(node);
                }
            }
            SyntaxKind::Parameter
                if !node_util::has_syntactic_modifier(
                    source,
                    node,
                    ModifierFlags::PARAMETER_PROPERTY_MODIFIER,
                ) =>
            {
                // Only consider parameter properties
            }
            SyntaxKind::Parameter
            | SyntaxKind::VariableDeclaration
            | SyntaxKind::BindingElement => {
                if let Some(name) = node_util::name_field_of(source, node) {
                    if node_util::is_binding_pattern(source, name) {
                        self.visit_children(name);
                    } else {
                        if let Some(initializer) = node_util::initializer_of(source, node) {
                            self.visit(initializer);
                        }
                        self.add(node);
                    }
                }
            }
            SyntaxKind::EnumMember
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::PropertySignature => {
                self.add(node);
            }
            SyntaxKind::ExportDeclaration => {
                // Handle named exports case e.g.:
                //    export {a, b as B} from "mod";
                let NodeData::ExportDeclaration(export) = data else {
                    return;
                };
                if let Some(clause) = export.export_clause {
                    match &source.arena.node(clause).data {
                        NodeData::NamedExports(named) => {
                            for element in list(source, named.elements) {
                                self.visit(element);
                            }
                        }
                        NodeData::NamespaceExport(namespace) => {
                            if let Some(name) = namespace.name {
                                self.visit(name);
                            }
                        }
                        _ => {}
                    }
                }
            }
            SyntaxKind::ImportDeclaration => {
                let NodeData::ImportDeclaration(import) = data else {
                    return;
                };
                let Some(clause) = import.import_clause else {
                    return;
                };
                let NodeData::ImportClause(clause) = &source.arena.node(clause).data else {
                    return;
                };
                // Handle default import case e.g.:
                //    import d from "mod";
                if let Some(name) = clause.name {
                    self.add(name);
                }
                // Handle named bindings in imports e.g.:
                //    import * as NS from "mod";
                //    import {a, b as B} from "mod";
                if let Some(bindings) = clause.named_bindings {
                    match &source.arena.node(bindings).data {
                        NodeData::NamespaceImport(_) => self.add(bindings),
                        NodeData::NamedImports(named) => {
                            for element in list(source, named.elements) {
                                self.visit(element);
                            }
                        }
                        _ => {}
                    }
                }
            }
            SyntaxKind::BinaryExpression => {
                if matches!(
                    get_assignment_declaration_kind(source, node),
                    AssignmentDeclarationKind::ExportsProperty
                        | AssignmentDeclarationKind::ThisProperty
                        | AssignmentDeclarationKind::Property
                ) {
                    self.add(node);
                }
                self.visit_children(node);
            }
            _ => self.visit_children(node),
        }
    }
}

/// tsgo `ast.GetDeclarationName`: the text of a declaration's own name (a
/// computed name's literal or property name); empty without one.
fn get_declaration_name(source: &SourceFile, declaration: NodeId) -> String {
    let Some(name) = node_util::get_non_assigned_name_of_declaration(source, declaration) else {
        return String::new();
    };
    match &source.arena.node(name).data {
        NodeData::ComputedPropertyName(computed) => {
            let Some(expression) = computed.expression else {
                return String::new();
            };
            if node_util::is_string_or_numeric_literal_like(source, expression) {
                return node_util::try_get_text_of_property_name(source, expression)
                    .unwrap_or_default();
            }
            match &source.arena.node(expression).data {
                NodeData::PropertyAccessExpression(access) => access
                    .name
                    .and_then(|name| node_util::id_text(source, name))
                    .unwrap_or_default()
                    .to_owned(),
                _ => String::new(),
            }
        }
        _ if is_property_name(source.arena.node(name).kind) => {
            node_util::try_get_text_of_property_name(source, name).unwrap_or_default()
        }
        _ => String::new(),
    }
}

/// tsgo `ast.IsPropertyName`.
fn is_property_name(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Identifier
            | SyntaxKind::PrivateIdentifier
            | SyntaxKind::StringLiteral
            | SyntaxKind::NumericLiteral
            | SyntaxKind::ComputedPropertyName
            | SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::BigIntLiteral
    )
}

/// tsgo `getMatchScore`: the characters of `name` the query's characters
/// (in order; an upper case one exactly, a lower case one in either case)
/// skip; none when they are not all found.
fn match_score(name: &str, query: &str) -> Option<usize> {
    let mut score = 0;
    let mut characters = name.chars();
    for query_character in query.chars() {
        let exact = query_character.is_uppercase();
        loop {
            let character = characters.next()?;
            if exact && character == query_character
                || !exact && character.to_lowercase().eq(query_character.to_lowercase())
            {
                break;
            }
            score += 1;
        }
    }
    Some(score)
}

/// tsgo `stringutil.CompareStringsCaseInsensitive`: by the characters'
/// lower case forms.
fn compare_strings_case_insensitive(left: &str, right: &str) -> Ordering {
    left.chars()
        .flat_map(char::to_lowercase)
        .cmp(right.chars().flat_map(char::to_lowercase))
}

/// tsgo `HasImplementationTSFileExtension`.
fn has_implementation_ts_file_extension(file_name: &str) -> bool {
    [".ts", ".tsx", ".mts", ".cts"]
        .iter()
        .any(|extension| file_name.ends_with(extension))
        && !tsc_program::is_declaration_file_name(file_name)
}

/// A node's children (tsgo `ForEachChild`).
fn children(source: &SourceFile, node: NodeId) -> Vec<NodeId> {
    let mut children = Vec::new();
    tsc_syntax::for_each_child(&source.arena, source.arena.node(node), |child| {
        children.push(child);
        false
    });
    children
}

/// The nodes of a list.
fn list(source: &SourceFile, list: Option<tsc_syntax::NodeArrayId>) -> Vec<NodeId> {
    list.map(|list| source.arena.node_array(list).nodes.to_vec())
        .unwrap_or_default()
}

/// tsgo `getSymbolKindFromNode`.
pub(crate) fn symbol_kind_from_node(request: &Request<'_, '_>, node: NodeId) -> SymbolKind {
    let source = request.state.binder.source_of_node(node);
    match request.node_kind(node) {
        SyntaxKind::SourceFile => {
            if source.external_module_indicator.is_some() {
                SymbolKind::MODULE
            } else {
                SymbolKind::FILE
            }
        }
        SyntaxKind::ModuleDeclaration => SymbolKind::NAMESPACE,
        SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression => SymbolKind::CLASS,
        SyntaxKind::InterfaceDeclaration => SymbolKind::INTERFACE,
        SyntaxKind::TypeAliasDeclaration
        | SyntaxKind::JSDocTypedefTag
        | SyntaxKind::JSDocCallbackTag => SymbolKind::CLASS,
        SyntaxKind::EnumDeclaration => SymbolKind::ENUM,
        SyntaxKind::VariableDeclaration => SymbolKind::VARIABLE,
        SyntaxKind::ArrowFunction
        | SyntaxKind::FunctionDeclaration
        | SyntaxKind::FunctionExpression => SymbolKind::FUNCTION,
        SyntaxKind::GetAccessor | SyntaxKind::SetAccessor => SymbolKind::PROPERTY,
        SyntaxKind::MethodDeclaration | SyntaxKind::MethodSignature => SymbolKind::METHOD,
        SyntaxKind::PropertyDeclaration
        | SyntaxKind::PropertySignature
        | SyntaxKind::PropertyAssignment
        | SyntaxKind::ShorthandPropertyAssignment
        | SyntaxKind::SpreadAssignment
        | SyntaxKind::IndexSignature => SymbolKind::PROPERTY,
        SyntaxKind::CallSignature => SymbolKind::METHOD,
        SyntaxKind::ConstructSignature => SymbolKind::CONSTRUCTOR,
        SyntaxKind::Constructor | SyntaxKind::ClassStaticBlockDeclaration => {
            SymbolKind::CONSTRUCTOR
        }
        SyntaxKind::TypeParameter => SymbolKind::TYPE_PARAMETER,
        SyntaxKind::EnumMember => SymbolKind::ENUM_MEMBER,
        SyntaxKind::Parameter => {
            if node_util::has_syntactic_modifier(
                source,
                node,
                ModifierFlags::PARAMETER_PROPERTY_MODIFIER,
            ) {
                SymbolKind::PROPERTY
            } else {
                SymbolKind::VARIABLE
            }
        }
        SyntaxKind::BinaryExpression | SyntaxKind::CallExpression => {
            match get_assignment_declaration_kind(source, node) {
                AssignmentDeclarationKind::ThisProperty
                | AssignmentDeclarationKind::Property
                | AssignmentDeclarationKind::ObjectDefinePropertyValue => SymbolKind::PROPERTY,
                _ => SymbolKind::VARIABLE,
            }
        }
        // String literals used as property names (e.g., in Object.defineProperty)
        SyntaxKind::StringLiteral
        | SyntaxKind::NoSubstitutionTemplateLiteral
        | SyntaxKind::NumericLiteral => SymbolKind::PROPERTY,
        _ => SymbolKind::VARIABLE,
    }
}

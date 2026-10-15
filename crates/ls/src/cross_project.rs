//! tsgo `ls/crossproject.go`: the language service's part of a search
//! across projects (tsgo `handleCrossProject`, whose loop over projects is
//! the server's) — one project's answer with the definition visible outside
//! the project and the original locations of the definitions, which lead to
//! more projects — and the combination of the projects' answers.

use std::collections::{BTreeMap, HashMap, HashSet};

use tsc_binder::node_util;
use tsc_checker::state::CheckResult;
use tsc_lsproto::{
    CallHierarchyIncomingCallsOrNull, CallHierarchyIncomingCallsResponse, DocumentUri,
    ImplementationResponse, Location, LocationOrLocationsOrDefinitionLinksOrNull, LocationsOrNull,
    Position, ReferencesResponse, RenameResponse,
    TextDocumentEditOrCreateFileOrRenameFileOrDeleteFile, TextEdit, VSReferenceItemsOrNull,
    VSReferencesResponse, WorkspaceEdit, WorkspaceEditOrNull,
};
use tsc_program::uri::file_name_to_document_uri;
use tsc_syntax::{NodeId, SyntaxKind};
use tsc_types::{ModifierFlags, SymbolId};

use crate::find_all_references::{
    DefinitionKind, LocationKey, SymbolAndEntries, SymbolAndEntriesData,
};
use crate::language_service::{LanguageService, LsError};
use crate::lsconv::TextScript;
use crate::request::Request;
use crate::utilities::{range_of_node_in, script_of};

/// A document and a position in it (tsgo `lsproto.HasTextDocumentPosition`).
#[derive(Clone, Debug, PartialEq)]
pub struct TextDocumentPosition {
    pub uri: DocumentUri,
    pub position: Position,
}

/// tsgo `nonLocalDefinition`: a definition visible outside its file, at the
/// start of its name.
#[derive(Clone, Debug)]
pub struct NonLocalDefinition {
    pub position: TextDocumentPosition,
    /// tsgo `GetSourcePosition`: the position in the source a declaration
    /// file's maps lead to.
    pub source_position: Option<TextDocumentPosition>,
    /// tsgo `GetGeneratedPosition`: the position in the declaration file the
    /// project emits for the file.
    pub generated_position: Option<TextDocumentPosition>,
}

/// One project's search (an item of tsgo `handleCrossProject`): its answer,
/// the definition visible outside the project (asked of the default project)
/// and the original locations of the definitions (tsgo
/// `forEachOriginalDefinitionLocation`), whose projects to search too.
#[derive(Debug)]
pub struct ProjectSearch<R> {
    pub response: R,
    pub definition: Option<NonLocalDefinition>,
    pub original_locations: Vec<TextDocumentPosition>,
}

/// Where a project's search is: an LSP position of the document, or a
/// position of the file (a code lens's data).
#[derive(Clone, Debug)]
pub enum SearchPosition {
    Lsp(Position),
    Offset(u32),
}

impl LanguageService<'_> {
    /// tsgo `handleCrossProject`'s search of one project: the symbols and
    /// entries at the position (tsgo `provideSymbolsAndEntries`), the answer
    /// `respond` makes of them, the first entry's definition visible outside
    /// the project (with `with_definition`, for the default project) and the
    /// entries' original definition locations. None for a program without
    /// files.
    #[allow(clippy::too_many_arguments)]
    fn search_project<R>(
        &mut self,
        uri: &DocumentUri,
        at: &SearchPosition,
        is_rename: bool,
        implementations: bool,
        links: bool,
        with_definition: bool,
        respond: impl FnOnce(&mut Request<'_, '_>, Option<&SymbolAndEntriesData>, u32) -> CheckResult<R>,
    ) -> Result<Option<ProjectSearch<R>>, LsError> {
        let file = self.file(uri);
        let run = |request: &mut Request<'_, '_>, position: u32| {
            let data =
                request.provide_symbols_and_entries(file, position, is_rename, implementations)?;
            let mut definition = None;
            let mut original_locations = Vec::new();
            for entry in data.iter().flat_map(|data| &data.symbols_and_entries) {
                if with_definition && definition.is_none() {
                    definition = request.non_local_definition(entry)?;
                }
                request.original_definition_locations(entry, &mut original_locations);
            }
            let response = respond(request, data.as_ref(), position)?;
            Ok(ProjectSearch {
                response,
                definition,
                original_locations,
            })
        };
        match at {
            SearchPosition::Lsp(position) => self.with_request(file, position, links, run),
            SearchPosition::Offset(position) => self.with_request_at(file, *position, links, run),
        }
    }

    /// tsgo `ProvideReferences`' search of this project (tsgo
    /// `symbolAndEntriesToReferences`).
    pub fn search_references(
        &mut self,
        uri: &DocumentUri,
        at: &SearchPosition,
        include_declaration: bool,
        with_definition: bool,
    ) -> Result<Option<ProjectSearch<ReferencesResponse>>, LsError> {
        self.search_project(
            uri,
            at,
            false,
            false,
            false,
            with_definition,
            |request, data, _| {
                Ok(request.symbol_and_entries_to_references(data, include_declaration))
            },
        )
    }

    /// tsgo `ProvideVSReferences`' search of this project (tsgo
    /// `symbolAndEntriesToVSReferences`).
    pub fn search_vs_references(
        &mut self,
        uri: &DocumentUri,
        position: &Position,
        with_definition: bool,
    ) -> Result<Option<ProjectSearch<VSReferencesResponse>>, LsError> {
        let vs_capability = self.capabilities._vs_supports_visual_studio_extensions;
        self.search_project(
            uri,
            &SearchPosition::Lsp(position.clone()),
            false,
            false,
            false,
            with_definition,
            |request, data, _| request.symbol_and_entries_to_vs_references(data, vs_capability),
        )
    }

    /// tsgo `provideImplementationsEx`'s search of this project (tsgo
    /// `symbolAndEntriesToImplementations`): links when the client takes
    /// them; for a code lens (tsgo's `requireLocationsResult` and
    /// `dropOriginNodes`), locations without the nodes around the position.
    pub fn search_implementations(
        &mut self,
        uri: &DocumentUri,
        at: &SearchPosition,
        code_lens: bool,
        with_definition: bool,
    ) -> Result<Option<ProjectSearch<ImplementationResponse>>, LsError> {
        let links = !code_lens && self.capabilities.text_document.implementation.link_support;
        self.search_project(
            uri,
            at,
            false,
            true,
            links,
            with_definition,
            |request, data, position| {
                Ok(request.symbol_and_entries_to_implementations(
                    data,
                    links,
                    code_lens.then_some(position),
                ))
            },
        )
    }

    /// tsgo `ProvideRename`'s search of this project (tsgo
    /// `symbolAndEntriesToRename`): the edits, none when the node cannot be
    /// renamed or two edits of a range differ.
    pub fn search_rename(
        &mut self,
        uri: &DocumentUri,
        position: &Position,
        new_name: &str,
        with_definition: bool,
    ) -> Result<Option<ProjectSearch<RenameResponse>>, LsError> {
        self.search_project(
            uri,
            &SearchPosition::Lsp(position.clone()),
            true,
            false,
            false,
            with_definition,
            |request, data, _| {
                Ok(
                    match request.symbol_and_entries_to_rename(new_name, data)? {
                        Some(changes) => WorkspaceEditOrNull::WorkspaceEdit(WorkspaceEdit {
                            changes: Some(changes),
                            ..WorkspaceEdit::default()
                        }),
                        None => WorkspaceEditOrNull::Null,
                    },
                )
            },
        )
    }

    /// tsgo `getIncomingCalls`' search of this project at a declaration's
    /// reference node (tsgo `symbolAndEntriesToIncomingCalls`).
    pub fn search_incoming_calls(
        &mut self,
        uri: &DocumentUri,
        at: &SearchPosition,
        with_definition: bool,
    ) -> Result<Option<ProjectSearch<CallHierarchyIncomingCallsResponse>>, LsError> {
        self.search_project(
            uri,
            at,
            false,
            false,
            false,
            with_definition,
            |request, data, _| request.symbol_and_entries_to_incoming_calls(data),
        )
    }
}

impl Request<'_, '_> {
    /// tsgo `canUseDefinitionSymbol`: the definition's symbol, for a symbol
    /// or `this` definition.
    fn definition_symbol(entry: &SymbolAndEntries) -> Option<SymbolId> {
        let definition = entry.definition.as_ref()?;
        match definition.kind {
            DefinitionKind::Symbol | DefinitionKind::This => definition.symbol,
            _ => None,
        }
    }

    /// tsgo `getFileAndStartPosFromDeclaration`: a declaration's file and the
    /// start of its name (or of it).
    fn file_and_start_of_declaration(&self, declaration: NodeId) -> (usize, usize) {
        let file = self.state.binder.file_index_of_node(declaration);
        let source = self.state.binder.source(file);
        let name = node_util::get_name_of_declaration(source, declaration).unwrap_or(declaration);
        (file, range_of_node_in(source, name).start)
    }

    /// A position of a file the snapshot has, as a document position (tsgo
    /// `ToLSPPosition` of `getScript(fileName)`).
    fn text_document_position(
        &self,
        file_name: &str,
        position: usize,
    ) -> Option<TextDocumentPosition> {
        let text = self.host.read_file(file_name)?;
        let script = TextScript::new(file_name, &text);
        Some(TextDocumentPosition {
            uri: DocumentUri(file_name_to_document_uri(file_name)),
            position: self.converters.to_lsp_position(&script, position),
        })
    }

    /// tsgo `getNonLocalDefinition`: the first declaration of the entry's
    /// definition that is visible outside its file (tsgo
    /// `isDefinitionVisible`), with where its name starts in the source its
    /// declaration map leads to and in the declaration file its project
    /// emits.
    pub(crate) fn non_local_definition(
        &mut self,
        entry: &SymbolAndEntries,
    ) -> CheckResult<Option<NonLocalDefinition>> {
        let Some(symbol) = Self::definition_symbol(entry) else {
            return Ok(None);
        };
        for declaration in self.declarations_of(symbol) {
            if !self.is_definition_visible(declaration)? {
                continue;
            }
            let (file, start) = self.file_and_start_of_declaration(declaration);
            let source = self.state.binder.source(file);
            let file_name = source.file_name.to_string_lossy().into_owned();
            let position = TextDocumentPosition {
                uri: DocumentUri(file_name_to_document_uri(&file_name)),
                position: self.converters.to_lsp_position(&script_of(source), start),
            };
            let source_position = self
                .source_maps
                .try_get_source_position(&file_name, start)
                .and_then(|mapped| self.text_document_position(&mapped.file_name, mapped.pos));
            let generated_position = self
                .try_get_generated_position(&file_name, start)
                .and_then(|mapped| self.text_document_position(&mapped.file_name, mapped.pos));
            return Ok(Some(NonLocalDefinition {
                position,
                source_position,
                generated_position,
            }));
        }
        Ok(None)
    }

    /// tsgo `isDefinitionVisible`: whether declaration emit would write the
    /// declaration, or (what it leaves out) a variable's initializer, a
    /// public class member, a constructor, an object literal's member, a
    /// class expression or a function expression whose parent is visible.
    fn is_definition_visible(&mut self, declaration: NodeId) -> CheckResult<bool> {
        if self.state.is_declaration_visible(declaration)? {
            return Ok(true);
        }
        let Some(parent) = self.parent_of(declaration) else {
            return Ok(false);
        };
        let source = self.state.binder.source_of_node(declaration);
        // Variable initializers are visible if variable is visible.
        if matches!(
            self.node_kind(parent),
            SyntaxKind::VariableDeclaration
                | SyntaxKind::Parameter
                | SyntaxKind::BindingElement
                | SyntaxKind::PropertyDeclaration
                | SyntaxKind::PropertyAssignment
                | SyntaxKind::EnumMember
                | SyntaxKind::ForStatement
                | SyntaxKind::ForInStatement
                | SyntaxKind::ForOfStatement
                | SyntaxKind::JsxAttribute
        ) && node_util::initializer_of(source, parent) == Some(declaration)
        {
            return self.is_definition_visible(parent);
        }
        match self.node_kind(declaration) {
            SyntaxKind::PropertyDeclaration
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::MethodDeclaration => {
                // Private properties and methods are not visible.
                if node_util::has_syntactic_modifier(source, declaration, ModifierFlags::PRIVATE)
                    || self
                        .name_of(declaration)
                        .is_some_and(|name| self.node_kind(name) == SyntaxKind::PrivateIdentifier)
                {
                    return Ok(false);
                }
                // Public ones are visible when their parents are.
                self.is_definition_visible(parent)
            }
            SyntaxKind::Constructor
            | SyntaxKind::PropertyAssignment
            | SyntaxKind::ShorthandPropertyAssignment
            | SyntaxKind::ObjectLiteralExpression
            | SyntaxKind::ClassExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::FunctionExpression => self.is_definition_visible(parent),
            _ => Ok(false),
        }
    }

    /// tsgo `forEachOriginalDefinitionLocation`: the declarations of the
    /// entry's definition in declaration files, at the sources their maps
    /// lead to, and in a referenced project's sources.
    pub(crate) fn original_definition_locations(
        &self,
        entry: &SymbolAndEntries,
        locations: &mut Vec<TextDocumentPosition>,
    ) {
        let Some(symbol) = Self::definition_symbol(entry) else {
            return;
        };
        for declaration in self.declarations_of(symbol) {
            let (file, start) = self.file_and_start_of_declaration(declaration);
            let source = self.state.binder.source(file);
            let file_name = source.file_name.to_string_lossy();
            if tsc_program::is_declaration_file_name(&file_name) {
                // Map to the source position.
                if let Some(location) = self
                    .source_maps
                    .try_get_source_position(&file_name, start)
                    .and_then(|mapped| self.text_document_position(&mapped.file_name, mapped.pos))
                {
                    locations.push(location);
                }
            } else if self.is_source_from_project_reference(&file_name) {
                locations.push(TextDocumentPosition {
                    uri: DocumentUri(file_name_to_document_uri(&file_name)),
                    position: self.converters.to_lsp_position(&script_of(source), start),
                });
            }
        }
    }
}

/// tsgo `combineReferences`: every project's locations, each once.
pub fn combine_references(responses: Vec<ReferencesResponse>) -> ReferencesResponse {
    let mut combined = Vec::new();
    let mut seen = HashSet::new();
    for response in responses {
        if let LocationsOrNull::Locations(locations) = response {
            combine_locations(&mut combined, locations, &mut seen);
        }
    }
    LocationsOrNull::Locations(combined)
}

/// tsgo `combineLocationArray`.
fn combine_locations(
    combined: &mut Vec<Location>,
    locations: Vec<Location>,
    seen: &mut HashSet<LocationKey>,
) {
    for location in locations {
        if seen.insert(LocationKey::of(&location)) {
            combined.push(location);
        }
    }
}

/// tsgo `combineVSReferences`: every project's items, numbered again across
/// the projects (a definition's number with them).
pub fn combine_vs_references(responses: Vec<VSReferencesResponse>) -> VSReferencesResponse {
    let mut combined = Vec::new();
    let mut next_id = 0;
    for response in responses {
        let VSReferenceItemsOrNull::VSReferenceItems(items) = response else {
            continue;
        };
        let mut ids = HashMap::new();
        for mut item in items {
            ids.insert(item._vs_id, next_id);
            item._vs_id = next_id;
            next_id += 1;
            if let Some(definition) = item._vs_definition_id {
                item._vs_definition_id = Some(ids.get(&definition).copied().unwrap_or_default());
            }
            combined.push(item);
        }
    }
    VSReferenceItemsOrNull::VSReferenceItems(combined)
}

/// tsgo `combineImplementations`: every project's links (by their target
/// ranges) each once; once a project answers locations, every project's
/// locations each once.
pub fn combine_implementations(responses: Vec<ImplementationResponse>) -> ImplementationResponse {
    let mut combined = Vec::new();
    let mut seen = HashSet::new();
    for response in &responses {
        match response {
            LocationOrLocationsOrDefinitionLinksOrNull::DefinitionLinks(links) => {
                for link in links {
                    if seen.insert(LocationKey::of(&Location {
                        uri: link.target_uri.clone(),
                        range: link.target_range.clone(),
                    })) {
                        combined.push(link.clone());
                    }
                }
            }
            LocationOrLocationsOrDefinitionLinksOrNull::Locations(_) => {
                let mut combined = Vec::new();
                let mut seen = HashSet::new();
                for response in responses {
                    if let LocationOrLocationsOrDefinitionLinksOrNull::Locations(locations) =
                        response
                    {
                        combine_locations(&mut combined, locations, &mut seen);
                    }
                }
                return LocationOrLocationsOrDefinitionLinksOrNull::Locations(combined);
            }
            _ => {}
        }
    }
    LocationOrLocationsOrDefinitionLinksOrNull::DefinitionLinks(combined)
}

/// tsgo `combineRenameResponse`: every project's file operations (a file's
/// rename once) and edits (a document's range once); none without either.
pub fn combine_rename(responses: Vec<RenameResponse>) -> RenameResponse {
    type Change = TextDocumentEditOrCreateFileOrRenameFileOrDeleteFile;
    let mut combined: BTreeMap<DocumentUri, Vec<TextEdit>> = BTreeMap::new();
    let mut seen_changes: HashMap<DocumentUri, HashSet<(u32, u32, u32, u32)>> = HashMap::new();
    let mut document_changes = Vec::new();
    let mut seen_renames = HashSet::new();
    for response in responses {
        let WorkspaceEditOrNull::WorkspaceEdit(edit) = response else {
            continue;
        };
        for change in edit.document_changes.into_iter().flatten() {
            if let Change::RenameFile(rename) = &change {
                if !seen_renames.insert((rename.old_uri.clone(), rename.new_uri.clone())) {
                    continue;
                }
            }
            document_changes.push(change);
        }
        for (uri, edits) in edit.changes.into_iter().flatten() {
            let seen = seen_changes.entry(uri.clone()).or_default();
            let changes = combined.entry(uri).or_default();
            for edit in edits {
                let range = &edit.range;
                if seen.insert((
                    range.start.line,
                    range.start.character,
                    range.end.line,
                    range.end.character,
                )) {
                    changes.push(edit);
                }
            }
        }
    }
    if document_changes.is_empty() && combined.is_empty() {
        return WorkspaceEditOrNull::Null;
    }
    WorkspaceEditOrNull::WorkspaceEdit(WorkspaceEdit {
        changes: (!combined.is_empty()).then_some(combined),
        document_changes: (!document_changes.is_empty()).then_some(document_changes),
        ..WorkspaceEdit::default()
    })
}

/// tsgo `combineIncomingCalls`: every project's calls, each caller (by its
/// item's location) once.
pub fn combine_incoming_calls(
    responses: Vec<CallHierarchyIncomingCallsResponse>,
) -> CallHierarchyIncomingCallsResponse {
    let mut combined = Vec::new();
    let mut seen = HashSet::new();
    for response in responses {
        if let CallHierarchyIncomingCallsOrNull::CallHierarchyIncomingCalls(calls) = response {
            for call in calls {
                if seen.insert(LocationKey::of(&Location {
                    uri: call.from.uri.clone(),
                    range: call.from.range.clone(),
                })) {
                    combined.push(call);
                }
            }
        }
    }
    CallHierarchyIncomingCallsOrNull::CallHierarchyIncomingCalls(combined)
}

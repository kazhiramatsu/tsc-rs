//! tsgo `ls/findallreferences.go`: find all references — the references of
//! the symbol (or the keyword, label, `this`, string literal or module) at
//! a position across the program's files, for `textDocument/references`,
//! `textDocument/_vs_references` and `textDocument/implementation`.

mod import_tracker;
mod search;

use std::collections::HashSet;
use std::ops::Range;

use tsc_binder::node_util;
use tsc_checker::state::CheckResult;
use tsc_lsproto::{
    self as lsproto, ImplementationParams, ImplementationResponse, Location, LocationLink,
    ReferenceParams, ReferencesResponse, VSClassifiedTextElement, VSClassifiedTextRun,
    VSReferenceItem, VSReferenceKind, VSReferencesResponse,
};
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{ModifierFlags, SymbolId};

use crate::language_service::{LanguageService, LsError};
use crate::request::Request;
use crate::utilities::{touching, Touching};

use search::get_property_symbol_of_object_binding_pattern_without_property_name;
pub(crate) use search::get_referenced_symbols_for_node;

/// tsgo `referenceUse`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReferenceUse {
    References,
    Rename,
}

/// tsgo `refOptions`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RefOptions {
    pub(crate) use_: ReferenceUse,
    pub(crate) implementations: bool,
    /// tsgo `useAliasesForRename` (the `providePrefixAndSuffixTextForRename`
    /// preference, by default true).
    pub(crate) use_aliases_for_rename: bool,
}

/// tsgo `DefinitionKind`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DefinitionKind {
    Symbol,
    Label,
    Keyword,
    This,
    String,
    TripleSlashReference,
}

/// tsgo `Definition`.
#[derive(Clone, Debug)]
pub(crate) struct Definition {
    pub(crate) kind: DefinitionKind,
    pub(crate) symbol: Option<SymbolId>,
    pub(crate) node: Option<Touching>,
}

/// tsgo `entryKind`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EntryKind {
    Range,
    Node,
    StringLiteral,
    SearchedLocalFoundProperty,
    SearchedPropertyFoundLocal,
}

/// tsgo `ReferenceEntry`: a node's name (or the node, a token) or a range
/// of a file, with the node whose range is its context.
#[derive(Clone, Debug)]
pub(crate) struct ReferenceEntry {
    pub(crate) kind: EntryKind,
    /// None for a range entry.
    pub(crate) node: Option<Touching>,
    pub(crate) context: Option<NodeId>,
    pub(crate) file: usize,
    pub(crate) range: Range<usize>,
}

/// tsgo `SymbolAndEntries`.
#[derive(Clone, Debug)]
pub(crate) struct SymbolAndEntries {
    pub(crate) definition: Option<Definition>,
    pub(crate) references: Vec<ReferenceEntry>,
}

impl SymbolAndEntries {
    /// tsgo `NewSymbolAndEntries`.
    pub(crate) fn new(
        kind: DefinitionKind,
        node: Option<Touching>,
        symbol: Option<SymbolId>,
        references: Vec<ReferenceEntry>,
    ) -> Self {
        Self {
            definition: Some(Definition { kind, symbol, node }),
            references,
        }
    }
}

/// tsgo `SymbolAndEntriesData`.
pub(crate) struct SymbolAndEntriesData {
    pub(crate) original_node: Option<Touching>,
    pub(crate) symbols_and_entries: Vec<SymbolAndEntries>,
}

impl LanguageService<'_> {
    /// tsgo `ProvideReferences` (one project; the other projects of the
    /// orchestrator come with L7e).
    pub fn provide_references(
        &mut self,
        params: &ReferenceParams,
    ) -> Result<ReferencesResponse, LsError> {
        let file = self.file(&params.text_document.uri);
        let include_declaration = params.context.include_declaration;
        let result = self.with_request(file, &params.position, false, |request, position| {
            let data = request.provide_symbols_and_entries(file, position, false, false)?;
            Ok(request.symbol_and_entries_to_references(data.as_ref(), include_declaration))
        })?;
        Ok(result.unwrap_or(ReferencesResponse::Locations(Vec::new())))
    }

    /// tsgo `ProvideImplementations` (one project).
    pub fn provide_implementations(
        &mut self,
        params: &ImplementationParams,
    ) -> Result<ImplementationResponse, LsError> {
        let file = self.file(&params.text_document.uri);
        let links = self.capabilities.text_document.implementation.link_support;
        let result = self.with_request(file, &params.position, links, |request, position| {
            let data = request.provide_symbols_and_entries(file, position, false, true)?;
            Ok(request.symbol_and_entries_to_implementations(data.as_ref(), links))
        })?;
        Ok(result.unwrap_or(ImplementationResponse::Locations(Vec::new())))
    }

    /// tsgo `ProvideVSReferences` (one project).
    pub fn provide_vs_references(
        &mut self,
        params: &ReferenceParams,
    ) -> Result<VSReferencesResponse, LsError> {
        let file = self.file(&params.text_document.uri);
        let vs_capability = self.capabilities._vs_supports_visual_studio_extensions;
        let result = self.with_request(file, &params.position, false, |request, position| {
            let data = request.provide_symbols_and_entries(file, position, false, false)?;
            request.symbol_and_entries_to_vs_references(data.as_ref(), vs_capability)
        })?;
        Ok(result.unwrap_or(VSReferencesResponse::VSReferenceItems(Vec::new())))
    }
}

impl Request<'_, '_> {
    /// tsgo `symbolAndEntriesToVSReferences`: each definition, then its
    /// references other than its declarations, read or written.
    fn symbol_and_entries_to_vs_references(
        &mut self,
        data: Option<&SymbolAndEntriesData>,
        vs_capability: bool,
    ) -> CheckResult<VSReferencesResponse> {
        let mut items = Vec::new();
        let mut id = 0;
        let project_name = self.project_id.unwrap_or_default().to_owned();
        let Some(data) = data else {
            return Ok(VSReferencesResponse::VSReferenceItems(items));
        };
        for entry in &data.symbols_and_entries {
            let Some(definition) = &entry.definition else {
                continue;
            };
            let Some((location, display_text)) = self
                .definition_to_referenced_symbol_definition_info(
                    definition,
                    data.original_node,
                    vs_capability,
                )?
            else {
                continue;
            };
            let definition_id = id;
            items.push(VSReferenceItem {
                _vs_id: definition_id,
                _vs_definition_id: None,
                _vs_kind: Some(vec![VSReferenceKind::UNKNOWN]),
                _vs_location: location,
                _vs_definition_text: Some(display_text),
                _vs_project_name: Some(project_name.clone()),
                _vs_containing_type: Some(String::new()),
            });
            id += 1;
            for reference in &entry.references {
                // Skip the declaration itself (already represented by the definition item)
                if definition.symbol.is_some()
                    && self.is_declaration_of_symbol(reference.node, definition.symbol)
                {
                    continue;
                }
                let kind = match reference.node {
                    Some(node)
                        if reference.kind != EntryKind::Range
                            && self.is_write_access_for_reference(node) =>
                    {
                        VSReferenceKind::WRITE
                    }
                    _ => VSReferenceKind::READ,
                };
                items.push(VSReferenceItem {
                    _vs_id: id,
                    _vs_definition_id: Some(definition_id),
                    _vs_kind: Some(vec![kind]),
                    _vs_location: self.location_of_entry(reference),
                    _vs_definition_text: None,
                    _vs_project_name: Some(project_name.clone()),
                    _vs_containing_type: None,
                });
                id += 1;
            }
        }
        Ok(VSReferencesResponse::VSReferenceItems(items))
    }

    /// tsgo `definitionToReferencedSymbolDefinitionInfo`: a definition's
    /// location and display text.
    fn definition_to_referenced_symbol_definition_info(
        &mut self,
        definition: &Definition,
        original_node: Option<Touching>,
        vs_capability: bool,
    ) -> CheckResult<Option<(Location, VSClassifiedTextElement)>> {
        let simple = |text: String, classification: &str| VSClassifiedTextElement {
            runs: vec![VSClassifiedTextRun {
                classification_type_name: classification.to_owned(),
                text,
                marker_tag_type: None,
                style: 0,
                _vs_type: Default::default(),
            }],
            _vs_type: Default::default(),
        };
        match definition.kind {
            DefinitionKind::Symbol => {
                let Some(symbol) = definition.symbol else {
                    return Ok(None);
                };
                let element =
                    self.definition_kind_and_display_parts(symbol, original_node, vs_capability)?;
                let node = match self.declarations_of(symbol).first() {
                    Some(&declaration) => {
                        Touching::Node(self.name_of(declaration).unwrap_or(declaration))
                    }
                    None => match original_node {
                        Some(node) => node,
                        None => return Ok(None),
                    },
                };
                Ok(Some((self.location_of_node(node), element)))
            }
            DefinitionKind::Label => {
                let Some(node) = definition.node else {
                    return Ok(None);
                };
                let text = self.node_text(node);
                Ok(Some((self.location_of_node(node), simple(text, "text"))))
            }
            DefinitionKind::Keyword => {
                let Some(node) = definition.node else {
                    return Ok(None);
                };
                let name = tsc_syntax::tokens::token_to_string(self.kind(node))
                    .unwrap_or_default()
                    .to_owned();
                Ok(Some((self.location_of_node(node), simple(name, "keyword"))))
            }
            DefinitionKind::This => {
                let (Some(node), Some(symbol)) = (definition.node, definition.symbol) else {
                    return Ok(None);
                };
                let element =
                    self.definition_kind_and_display_parts(symbol, Some(node), vs_capability)?;
                Ok(Some((self.location_of_node(node), element)))
            }
            DefinitionKind::String => {
                let Some(node) = definition.node else {
                    return Ok(None);
                };
                let text = self.node_text(node);
                Ok(Some((self.location_of_node(node), simple(text, "string"))))
            }
            // tsgo never records the referenced file of a triple-slash
            // reference's definition.
            DefinitionKind::TripleSlashReference => Ok(None),
        }
    }

    /// tsgo `getDefinitionKindAndDisplayParts`: the symbol's quick info, as
    /// runs for a client with Visual Studio extensions, else as one run.
    fn definition_kind_and_display_parts(
        &mut self,
        symbol: SymbolId,
        original_node: Option<Touching>,
        vs_capability: bool,
    ) -> CheckResult<VSClassifiedTextElement> {
        let meaning = search::get_intersecting_meaning_from_declarations(
            self,
            original_node,
            symbol,
            crate::meaning::SemanticMeaning::ALL,
        );
        let node = original_node.expect("a definition's original node");
        let info = self.quick_info_and_declaration_at_location(
            Some(symbol),
            node,
            &mut tsc_checker::VerbosityContext::default(),
            meaning,
            vs_capability,
        )?;
        if vs_capability {
            return Ok(VSClassifiedTextElement {
                runs: info.runs.unwrap_or_default(),
                _vs_type: Default::default(),
            });
        }
        Ok(VSClassifiedTextElement {
            runs: vec![VSClassifiedTextRun {
                classification_type_name: "text".to_owned(),
                text: info.text,
                marker_tag_type: None,
                style: 0,
                _vs_type: Default::default(),
            }],
            _vs_type: Default::default(),
        })
    }

    /// tsgo `getLocationOfEntryForFeature` of a node entry.
    fn location_of_node(&self, node: Touching) -> Location {
        let (file, range) = self.range_of_touching(node);
        let source = self.state.binder.source(file);
        self.source_maps.location(source, range)
    }

    /// tsgo `Node.Text` of a name, label or string literal.
    fn node_text(&self, node: Touching) -> String {
        let Touching::Node(id) = node else {
            return String::new();
        };
        match &self.state.binder.node_record(id).data {
            NodeData::StringLiteral(data) => data.text.to_string_lossy().into_owned(),
            NodeData::NoSubstitutionTemplateLiteral(data) => {
                data.text.to_string_lossy().into_owned()
            }
            _ => node_util::id_text(self.state.binder.source_of_node(id), id)
                .unwrap_or_default()
                .to_owned(),
        }
    }

    /// tsgo `ast.IsWriteAccessForReference`.
    fn is_write_access_for_reference(&self, node: Touching) -> bool {
        let id = match node {
            Touching::Node(id) => id,
            Touching::Token { kind, .. } => return kind == SyntaxKind::DefaultKeyword,
        };
        let declaration = self.declaration_from_name(id);
        declaration.is_some_and(|declaration| self.declaration_is_write_access(declaration))
            || self.node_kind(id) == SyntaxKind::DefaultKeyword
            || self.state.is_write_access(id)
    }

    /// tsgo `ast.declarationIsWriteAccess` (tsgo panics on a kind it does not
    /// know).
    fn declaration_is_write_access(&self, declaration: NodeId) -> bool {
        let record = self.state.binder.node_record(declaration);
        // Consider anything in an ambient declaration to be a write access since it may be coming from JS.
        if tsc_types::NodeFlags::from_bits(record.flags).intersects(tsc_types::NodeFlags::AMBIENT) {
            return true;
        }
        let source = self.state.binder.source_of_node(declaration);
        match &record.data {
            NodeData::BinaryExpression(_)
            | NodeData::BindingElement(_)
            | NodeData::ClassDeclaration(_)
            | NodeData::ClassExpression(_)
            | NodeData::EnumDeclaration(_)
            | NodeData::EnumMember(_)
            | NodeData::ExportSpecifier(_)
            | NodeData::ImportClause(_)
            | NodeData::ImportEqualsDeclaration(_)
            | NodeData::ImportSpecifier(_)
            | NodeData::InterfaceDeclaration(_)
            | NodeData::JSDocCallbackTag(_)
            | NodeData::JSDocTypedefTag(_)
            | NodeData::JsxAttribute(_)
            | NodeData::ModuleDeclaration(_)
            | NodeData::NamespaceExportDeclaration(_)
            | NodeData::NamespaceImport(_)
            | NodeData::NamespaceExport(_)
            | NodeData::Parameter(_)
            | NodeData::ShorthandPropertyAssignment(_)
            | NodeData::TypeAliasDeclaration(_)
            | NodeData::TypeParameter(_) => true,
            // In `({ x: y } = 0);`, `x` is not a write access.
            NodeData::PropertyAssignment(_) => !record.parent.is_some_and(|parent| {
                self.state
                    .is_array_literal_or_object_literal_destructuring_pattern(parent)
            }),
            // functions considered write if they provide a value (have a body)
            NodeData::FunctionDeclaration(_)
            | NodeData::FunctionExpression(_)
            | NodeData::Constructor(_)
            | NodeData::MethodDeclaration(_)
            | NodeData::GetAccessor(_)
            | NodeData::SetAccessor(_) => node_util::body_of(source, declaration).is_some(),
            // variable/property write if initializer present or is in catch clause
            NodeData::VariableDeclaration(data) => {
                data.initializer.is_some()
                    || record
                        .parent
                        .is_some_and(|parent| self.node_kind(parent) == SyntaxKind::CatchClause)
            }
            NodeData::PropertyDeclaration(data) => {
                data.initializer.is_some()
                    || record
                        .parent
                        .is_some_and(|parent| self.node_kind(parent) == SyntaxKind::CatchClause)
            }
            _ => false,
        }
    }

    /// tsgo `provideSymbolsAndEntriesAtPosition` (a file without a content
    /// mapper: one position).
    pub(crate) fn provide_symbols_and_entries(
        &mut self,
        file: usize,
        position: u32,
        is_rename: bool,
        implementations: bool,
    ) -> CheckResult<Option<SymbolAndEntriesData>> {
        let source = self.state.binder.source(file);
        let found = crate::astnav::Navigator::new(source).touching_property_name(position);
        let Some(mut node) = touching(found) else {
            return Ok(None);
        };
        if is_rename {
            // Adjust modifier/keyword nodes to the declaration name, matching Strada's findRenameLocations.
            node = self.adjusted_location(node, true);
        }
        if is_rename && !self.node_is_eligible_for_rename(node)
            || implementations && self.kind(node) == SyntaxKind::SourceFile
        {
            return Ok(None);
        }
        let entries = self.get_symbol_and_entries(position, node, is_rename, implementations)?;
        if !implementations {
            return Ok(Some(SymbolAndEntriesData {
                original_node: Some(node),
                symbols_and_entries: entries,
            }));
        }
        let mut implementation_entries = Vec::new();
        let mut queue = std::collections::VecDeque::new();
        let mut seen_nodes: HashSet<Option<Touching>> = HashSet::new();
        let mut seen_definitions: HashSet<Option<SymbolId>> = HashSet::new();
        let mut add_to_queue =
            |symbol_and_entries: Vec<SymbolAndEntries>,
             queue: &mut std::collections::VecDeque<ReferenceEntry>| {
                for entry in symbol_and_entries {
                    let mut new_references = Vec::new();
                    for reference in entry.references {
                        if seen_nodes.insert(reference.node) {
                            queue.push_back(reference.clone());
                            new_references.push(reference);
                        }
                    }
                    let definition_symbol = entry
                        .definition
                        .as_ref()
                        .and_then(|definition| definition.symbol);
                    if !new_references.is_empty()
                        || entry.definition.is_none()
                        || seen_definitions.insert(definition_symbol)
                    {
                        implementation_entries.push(SymbolAndEntries {
                            definition: entry.definition,
                            references: new_references,
                        });
                    }
                }
            };
        add_to_queue(entries, &mut queue);
        while let Some(entry) = queue.pop_front() {
            if let Some(node) = entry.node {
                let more =
                    self.get_symbol_and_entries(self.pos(node), node, is_rename, implementations)?;
                add_to_queue(more, &mut queue);
            }
        }
        Ok(Some(SymbolAndEntriesData {
            original_node: Some(node),
            symbols_and_entries: implementation_entries,
        }))
    }

    /// tsgo `getSymbolAndEntries`.
    fn get_symbol_and_entries(
        &mut self,
        position: u32,
        node: Touching,
        is_rename: bool,
        implementations: bool,
    ) -> CheckResult<Vec<SymbolAndEntries>> {
        let options = if is_rename {
            RefOptions {
                use_: ReferenceUse::Rename,
                implementations: false,
                use_aliases_for_rename: self.preferences.provide_prefix_and_suffix_text_for_rename
                    != Some(false),
            }
        } else {
            RefOptions {
                use_: ReferenceUse::References,
                implementations,
                use_aliases_for_rename: false,
            }
        };
        let files: Vec<usize> = (0..self.state.binder.file_count()).collect();
        get_referenced_symbols_for_node(self, position, node, &files, options)
    }

    /// tsgo `symbolAndEntriesToReferences`: the locations of the
    /// references, each once.
    fn symbol_and_entries_to_references(
        &mut self,
        data: Option<&SymbolAndEntriesData>,
        include_declaration: bool,
    ) -> ReferencesResponse {
        let mut locations = Vec::new();
        let mut seen_locations: HashSet<LocationKey> = HashSet::new();
        for symbol in data
            .map(|data| data.symbols_and_entries.as_slice())
            .unwrap_or_default()
        {
            for location in
                self.convert_symbol_and_entries_to_locations(symbol, include_declaration)
            {
                if seen_locations.insert(LocationKey::of(&location)) {
                    locations.push(location);
                }
            }
        }
        ReferencesResponse::Locations(locations)
    }

    /// tsgo `symbolAndEntriesToImplementations`.
    fn symbol_and_entries_to_implementations(
        &mut self,
        data: Option<&SymbolAndEntriesData>,
        links: bool,
    ) -> ImplementationResponse {
        let mut seen_nodes: HashSet<Option<Touching>> = HashSet::new();
        let mut entries = Vec::new();
        for entry in data
            .map(|data| data.symbols_and_entries.as_slice())
            .unwrap_or_default()
        {
            for reference in &entry.references {
                if seen_nodes.insert(reference.node) {
                    entries.push(reference.clone());
                }
            }
        }
        // tsgo `combineImplementations` (the server's cross-project search)
        // keeps each location once.
        let mut seen_locations: HashSet<LocationKey> = HashSet::new();
        if links {
            let mut links = self.convert_entries_to_location_links(&entries);
            links.retain(|link| {
                seen_locations.insert(LocationKey::of(&Location {
                    uri: link.target_uri.clone(),
                    range: link.target_selection_range.clone(),
                }))
            });
            return ImplementationResponse::DefinitionLinks(links);
        }
        let mut locations = self.convert_entries_to_locations(&entries);
        locations.retain(|location| seen_locations.insert(LocationKey::of(location)));
        ImplementationResponse::Locations(locations)
    }

    /// tsgo `convertSymbolAndEntriesToLocations`.
    fn convert_symbol_and_entries_to_locations(
        &mut self,
        symbol: &SymbolAndEntries,
        include_declarations: bool,
    ) -> Vec<Location> {
        let definition_symbol = symbol
            .definition
            .as_ref()
            .and_then(|definition| definition.symbol);
        let references: Vec<ReferenceEntry> =
            if !include_declarations && symbol.definition.is_some() {
                symbol
                    .references
                    .iter()
                    .filter(|entry| !self.is_declaration_of_symbol(entry.node, definition_symbol))
                    .cloned()
                    .collect()
            } else {
                symbol.references.clone()
            };
        self.convert_entries_to_locations(&references)
    }

    /// tsgo `convertEntriesToLocations`.
    pub(crate) fn convert_entries_to_locations(&self, entries: &[ReferenceEntry]) -> Vec<Location> {
        entries
            .iter()
            .map(|entry| self.location_of_entry(entry))
            .collect()
    }

    /// tsgo `getLocationOfEntryForFeature`: the entry's range (through a
    /// declaration map).
    pub(crate) fn location_of_entry(&self, entry: &ReferenceEntry) -> Location {
        let source = self.state.binder.source(entry.file);
        self.source_maps.location(source, entry.range.clone())
    }

    /// tsgo `convertEntriesToLocationLinks`: each entry's range, within its
    /// context's.
    fn convert_entries_to_location_links(&self, entries: &[ReferenceEntry]) -> Vec<LocationLink> {
        let mut links = Vec::with_capacity(entries.len());
        for entry in entries {
            let location = self.location_of_entry(entry);
            let target_selection_range = location.range.clone();
            let mut target_range = target_selection_range.clone();
            // For entries with nodes, compute ranges directly from the node
            if entry.node.is_some() {
                if let Some(context) = entry.context {
                    let context_range = crate::utilities::range_of_node(self.state, context);
                    if context_range != entry.range {
                        let source = self.state.binder.source(entry.file);
                        let context_location = self.source_maps.location(source, context_range);
                        if context_location.uri == location.uri {
                            target_range = context_location.range;
                        }
                    }
                }
            }
            let source = self.state.binder.source(entry.file);
            links.push(LocationLink {
                origin_selection_range: None,
                target_uri: lsproto::DocumentUri(tsc_program::uri::file_name_to_document_uri(
                    &source.file_name.to_string_lossy(),
                )),
                target_range,
                target_selection_range,
            });
        }
        links
    }

    /// tsgo `isDeclarationOfSymbol`: whether the reference is at a
    /// declaration of the symbol.
    fn is_declaration_of_symbol(&self, node: Option<Touching>, target: Option<SymbolId>) -> bool {
        let (Some(node), Some(target)) = (node, target) else {
            return false;
        };
        let source = match node {
            Touching::Node(id) => {
                if let Some(declaration) = self.declaration_from_name(id) {
                    Some(declaration)
                } else if self.is_literal_computed_property_declaration_name(id) {
                    self.state
                        .binder
                        .node_record(id)
                        .parent
                        .and_then(|parent| self.state.binder.node_record(parent).parent)
                } else if self.node_kind(id) == SyntaxKind::DefaultKeyword {
                    self.state.binder.node_record(id).parent
                } else {
                    None
                }
            }
            Touching::Token { kind, parent, .. } => match kind {
                SyntaxKind::DefaultKeyword => Some(parent),
                SyntaxKind::ConstructorKeyword
                    if self.node_kind(parent) == SyntaxKind::Constructor =>
                {
                    self.state.binder.node_record(parent).parent
                }
                _ => None,
            },
        };
        source.is_some_and(|source| self.declarations_of(target).contains(&source))
    }

    /// tsgo `ast.GetDeclarationFromName`.
    pub(crate) fn declaration_from_name(&self, name: NodeId) -> Option<NodeId> {
        let parent = self.state.binder.node_record(name).parent?;
        let kind = self.node_kind(name);
        let source = self.state.binder.source_of_node(name);
        match kind {
            SyntaxKind::StringLiteral
            | SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::NumericLiteral
            | SyntaxKind::Identifier => {
                if kind != SyntaxKind::Identifier
                    && self.node_kind(parent) == SyntaxKind::ComputedPropertyName
                {
                    return self.state.binder.node_record(parent).parent;
                }
                if node_util::is_declaration_node(source, parent) {
                    return (self.name_of(parent) == Some(name)).then_some(parent);
                }
                if self.node_kind(parent) == SyntaxKind::QualifiedName {
                    let tag = self.state.binder.node_record(parent).parent?;
                    return (self.node_kind(tag) == SyntaxKind::JSDocParameterTag
                        && self.name_of(tag) == Some(parent))
                    .then_some(tag);
                }
                let binary = self.state.binder.node_record(parent).parent?;
                let NodeData::BinaryExpression(data) = &self.state.binder.node_record(binary).data
                else {
                    return None;
                };
                if tsc_binder::assignment::get_assignment_declaration_kind(source, binary)
                    == tsc_binder::assignment::AssignmentDeclarationKind::None
                {
                    return None;
                }
                // (binExp.left as BindableStaticNameExpression).symbol || binExp.symbol
                let left_has_symbol = data
                    .left
                    .is_some_and(|left| self.state.binder.node_symbol(left).is_some());
                if (left_has_symbol || self.state.binder.node_symbol(binary).is_some())
                    && node_util::get_name_of_declaration(source, binary) == Some(name)
                {
                    return Some(binary);
                }
                None
            }
            SyntaxKind::PrivateIdentifier => (node_util::is_declaration_node(source, parent)
                && self.name_of(parent) == Some(name))
            .then_some(parent),
            _ => None,
        }
    }

    /// tsgo `ast.IsLiteralComputedPropertyDeclarationName`.
    fn is_literal_computed_property_declaration_name(&self, node: NodeId) -> bool {
        let Some(parent) = self.state.binder.node_record(node).parent else {
            return false;
        };
        matches!(
            self.node_kind(node),
            SyntaxKind::StringLiteral | SyntaxKind::NumericLiteral
        ) && self.node_kind(parent) == SyntaxKind::ComputedPropertyName
            && self
                .state
                .binder
                .node_record(parent)
                .parent
                .is_some_and(|declaration| {
                    crate::utilities::has_declaration_data(self.node_kind(declaration))
                })
    }

    /// tsgo `nodeIsEligibleForRename` (rename.go).
    pub(crate) fn node_is_eligible_for_rename(&self, node: Touching) -> bool {
        match self.kind(node) {
            SyntaxKind::Identifier
            | SyntaxKind::PrivateIdentifier
            | SyntaxKind::StringLiteral
            | SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::ThisKeyword => true,
            SyntaxKind::NumericLiteral => match node {
                Touching::Node(id) => {
                    self.is_literal_name_of_property_declaration_or_index_access(id)
                }
                Touching::Token { .. } => false,
            },
            _ => false,
        }
    }

    /// tsgo `newNodeEntryWithKind`: an entry at the node's name, or the node.
    pub(crate) fn new_node_entry_with_kind(
        &self,
        node: Touching,
        kind: EntryKind,
    ) -> ReferenceEntry {
        let named = match node {
            Touching::Node(id) => self.name_of(id).map_or(node, Touching::Node),
            token => token,
        };
        let context = self.context_node_for_node_entry(node);
        let (file, range) = self.range_of_touching(named);
        ReferenceEntry {
            kind,
            node: Some(named),
            context,
            file,
            range,
        }
    }

    /// tsgo `newNodeEntry`.
    pub(crate) fn new_node_entry(&self, node: Touching) -> ReferenceEntry {
        self.new_node_entry_with_kind(node, EntryKind::Node)
    }

    /// tsgo `getRangeOfNode` (without an end node) of a node or token: from
    /// its token's start, a string literal's without its quotes.
    pub(crate) fn range_of_touching(&self, node: Touching) -> (usize, Range<usize>) {
        match node {
            Touching::Node(id) => (
                self.state.binder.file_index_of_node(id),
                crate::utilities::range_of_node(self.state, id),
            ),
            Touching::Token {
                pos, end, parent, ..
            } => {
                let file = self.state.binder.file_index_of_node(parent);
                let text = self.state.binder.source(file).text();
                let start = tsc_syntax::skip_trivia(text, pos as usize);
                (file, start..end as usize)
            }
        }
    }

    /// tsgo `getContextNodeForNodeEntry`.
    fn context_node_for_node_entry(&self, node: Touching) -> Option<NodeId> {
        if let Touching::Node(id) = node {
            if node_util::is_declaration_node(self.state.binder.source_of_node(id), id) {
                return crate::definition::context_node(self.state, id);
            }
        }
        let parent = self.parent(node)?;
        let parent_kind = self.node_kind(parent);
        let source = self.state.binder.source_of_node(parent);
        if !node_util::is_declaration_node(source, parent)
            && parent_kind != SyntaxKind::ExportAssignment
        {
            // Special property assignment in javascript
            if let Touching::Node(id) = node {
                if self.state.is_in_js_file(id) {
                    let binary_expression = if parent_kind == SyntaxKind::BinaryExpression {
                        Some(parent)
                    } else if matches!(
                        parent_kind,
                        SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
                    ) {
                        self.state.binder.node_record(parent).parent.filter(|&grandparent| {
                            matches!(
                                &self.state.binder.node_record(grandparent).data,
                                NodeData::BinaryExpression(binary) if binary.left == Some(parent)
                            )
                        })
                    } else {
                        None
                    };
                    if let Some(binary_expression) = binary_expression {
                        let binary_source = self.state.binder.source_of_node(binary_expression);
                        if tsc_binder::assignment::get_assignment_declaration_kind(
                            binary_source,
                            binary_expression,
                        ) != tsc_binder::assignment::AssignmentDeclarationKind::None
                        {
                            return crate::definition::context_node(self.state, binary_expression);
                        }
                    }
                }
            }
            // Jsx Tags
            match parent_kind {
                SyntaxKind::JsxOpeningElement | SyntaxKind::JsxClosingElement => {
                    return self.state.binder.node_record(parent).parent;
                }
                SyntaxKind::JsxSelfClosingElement
                | SyntaxKind::LabeledStatement
                | SyntaxKind::BreakStatement
                | SyntaxKind::ContinueStatement => return Some(parent),
                SyntaxKind::StringLiteral | SyntaxKind::NoSubstitutionTemplateLiteral => {
                    // tsgo tests the node's parent as a module specifier,
                    // which a string literal never is.
                }
                _ => {}
            }
            // Handle computed property name
            let mut current = match node {
                Touching::Node(id) => Some(id),
                Touching::Token { parent, .. } => Some(parent),
            };
            while let Some(id) = current {
                if self.node_kind(id) == SyntaxKind::ComputedPropertyName {
                    let declaration = self.state.binder.node_record(id).parent?;
                    return crate::definition::context_node(self.state, declaration);
                }
                current = self.state.binder.node_record(id).parent;
            }
            return None;
        }
        let parent_name = self.name_of(parent);
        let is_parent_name = matches!(node, Touching::Node(id) if parent_name == Some(id));
        let is_property_name_of_specifier_or_binding =
            match (node, &self.state.binder.node_record(parent).data) {
                (Touching::Node(id), NodeData::ImportSpecifier(data)) => {
                    data.property_name == Some(id)
                }
                (Touching::Node(id), NodeData::ExportSpecifier(data)) => {
                    data.property_name == Some(id)
                }
                (Touching::Node(id), NodeData::BindingElement(data)) => {
                    data.property_name == Some(id)
                }
                _ => false,
            };
        let is_default_export = self.kind(node) == SyntaxKind::DefaultKeyword
            && node_util::has_syntactic_modifier(source, parent, ModifierFlags::EXPORT_DEFAULT);
        if is_parent_name
            || parent_kind == SyntaxKind::Constructor
            || parent_kind == SyntaxKind::ExportAssignment
            || is_property_name_of_specifier_or_binding
            || is_default_export
        {
            return crate::definition::context_node(self.state, parent);
        }
        None
    }

    /// tsgo `isLiteralNameOfPropertyDeclarationOrIndexAccess`.
    pub(crate) fn is_literal_name_of_property_declaration_or_index_access(
        &self,
        node: NodeId,
    ) -> bool {
        let Some(parent) = self.state.binder.node_record(node).parent else {
            return false;
        };
        match &self.state.binder.node_record(parent).data {
            NodeData::PropertyDeclaration(_)
            | NodeData::PropertySignature(_)
            | NodeData::PropertyAssignment(_)
            | NodeData::EnumMember(_)
            | NodeData::MethodDeclaration(_)
            | NodeData::MethodSignature(_)
            | NodeData::GetAccessor(_)
            | NodeData::SetAccessor(_)
            | NodeData::ModuleDeclaration(_) => {
                node_util::get_name_of_declaration(self.state.binder.source_of_node(parent), parent)
                    == Some(node)
            }
            NodeData::ElementAccessExpression(data) => data.argument_expression == Some(node),
            NodeData::ComputedPropertyName(_) => true,
            NodeData::LiteralType(_) => {
                self.state
                    .binder
                    .node_record(parent)
                    .parent
                    .is_some_and(|grandparent| {
                        self.node_kind(grandparent) == SyntaxKind::IndexedAccessType
                    })
            }
            _ => false,
        }
    }
}

/// A location's identity for tsgo's `collections.Set[lsproto.Location]`.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct LocationKey {
    uri: String,
    range: (u32, u32, u32, u32),
}

impl LocationKey {
    pub(crate) fn of(location: &Location) -> Self {
        Self {
            uri: location.uri.0.clone(),
            range: (
                location.range.start.line,
                location.range.start.character,
                location.range.end.line,
                location.range.end.character,
            ),
        }
    }
}

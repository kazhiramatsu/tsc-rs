//! tsgo `ls/codelens.go`: the references and implementations lenses of a
//! file's declarations (`textDocument/codeLens`), and their counts with
//! the locations when resolved (`codeLens/resolve`).

use std::collections::HashSet;

use tsc_binder::node_util;
use tsc_diagnostics::gen;
use tsc_diagnostics::spanmap::{Fidelity, FEATURE_CODE_LENS};
use tsc_diagnostics::MessageCatalog;
use tsc_lsproto::{
    CodeLens, CodeLensData, CodeLensKind, CodeLensResponse, CodeLensesOrNull, Command, DocumentUri,
    ImplementationResponse, Location, ReferencesResponse,
};
use tsc_syntax::{skip_trivia, NodeId, SyntaxKind};
use tsc_types::{ModifierFlags, SymbolId};

use crate::cross_project::{ProjectSearch, SearchPosition};
use crate::language_service::{LanguageService, LsError};
use crate::lsutil::{localize, CodeLensUserPreferences};
use crate::request::Request;

impl LanguageService<'_> {
    /// tsgo `ProvideCodeLenses`: a lens of each kind the preferences enable
    /// on each declaration that takes one in the file and its supplemental
    /// outputs, the overloads of a symbol once.
    pub fn provide_code_lenses(&mut self, uri: &DocumentUri) -> Result<CodeLensResponse, LsError> {
        let preferences = &self.user_preferences_ref().code_lens_user_preferences;
        if preferences.references_code_lens_enabled != Some(true)
            && preferences.implementations_code_lens_enabled != Some(true)
        {
            return Ok(CodeLensesOrNull::Null);
        }
        let file = self.file(uri);
        let lenses = self.with_program_request(false, |request| {
            let mut collector = CodeLensCollector {
                uri,
                preferences,
                lenses: Vec::new(),
                seen: HashSet::new(),
                last_symbol: None,
                supplemental_file_index: None,
            };
            for projection in request.source_projections(file) {
                // Keeps track of the last symbol to avoid duplicating code lenses across overloads.
                collector.last_symbol = None;
                collector.supplemental_file_index = request.supplemental_file_index(projection);
                let root = request.state.binder.source(projection).root;
                collector.visit(request, root);
            }
            Ok(collector.lenses)
        })?;
        Ok(CodeLensesOrNull::CodeLenses(lenses.unwrap_or_default()))
    }

    /// tsgo `ResolveCodeLens` of one project (no orchestrator; the server
    /// searches across projects): the lens's command over the references
    /// (the declarations left out) or implementations (the lens's own node
    /// left out) at the lens's data position.
    pub fn resolve_code_lens(
        &mut self,
        code_lens: CodeLens,
        show_locations_command_name: Option<&str>,
    ) -> Result<CodeLens, LsError> {
        let data = code_lens.data.clone().expect("a code lens's data");
        let at = SearchPosition::Offset {
            position: data.position as u32,
            supplemental_file_index: data.supplemental_file_index,
        };
        let locations = if data.kind == CodeLensKind::REFERENCES {
            match self.search_references(&data.uri, &at, false, false)? {
                Some(ProjectSearch {
                    response: ReferencesResponse::Locations(locations),
                    ..
                }) => locations,
                _ => Vec::new(),
            }
        } else if data.kind == CodeLensKind::IMPLEMENTATIONS {
            match self.search_implementations(&data.uri, &at, true, false)? {
                Some(ProjectSearch {
                    response: ImplementationResponse::Locations(locations),
                    ..
                }) => locations,
                _ => Vec::new(),
            }
        } else {
            Vec::new()
        };
        Ok(code_lens_with_command(
            self.catalog,
            code_lens,
            locations,
            show_locations_command_name,
        ))
    }
}

/// The end of tsgo `ResolveCodeLens`: the lens's command, titled by the
/// count of its locations, showing them with the client's command when it
/// gave one.
pub fn code_lens_with_command(
    catalog: Option<&dyn MessageCatalog>,
    mut code_lens: CodeLens,
    locations: Vec<Location>,
    show_locations_command_name: Option<&str>,
) -> CodeLens {
    let data = code_lens.data.clone().expect("a code lens's data");
    let references = data.kind == CodeLensKind::REFERENCES;
    let implementations = data.kind == CodeLensKind::IMPLEMENTATIONS;
    let count = locations.len().to_string();
    let title = match (references, implementations, locations.len()) {
        (true, _, 1) => localize(catalog, &gen::_1_reference, &[]),
        (true, _, _) => localize(catalog, &gen::_0_references, &[&count]),
        (_, true, 1) => localize(catalog, &gen::_1_implementation, &[]),
        (_, true, _) => localize(catalog, &gen::_0_implementations, &[&count]),
        _ => String::new(),
    };
    let mut command = Command {
        title,
        ..Command::default()
    };
    if let (false, Some(name)) = (locations.is_empty(), show_locations_command_name) {
        command.command = name.to_owned();
        command.arguments = Some(vec![
            serde_json::to_value(&data.uri).expect("a URI"),
            serde_json::to_value(&code_lens.range.start).expect("a position"),
            serde_json::to_value(&locations).expect("locations"),
        ]);
    }
    code_lens.command = Some(command);
    code_lens
}

/// tsgo `ProvideCodeLenses`'s walk: the lenses so far, each kind and range
/// once, and the last symbol a lens was considered for.
struct CodeLensCollector<'u, 'p> {
    uri: &'u DocumentUri,
    preferences: &'p CodeLensUserPreferences,
    lenses: Vec<CodeLens>,
    seen: HashSet<(String, (u32, u32, u32, u32))>,
    last_symbol: Option<Option<SymbolId>>,
    /// The projection's index among the file's supplemental outputs.
    supplemental_file_index: Option<i32>,
}

impl CodeLensCollector<'_, '_> {
    fn visit(&mut self, request: &Request<'_, '_>, node: NodeId) {
        let current = request.state.binder.node_symbol(node);
        if self.last_symbol != Some(current) {
            self.last_symbol = Some(current);
            if self.preferences.references_code_lens_enabled == Some(true)
                && is_valid_reference_lens_node(request, node, self.preferences)
            {
                self.add(request, node, CodeLensKind::REFERENCES);
            }
            if self.preferences.implementations_code_lens_enabled == Some(true)
                && is_valid_implementations_code_lens_node(request, node, self.preferences)
            {
                self.add(request, node, CodeLensKind::IMPLEMENTATIONS);
            }
        }
        let saved = self.last_symbol;
        for child in request.children(node) {
            self.visit(request, child);
        }
        self.last_symbol = saved;
    }

    /// tsgo `newCodeLensForNode`: a lens over the declaration from its
    /// name's first token.
    fn add(&mut self, request: &Request<'_, '_>, node: NodeId, kind: CodeLensKind) {
        let source = request.state.binder.source_of_node(node);
        let node_for_range = request.name_of(node).unwrap_or(node);
        let pos = skip_trivia(
            source.text(),
            request.state.binder.node_record(node_for_range).pos as usize,
        );
        let (range, fidelity) = request.lsp_range_for_feature(
            source,
            pos..request.state.binder.node_record(node).end as usize,
            FEATURE_CODE_LENS,
        );
        if fidelity == Fidelity::None {
            return;
        }
        let key = (
            kind.0.to_string(),
            (
                range.start.line,
                range.start.character,
                range.end.line,
                range.end.character,
            ),
        );
        if !self.seen.insert(key) {
            return;
        }
        self.lenses.push(CodeLens {
            range,
            command: None,
            data: Some(CodeLensData {
                kind,
                uri: self.uri.clone(),
                position: pos as i32,
                supplemental_file_index: self.supplemental_file_index,
            }),
        });
    }
}

/// tsgo `isValidImplementationsCodeLensNode`: interfaces, their methods
/// and (with the preference) class methods that are not private, and what
/// is abstract.
fn is_valid_implementations_code_lens_node(
    request: &Request<'_, '_>,
    node: NodeId,
    preferences: &CodeLensUserPreferences,
) -> bool {
    let source = request.state.binder.source_of_node(node);
    let parent_kind = request
        .state
        .binder
        .node_record(node)
        .parent
        .map(|parent| request.node_kind(parent));
    let has_modifier =
        |flags: ModifierFlags| node_util::has_syntactic_modifier(source, node, flags);
    match request.node_kind(node) {
        // Always show on interfaces
        SyntaxKind::InterfaceDeclaration => true,
        // If configured, show on interface methods
        SyntaxKind::MethodSignature => {
            preferences.implementations_code_lens_show_on_interface_methods == Some(true)
                && parent_kind == Some(SyntaxKind::InterfaceDeclaration)
        }
        // If configured, show on all class methods - but not private ones.
        SyntaxKind::MethodDeclaration
            if preferences.implementations_code_lens_show_on_all_class_methods == Some(true)
                && parent_kind == Some(SyntaxKind::ClassDeclaration) =>
        {
            !has_modifier(ModifierFlags::PRIVATE)
                && request
                    .name_of(node)
                    .is_none_or(|name| request.node_kind(name) != SyntaxKind::PrivateIdentifier)
        }
        // Always show on abstract classes/properties/methods
        SyntaxKind::MethodDeclaration
        | SyntaxKind::ClassDeclaration
        | SyntaxKind::Constructor
        | SyntaxKind::GetAccessor
        | SyntaxKind::SetAccessor
        | SyntaxKind::PropertyDeclaration => has_modifier(ModifierFlags::ABSTRACT),
        _ => false,
    }
}

/// tsgo `isValidReferenceLensNode`: exported functions and variables (all
/// functions with the preference), types, enums and their members, and the
/// members of classes, interfaces and type literals.
fn is_valid_reference_lens_node(
    request: &Request<'_, '_>,
    node: NodeId,
    preferences: &CodeLensUserPreferences,
) -> bool {
    let source = request.state.binder.source_of_node(node);
    match request.node_kind(node) {
        SyntaxKind::FunctionDeclaration
            if preferences.references_code_lens_show_on_all_functions == Some(true) =>
        {
            true
        }
        SyntaxKind::FunctionDeclaration | SyntaxKind::VariableDeclaration => {
            node_util::get_combined_modifier_flags(source, node).intersects(ModifierFlags::EXPORT)
        }
        SyntaxKind::ClassDeclaration
        | SyntaxKind::InterfaceDeclaration
        | SyntaxKind::TypeAliasDeclaration
        | SyntaxKind::EnumDeclaration
        | SyntaxKind::EnumMember => true,
        SyntaxKind::MethodDeclaration
        | SyntaxKind::MethodSignature
        | SyntaxKind::Constructor
        | SyntaxKind::GetAccessor
        | SyntaxKind::SetAccessor
        | SyntaxKind::PropertyDeclaration
        | SyntaxKind::PropertySignature => request
            .state
            .binder
            .node_record(node)
            .parent
            .is_some_and(|parent| {
                matches!(
                    request.node_kind(parent),
                    SyntaxKind::ClassDeclaration
                        | SyntaxKind::InterfaceDeclaration
                        | SyntaxKind::TypeLiteral
                )
            }),
        _ => false,
    }
}

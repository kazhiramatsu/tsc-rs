//! tsgo `ls/rename.go`: whether what is at a position can be renamed
//! (`textDocument/prepareRename`), and the edits that rename it
//! (`textDocument/rename`) — its references, with prefix or suffix text
//! where an alias keeps the other name.

use std::collections::{BTreeMap, HashMap};

use tsc_checker::state::CheckResult;
use tsc_diagnostics::gen;
use tsc_lsproto::{
    DocumentUri, Position, RenameParams, RenameResponse, TextEdit, WorkspaceEdit,
    WorkspaceEditOrNull,
};
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{JsStr, SymbolFlags, SymbolId, TypeFlags};

use crate::astnav::Navigator;
use crate::find_all_references::{
    is_object_binding_element_without_property_name, EntryKind, ReferenceEntry,
    SymbolAndEntriesData,
};
use crate::language_service::{LanguageService, LsError};
use crate::lsutil::{get_quote_preference, localize, QuotePreference};
use crate::request::Request;
use crate::utilities::{touching, Touching};

/// tsgo `RenameInfo`.
#[derive(Clone, Debug, Default)]
pub struct RenameInfo {
    pub can_rename: bool,
    pub localized_error_message: String,
    pub display_name: String,
    pub trigger_span: tsc_lsproto::Range,
    /// The file a module specifier's rename renames (empty without one).
    pub file_to_rename: String,
    pub new_file_name: String,
}

impl LanguageService<'_> {
    /// tsgo `GetRenameInfo` (a file without a content mapper: one position).
    pub fn get_rename_info(
        &mut self,
        new_name: &str,
        uri: &DocumentUri,
        position: &Position,
    ) -> Result<RenameInfo, LsError> {
        let file = self.file(uri);
        let result = self.with_request(file, position, false, |request, position| {
            let source = request.state.binder.source(file);
            let Some(node) = touching(Navigator::new(source).touching_property_name(position))
            else {
                return Ok(None);
            };
            let node = request.adjusted_location(node, true);
            if request.node_is_eligible_for_rename(node) {
                if let Some(info) = request.rename_info_for_node(new_name, node)? {
                    return Ok(Some(info));
                }
            }
            Ok(None)
        })?;
        match result.flatten() {
            Some(info) => Ok(info),
            None => Ok(rename_info_error(localize(
                self.catalog,
                &gen::You_cannot_rename_this_element,
                &[],
            ))),
        }
    }

    /// tsgo `ProvideRename` (one project; the cross-project search is
    /// L7e).
    pub fn provide_rename(&mut self, params: &RenameParams) -> Result<RenameResponse, LsError> {
        let file = self.file(&params.text_document.uri);
        let result = self.with_request(file, &params.position, false, |request, position| {
            let data = request.provide_symbols_and_entries(file, position, true, false)?;
            request.symbol_and_entries_to_rename(&params.new_name, data.as_ref())
        })?;
        // tsgo `combineRenameResponse` of the one project's answer.
        Ok(match result.flatten() {
            Some(changes) if !changes.is_empty() => {
                WorkspaceEditOrNull::WorkspaceEdit(WorkspaceEdit {
                    changes: Some(changes),
                    ..WorkspaceEdit::default()
                })
            }
            _ => WorkspaceEditOrNull::Null,
        })
    }
}

/// tsgo `renameEditKey`: a document and a range in it.
type EditKey = (String, (u32, u32, u32, u32));

/// tsgo `getRenameInfoError`.
fn rename_info_error(message: String) -> RenameInfo {
    RenameInfo {
        can_rename: false,
        localized_error_message: message,
        ..RenameInfo::default()
    }
}

impl Request<'_, '_> {
    /// tsgo `getRenameInfoForNode`: none when the node cannot be renamed
    /// without a reason to give.
    pub(crate) fn rename_info_for_node(
        &mut self,
        new_name: &str,
        node: Touching,
    ) -> CheckResult<Option<RenameInfo>> {
        let Some(symbol) = self.symbol_at(node)? else {
            let Touching::Node(id) = node else {
                return Ok(None);
            };
            if is_string_literal_like(self.node_kind(id)) {
                // Allow renaming of string literal types with contextual string literal types
                if let Some(ty) = self.contextual_type_from_parent_or_ancestor_type_node(id)? {
                    let flags = self.state.tables.flags_of(ty);
                    let string_literals = flags.intersects(TypeFlags::STRING_LITERAL)
                        || flags.intersects(TypeFlags::UNION)
                            && self.state.get_types_of_type(ty).into_iter().all(|member| {
                                self.state
                                    .tables
                                    .flags_of(member)
                                    .intersects(TypeFlags::STRING_LITERAL)
                            });
                    if string_literals {
                        let text = self.node_text(Touching::Node(id));
                        return Ok(Some(self.rename_info_success(node, text)));
                    }
                }
            } else if self.is_label_name(id) {
                let text = self.node_text(Touching::Node(id));
                return Ok(Some(self.rename_info_success(node, text)));
            }
            return Ok(None);
        };

        // Only allow a symbol to be renamed if it actually has at least one declaration.
        if self.declarations_of(symbol).is_empty() {
            return Ok(None);
        }

        if let Some(message) = self.rename_blocked_reason(node, symbol)? {
            return Ok(Some(rename_info_error(localize(
                self.catalog,
                message,
                &[],
            ))));
        }

        if let Touching::Node(id) = node {
            if is_string_literal_like(self.node_kind(id))
                && crate::find_all_references::try_get_import_from_module_specifier(self, id)
                    .is_some()
            {
                if self.preferences.allow_rename_of_import_path == Some(true) {
                    return self.rename_info_for_module(new_name, id, symbol).map(Some);
                }
                return Ok(None);
            }
        }

        let display_name = self.symbol_to_string(symbol)?;
        Ok(Some(self.rename_info_success(node, display_name)))
    }

    /// tsgo `renameBlockedReason`: the message when the symbol is a
    /// library's, a `default` import's or another package's.
    fn rename_blocked_reason(
        &mut self,
        node: Touching,
        symbol: SymbolId,
    ) -> CheckResult<Option<&'static tsc_diagnostics::DiagnosticMessage>> {
        for declaration in self.declarations_of(symbol) {
            if self.is_defined_in_library_file(declaration) {
                return Ok(Some(
                    &gen::You_cannot_rename_elements_that_are_defined_in_the_standard_TypeScript_library,
                ));
            }
        }

        // Cannot rename `default` as in `import { default as foo } from "./someModule"`
        if let Touching::Node(id) = node {
            if self.node_kind(id) == SyntaxKind::Identifier
                && self.node_text(Touching::Node(id)) == "default"
                && self
                    .state
                    .binder
                    .symbol(symbol)
                    .parent
                    .is_some_and(|parent| {
                        self.state
                            .symbol_flags(parent)
                            .intersects(SymbolFlags::MODULE)
                    })
            {
                return Ok(Some(&gen::You_cannot_rename_this_element));
            }
        }

        let file = self.state.binder.file_index_of_node(match node {
            Touching::Node(id) => id,
            Touching::Token { parent, .. } => parent,
        });
        self.would_rename_in_other_node_modules(file, symbol)
    }

    /// tsgo `isDefinedInLibraryFile`: a declaration of a default library's
    /// declaration file.
    fn is_defined_in_library_file(&self, declaration: NodeId) -> bool {
        let file = self.state.binder.file_index_of_node(declaration);
        let source = self.state.binder.source(file);
        self.state
            .binder
            .file_facts(tsc_checker::program::ProgramFileId::from_raw(file as u32))
            .is_default_library()
            && source.is_declaration_file
    }

    /// tsgo `wouldRenameInOtherNodeModules`.
    fn would_rename_in_other_node_modules(
        &mut self,
        original_file: usize,
        symbol: SymbolId,
    ) -> CheckResult<Option<&'static tsc_diagnostics::DiagnosticMessage>> {
        let mut symbol = symbol;
        if self.preferences.provide_prefix_and_suffix_text_for_rename == Some(false)
            && self
                .state
                .symbol_flags(symbol)
                .intersects(SymbolFlags::ALIAS)
        {
            let import_specifier = self
                .declarations_of(symbol)
                .into_iter()
                .find(|&declaration| self.node_kind(declaration) == SyntaxKind::ImportSpecifier);
            if let Some(import_specifier) = import_specifier {
                if matches!(
                    &self.state.binder.node_record(import_specifier).data,
                    NodeData::ImportSpecifier(data) if data.property_name.is_none()
                ) {
                    symbol = self.state.get_aliased_symbol(symbol)?;
                }
            }
        }
        let declarations = self.declarations_of(symbol);
        if declarations.is_empty() {
            return Ok(None);
        }
        let file_name = |request: &Self, file: usize| {
            request
                .state
                .binder
                .source(file)
                .file_name
                .to_string_lossy()
                .into_owned()
        };
        let original_name = file_name(self, original_file);
        let Some(original_package) =
            tsc_program::node_module_package_root(JsStr::from_str(&original_name), false)
        else {
            // Original source file is not in node_modules.
            for declaration in declarations {
                let name = file_name(self, self.state.binder.file_index_of_node(declaration));
                if name.contains("/node_modules/") {
                    return Ok(Some(
                        &gen::You_cannot_rename_elements_that_are_defined_in_a_node_modules_folder,
                    ));
                }
            }
            return Ok(None);
        };
        // Original source file is in node_modules.
        for declaration in declarations {
            let name = file_name(self, self.state.binder.file_index_of_node(declaration));
            if let Some(package) =
                tsc_program::node_module_package_root(JsStr::from_str(&name), false)
            {
                if package != original_package {
                    return Ok(Some(
                        &gen::You_cannot_rename_elements_that_are_defined_in_another_node_modules_folder,
                    ));
                }
            }
        }
        Ok(None)
    }

    /// tsgo `getRenameInfoForModule`: a relative module specifier renames
    /// its file, when the editor can.
    fn rename_info_for_module(
        &mut self,
        new_name: &str,
        specifier: NodeId,
        module_symbol: SymbolId,
    ) -> CheckResult<RenameInfo> {
        let text = self.node_text(Touching::Node(specifier));
        if !crate::references::is_external_module_name_relative(&tsc_types::JsString::from(
            text.as_str(),
        )) {
            return Ok(rename_info_error(localize(
                self.catalog,
                &gen::You_cannot_rename_a_module_via_a_global_import,
                &[],
            )));
        }
        let workspace_edit = &self.capabilities.workspace.workspace_edit;
        if !workspace_edit.document_changes
            || !workspace_edit
                .resource_operations
                .contains(&tsc_lsproto::ResourceOperationKind::RENAME)
        {
            return Ok(rename_info_error(localize(
                self.catalog,
                &gen::File_rename_is_not_supported_by_the_editor,
                &[],
            )));
        }
        let Some(module_source_file) = self
            .declarations_of(module_symbol)
            .into_iter()
            .find(|&declaration| self.node_kind(declaration) == SyntaxKind::SourceFile)
        else {
            return Ok(RenameInfo::default());
        };
        let file_name = self
            .state
            .binder
            .source_of_node(module_source_file)
            .file_name
            .to_string_lossy()
            .into_owned();
        let without_index = if !text.ends_with("/index") && !text.ends_with("/index.js") {
            try_remove_index_file_name(&file_name)
        } else {
            None
        };
        let display_name = without_index.unwrap_or_else(|| file_name.clone());
        let new_file_name = self.new_file_name_for_module_rename(&display_name, &text, new_name);

        // Span should only be the last component of the path. + 1 to account for the quote character.
        let index_after_last_slash = text.rfind('/').map_or(0, |index| index + 1);
        let source = self.state.binder.source_of_node(specifier);
        let start = crate::utilities::token_start(source, specifier) + 1 + index_after_last_slash;
        let length = text.len() - index_after_last_slash;
        let trigger_span = self.lsp_range(source, start..start + length);
        Ok(RenameInfo {
            can_rename: true,
            localized_error_message: String::new(),
            display_name: text[index_after_last_slash..].to_owned(),
            trigger_span,
            file_to_rename: display_name,
            new_file_name,
        })
    }

    /// tsgo `getNewFileNameForModuleRename`: the new name in the old path's
    /// directory, with the old file's extension when the new name has none
    /// (or the specifier's).
    fn new_file_name_for_module_rename(
        &self,
        old_path: &str,
        specifier_text: &str,
        new_name: &str,
    ) -> String {
        let directory = tsc_program::get_directory_path(JsStr::from_str(old_path));
        let new_path = tsc_program::normalize_path(
            tsc_program::combine_paths(directory.as_js(), JsStr::from_str(new_name)).as_js(),
        )
        .to_string_lossy()
        .into_owned();
        let old_extension = if tsc_program::is_declaration_file_name(old_path) {
            tsc_program::declaration_file_extension(old_path)
        } else {
            tsc_program::any_extension(old_path)
        };
        if !tsc_program::has_extension(&new_path) {
            return format!("{new_path}{old_extension}");
        }
        if tsc_program::any_extension(&new_path) == tsc_program::any_extension(specifier_text) {
            return tsc_program::change_any_extension(&new_path, old_extension);
        }
        new_path
    }

    /// tsgo `getRenameInfoSuccess`: the node's range (a string literal's
    /// without its quotes).
    fn rename_info_success(&self, node: Touching, display_name: String) -> RenameInfo {
        let (file, mut range) = match node {
            Touching::Node(id) => {
                let file = self.state.binder.file_index_of_node(id);
                let source = self.state.binder.source(file);
                let start = crate::utilities::token_start(source, id);
                (file, start..self.state.binder.node_record(id).end as usize)
            }
            token => self.range_of_touching(token),
        };
        if is_string_literal_like(self.kind(node)) {
            // Exclude the quotes
            range = range.start + 1..range.end - 1;
        }
        RenameInfo {
            can_rename: true,
            localized_error_message: String::new(),
            display_name,
            trigger_span: self.lsp_range(self.state.binder.source(file), range),
            file_to_rename: String::new(),
            new_file_name: String::new(),
        }
    }

    /// tsgo `symbolAndEntriesToRename`: the edits by document, none when
    /// the original node cannot be renamed or two edits of one range
    /// disagree.
    fn symbol_and_entries_to_rename(
        &mut self,
        new_name: &str,
        data: Option<&SymbolAndEntriesData>,
    ) -> CheckResult<Option<BTreeMap<DocumentUri, Vec<TextEdit>>>> {
        let Some(original_node) = data.and_then(|data| data.original_node) else {
            return Ok(None);
        };
        if !self.node_is_eligible_for_rename(original_node) {
            return Ok(None);
        }
        // Defense-in-depth: validate rename eligibility even if the client skipped prepareRename.
        match self.rename_info_for_node(new_name, original_node)? {
            Some(info) if info.can_rename => {}
            _ => return Ok(None),
        }
        let data = data.expect("data with an original node");
        let original_file = self.state.binder.file_index_of_node(match original_node {
            Touching::Node(id) => id,
            Touching::Token { parent, .. } => parent,
        });
        let quote_preference =
            get_quote_preference(self.state.binder.source(original_file), self.preferences);
        let use_aliases_for_rename =
            self.preferences.provide_prefix_and_suffix_text_for_rename != Some(false);
        let entries: Vec<ReferenceEntry> = data
            .symbols_and_entries
            .iter()
            .flat_map(|entry| entry.references.iter().cloned())
            .collect();

        let mut edits: Vec<(DocumentUri, TextEdit)> = Vec::new();
        for entry in &entries {
            if self.preferences.allow_rename_of_import_path != Some(true) {
                if let Some(Touching::Node(id)) = entry.node {
                    if is_string_literal_like(self.node_kind(id))
                        && crate::find_all_references::try_get_import_from_module_specifier(
                            self, id,
                        )
                        .is_some()
                    {
                        continue;
                    }
                }
            }
            let location = self.location_of_entry(entry);
            let new_text = self.text_for_rename(
                original_node,
                entry,
                new_name,
                quote_preference,
                use_aliases_for_rename,
            )?;
            edits.push((
                location.uri,
                TextEdit {
                    range: location.range,
                    new_text,
                },
            ));
        }

        // tsgo `deduplicateRenameEdits`.
        let mut texts: HashMap<EditKey, String> = HashMap::new();
        let mut changes: BTreeMap<DocumentUri, Vec<TextEdit>> = BTreeMap::new();
        for (uri, edit) in edits {
            let range = &edit.range;
            let key = (
                uri.0.clone(),
                (
                    range.start.line,
                    range.start.character,
                    range.end.line,
                    range.end.character,
                ),
            );
            if let Some(existing) = texts.get(&key) {
                if *existing != edit.new_text {
                    return Ok(None);
                }
                continue;
            }
            texts.insert(key, edit.new_text.clone());
            changes.entry(uri).or_default().push(edit);
        }
        Ok(Some(changes))
    }

    /// tsgo `getTextForRename`: the new name, with the old one where an
    /// alias keeps it (`x: y`, `x as y`), quoted for a numeric index.
    fn text_for_rename(
        &mut self,
        original_node: Touching,
        entry: &ReferenceEntry,
        new_text: &str,
        quote_preference: QuotePreference,
        use_aliases_for_rename: bool,
    ) -> CheckResult<String> {
        let original_kind = self.kind(original_node);
        if use_aliases_for_rename
            && entry.kind != EntryKind::Range
            && (original_kind == SyntaxKind::Identifier || is_string_literal_like(original_kind))
        {
            if let (Some(Touching::Node(node)), Touching::Node(original)) =
                (entry.node, original_node)
            {
                let kind = entry.kind;
                let parent = self.state.binder.node_record(node).parent;
                let name = self.node_text(Touching::Node(original));
                let parent_data =
                    parent.map(|parent| self.state.binder.node_record(parent).data.clone());
                let is_shorthand_assignment =
                    matches!(parent_data, Some(NodeData::ShorthandPropertyAssignment(_)));
                let is_binding_name = parent.is_some_and(|parent| {
                    is_object_binding_element_without_property_name(self, parent)
                        && self.name_of(parent) == Some(node)
                        && matches!(
                            &self.state.binder.node_record(parent).data,
                            NodeData::BindingElement(data) if data.dot_dot_dot_token.is_none()
                        )
                });
                if is_shorthand_assignment || is_binding_name {
                    if kind == EntryKind::SearchedLocalFoundProperty {
                        return Ok(format!("{name}: {new_text}"));
                    }
                    if kind == EntryKind::SearchedPropertyFoundLocal {
                        return Ok(format!("{new_text}: {name}"));
                    }
                    // In `const o = { x }; o.x`, symbolAtLocation at `x` in `{ x }` is the property symbol.
                    // For a binding element `const { x } = o;`, symbolAtLocation at `x` is the property symbol.
                    if is_shorthand_assignment {
                        let parent = parent.expect("a shorthand assignment");
                        let grandparent = self.state.binder.node_record(parent).parent;
                        let module_exports = grandparent.is_some_and(|object| {
                            self.node_kind(object) == SyntaxKind::ObjectLiteralExpression
                                && self.state.binder.node_record(object).parent.is_some_and(|binary| {
                                    match &self.state.binder.node_record(binary).data {
                                        NodeData::BinaryExpression(data) => data.left.is_some_and(|left| {
                                            tsc_binder::assignment::is_module_exports_access_expression(
                                                self.state.binder.source_of_node(left),
                                                left,
                                            )
                                        }),
                                        _ => false,
                                    }
                                })
                        });
                        if module_exports {
                            return Ok(format!("{name}: {new_text}"));
                        }
                        return Ok(format!("{new_text}: {name}"));
                    }
                    return Ok(format!("{name}: {new_text}"));
                }
                match parent_data {
                    Some(NodeData::ImportSpecifier(data)) if data.property_name.is_none() => {
                        // If the original symbol was using this alias, just rename the alias.
                        let original_parent = self.state.binder.node_record(original).parent;
                        let original_symbol = match original_parent {
                            Some(parent)
                                if self.node_kind(parent) == SyntaxKind::ExportSpecifier =>
                            {
                                self.state
                                    .get_export_specifier_local_target_symbol(parent)?
                            }
                            _ => self.state.get_symbol_at_location(original)?,
                        };
                        let parent = parent.expect("an import specifier");
                        if original_symbol
                            .is_some_and(|symbol| self.declarations_of(symbol).contains(&parent))
                        {
                            return Ok(format!("{name} as {new_text}"));
                        }
                        return Ok(new_text.to_owned());
                    }
                    Some(NodeData::ExportSpecifier(data)) if data.property_name.is_none() => {
                        // If the symbol for the node is same as declared node symbol use prefix text
                        if original == node
                            || self.state.get_symbol_at_location(original)?
                                == self.state.get_symbol_at_location(node)?
                        {
                            return Ok(format!("{name} as {new_text}"));
                        }
                        return Ok(format!("{new_text} as {name}"));
                    }
                    _ => {}
                }
            }
        }

        // If the node is a numerical indexing literal, then add quotes around the property access.
        if entry.kind != EntryKind::Range {
            if let Some(Touching::Node(node)) = entry.node {
                if self.node_kind(node) == SyntaxKind::NumericLiteral
                    && self
                        .state
                        .binder
                        .node_record(node)
                        .parent
                        .is_some_and(|parent| {
                            matches!(
                                self.node_kind(parent),
                                SyntaxKind::PropertyAccessExpression
                                    | SyntaxKind::ElementAccessExpression
                            )
                        })
                {
                    let quote = if quote_preference == QuotePreference::SINGLE {
                        "'"
                    } else {
                        "\""
                    };
                    return Ok(format!("{quote}{new_text}{quote}"));
                }
            }
        }

        Ok(new_text.to_owned())
    }
}

/// tsgo `ast.IsStringLiteralLike`.
fn is_string_literal_like(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::StringLiteral | SyntaxKind::NoSubstitutionTemplateLiteral
    )
}

/// tsgo `tryRemoveIndexFileName`: an index file's directory (none at a
/// root).
fn try_remove_index_file_name(file_name: &str) -> Option<String> {
    let base_name = file_name.rsplit('/').next().unwrap_or(file_name);
    if tsc_program::remove_file_extension(JsStr::from_str(base_name)).as_js()
        != JsStr::from_str("index")
    {
        return None;
    }
    let directory = &file_name[..file_name.len() - base_name.len()];
    let directory = directory.strip_suffix('/').unwrap_or(directory);
    if directory.is_empty() {
        // tsgo: the root itself (`/index.ts`) keeps its file name.
        return None;
    }
    Some(directory.to_owned())
}

//! tsgo `ls/file_rename.go`: the edits a file's or a directory's rename
//! needs — the config's paths to it, and the module specifiers that import
//! it or that a moved file writes.

use std::sync::Arc;

use tsc_lsproto::{
    DocumentUri, IntegerOrNull, OptionalVersionedTextDocumentIdentifier, RenameFile,
    TextDocumentEdit, TextDocumentEditOrCreateFileOrRenameFileOrDeleteFile,
    TextEditOrAnnotatedTextEditOrSnippetTextEdit,
};
use tsc_program::uri::{file_name_to_document_uri, uri_to_file_name};
use tsc_program::ConfigRootPlan;
use tsc_syntax::{NodeData, NodeId, SourceFile, SyntaxKind};
use tsc_types::{JsStr, JsString};

use crate::change::Tracker;
use crate::completions::{possible_original_input_extension_for_extension, trim_contained_path};
use crate::language_service::{LanguageService, LsError};
use crate::lsutil::new_line_kind;
use crate::references::{collect_external_module_references, is_external_module_name_relative};
use crate::request::Request;
use crate::utilities::{resolved_module_from_module_specifier, token_start, Touching};

/// A change of the edits of a file rename.
type DocumentChange = TextDocumentEditOrCreateFileOrRenameFileOrDeleteFile;

impl LanguageService<'_> {
    /// tsgo `GetEditsForFileRename`: the renames of a declaration file's
    /// original (`foo.css` with `foo.d.css.ts`), then each file's edits.
    pub fn get_edits_for_file_rename(
        &mut self,
        old_uri: &DocumentUri,
        new_uri: &DocumentUri,
    ) -> Result<Vec<DocumentChange>, LsError> {
        let old_path = uri_to_file_name(old_uri.as_str()).unwrap_or_default();
        let new_path = uri_to_file_name(new_uri.as_str()).unwrap_or_default();
        let host = self.host;
        let updater = PathUpdater {
            old: old_path.clone(),
            new: new_path.clone(),
            case_sensitive: host.use_case_sensitive_file_names(),
        };
        // tsgo `program.CommandLine().ConfigFile.SourceFile`.
        let config = self
            .program
            .prepared()
            .program_options()
            .config_file_path()
            .and_then(|path| host.config_plan(&path.display().to_string_lossy()))
            .map(|plan| {
                let source = plan.source();
                let file = tsc_syntax::parse_json_source_text_from_snapshot(
                    source.file_name.clone(),
                    Arc::clone(source.snapshot()),
                );
                (plan, file)
            });
        let format_settings = self.format_options().clone();
        let edits = self.with_program_request(false, |request| {
            let mut tracker = Tracker::new(new_line_kind(request.state.options), format_settings);
            if let Some((plan, config_file)) = &config {
                if let Err(error) = update_tsconfig_files(plan, config_file, &mut tracker, &updater)
                {
                    return Ok(Err(error));
                }
            }
            update_imports_for_file_rename(request, &mut tracker, &updater)?;
            match tracker.get_changes(request.converters, &request.state.binder) {
                Ok(changes) => Ok(Ok(changes.files)),
                Err(error) => Ok(Err(LsError(error.to_string()))),
            }
        })?;
        let edits = match edits {
            Some(edits) => edits?,
            None => Vec::new(),
        };

        let mut document_changes = Vec::new();
        // When renaming e.g. `foo.d.css.ts` -> `bar.d.css.ts`, also rename `foo.css` -> `bar.css` if it exists.
        if tsc_program::is_declaration_file_name(&old_path)
            && tsc_program::is_declaration_file_name(&new_path)
        {
            let original_extensions = possible_original_input_extension_for_extension(
                tsc_program::declaration_file_extension(&old_path),
            );
            for extension in original_extensions {
                let old_original_path =
                    tsc_program::change_full_extension(JsStr::from_str(&old_path), &extension)
                        .to_string_lossy()
                        .into_owned();
                if host.file_exists(&old_original_path) {
                    let new_original_extensions = possible_original_input_extension_for_extension(
                        tsc_program::declaration_file_extension(&new_path),
                    );
                    if new_original_extensions.contains(&extension) {
                        let new_original_path = tsc_program::change_full_extension(
                            JsStr::from_str(&new_path),
                            &extension,
                        )
                        .to_string_lossy()
                        .into_owned();
                        document_changes.push(DocumentChange::RenameFile(RenameFile {
                            kind: Default::default(),
                            annotation_id: None,
                            old_uri: DocumentUri(file_name_to_document_uri(&old_original_path)),
                            new_uri: DocumentUri(file_name_to_document_uri(&new_original_path)),
                            options: None,
                        }));
                    }
                }
            }
        }

        for (file_name, edits) in edits {
            document_changes.push(DocumentChange::TextDocumentEdit(TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier {
                    uri: DocumentUri(file_name_to_document_uri(&file_name)),
                    version: IntegerOrNull::Null,
                },
                edits: edits
                    .into_iter()
                    .map(TextEditOrAnnotatedTextEditOrSnippetTextEdit::TextEdit)
                    .collect(),
            }));
        }
        Ok(document_changes)
    }
}

/// tsgo `pathUpdater` (`createPathUpdater`): a path's name after the
/// rename — the renamed file's new name, or the new name of a file below
/// the renamed directory.
struct PathUpdater {
    old: String,
    new: String,
    case_sensitive: bool,
}

impl PathUpdater {
    fn update(&self, path: &str) -> Option<String> {
        if paths_equal(path, &self.old, self.case_sensitive) {
            return Some(self.new.clone());
        }
        let relative = trim_contained_path(&self.old, path, self.case_sensitive)?;
        Some(resolve_path(&self.new, &relative))
    }
}

/// tsgo `CaseSensitivity.CompareFilePaths(a, b) == 0`: the roots compared
/// without case, the rest with the file system's.
fn paths_equal(left: &str, right: &str, case_sensitive: bool) -> bool {
    left == right
        || !case_sensitive
            && tsc_program::to_file_name_lower_case_js(JsStr::from_str(left))
                == tsc_program::to_file_name_lower_case_js(JsStr::from_str(right))
}

/// tsgo `ResolveFile`: `path` against `directory`, normalized.
fn resolve_path(directory: &str, path: &str) -> String {
    tsc_program::get_normalized_absolute_path(JsStr::from_str(path), JsStr::from_str(directory))
        .to_string_lossy()
        .into_owned()
}

/// tsgo `Directory()` of a file name.
fn directory_of(file_name: &str) -> String {
    tsc_program::get_directory_path(JsStr::from_str(file_name))
        .to_string_lossy()
        .into_owned()
}

/// tsgo `relativePathFromDirectory`: `to` relative to `directory` (a
/// config's spelling), `to` itself without a shared root.
fn relative_path_from_directory(directory: &str, to: &str, case_sensitive: bool) -> String {
    tsc_program::relative_path_from_directory(
        JsStr::from_str(directory),
        JsStr::from_str(to),
        case_sensitive,
    )
    .to_string_lossy()
    .into_owned()
}

/// tsgo `relativeImportPathFromDirectory`: `to` relative to `directory` as
/// a module specifier (`./` or `../`), `to` itself without a shared root.
fn relative_import_path_from_directory(directory: &str, to: &str, case_sensitive: bool) -> String {
    let relative = relative_path_from_directory(directory, to, case_sensitive);
    if relative.starts_with('/') {
        return relative;
    }
    tsc_program::ensure_path_is_non_module_name(&relative)
}

/// tsgo `updateTsconfigFiles`: the config's `files`, `include`, `exclude`
/// and path options that name the renamed file (or a file below the
/// renamed directory) renamed; a file an `include` matched and no longer
/// matches added to it.
fn update_tsconfig_files<'t>(
    plan: &ConfigRootPlan,
    config_file: &'t SourceFile,
    tracker: &mut Tracker<'t>,
    updater: &PathUpdater,
) -> Result<(), LsError> {
    let config_directory = directory_of(&config_file.file_name.to_string_lossy());
    let Some(object) = config_object_literal(config_file) else {
        return Ok(());
    };
    for (name, initializer) in object_properties(config_file, object) {
        match name.as_str() {
            "files" | "include" | "exclude" => {
                let found_exact_match = update_paths_property(
                    config_file,
                    &config_directory,
                    initializer,
                    tracker,
                    updater,
                );
                let elements = match &config_file.arena.node(initializer).data {
                    NodeData::ArrayLiteralExpression(data) if name == "include" => data.elements,
                    _ => continue,
                };
                if found_exact_match {
                    continue;
                }
                let matched = |path: &str| plan.matched_include_spec(path, updater.case_sensitive);
                let Some((_, false)) = matched(&updater.old) else {
                    continue;
                };
                if matched(&updater.new).is_some() {
                    continue;
                }
                let Some(&last) = elements
                    .and_then(|elements| config_file.arena.node_array(elements).nodes.last())
                else {
                    continue;
                };
                let new_path_text = relative_path_from_directory(
                    &config_directory,
                    &updater.new,
                    updater.case_sensitive,
                );
                let target = tracker.source(config_file);
                let literal = tracker
                    .arena()
                    .factory()
                    .create_string_literal(target, new_path_text.as_str(), false)
                    .map_err(|error| LsError(error.to_string()))?;
                tracker
                    .insert_node_after(config_file, last, literal)
                    .map_err(|error| LsError(error.to_string()))?;
            }
            "compilerOptions" => {
                if config_file.arena.node(initializer).kind != SyntaxKind::ObjectLiteralExpression {
                    continue;
                }
                for (option_name, value) in object_properties(config_file, initializer) {
                    if let Some(option) =
                        tsc_program::compiler_option_declaration(JsStr::from_str(&option_name))
                    {
                        let element_is_path = matches!(
                            option.value_kind(),
                            tsc_program::CompilerOptionValueKind::List(list)
                                if list.element_kind() == tsc_program::CompilerOptionListElementKind::FilePath
                        );
                        if option.is_file_path() || element_is_path {
                            update_paths_property(
                                config_file,
                                &config_directory,
                                value,
                                tracker,
                                updater,
                            );
                            continue;
                        }
                    }
                    if option_name != "paths"
                        || config_file.arena.node(value).kind != SyntaxKind::ObjectLiteralExpression
                    {
                        continue;
                    }
                    for (_, mapping) in object_properties(config_file, value) {
                        let NodeData::ArrayLiteralExpression(data) =
                            &config_file.arena.node(mapping).data
                        else {
                            continue;
                        };
                        for &element in data
                            .elements
                            .map(|elements| config_file.arena.node_array(elements).nodes)
                            .unwrap_or_default()
                        {
                            try_update_config_string(
                                config_file,
                                &config_directory,
                                element,
                                tracker,
                                updater,
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// tsgo `updatePathsProperty`: the property's string (or each string of
/// its array) renamed; whether one named the renamed path.
fn update_paths_property<'t>(
    config_file: &'t SourceFile,
    config_directory: &str,
    initializer: NodeId,
    tracker: &mut Tracker<'t>,
    updater: &PathUpdater,
) -> bool {
    let elements = match &config_file.arena.node(initializer).data {
        NodeData::ArrayLiteralExpression(data) => data
            .elements
            .map(|elements| config_file.arena.node_array(elements).nodes.to_vec())
            .unwrap_or_default(),
        _ => vec![initializer],
    };
    let mut found_exact_match = false;
    for element in elements {
        found_exact_match =
            try_update_config_string(config_file, config_directory, element, tracker, updater)
                || found_exact_match;
    }
    found_exact_match
}

/// tsgo `tryUpdateConfigString`: a string naming a renamed path written
/// relative to the config's directory again.
fn try_update_config_string<'t>(
    config_file: &'t SourceFile,
    config_directory: &str,
    element: NodeId,
    tracker: &mut Tracker<'t>,
    updater: &PathUpdater,
) -> bool {
    let NodeData::StringLiteral(literal) = &config_file.arena.node(element).data else {
        return false;
    };
    let element_file_name = resolve_path(config_directory, &literal.text.to_string_lossy());
    let Some(updated) = updater.update(&element_file_name) else {
        return false;
    };
    tracker.replace_range_with_text(
        config_file,
        string_text_range(config_file, element),
        relative_path_from_directory(config_directory, &updated, updater.case_sensitive),
    );
    true
}

/// tsgo `getTsConfigObjectLiteralExpression`: the object of the config's
/// first statement.
fn config_object_literal(config_file: &SourceFile) -> Option<NodeId> {
    let NodeData::SourceFile(data) = &config_file.arena.node(config_file.root).data else {
        return None;
    };
    let &statement = config_file
        .arena
        .node_array(data.statements?)
        .nodes
        .first()?;
    let NodeData::ExpressionStatement(statement) = &config_file.arena.node(statement).data else {
        return None;
    };
    let expression = statement.expression?;
    (config_file.arena.node(expression).kind == SyntaxKind::ObjectLiteralExpression)
        .then_some(expression)
}

/// tsgo `forEachObjectProperty`: the object's property assignments with a
/// name text, and their initializers.
fn object_properties(config_file: &SourceFile, object: NodeId) -> Vec<(String, NodeId)> {
    let NodeData::ObjectLiteralExpression(data) = &config_file.arena.node(object).data else {
        return Vec::new();
    };
    data.properties
        .map(|properties| config_file.arena.node_array(properties).nodes)
        .unwrap_or_default()
        .iter()
        .filter_map(|&property| {
            let NodeData::PropertyAssignment(assignment) = &config_file.arena.node(property).data
            else {
                return None;
            };
            let name = match &config_file.arena.node(assignment.name?).data {
                NodeData::StringLiteral(name) => name.text.to_string_lossy().into_owned(),
                NodeData::NoSubstitutionTemplateLiteral(name) => {
                    name.text.to_string_lossy().into_owned()
                }
                NodeData::NumericLiteral(name) => name.text.clone(),
                NodeData::Identifier(name) => name.escaped_text.to_string(),
                _ => return None,
            };
            Some((name, assignment.initializer?))
        })
        .collect()
}

/// tsgo `createStringTextRange`: a string literal's text, without its
/// quotes.
fn string_text_range(source: &SourceFile, literal: NodeId) -> (usize, usize) {
    (
        token_start(source, literal) + 1,
        source.arena.node(literal).end as usize - 1,
    )
}

/// tsgo `updateRelativePath`: a relative specifier of the file at
/// `old_import_from_path`, written from `new_import_from_path` to where its
/// target goes.
fn update_relative_path(
    updater: &PathUpdater,
    old_import_from_path: &str,
    new_import_from_path: &str,
    relative_specifier: &str,
) -> String {
    let old_absolute = resolve_path(&directory_of(old_import_from_path), relative_specifier);
    let new_absolute = updater.update(&old_absolute).unwrap_or(old_absolute);
    relative_import_path_from_directory(
        &directory_of(new_import_from_path),
        &new_absolute,
        updater.case_sensitive,
    )
}

/// tsgo `updateImportsForFileRename`: in every file, the relative
/// triple-slash references and the module specifiers whose targets (or
/// whose file) the rename moves.
fn update_imports_for_file_rename<'a: 't, 't>(
    request: &mut Request<'_, 'a>,
    tracker: &mut Tracker<'t>,
    updater: &PathUpdater,
) -> tsc_checker::state::CheckResult<()> {
    let file_count = request.state.binder.file_count();
    let preferences = request.preferences.module_specifier_preferences();
    let moved_files: Vec<(usize, String)> = (0..file_count)
        .filter_map(|file| {
            let file_name = request
                .state
                .binder
                .source(file)
                .file_name
                .to_string_lossy();
            Some((file, updater.update(&file_name)?))
        })
        .collect();
    for file in 0..file_count {
        let source: &'a SourceFile = request.state.binder.source(file);
        let old_file_name = source.file_name.to_string_lossy().into_owned();
        let new_from_old = updater.update(&old_file_name);
        let file_moved = new_from_old.is_some();
        let new_import_from_path = new_from_old.unwrap_or_else(|| old_file_name.clone());

        for reference in &source.referenced_files {
            let text = reference.file_name.to_string_lossy();
            if !is_external_module_name_relative(&reference.file_name) {
                continue;
            }
            let updated =
                update_relative_path(updater, &old_file_name, &new_import_from_path, &text);
            if updated != text {
                tracker.replace_range_with_text(
                    source,
                    (reference.pos as usize, reference.end as usize),
                    updated,
                );
            }
        }

        for literal in collect_external_module_references(source).imports {
            let Some(updated) = updated_import_specifier(
                request,
                source,
                literal,
                updater,
                &moved_files,
                &new_import_from_path,
                file_moved,
                &preferences,
            )?
            else {
                continue;
            };
            if updated != request.node_text(Touching::Node(literal)) {
                tracker.replace_range_with_text(
                    source,
                    string_text_range(source, literal),
                    updated,
                );
            }
        }
    }
    Ok(())
}

/// tsgo `getUpdatedImportSpecifier`: the new specifier of an import, none
/// to keep it (an ambient module's, or one neither of whose files moved).
#[allow(clippy::too_many_arguments)]
fn updated_import_specifier(
    request: &mut Request<'_, '_>,
    source: &SourceFile,
    literal: NodeId,
    updater: &PathUpdater,
    moved_files: &[(usize, String)],
    new_import_from_path: &str,
    importing_file_moved: bool,
    preferences: &tsc_checker::ModuleSpecifierUserPreferences,
) -> tsc_checker::state::CheckResult<Option<String>> {
    let text = request.node_text(Touching::Node(literal));
    if let Some(symbol) = request.state.get_symbol_at_location(literal)? {
        if is_ambient_module_symbol(request, symbol) {
            return Ok(None);
        }
    }
    let importing_name = JsString::from(new_import_from_path);
    let update = |request: &Request<'_, '_>, to: &str| {
        request
            .state
            .update_module_specifier(&importing_name, literal, &JsString::from(to), preferences)
            .to_string_lossy()
            .into_owned()
    };
    // tsgo `getSourceFileToImport`.
    let resolved = resolved_module_from_module_specifier(
        request.state,
        request.prepared,
        source,
        literal,
        &text,
    )
    .flatten();
    let Some(old_file_name) = resolved else {
        // First fall back: try every file affected by the rename to see if any of them would match the import specifier, and if so, obtain the updated specifier for that file.
        for (file, new_file_name) in moved_files {
            let candidate = request
                .state
                .binder
                .source(*file)
                .file_name
                .to_string_lossy()
                .into_owned();
            if update(request, &candidate) != text {
                continue;
            }
            let updated = update(request, new_file_name);
            if !updated.is_empty() && updated != text {
                return Ok(Some(updated));
            }
            break;
        }
        // Fall back to a regular path update for unresolved module.
        if is_external_module_name_relative(&JsString::from(text.as_str())) {
            let old_file_name = source.file_name.to_string_lossy();
            return Ok(Some(update_relative_path(
                updater,
                &old_file_name,
                new_import_from_path,
                &text,
            )));
        }
        return Ok(None);
    };
    let (new_file_name, updated) = match updater.update(&old_file_name) {
        Some(new_file_name) => (new_file_name, true),
        None => (old_file_name, false),
    };
    // Optimization: neither the importing or imported file changed.
    let relative_import_moved =
        importing_file_moved && is_external_module_name_relative(&JsString::from(text.as_str()));
    if !(updated || relative_import_moved) {
        return Ok(None);
    }
    let updated = update(request, &new_file_name);
    Ok((!updated.is_empty()).then_some(updated))
}

/// tsgo `isAmbientModuleSymbol`: a symbol declared by a `declare module
/// "name"`.
fn is_ambient_module_symbol(request: &Request<'_, '_>, symbol: tsc_types::SymbolId) -> bool {
    request.declarations_of(symbol).into_iter().any(|declaration| {
        matches!(
            &request.state.binder.node_record(declaration).data,
            NodeData::ModuleDeclaration(data) if data.name.is_some_and(|name| {
                matches!(request.node_kind(name), SyntaxKind::StringLiteral | SyntaxKind::NoSubstitutionTemplateLiteral)
            })
        )
    })
}

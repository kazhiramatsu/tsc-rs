//! tsgo `ls/autoimport/fix.go`'s `Fix.Edits`: the text edits that make an
//! auto-import fix in a file, and their description.

use tsc_checker::program::ProgramBinder;
use tsc_diagnostics::MessageCatalog;
use tsc_emitter::{TransformError, TransformFlags, TransformNode};
use tsc_lsproto::{AddAsTypeOnly, AutoImportFix, AutoImportFixKind, ImportKind, TextEdit};
use tsc_syntax::nodes::{BindingElementData, CallExpressionData};
use tsc_syntax::{NodeData, NodeId, SourceFile, SyntaxKind};
use tsc_types::{CompilerOptions, NodeFlags};

use super::view::{
    is_variable_declaration_initialized_to_require, try_get_import_from_module_specifier, Fix,
};
use crate::astnav::{Found, Navigator};
use crate::change::{LeadingTriviaOption, NodeOptions, Tracker};
use crate::lsconv::{Converters, Script};
use crate::lsutil::{
    binary_search_unique, compare_import_or_export_specifiers,
    compare_imports_or_require_statements, get_named_import_specifier_comparer_with_detection,
    get_organize_imports_string_comparer_with_detection, get_quote_preference, new_line_kind,
    FormatCodeSettings, QuotePreference, UserPreferences,
};
use crate::references::{collect_external_module_references, statements};
use crate::tree::View;
use crate::utilities::script_of;

/// tsgo `newImportBinding`.
#[derive(Clone)]
pub(super) struct NewImportBinding {
    pub(super) kind: ImportKind,
    pub(super) property_name: String,
    pub(super) name: String,
    pub(super) add_as_type_only: AddAsTypeOnly,
}

impl NewImportBinding {
    pub(super) fn new(name: &str, add_as_type_only: AddAsTypeOnly) -> Self {
        Self {
            kind: ImportKind::default(),
            property_name: String::new(),
            name: name.to_owned(),
            add_as_type_only,
        }
    }
}

use crate::lsutil::localize;

/// tsgo `Fix.Edits`: the edits that make `fix` in `file`, their
/// description, and whether they could all be written back to the file's
/// original text (a content mapper's output's). An edit the tracker cannot
/// make is an error.
#[allow(clippy::too_many_arguments)]
pub(crate) fn fix_edits<'a>(
    fix: &Fix,
    file: &'a SourceFile,
    options: &CompilerOptions,
    format_settings: FormatCodeSettings,
    converters: &Converters,
    binder: &ProgramBinder<'a>,
    preferences: &UserPreferences,
    catalog: Option<&dyn MessageCatalog>,
) -> Result<(Vec<TextEdit>, String, bool), String> {
    let mut tracker = Tracker::new(new_line_kind(options), format_settings);
    let description = make_fix(
        &mut tracker,
        fix,
        file,
        options,
        preferences,
        converters,
        catalog,
    )
    .map_err(|error| error.to_string())?;
    let (edits, safe) = file_edits(tracker, file, converters, binder)?;
    Ok((edits, description, safe))
}

/// tsgo `fileEdits`: the tracker's edits of the file's original file, and
/// whether every edit could be written back.
pub(crate) fn file_edits<'a>(
    tracker: Tracker<'a>,
    file: &'a SourceFile,
    converters: &Converters,
    binder: &ProgramBinder<'a>,
) -> Result<(Vec<TextEdit>, bool), String> {
    let changes = tracker
        .get_changes(converters, binder)
        .map_err(|error| error.to_string())?;
    let safe = changes.unmappable.is_empty();
    Ok((changes.of(script_of(file).original_file_name()), safe))
}

/// The edits of `Fix.Edits` recorded by the tracker, and their description.
fn make_fix<'a>(
    tracker: &mut Tracker<'a>,
    fix: &Fix,
    file: &'a SourceFile,
    options: &CompilerOptions,
    preferences: &UserPreferences,
    converters: &Converters,
    catalog: Option<&dyn MessageCatalog>,
) -> Result<String, TransformError> {
    let auto_import = &fix.auto_import;
    match auto_import.kind {
        AutoImportFixKind::USE_NAMESPACE => Ok(add_namespace_qualifier(
            auto_import,
            tracker,
            file,
            converters,
            catalog,
        )),
        AutoImportFixKind::ADD_TO_EXISTING => {
            let imports = collect_external_module_references(file).imports;
            let Some(&module_specifier) = imports.get(auto_import.import_index as usize) else {
                panic!("import index out of range");
            };
            let (clause_or_pattern, default_import, named_import) =
                get_add_to_existing_import_fix(file, auto_import, module_specifier);
            add_to_existing_import(
                tracker,
                file,
                clause_or_pattern,
                default_import.as_ref(),
                named_import.as_slice(),
                preferences,
            )?;
            Ok(localize(
                catalog,
                &tsc_diagnostics::gen::Update_import_from_0,
                &[&auto_import.module_specifier],
            ))
        }
        AutoImportFixKind::ADD_NEW => {
            let default_import = (auto_import.import_kind == ImportKind::DEFAULT)
                .then(|| NewImportBinding::new(&auto_import.name, auto_import.add_as_type_only));
            let named_imports: Vec<NewImportBinding> = (auto_import.import_kind
                == ImportKind::NAMED)
                .then(|| NewImportBinding::new(&auto_import.name, auto_import.add_as_type_only))
                .into_iter()
                .collect();
            let namespace_like_import = (auto_import.import_kind == ImportKind::NAMESPACE
                || auto_import.import_kind == ImportKind::COMMON_JS)
                .then(|| NewImportBinding {
                    kind: auto_import.import_kind,
                    ..NewImportBinding::new(&auto_import.name, AddAsTypeOnly::default())
                });
            let quote_preference = get_quote_preference(file, preferences);
            let declarations = if auto_import.use_require {
                get_new_requires(
                    tracker,
                    file,
                    &auto_import.module_specifier,
                    quote_preference,
                    default_import.as_ref(),
                    &named_imports,
                    namespace_like_import.as_ref(),
                )?
            } else {
                get_new_imports(
                    tracker,
                    file,
                    &auto_import.module_specifier,
                    quote_preference,
                    default_import.as_ref(),
                    &named_imports,
                    namespace_like_import.as_ref(),
                    options,
                    preferences,
                )?
            };
            insert_imports(tracker, file, declarations, true, preferences)?;
            Ok(localize(
                catalog,
                &tsc_diagnostics::gen::Add_import_from_0,
                &[&auto_import.module_specifier],
            ))
        }
        AutoImportFixKind::PROMOTE_TYPE_ONLY => {
            let alias_declaration = fix
                .type_only_alias_declaration
                .expect("a type-only promotion's declaration");
            let promoted =
                promote_from_type_only(tracker, alias_declaration, options, file, preferences)?;
            if file.arena.node(promoted).kind == SyntaxKind::ImportSpecifier {
                let clause = file
                    .arena
                    .node(promoted)
                    .parent
                    .and_then(|named| file.arena.node(named).parent)
                    .expect("an import specifier's clause");
                return Ok(localize(
                    catalog,
                    &tsc_diagnostics::gen::Remove_type_from_import_of_0_from_1,
                    &[&auto_import.name, &get_module_specifier_text(file, clause)],
                ));
            }
            Ok(localize(
                catalog,
                &tsc_diagnostics::gen::Remove_type_from_import_declaration_from_0,
                &[&get_module_specifier_text(file, promoted)],
            ))
        }
        AutoImportFixKind::JSDOC_TYPE_IMPORT => Ok(add_import_type(
            auto_import,
            file,
            preferences,
            tracker,
            converters,
            catalog,
        )),
        _ => panic!("unimplemented fix edit"),
    }
}

/// tsgo `addImportType`.
pub(super) fn add_import_type<'a>(
    fix: &AutoImportFix,
    file: &'a SourceFile,
    preferences: &UserPreferences,
    tracker: &mut Tracker<'a>,
    converters: &Converters,
    catalog: Option<&dyn MessageCatalog>,
) -> String {
    let Some(usage_position) = fix.usage_position.as_ref() else {
        panic!("UsagePosition must be set for JSDoc type import fix");
    };
    let quote = if get_quote_preference(file, preferences) == QuotePreference::SINGLE {
        '\''
    } else {
        '"'
    };
    let import_type_prefix = format!("import({quote}{}{quote}).", fix.module_specifier);
    tracker.insert_text(file, converters, usage_position, import_type_prefix.clone());
    localize(
        catalog,
        &tsc_diagnostics::gen::Change_0_to_1,
        &[&fix.name, &format!("{import_type_prefix}{}", fix.name)],
    )
}

/// tsgo `addNamespaceQualifier`.
pub(super) fn add_namespace_qualifier<'a>(
    fix: &AutoImportFix,
    tracker: &mut Tracker<'a>,
    file: &'a SourceFile,
    converters: &Converters,
    catalog: Option<&dyn MessageCatalog>,
) -> String {
    let Some(usage_position) = fix
        .usage_position
        .as_ref()
        .filter(|_| !fix.namespace_prefix.is_empty())
    else {
        panic!("namespace fix requires usage position and prefix");
    };
    let qualified = format!("{}.{}", fix.namespace_prefix, fix.name);
    tracker.insert_text(
        file,
        converters,
        usage_position,
        format!("{}.", fix.namespace_prefix),
    );
    localize(
        catalog,
        &tsc_diagnostics::gen::Change_0_to_1,
        &[&fix.name, &qualified],
    )
}

/// tsgo `getAddToExistingImportFix`: the import clause or `require`
/// binding pattern of the fix's import, and the binding to add.
pub(super) fn get_add_to_existing_import_fix(
    file: &SourceFile,
    fix: &AutoImportFix,
    module_specifier: NodeId,
) -> (NodeId, Option<NewImportBinding>, Option<NewImportBinding>) {
    let Some(import_node) = try_get_import_from_module_specifier(file, module_specifier) else {
        panic!("expected import declaration");
    };
    let clause_or_pattern = match &file.arena.node(import_node).data {
        NodeData::ImportDeclaration(data) => data.import_clause.expect("expected import clause"),
        NodeData::CallExpression(_) => {
            let declaration = file
                .arena
                .node(import_node)
                .parent
                .filter(|&parent| is_variable_declaration_initialized_to_require(file, parent))
                .expect("expected require call expression to be in variable declaration");
            match &file.arena.node(declaration).data {
                NodeData::VariableDeclaration(data) => data
                    .name
                    .filter(|&name| file.arena.node(name).kind == SyntaxKind::ObjectBindingPattern)
                    .expect("expected object binding pattern in variable declaration"),
                _ => unreachable!("a variable declaration"),
            }
        }
        _ => panic!("expected import declaration or require call expression"),
    };
    let binding = |kind: ImportKind| NewImportBinding {
        kind,
        ..NewImportBinding::new(&fix.name, fix.add_as_type_only)
    };
    let default_import =
        (fix.import_kind == ImportKind::DEFAULT).then(|| binding(ImportKind::DEFAULT));
    let named_import = (fix.import_kind == ImportKind::NAMED).then(|| binding(ImportKind::NAMED));
    (clause_or_pattern, default_import, named_import)
}

/// tsgo `addToExistingImport`.
pub(super) fn add_to_existing_import<'a>(
    tracker: &mut Tracker<'a>,
    file: &'a SourceFile,
    clause_or_pattern: NodeId,
    default_import: Option<&NewImportBinding>,
    named_imports: &[NewImportBinding],
    preferences: &UserPreferences,
) -> Result<(), TransformError> {
    let clause = match &file.arena.node(clause_or_pattern).data {
        NodeData::ObjectBindingPattern(_) => {
            if let Some(default_import) = default_import {
                add_element_to_binding_pattern(
                    tracker,
                    file,
                    clause_or_pattern,
                    &default_import.name,
                    "default",
                )?;
            }
            for named_import in named_imports {
                add_element_to_binding_pattern(
                    tracker,
                    file,
                    clause_or_pattern,
                    &named_import.name,
                    "",
                )?;
            }
            return Ok(());
        }
        NodeData::ImportClause(clause) => clause,
        _ => panic!(
            "Unsupported clause kind: {:?} for addToExistingImport",
            file.arena.node(clause_or_pattern).kind
        ),
    };
    let source = tracker.source(file);

    // promoteFromTypeOnly = true if we need to promote the entire original clause from type only
    let promote_from_type_only = clause.is_type_only
        && named_imports
            .iter()
            .chain(default_import)
            .any(|import| import.add_as_type_only == AddAsTypeOnly::NOT_ALLOWED);

    let existing_specifiers: Vec<NodeId> = match clause
        .named_bindings
        .map(|bindings| &file.arena.node(bindings).data)
    {
        Some(NodeData::NamedImports(named)) => named
            .elements
            .map(|elements| file.arena.node_array(elements).nodes.to_vec())
            .unwrap_or_default(),
        _ => Vec::new(),
    };

    if let Some(default_import) = default_import {
        debug_assert!(
            clause.name.is_none(),
            "Cannot add a default import to an import clause that already has one"
        );
        let identifier = tracker
            .arena()
            .factory()
            .create_identifier(source, default_import.name.clone())?;
        tracker.insert_node_at(
            file,
            start_of(file, clause_or_pattern),
            identifier,
            NodeOptions {
                suffix: ", ".to_owned(),
                ..NodeOptions::default()
            },
        );
    }

    if !named_imports.is_empty() {
        let declaration = file
            .arena
            .node(clause_or_pattern)
            .parent
            .expect("an import clause's declaration");
        let ((comparer, type_order), is_sorted) =
            get_named_import_specifier_comparer_with_detection(
                tracker.syntax(file),
                declaration,
                Some(statements(file, file.root)),
                preferences,
            );
        let mut new_specifiers = Vec::with_capacity(named_imports.len());
        for named_import in named_imports {
            let mut factory = tracker.arena().factory();
            let property_name = (!named_import.property_name.is_empty())
                .then(|| factory.create_identifier(source, named_import.property_name.clone()))
                .transpose()?;
            let name = factory.create_identifier(source, named_import.name.clone())?;
            new_specifiers.push(factory.create_import_specifier(
                source,
                (!clause.is_type_only || promote_from_type_only)
                    && should_use_type_only(named_import.add_as_type_only, preferences),
                property_name,
                name,
            )?);
        }
        {
            let arena = tracker.syntax(file);
            new_specifiers.sort_by(|a, b| {
                compare_import_or_export_specifiers(arena, a.node(), b.node(), comparer, type_order)
            });
        }
        if !existing_specifiers.is_empty() && is_sorted != Some(false) {
            // The sorting preference computed earlier may or may not have validated that these particular
            // import specifiers are sorted. If they aren't, `getImportSpecifierInsertionIndex` will return
            // nonsense. So if there are existing specifiers, even if we know the sorting preference, we
            // need to ensure that the existing specifiers are sorted according to the preference in order
            // to do a sorted insertion.

            // If we're promoting the clause from type-only, we need to transform the existing imports
            // before attempting to insert the new named imports (for comparison purposes only)
            let specs_to_compare_against = if promote_from_type_only {
                let mut synthetic = Vec::with_capacity(existing_specifiers.len());
                for &existing in &existing_specifiers {
                    let NodeData::ImportSpecifier(spec) = &file.arena.node(existing).data else {
                        unreachable!("an import specifier");
                    };
                    let property_name = spec
                        .property_name
                        .map(|property_name| TransformNode::new(source, property_name));
                    let name = TransformNode::new(source, spec.name.expect("a specifier's name"));
                    synthetic.push(
                        tracker
                            .arena()
                            .factory()
                            .create_import_specifier(source, true, property_name, name)?
                            .node(),
                    );
                }
                synthetic
            } else {
                existing_specifiers.clone()
            };
            let named_bindings = clause.named_bindings.expect("existing named imports");
            for specifier in new_specifiers {
                let insertion_index = {
                    let arena = tracker.syntax(file);
                    binary_search_unique(&specs_to_compare_against, |&value| {
                        compare_import_or_export_specifiers(
                            arena,
                            value,
                            specifier.node(),
                            comparer,
                            type_order,
                        )
                    })
                };
                tracker.insert_import_specifier_at_index(
                    file,
                    specifier,
                    named_bindings,
                    insertion_index,
                )?;
            }
        } else if let Some(&last) = existing_specifiers.last() {
            for specifier in new_specifiers {
                tracker.insert_node_in_list_after(file, last, specifier, None)?;
            }
        } else if !new_specifiers.is_empty() {
            let mut factory = tracker.arena().factory();
            let elements = factory.create_node_array(source, new_specifiers)?;
            let named = factory.create_named_imports(source, elements)?;
            if let Some(named_bindings) = clause.named_bindings {
                tracker.replace_node(file, named_bindings, named, None);
            } else {
                let name = clause
                    .name
                    .expect("Import clause must have either named imports or a default import");
                tracker.insert_node_after(file, name, named)?;
            }
        }
    }

    if promote_from_type_only {
        // Delete the 'type' keyword from the import clause
        // The first child of a type-only import clause is the 'type' keyword
        // import type { foo } from './bar'
        //        ^^^^
        let navigator = Navigator::new(file);
        let type_keyword = navigator
            .child_of_kind(View::Node(clause_or_pattern), SyntaxKind::TypeKeyword)
            .expect("type-only import clause should have a type keyword");
        tracker.delete_keyword(file, type_keyword);

        // Add 'type' modifier to existing specifiers (not newly added ones)
        // We preserve the type-onlyness of existing specifiers regardless of whether
        // it would make a difference in emit (user preference).
        for &specifier in &existing_specifiers {
            let is_type_only = matches!(
                &file.arena.node(specifier).data,
                NodeData::ImportSpecifier(data) if data.is_type_only
            );
            if !is_type_only {
                tracker.insert_modifier_before(file, SyntaxKind::TypeKeyword, specifier)?;
            }
        }
    }
    Ok(())
}

/// tsgo `promoteFromTypeOnly`: the `type` of the alias's import removed;
/// the declaration promoted.
fn promote_from_type_only<'a>(
    tracker: &mut Tracker<'a>,
    alias_declaration: NodeId,
    options: &CompilerOptions,
    file: &'a SourceFile,
    preferences: &UserPreferences,
) -> Result<NodeId, TransformError> {
    // See comment in `doAddExistingFix` on constant with the same name.
    let convert_existing_to_type_only = options.verbatim_module_syntax == Some(true);
    let node = file.arena.node(alias_declaration);
    match &node.data {
        NodeData::ImportSpecifier(spec) => {
            let parent = node
                .parent
                .expect("ImportSpecifier parent must be NamedImports");
            if spec.is_type_only {
                if let NodeData::NamedImports(named) = &file.arena.node(parent).data {
                    let elements = named
                        .elements
                        .map(|elements| file.arena.node_array(elements).nodes)
                        .unwrap_or_default();
                    if elements.len() > 1 {
                        // Create a synthetic specifier with isTypeOnly=false to compute sorted position
                        let source = tracker.source(file);
                        let mut factory = tracker.arena().factory();
                        let property_name = spec
                            .property_name
                            .map(|property_name| {
                                factory.create_identifier(source, node_text(file, property_name))
                            })
                            .transpose()?;
                        let name = factory.create_identifier(
                            source,
                            node_text(file, spec.name.expect("a specifier's name")),
                        )?;
                        let new_specifier =
                            factory.create_import_specifier(source, false, property_name, name)?;
                        let declaration = file
                            .arena
                            .node(parent)
                            .parent
                            .and_then(|clause| file.arena.node(clause).parent)
                            .expect("an import declaration");
                        let ((comparer, type_order), _) =
                            get_named_import_specifier_comparer_with_detection(
                                tracker.syntax(file),
                                declaration,
                                Some(statements(file, file.root)),
                                preferences,
                            );
                        let insertion_index = {
                            let arena = tracker.syntax(file);
                            binary_search_unique(elements, |&value| {
                                compare_import_or_export_specifiers(
                                    arena,
                                    value,
                                    new_specifier.node(),
                                    comparer,
                                    type_order,
                                )
                            })
                        };
                        let current_index = elements
                            .iter()
                            .position(|&element| element == alias_declaration);
                        if Some(insertion_index) != current_index {
                            tracker.delete(file, alias_declaration);
                            tracker.insert_import_specifier_at_index(
                                file,
                                new_specifier,
                                parent,
                                insertion_index,
                            )?;
                            return Ok(alias_declaration);
                        }
                    }
                    // If no re-sorting needed, just remove the 'type' keyword
                    let navigator = Navigator::new(file);
                    let first_token = navigator
                        .first_token(View::Node(alias_declaration))
                        .expect("a specifier's first token");
                    let type_keyword_pos = navigator.start_of_node(first_token, false) as usize;
                    let target = spec
                        .property_name
                        .or(spec.name)
                        .expect("a specifier's name");
                    let target_pos = start_of(file, target);
                    tracker.replace_range_with_text(
                        file,
                        (type_keyword_pos, target_pos),
                        String::new(),
                    );
                }
                return Ok(alias_declaration);
            }
            // The parent import clause is type-only
            assert!(
                file.arena.node(parent).kind == SyntaxKind::NamedImports,
                "ImportSpecifier parent must be NamedImports"
            );
            let clause = file
                .arena
                .node(parent)
                .parent
                .filter(|&clause| file.arena.node(clause).kind == SyntaxKind::ImportClause)
                .expect("NamedImports parent must be ImportClause");
            promote_import_clause(
                tracker,
                clause,
                file,
                preferences,
                convert_existing_to_type_only,
                Some(alias_declaration),
            )?;
            Ok(clause)
        }
        NodeData::ImportClause(_) => {
            promote_import_clause(
                tracker,
                alias_declaration,
                file,
                preferences,
                convert_existing_to_type_only,
                Some(alias_declaration),
            )?;
            Ok(alias_declaration)
        }
        NodeData::NamespaceImport(_) => {
            // Promote the parent import clause
            let clause = node
                .parent
                .filter(|&clause| file.arena.node(clause).kind == SyntaxKind::ImportClause)
                .expect("NamespaceImport parent must be ImportClause");
            promote_import_clause(
                tracker,
                clause,
                file,
                preferences,
                convert_existing_to_type_only,
                Some(alias_declaration),
            )?;
            Ok(clause)
        }
        NodeData::ImportEqualsDeclaration(_) => {
            // Remove the 'type' keyword (which is the second token: 'import' 'type' name '=' ...)
            let mut scanner = tsc_syntax::TokenScanner::new(file, node.pos as usize);
            // Skip 'import' keyword to get to 'type'
            scanner.scan();
            delete_type_keyword(tracker, file, scanner.token_start());
            Ok(alias_declaration)
        }
        _ => panic!("Unexpected alias declaration kind: {:?}", node.kind),
    }
}

/// tsgo `promoteImportClause`: the `type` of an import clause removed (and
/// under `verbatimModuleSyntax` the other specifiers made type-only).
fn promote_import_clause<'a>(
    tracker: &mut Tracker<'a>,
    clause: NodeId,
    file: &'a SourceFile,
    preferences: &UserPreferences,
    convert_existing_to_type_only: bool,
    alias_declaration: Option<NodeId>,
) -> Result<(), TransformError> {
    let NodeData::ImportClause(data) = &file.arena.node(clause).data else {
        unreachable!("an import clause");
    };
    // Delete the 'type' keyword
    if data.phase_modifier == Some(SyntaxKind::TypeKeyword) {
        delete_type_keyword(tracker, file, file.arena.node(clause).pos as usize);
    }
    // Handle verbatimModuleSyntax conversion
    // If convertExistingToTypeOnly is true, we need to add 'type' to other specifiers
    // in the same import declaration
    if !convert_existing_to_type_only {
        return Ok(());
    }
    let Some(named_bindings) = data.named_bindings else {
        return Ok(());
    };
    let NodeData::NamedImports(named) = &file.arena.node(named_bindings).data else {
        return Ok(());
    };
    let elements = named
        .elements
        .map(|elements| file.arena.node_array(elements).nodes)
        .unwrap_or_default();
    if elements.len() <= 1 {
        return Ok(());
    }
    let declaration = file
        .arena
        .node(clause)
        .parent
        .expect("an import declaration");
    // Check if the list is sorted and if we need to reorder
    let (_, is_sorted) = get_named_import_specifier_comparer_with_detection(
        tracker.syntax(file),
        declaration,
        Some(statements(file, file.root)),
        preferences,
    );
    let alias_specifier = alias_declaration
        .filter(|&alias| file.arena.node(alias).kind == SyntaxKind::ImportSpecifier);
    // If the alias declaration is an ImportSpecifier and the list is sorted,
    // move it to index 0 (since it will be the only non-type-only import)
    if let Some(alias) = alias_specifier.filter(|_| is_sorted != Some(false)) {
        // If not already at index 0, move it there
        if elements
            .iter()
            .position(|&element| element == alias)
            .is_some_and(|index| index > 0)
        {
            // Delete the specifier from its current position
            tracker.delete(file, alias);
            // Insert it at index 0
            let source = tracker.source(file);
            tracker.insert_import_specifier_at_index(
                file,
                TransformNode::new(source, alias),
                named_bindings,
                0,
            )?;
        }
    }
    // Add 'type' keyword to all other import specifiers that aren't already type-only
    for &element in elements {
        // Skip the specifier being promoted (if aliasDeclaration is an ImportSpecifier)
        if Some(element) == alias_specifier {
            continue;
        }
        // Skip if already type-only
        let is_type_only = matches!(
            &file.arena.node(element).data,
            NodeData::ImportSpecifier(spec) if spec.is_type_only
        );
        if !is_type_only {
            tracker.insert_modifier_before(file, SyntaxKind::TypeKeyword, element)?;
        }
    }
    Ok(())
}

/// tsgo `deleteTypeKeyword`: the `type` keyword token at `start` and the
/// spaces and tabs after it.
fn delete_type_keyword<'a>(tracker: &mut Tracker<'a>, file: &'a SourceFile, start: usize) {
    let scanner = tsc_syntax::TokenScanner::new(file, start);
    if scanner.token() != SyntaxKind::TypeKeyword {
        return;
    }
    let type_start = scanner.token_start();
    let mut type_end = scanner.token_end();
    // Skip trailing whitespace
    let bytes = file.text().as_bytes();
    while type_end < bytes.len() && matches!(bytes[type_end], b' ' | b'\t') {
        type_end += 1;
    }
    tracker.replace_range_with_text(file, (type_start, type_end), String::new());
}

/// tsgo `getModuleSpecifierText`: the module specifier of a promoted
/// declaration (an import clause or an `import =`).
fn get_module_specifier_text(file: &SourceFile, promoted: NodeId) -> String {
    let specifier_text = |expression: NodeId| match &file.arena.node(expression).data {
        NodeData::StringLiteral(data) => data.text.to_string_lossy().into_owned(),
        NodeData::NoSubstitutionTemplateLiteral(data) => data.text.to_string_lossy().into_owned(),
        _ => text_of_node(file, expression),
    };
    if let NodeData::ImportEqualsDeclaration(data) = &file.arena.node(promoted).data {
        let reference = data.module_reference.expect("an import = reference");
        if let NodeData::ExternalModuleReference(external) = &file.arena.node(reference).data {
            if let Some(expression) = external.expression {
                return specifier_text(expression);
            }
        }
        return text_of_node(file, reference);
    }
    let declaration = file
        .arena
        .node(promoted)
        .parent
        .expect("an import declaration");
    let NodeData::ImportDeclaration(data) = &file.arena.node(declaration).data else {
        unreachable!("an import declaration");
    };
    specifier_text(data.module_specifier.expect("a module specifier"))
}

/// tsgo `GetTextOfNode`: the node's source text without its trivia.
fn text_of_node(file: &SourceFile, node: NodeId) -> String {
    file.text()[start_of(file, node)..file.arena.node(node).end as usize].to_owned()
}

/// tsgo `Node.Text()` of an identifier or a string literal name.
fn node_text(file: &SourceFile, node: NodeId) -> String {
    crate::lsutil::node_name_text(&file.arena, node)
}

/// tsgo `addElementToBindingPattern`. tsgo makes the property name the
/// element's initializer (`{ x, foo = default }`).
fn add_element_to_binding_pattern<'a>(
    tracker: &mut Tracker<'a>,
    file: &'a SourceFile,
    pattern: NodeId,
    name: &str,
    property_name: &str,
) -> Result<(), TransformError> {
    let source = tracker.source(file);
    let mut factory = tracker.arena().factory();
    let name = factory.create_identifier(source, name.to_owned())?;
    let initializer = (!property_name.is_empty())
        .then(|| factory.create_identifier(source, property_name.to_owned()))
        .transpose()?;
    let element = factory.create_node(
        source,
        NodeData::BindingElement(BindingElementData {
            name: Some(name.node()),
            property_name: None,
            dot_dot_dot_token: None,
            initializer: initializer.map(TransformNode::node),
        }),
        TransformFlags::NONE,
    )?;
    let NodeData::ObjectBindingPattern(data) = &file.arena.node(pattern).data else {
        unreachable!("an object binding pattern");
    };
    let elements = data.elements.expect("a binding pattern's elements");
    if let Some(&last) = file.arena.node_array(elements).nodes.last() {
        tracker.insert_node_in_list_after(file, last, element, Some(elements))?;
    } else {
        let mut factory = tracker.arena().factory();
        let elements = factory.create_node_array(source, vec![element])?;
        let new_pattern = factory.create_object_binding_pattern(source, elements)?;
        tracker.replace_node(file, pattern, new_pattern, None);
    }
    Ok(())
}

/// tsgo `getNewImports`.
#[allow(clippy::too_many_arguments)]
pub(super) fn get_new_imports<'a>(
    tracker: &mut Tracker<'a>,
    file: &'a SourceFile,
    module_specifier: &str,
    quote_preference: QuotePreference,
    default_import: Option<&NewImportBinding>,
    named_imports: &[NewImportBinding],
    namespace_like_import: Option<&NewImportBinding>,
    options: &CompilerOptions,
    preferences: &UserPreferences,
) -> Result<Vec<TransformNode>, TransformError> {
    let source = tracker.source(file);
    let mut factory = tracker.arena().factory();
    let module_specifier_string_literal = factory.create_string_literal(
        source,
        module_specifier,
        quote_preference == QuotePreference::SINGLE,
    )?;
    let mut statements = Vec::new();
    if default_import.is_some() || !named_imports.is_empty() {
        // `verbatimModuleSyntax` should prefer top-level `import type` -
        // even though it's not an error, it would add unnecessary runtime emit.
        let top_level_type_only = default_import
            .is_none_or(|import| needs_type_only(import.add_as_type_only))
            && named_imports
                .iter()
                .all(|import| needs_type_only(import.add_as_type_only))
            || (options.verbatim_module_syntax == Some(true)
                || preferences.prefer_type_only_auto_imports == Some(true))
                && default_import
                    .is_none_or(|import| import.add_as_type_only != AddAsTypeOnly::NOT_ALLOWED)
                && !named_imports
                    .iter()
                    .any(|import| import.add_as_type_only == AddAsTypeOnly::NOT_ALLOWED);
        let default_import_node = default_import
            .map(|import| factory.create_identifier(source, import.name.clone()))
            .transpose()?;
        let mut specifiers = Vec::with_capacity(named_imports.len());
        for named_import in named_imports {
            let property_name = (!named_import.property_name.is_empty())
                .then(|| factory.create_identifier(source, named_import.property_name.clone()))
                .transpose()?;
            let name = factory.create_identifier(source, named_import.name.clone())?;
            specifiers.push(factory.create_import_specifier(
                source,
                !top_level_type_only
                    && should_use_type_only(named_import.add_as_type_only, preferences),
                property_name,
                name,
            )?);
        }
        // tsgo `makeImport`.
        let named_bindings = if specifiers.is_empty() {
            None
        } else {
            let elements = factory.create_node_array(source, specifiers)?;
            Some(factory.create_named_imports(source, elements)?)
        };
        let import_clause = if default_import_node.is_some() || named_bindings.is_some() {
            Some(factory.create_import_clause(
                source,
                top_level_type_only.then_some(SyntaxKind::TypeKeyword),
                default_import_node,
                named_bindings,
            )?)
        } else {
            None
        };
        statements.push(factory.create_import_declaration(
            source,
            None,
            import_clause,
            module_specifier_string_literal,
            None,
        )?);
    }
    if let Some(namespace_like_import) = namespace_like_import {
        let type_only = should_use_type_only(namespace_like_import.add_as_type_only, preferences);
        let name = factory.create_identifier(source, namespace_like_import.name.clone())?;
        let declaration = if namespace_like_import.kind == ImportKind::COMMON_JS {
            let reference = factory
                .create_external_module_reference(source, module_specifier_string_literal)?;
            factory.create_import_equals_declaration(source, None, type_only, name, reference)?
        } else {
            let namespace_import = factory.create_namespace_import(source, name)?;
            let clause = factory.create_import_clause(
                source,
                type_only.then_some(SyntaxKind::TypeKeyword),
                None,
                Some(namespace_import),
            )?;
            factory.create_import_declaration(
                source,
                None,
                Some(clause),
                module_specifier_string_literal,
                None,
            )?
        };
        statements.push(declaration);
    }
    assert!(
        !statements.is_empty(),
        "No statements to insert for new imports"
    );
    Ok(statements)
}

/// tsgo `getNewRequires`.
pub(super) fn get_new_requires<'a>(
    tracker: &mut Tracker<'a>,
    file: &'a SourceFile,
    module_specifier: &str,
    quote_preference: QuotePreference,
    default_import: Option<&NewImportBinding>,
    named_imports: &[NewImportBinding],
    namespace_like_import: Option<&NewImportBinding>,
) -> Result<Vec<TransformNode>, TransformError> {
    let source = tracker.source(file);
    let mut factory = tracker.arena().factory();
    let quoted_module_specifier = factory.create_string_literal(
        source,
        module_specifier,
        quote_preference == QuotePreference::SINGLE,
    )?;
    let mut statements = Vec::new();
    let mut binding_element =
        |property_name: Option<&str>, name: &str| -> Result<TransformNode, TransformError> {
            let property_name = property_name
                .map(|property_name| factory.create_identifier(source, property_name.to_owned()))
                .transpose()?;
            let name = factory.create_identifier(source, name.to_owned())?;
            factory.create_node(
                source,
                NodeData::BindingElement(BindingElementData {
                    name: Some(name.node()),
                    property_name: property_name.map(TransformNode::node),
                    dot_dot_dot_token: None,
                    initializer: None,
                }),
                TransformFlags::NONE,
            )
        };

    // const { default: foo, bar, etc } = require('./mod');
    if default_import.is_some() || !named_imports.is_empty() {
        let mut binding_elements = Vec::with_capacity(named_imports.len() + 1);
        if let Some(default_import) = default_import {
            binding_elements.push(binding_element(Some("default"), &default_import.name)?);
        }
        for named_import in named_imports {
            let property_name = (!named_import.property_name.is_empty())
                .then_some(named_import.property_name.as_str());
            binding_elements.push(binding_element(property_name, &named_import.name)?);
        }
        let elements = factory.create_node_array(source, binding_elements)?;
        let pattern = factory.create_object_binding_pattern(source, elements)?;
        statements.push(create_const_equals_require_declaration(
            tracker,
            file,
            pattern,
            quoted_module_specifier,
        )?);
    }

    // const foo = require('./mod');
    if let Some(namespace_like_import) = namespace_like_import {
        let name = tracker
            .arena()
            .factory()
            .create_identifier(source, namespace_like_import.name.clone())?;
        statements.push(create_const_equals_require_declaration(
            tracker,
            file,
            name,
            quoted_module_specifier,
        )?);
    }
    debug_assert!(!statements.is_empty());
    Ok(statements)
}

/// tsgo `createConstEqualsRequireDeclaration`.
fn create_const_equals_require_declaration<'a>(
    tracker: &mut Tracker<'a>,
    file: &'a SourceFile,
    name: TransformNode,
    quoted_module_specifier: TransformNode,
) -> Result<TransformNode, TransformError> {
    let source = tracker.source(file);
    let mut factory = tracker.arena().factory();
    let require = factory.create_identifier(source, "require")?;
    let arguments = factory.create_node_array(source, vec![quoted_module_specifier])?;
    let call = factory.create_node(
        source,
        NodeData::CallExpression(CallExpressionData {
            expression: Some(require.node()),
            question_dot_token: None,
            type_arguments: None,
            arguments: Some(arguments.array()),
        }),
        TransformFlags::NONE,
    )?;
    let declaration = factory.create_variable_declaration(source, name, None, None, Some(call))?;
    let declarations = factory.create_node_array(source, vec![declaration])?;
    let list = factory.create_variable_declaration_list(source, declarations, NodeFlags::CONST)?;
    factory.create_variable_statement(source, None, list)
}

/// tsgo `insertImports`.
pub(super) fn insert_imports<'a>(
    tracker: &mut Tracker<'a>,
    file: &'a SourceFile,
    imports: Vec<TransformNode>,
    blank_line_between: bool,
    preferences: &UserPreferences,
) -> Result<(), TransformError> {
    let statements = statements(file, file.root);
    let existing_import_statements: Vec<NodeId> =
        if tracker.syntax(file).node(imports[0].node()).kind == SyntaxKind::VariableStatement {
            statements
                .iter()
                .copied()
                .filter(|&statement| is_require_variable_statement(file, statement))
                .collect()
        } else {
            statements
                .iter()
                .copied()
                .filter(|&statement| {
                    matches!(
                        file.arena.node(statement).kind,
                        SyntaxKind::ImportDeclaration | SyntaxKind::ImportEqualsDeclaration
                    )
                })
                .collect()
        };
    let (comparer, is_sorted) = get_organize_imports_string_comparer_with_detection(
        tracker.syntax(file),
        &existing_import_statements,
        preferences,
    );
    let mut sorted_new_imports = imports;
    {
        let arena = tracker.syntax(file);
        sorted_new_imports.sort_by(|a, b| {
            compare_imports_or_require_statements(arena, a.node(), b.node(), comparer)
        });
    }

    if !existing_import_statements.is_empty() && is_sorted {
        // Existing imports are sorted, insert each new import at the correct position
        for new_import in sorted_new_imports {
            let insertion_index = {
                let arena = tracker.syntax(file);
                binary_search_unique(&existing_import_statements, |&value| {
                    compare_imports_or_require_statements(arena, value, new_import.node(), comparer)
                })
            };
            if insertion_index == 0 {
                // If the first import is top-of-file, insert after the leading comment which is likely the header.
                let leading_trivia_option =
                    if Some(&existing_import_statements[0]) == statements.first() {
                        LeadingTriviaOption::Exclude
                    } else {
                        LeadingTriviaOption::None
                    };
                tracker.insert_node_before(
                    file,
                    existing_import_statements[0],
                    new_import,
                    false,
                    leading_trivia_option,
                );
            } else {
                let previous_import = existing_import_statements[insertion_index - 1];
                tracker.insert_node_after(file, previous_import, new_import)?;
            }
        }
    } else if let Some(&last) = existing_import_statements.last() {
        tracker.insert_nodes_after(file, last, sorted_new_imports)?;
    } else {
        tracker.insert_at_top_of_file(file, sorted_new_imports, blank_line_between);
    }
    Ok(())
}

/// tsgo `ast.IsRequireVariableStatement`.
fn is_require_variable_statement(file: &SourceFile, statement: NodeId) -> bool {
    let NodeData::VariableStatement(data) = &file.arena.node(statement).data else {
        return false;
    };
    let Some(NodeData::VariableDeclarationList(list)) = data
        .declaration_list
        .map(|list| &file.arena.node(list).data)
    else {
        return false;
    };
    let declarations = list
        .declarations
        .map(|declarations| file.arena.node_array(declarations).nodes)
        .unwrap_or_default();
    !declarations.is_empty()
        && declarations
            .iter()
            .all(|&declaration| is_variable_declaration_initialized_to_require(file, declaration))
}

/// tsgo `needsTypeOnly`.
fn needs_type_only(add_as_type_only: AddAsTypeOnly) -> bool {
    add_as_type_only == AddAsTypeOnly::REQUIRED
}

/// tsgo `shouldUseTypeOnly`.
fn should_use_type_only(add_as_type_only: AddAsTypeOnly, preferences: &UserPreferences) -> bool {
    needs_type_only(add_as_type_only)
        || add_as_type_only != AddAsTypeOnly::NOT_ALLOWED
            && preferences.prefer_type_only_auto_imports == Some(true)
}

/// tsgo `astnav.GetStartOfNode(node, file, false)`.
fn start_of(file: &SourceFile, node: NodeId) -> usize {
    Navigator::new(file).start_of_node(Found::Node(View::Node(node)), false) as usize
}

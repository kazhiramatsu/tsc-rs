//! tsgo `ls/autoimport/fix.go`'s `Fix.Edits`: the text edits that make an
//! auto-import fix in a file, and their description.

use tsc_diagnostics::{DiagnosticMessage, MessageCatalog};
use tsc_emitter::{TransformError, TransformFlags, TransformNode};
use tsc_lsproto::{AddAsTypeOnly, AutoImportFix, AutoImportFixKind, ImportKind, TextEdit};
use tsc_syntax::nodes::{BindingElementData, CallExpressionData};
use tsc_syntax::{NodeData, NodeId, SourceFile, SyntaxKind};
use tsc_types::{CompilerOptions, NodeFlags};

use super::view::{
    is_variable_declaration_initialized_to_require, try_get_import_from_module_specifier,
};
use crate::astnav::{Found, Navigator};
use crate::change::{LeadingTriviaOption, NodeOptions, Tracker};
use crate::lsconv::Converters;
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
struct NewImportBinding {
    kind: ImportKind,
    property_name: String,
    name: String,
    add_as_type_only: AddAsTypeOnly,
}

impl NewImportBinding {
    fn new(name: &str, add_as_type_only: AddAsTypeOnly) -> Self {
        Self {
            kind: ImportKind::default(),
            property_name: String::new(),
            name: name.to_owned(),
            add_as_type_only,
        }
    }
}

/// tsgo `Fix.Edits`: the edits that make `fix` in `file`, and their
/// description. An edit the tracker cannot make is an error.
#[allow(clippy::too_many_arguments)]
pub(crate) fn fix_edits(
    fix: &AutoImportFix,
    file: &SourceFile,
    options: &CompilerOptions,
    format_settings: FormatCodeSettings,
    converters: &Converters,
    preferences: &UserPreferences,
    catalog: Option<&dyn MessageCatalog>,
) -> Result<(Vec<TextEdit>, String), String> {
    let script = script_of(file);
    let usage_position = fix
        .usage_position
        .as_ref()
        .map(|position| converters.from_lsp_position(&script, position));
    let mut tracker = Tracker::new(new_line_kind(options), format_settings);
    let description = make_fix(
        &mut tracker,
        fix,
        file,
        options,
        preferences,
        usage_position,
        catalog,
    )
    .map_err(|error| error.to_string())?;
    let changes = tracker.get_changes().map_err(|error| error.to_string())?;
    // tsgo `fileEdits`: the file's edits.
    let edits = changes
        .into_iter()
        .filter(|changes| std::ptr::eq(changes.file, file))
        .flat_map(|changes| changes.changes)
        .map(|change| TextEdit {
            range: converters.to_lsp_range(&script, change.pos..change.end),
            new_text: change.new_text,
        })
        .collect();
    Ok((edits, description))
}

/// The edits of `Fix.Edits` recorded by the tracker, and their description.
fn make_fix<'a>(
    tracker: &mut Tracker<'a>,
    fix: &AutoImportFix,
    file: &'a SourceFile,
    options: &CompilerOptions,
    preferences: &UserPreferences,
    usage_position: Option<usize>,
    catalog: Option<&dyn MessageCatalog>,
) -> Result<String, TransformError> {
    let localize = |message: &'static DiagnosticMessage, args: &[&str]| {
        let args: Vec<String> = args.iter().map(|&arg| arg.to_owned()).collect();
        tsc_diagnostics::format_message(message.template_in(catalog), &args)
    };
    match fix.kind {
        AutoImportFixKind::USE_NAMESPACE => {
            // tsgo `addNamespaceQualifier`.
            let Some(usage_position) = usage_position.filter(|_| !fix.namespace_prefix.is_empty())
            else {
                panic!("namespace fix requires usage position and prefix");
            };
            let qualified = format!("{}.{}", fix.namespace_prefix, fix.name);
            tracker.insert_text_at(file, usage_position, format!("{}.", fix.namespace_prefix));
            Ok(localize(
                &tsc_diagnostics::gen::Change_0_to_1,
                &[&fix.name, &qualified],
            ))
        }
        AutoImportFixKind::ADD_TO_EXISTING => {
            let imports = collect_external_module_references(file).imports;
            let Some(&module_specifier) = imports.get(fix.import_index as usize) else {
                panic!("import index out of range");
            };
            let (clause_or_pattern, default_import, named_import) =
                get_add_to_existing_import_fix(file, fix, module_specifier);
            add_to_existing_import(
                tracker,
                file,
                clause_or_pattern,
                default_import.as_ref(),
                named_import.as_slice(),
                preferences,
            )?;
            Ok(localize(
                &tsc_diagnostics::gen::Update_import_from_0,
                &[&fix.module_specifier],
            ))
        }
        AutoImportFixKind::ADD_NEW => {
            let default_import = (fix.import_kind == ImportKind::DEFAULT)
                .then(|| NewImportBinding::new(&fix.name, fix.add_as_type_only));
            let named_imports: Vec<NewImportBinding> = (fix.import_kind == ImportKind::NAMED)
                .then(|| NewImportBinding::new(&fix.name, fix.add_as_type_only))
                .into_iter()
                .collect();
            let namespace_like_import = (fix.import_kind == ImportKind::NAMESPACE
                || fix.import_kind == ImportKind::COMMON_JS)
                .then(|| NewImportBinding {
                    kind: fix.import_kind,
                    ..NewImportBinding::new(&fix.name, AddAsTypeOnly::default())
                });
            let quote_preference = get_quote_preference(file, preferences);
            let declarations = if fix.use_require {
                get_new_requires(
                    tracker,
                    file,
                    &fix.module_specifier,
                    quote_preference,
                    default_import.as_ref(),
                    &named_imports,
                    namespace_like_import.as_ref(),
                )?
            } else {
                get_new_imports(
                    tracker,
                    file,
                    &fix.module_specifier,
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
                &tsc_diagnostics::gen::Add_import_from_0,
                &[&fix.module_specifier],
            ))
        }
        AutoImportFixKind::JSDOC_TYPE_IMPORT => {
            // tsgo `addImportType`.
            let Some(usage_position) = usage_position else {
                panic!("UsagePosition must be set for JSDoc type import fix");
            };
            let quote = if get_quote_preference(file, preferences) == QuotePreference::SINGLE {
                '\''
            } else {
                '"'
            };
            let import_type_prefix = format!("import({quote}{}{quote}).", fix.module_specifier);
            tracker.insert_text_at(file, usage_position, import_type_prefix.clone());
            Ok(localize(
                &tsc_diagnostics::gen::Change_0_to_1,
                &[&fix.name, &format!("{import_type_prefix}{}", fix.name)],
            ))
        }
        // The promotion of a type-only import comes with the code fixes'
        // declaration (L5b).
        _ => panic!("unimplemented fix edit"),
    }
}

/// tsgo `getAddToExistingImportFix`: the import clause or `require`
/// binding pattern of the fix's import, and the binding to add.
fn get_add_to_existing_import_fix(
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
fn add_to_existing_import<'a>(
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
fn get_new_imports<'a>(
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
fn get_new_requires<'a>(
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
fn insert_imports<'a>(
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

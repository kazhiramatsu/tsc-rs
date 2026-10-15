//! tsgo `ls/organizeimports.go`: organize imports as a file's edits — the
//! imports of each group without the unused ones, combined and sorted, and
//! the exports combined and sorted.
//!
//! The declarations live in the change tracker's copy of the file
//! ([`Tracker::syntax`]): the parsed ones keep their ids there, and the
//! factory adds the updated ones beside them.

use std::cmp::Ordering;

use tsc_checker::state::{CheckResult, CheckerState};
use tsc_emitter::{range_is_on_single_line, EmitFlags, TransformNode, TransformNodeArray};
use tsc_lsproto::{CodeActionKind, TextEdit};
use tsc_syntax::nodes::ImportClauseData;
use tsc_syntax::{skip_trivia, NodeArena, NodeData, NodeId, SourceFile, SyntaxKind, TokenScanner};
use tsc_types::{NodeFlags, SymbolId};

use crate::astnav::{Found, Navigator};
use crate::change::{LeadingTriviaOption, NodeOptions, Tracker, TrailingTriviaOption};
use crate::language_service::{LanguageService, LsError};
use crate::lsutil::{
    compare_import_or_export_specifiers, compare_imports_or_require_statements,
    compare_module_specifiers, detect_module_specifier_case_by_sort,
    detect_named_import_organization_by_sort, filter_import_declarations, get_detection_lists,
    get_external_module_name, get_named_import_specifier_comparer, new_line_kind, node_name_text,
    resolve_organize_imports_sort, OrganizeImportsSort, OrganizeImportsTypeOrder, StringComparer,
    UserPreferences,
};
use crate::references::{collect_external_module_references, statements};

impl LanguageService<'_> {
    /// tsgo `OrganizeImports`: the edits of each file, by file name.
    pub(crate) fn organize_imports(
        &mut self,
        file: usize,
        kind: &CodeActionKind,
    ) -> Result<Vec<(String, Vec<TextEdit>)>, LsError> {
        let format_settings = self.format_options().clone();
        let changes = self.with_program_request(false, |request| {
            let source = request.state.binder.source(file);
            let mut tracker = Tracker::new(new_line_kind(request.state.options), format_settings);
            organize_imports(
                request.state,
                request.preferences,
                source,
                kind,
                &mut tracker,
            )?;
            // Unmappable files are dropped by GetChanges, so a content-mapped file whose imports cannot be
            // faithfully rewritten yields no edits rather than a corrupting one.
            Ok(tracker
                .get_changes(request.converters, &request.state.binder)
                .map(|changes| changes.files))
        })?;
        match changes {
            Some(Ok(changes)) => Ok(changes),
            // tsgo panics on an edit it cannot make.
            Some(Err(error)) => Err(LsError(format!("organize imports: {error}"))),
            None => Ok(Vec::new()),
        }
    }
}

/// tsgo `organizeImportsComparerSettings`.
#[derive(Clone, Copy)]
struct ComparerSettings {
    module_specifier: StringComparer,
    named_import: Option<StringComparer>,
    type_order: OrganizeImportsTypeOrder,
}

impl ComparerSettings {
    /// tsgo `GetNamedImportSpecifierComparer(UserPreferences{OrganizeImportsTypeOrder: typeOrder}, namedImportComparer)`.
    fn specifier_comparer(self) -> (StringComparer, OrganizeImportsTypeOrder) {
        get_named_import_specifier_comparer(
            &UserPreferences {
                organize_imports_type_order: self.type_order,
                ..UserPreferences::default()
            },
            self.named_import,
        )
    }
}

/// What the workers read of the file and the program.
struct Organizer<'c, 's, 'a> {
    checker: &'c mut CheckerState<'s>,
    source: &'a SourceFile,
    /// tsgo `SubtreeContainsJsx` of the file.
    jsx_elements_present: bool,
    /// The JSX emit is `react` or `react-native`.
    jsx_mode_needs_explicit_import: bool,
    comparer: ComparerSettings,
    should_sort: bool,
    should_combine: bool,
    should_remove: bool,
}

/// tsgo `OrganizeImports` with its tracker.
fn organize_imports<'a>(
    checker: &mut CheckerState<'_>,
    preferences: &UserPreferences,
    source: &'a SourceFile,
    kind: &CodeActionKind,
    tracker: &mut Tracker<'a>,
) -> CheckResult<()> {
    let should_sort = *kind == CodeActionKind::SOURCE_SORT_IMPORTS_TS
        || *kind == CodeActionKind::SOURCE_ORGANIZE_IMPORTS_TS;
    let should_remove = *kind == CodeActionKind::SOURCE_REMOVE_UNUSED_IMPORTS_TS
        || *kind == CodeActionKind::SOURCE_ORGANIZE_IMPORTS_TS;
    let organize_exports = *kind != CodeActionKind::SOURCE_REMOVE_UNUSED_IMPORTS_TS;
    let arena = &source.arena;
    let top_level_import_decls = filter_import_declarations(arena, statements(source, source.root));
    let top_level_import_group_decls = group_by_newline_contiguous(source, &top_level_import_decls);

    let (comparers_to_test, type_orders_to_test) = get_detection_lists(preferences);
    let default_comparer = comparers_to_test[0];
    let sort = resolve_organize_imports_sort(preferences);
    let (module_specifier_comparer, mut named_import_comparer) =
        if sort == OrganizeImportsSort::AUTO {
            let (detected, _) = detect_module_specifier_case_by_sort(
                arena,
                &top_level_import_group_decls,
                &comparers_to_test,
            );
            (detected, None)
        } else {
            (default_comparer, Some(default_comparer))
        };
    let mut type_order = preferences.organize_imports_type_order;
    if type_order == OrganizeImportsTypeOrder::AUTO || sort == OrganizeImportsSort::AUTO {
        if let Some(found) = detect_named_import_organization_by_sort(
            arena,
            &top_level_import_decls,
            &comparers_to_test,
            &type_orders_to_test,
        ) {
            if named_import_comparer.is_none() || sort == OrganizeImportsSort::AUTO {
                named_import_comparer = Some(found.comparer);
            }
            if type_order == OrganizeImportsTypeOrder::AUTO {
                type_order = found.type_order;
            }
        }
    }

    let mut organizer = Organizer {
        jsx_elements_present: arena.nodes().iter().any(|node| {
            matches!(
                node.kind,
                SyntaxKind::JsxElement
                    | SyntaxKind::JsxSelfClosingElement
                    | SyntaxKind::JsxFragment
            )
        }),
        // `react` or `react-native`.
        jsx_mode_needs_explicit_import: matches!(checker.options.jsx, Some(2 | 3)),
        checker,
        source,
        comparer: ComparerSettings {
            module_specifier: module_specifier_comparer,
            named_import: named_import_comparer,
            type_order,
        },
        should_sort,
        should_combine: should_sort,
        should_remove,
    };
    for group in &top_level_import_group_decls {
        organizer.organize_imports_worker(group, tracker)?;
    }
    if organize_exports {
        for group in get_top_level_export_groups(source) {
            organizer.organize_exports_worker(&group, tracker);
        }
    }
    for &statement in statements(source, source.root) {
        if !is_ambient_module(arena, statement) {
            continue;
        }
        let body = match &arena.node(statement).data {
            NodeData::ModuleDeclaration(module) => module.body,
            _ => None,
        };
        let Some(body) = body else {
            continue;
        };
        let body_statements = statements(source, body);
        let ambient_module_import_decls = filter_import_declarations(arena, body_statements);
        for group in group_by_newline_contiguous(source, &ambient_module_import_decls) {
            organizer.organize_imports_worker(&group, tracker)?;
        }
        if organize_exports {
            let ambient_module_export_decls: Vec<NodeId> = body_statements
                .iter()
                .copied()
                .filter(|&statement| arena.node(statement).kind == SyntaxKind::ExportDeclaration)
                .collect();
            organizer.organize_exports_worker(&ambient_module_export_decls, tracker);
        }
    }
    Ok(())
}

/// tsgo `IsAmbientModule`: a module declaration named by a string literal,
/// or a global augmentation.
fn is_ambient_module(arena: &NodeArena, node: NodeId) -> bool {
    let record = arena.node(node);
    let NodeData::ModuleDeclaration(module) = &record.data else {
        return false;
    };
    module
        .name
        .is_some_and(|name| arena.node(name).kind == SyntaxKind::StringLiteral)
        || NodeFlags::from_bits(record.flags).contains(NodeFlags::GLOBAL_AUGMENTATION)
}

impl<'a> Organizer<'_, '_, 'a> {
    /// tsgo `organizeImportsWorker`.
    fn organize_imports_worker(
        &mut self,
        old_import_decls: &[NodeId],
        tracker: &mut Tracker<'a>,
    ) -> CheckResult<()> {
        if old_import_decls.is_empty() {
            return Ok(());
        }
        let source = self.source;
        let file = tracker.source(source);
        // Header comment preservation is handled via LeadingTriviaOptionExclude in the change tracker below
        let mut processed_imports: Vec<TransformNode> = old_import_decls
            .iter()
            .map(|&decl| TransformNode::new(file, decl))
            .collect();
        if self.should_remove {
            processed_imports = self.remove_unused_imports(&processed_imports, tracker)?;
        }
        let comparer = self.comparer;
        let mut new_import_decls = Vec::new();
        if self.should_combine {
            let mut grouped = group_by_module_specifier(tracker.syntax(source), &processed_imports);
            if self.should_sort {
                let arena = tracker.syntax(source);
                grouped.sort_by(|a, b| {
                    compare_module_specifiers(
                        arena,
                        module_specifier(arena, a[0].node()),
                        module_specifier(arena, b[0].node()),
                        comparer.module_specifier,
                    )
                });
            }
            let specifier_comparer = comparer.specifier_comparer();
            for import_group in grouped {
                let mut coalesced = self.coalesce_imports_worker(
                    &import_group,
                    comparer.module_specifier,
                    specifier_comparer,
                    tracker,
                );
                if self.should_sort {
                    let arena = tracker.syntax(source);
                    coalesced.sort_by(|a, b| {
                        compare_imports_or_require_statements(
                            arena,
                            a.node(),
                            b.node(),
                            comparer.module_specifier,
                        )
                    });
                }
                new_import_decls.extend(coalesced);
            }
        } else {
            new_import_decls = processed_imports;
        }
        if self.should_sort && !self.should_combine {
            let arena = tracker.syntax(source);
            new_import_decls.sort_by(|a, b| {
                compare_imports_or_require_statements(
                    arena,
                    a.node(),
                    b.node(),
                    comparer.module_specifier,
                )
            });
        }
        let first = old_import_decls[0];
        if new_import_decls.is_empty() {
            tracker.delete_node_range(
                source,
                first,
                old_import_decls[old_import_decls.len() - 1],
                LeadingTriviaOption::Exclude, // Preserve header comment
                TrailingTriviaOption::Include,
            );
        } else {
            for &import in &new_import_decls {
                tracker.set_emit_flags(import, EmitFlags::NO_LEADING_COMMENTS);
            }
            let options = NodeOptions {
                leading_trivia_option: LeadingTriviaOption::Exclude, // Preserve header comment
                trailing_trivia_option: TrailingTriviaOption::Include,
                suffix: "\n".to_owned(),
                ..NodeOptions::default()
            };
            tracker.replace_node_with_nodes(source, first, new_import_decls, Some(options));
            for &decl in &old_import_decls[1..] {
                tracker.delete(source, decl);
            }
        }
        Ok(())
    }

    /// tsgo `removeUnusedImports`.
    fn remove_unused_imports(
        &mut self,
        old_imports: &[TransformNode],
        tracker: &mut Tracker<'a>,
    ) -> CheckResult<Vec<TransformNode>> {
        let source = self.source;
        let arena = &source.arena;
        let mut used_imports = Vec::with_capacity(old_imports.len());
        for &import_decl in old_imports {
            let file = import_decl.source();
            let Some((clause_id, clause)) = import_clause(arena, import_decl.node()) else {
                used_imports.push(import_decl);
                continue;
            };
            let mut name = clause.name;
            if let Some(identifier) = name {
                if !self.is_declaration_used(identifier)? {
                    name = None;
                }
            }
            let mut named_bindings = clause
                .named_bindings
                .map(|bindings| TransformNode::new(file, bindings));
            if let Some(original_bindings) = clause.named_bindings {
                match &arena.node(original_bindings).data {
                    NodeData::NamespaceImport(namespace_import) => {
                        if let Some(identifier) = namespace_import.name {
                            if !self.is_declaration_used(identifier)? {
                                named_bindings = None;
                            }
                        }
                    }
                    NodeData::NamedImports(named_imports) => {
                        let elements = named_imports
                            .elements
                            .map(|list| arena.node_array(list).nodes)
                            .unwrap_or_default();
                        // tsgo `filterUsedImportSpecifiers`.
                        let mut new_elements = Vec::new();
                        for &element in elements {
                            let NodeData::ImportSpecifier(specifier) = &arena.node(element).data
                            else {
                                continue;
                            };
                            if let Some(identifier) = specifier.name {
                                if self.is_declaration_used(identifier)? {
                                    new_elements.push(TransformNode::new(file, element));
                                }
                            }
                        }
                        if new_elements.is_empty() {
                            named_bindings = None;
                        } else if new_elements.len() < elements.len() {
                            let mut factory = tracker.arena().factory();
                            let new_list = factory
                                .create_node_array(file, new_elements)
                                .expect("a list of the file");
                            named_bindings = Some(
                                factory
                                    .update_named_imports(
                                        TransformNode::new(file, original_bindings),
                                        new_list,
                                    )
                                    .expect("named imports"),
                            );
                        }
                        if let Some(bindings) = named_bindings {
                            let original = arena.node(original_bindings);
                            if !is_synthesized(original.pos)
                                && !range_is_on_single_line(source, original.pos, original.end)
                            {
                                tracker.set_emit_flags(bindings, EmitFlags::MULTI_LINE);
                            }
                        }
                    }
                    _ => {}
                }
            }
            if name.is_some() || named_bindings.is_some() {
                let new_clause = tracker
                    .arena()
                    .factory()
                    .update_import_clause(
                        TransformNode::new(file, clause_id),
                        clause.phase_modifier,
                        name.map(|name| TransformNode::new(file, name)),
                        named_bindings,
                    )
                    .expect("an import clause");
                used_imports.push(update_import_declaration(
                    tracker,
                    import_decl,
                    Some(new_clause),
                ));
            } else if has_module_declaration_matching_specifier(
                source,
                module_specifier(arena, import_decl.node()),
            ) {
                if source.is_declaration_file {
                    used_imports.push(update_import_declaration(tracker, import_decl, None));
                } else {
                    used_imports.push(import_decl);
                }
            }
        }
        Ok(used_imports)
    }

    /// tsgo `Checker.IsDeclarationUsed`: whether an import's name is used
    /// in the file.
    fn is_declaration_used(&mut self, identifier: NodeId) -> CheckResult<bool> {
        let source = self.source;
        let identifier_text = node_name_text(&source.arena, identifier);
        if self.jsx_elements_present && self.jsx_mode_needs_explicit_import {
            if self.checker.get_jsx_namespace(source.root).unescape() == identifier_text.as_str() {
                return Ok(true);
            }
            if self
                .checker
                .get_jsx_fragment_factory(source.root)
                .is_some_and(|factory| factory.unescape() == identifier_text.as_str())
            {
                return Ok(true);
            }
        }
        let Some(symbol) = self.checker.get_symbol_at_location(identifier)? else {
            return Ok(true);
        };
        self.is_symbol_referenced_in_file(identifier, &identifier_text, symbol)
    }

    /// tsgo `Checker.IsSymbolReferencedInFile`: whether an identifier of the
    /// file besides the definition refers to the symbol.
    fn is_symbol_referenced_in_file(
        &mut self,
        definition: NodeId,
        identifier_text: &str,
        symbol: SymbolId,
    ) -> CheckResult<bool> {
        let source = self.source;
        let arena = &source.arena;
        for token in possible_symbol_reference_nodes(source, identifier_text) {
            if arena.node(token).kind != SyntaxKind::Identifier
                || token == definition
                || node_name_text(arena, token) != identifier_text
            {
                continue;
            }
            let reference_symbol = self.checker.get_symbol_at_location(token)?;
            if reference_symbol == Some(symbol) {
                return Ok(true);
            }
            let Some(parent) = arena.node(token).parent else {
                continue;
            };
            match &arena.node(parent).data {
                NodeData::ShorthandPropertyAssignment(_) => {
                    if self.checker.get_shorthand_assignment_value_symbol(parent)? == Some(symbol) {
                        return Ok(true);
                    }
                }
                NodeData::ExportSpecifier(specifier) => {
                    // tsgo `getLocalSymbolForExportSpecifier`.
                    let is_alias = match specifier.property_name {
                        // Given `export { foo as bar } [from "someModule"]`: It's an alias at `foo`, but at `bar` it's a new symbol.
                        Some(property_name) => property_name == token,
                        // `export { foo } from "foo"` is a re-export.
                        // `export { foo };` is not a re-export, it creates an alias for the local variable `foo`.
                        None => arena
                            .node(parent)
                            .parent
                            .and_then(|named_exports| arena.node(named_exports).parent)
                            .is_some_and(|declaration| {
                                module_specifier(arena, declaration).is_none()
                            }),
                    };
                    let local_symbol = if is_alias {
                        self.checker
                            .get_export_specifier_local_target_symbol(parent)?
                            .or(reference_symbol)
                    } else {
                        reference_symbol
                    };
                    if local_symbol == Some(symbol) {
                        return Ok(true);
                    }
                }
                _ => {}
            }
        }
        Ok(false)
    }

    /// tsgo `coalesceImportsWorker`: the imports of one module combined,
    /// by their attributes.
    fn coalesce_imports_worker(
        &self,
        import_decls: &[TransformNode],
        comparer: StringComparer,
        specifier_comparer: (StringComparer, OrganizeImportsTypeOrder),
        tracker: &mut Tracker<'a>,
    ) -> Vec<TransformNode> {
        let source = self.source;
        let compare_specifiers = |arena: &NodeArena, a: NodeId, b: NodeId| {
            compare_import_or_export_specifiers(
                arena,
                a,
                b,
                specifier_comparer.0,
                specifier_comparer.1,
            )
        };
        let mut import_groups_by_attributes: Vec<(String, Vec<TransformNode>)> = Vec::new();
        {
            let arena = tracker.syntax(source);
            for &import_decl in import_decls {
                let attributes = match &arena.node(import_decl.node()).data {
                    NodeData::ImportDeclaration(data) => data.attributes,
                    _ => None,
                };
                let key = get_import_attributes_key(arena, attributes);
                match import_groups_by_attributes
                    .iter_mut()
                    .find(|(known, _)| *known == key)
                {
                    Some((_, group)) => group.push(import_decl),
                    None => import_groups_by_attributes.push((key, vec![import_decl])),
                }
            }
        }
        let mut coalesced_imports = Vec::new();
        for (_, import_group_same_attrs) in import_groups_by_attributes {
            let mut categorized =
                get_categorized_imports(tracker.syntax(source), &import_group_same_attrs);
            if let Some(import) = categorized.import_without_clause {
                coalesced_imports.push(import);
            }
            {
                let arena = tracker.syntax(source);
                let clause_of = |import: &TransformNode| {
                    import_clause(arena, import.node()).expect("a source phase import")
                };
                categorized.source_phase_imports.sort_by(|a, b| {
                    let (a, a_clause) = clause_of(a);
                    let (b, b_clause) = clause_of(b);
                    match (a_clause.name, b_clause.name) {
                        (None, None) => Ordering::Equal,
                        (None, _) => Ordering::Greater,
                        (_, None) => Ordering::Less,
                        _ => compare_specifiers(arena, a, b),
                    }
                });
            }
            coalesced_imports.extend(categorized.source_phase_imports.iter().copied());
            for (index, mut group) in [categorized.regular_imports, categorized.type_only_imports]
                .into_iter()
                .enumerate()
            {
                if group.is_empty() {
                    continue;
                }
                let is_type_only = index == 1;
                if !is_type_only
                    && group.default_imports.len() == 1
                    && group.namespace_imports.len() == 1
                    && group.named_imports.is_empty()
                {
                    let default_import = group.default_imports[0];
                    let file = default_import.source();
                    let (default_clause_id, default_clause, namespace_bindings) = {
                        let arena = tracker.syntax(source);
                        let (id, clause) =
                            import_clause(arena, default_import.node()).expect("a default import");
                        let bindings = import_clause(arena, group.namespace_imports[0].node())
                            .and_then(|(_, clause)| clause.named_bindings);
                        (id, clause, bindings)
                    };
                    let new_clause = tracker
                        .arena()
                        .factory()
                        .update_import_clause(
                            TransformNode::new(file, default_clause_id),
                            default_clause.phase_modifier,
                            default_clause
                                .name
                                .map(|name| TransformNode::new(file, name)),
                            namespace_bindings.map(|bindings| TransformNode::new(file, bindings)),
                        )
                        .expect("an import clause");
                    coalesced_imports.push(update_import_declaration(
                        tracker,
                        default_import,
                        Some(new_clause),
                    ));
                    continue;
                }
                {
                    let arena = tracker.syntax(source);
                    let namespace_name = |import: &TransformNode| {
                        import_clause(arena, import.node())
                            .and_then(|(_, clause)| clause.named_bindings)
                            .and_then(|bindings| match &arena.node(bindings).data {
                                NodeData::NamespaceImport(namespace_import) => {
                                    namespace_import.name
                                }
                                _ => None,
                            })
                            .map_or_else(String::new, |name| node_name_text(arena, name))
                    };
                    group
                        .namespace_imports
                        .sort_by(|a, b| comparer.compare(&namespace_name(a), &namespace_name(b)));
                }
                for &namespace_import in &group.namespace_imports {
                    let file = namespace_import.source();
                    let (clause_id, clause) =
                        import_clause(tracker.syntax(source), namespace_import.node())
                            .expect("a namespace import");
                    let new_clause = tracker
                        .arena()
                        .factory()
                        .update_import_clause(
                            TransformNode::new(file, clause_id),
                            clause.phase_modifier,
                            None,
                            clause
                                .named_bindings
                                .map(|bindings| TransformNode::new(file, bindings)),
                        )
                        .expect("an import clause");
                    coalesced_imports.push(update_import_declaration(
                        tracker,
                        namespace_import,
                        Some(new_clause),
                    ));
                }
                let first_default_import = group.default_imports.first().copied();
                let first_named_import = group.named_imports.first().copied();
                let Some(import_decl) = first_default_import.or(first_named_import) else {
                    continue;
                };
                let file = import_decl.source();
                let mut new_default_import = None;
                let mut new_import_specifiers = Vec::new();
                if group.default_imports.len() == 1 {
                    new_default_import =
                        import_clause(tracker.syntax(source), group.default_imports[0].node())
                            .and_then(|(_, clause)| clause.name)
                            .map(|name| TransformNode::new(file, name));
                } else {
                    for &default_import in &group.default_imports {
                        let default_name =
                            import_clause(tracker.syntax(source), default_import.node())
                                .and_then(|(_, clause)| clause.name)
                                .expect("a default import");
                        let mut factory = tracker.arena().factory();
                        let property_name = factory
                            .create_identifier(file, "default")
                            .expect("an identifier");
                        new_import_specifiers.push(
                            factory
                                .create_import_specifier(
                                    file,
                                    false,
                                    Some(property_name),
                                    TransformNode::new(file, default_name),
                                )
                                .expect("an import specifier"),
                        );
                    }
                }
                new_import_specifiers.extend(get_new_import_specifiers(
                    tracker,
                    source,
                    &group.named_imports,
                ));
                {
                    let arena = tracker.syntax(source);
                    new_import_specifiers
                        .sort_by(|a, b| compare_specifiers(arena, a.node(), b.node()));
                }
                let first_named_bindings = first_named_import.and_then(|named_import| {
                    import_clause(tracker.syntax(source), named_import.node())
                        .and_then(|(_, clause)| clause.named_bindings)
                });
                let new_named_imports = if new_import_specifiers.is_empty() {
                    if new_default_import.is_some() {
                        None
                    } else {
                        let mut factory = tracker.arena().factory();
                        let list = factory.create_node_array(file, Vec::new()).expect("a list");
                        Some(
                            factory
                                .create_named_imports(file, list)
                                .expect("named imports"),
                        )
                    }
                } else {
                    let sorted_list = sorted_specifier_list(
                        tracker,
                        source,
                        file,
                        new_import_specifiers,
                        first_named_bindings,
                    );
                    let mut factory = tracker.arena().factory();
                    Some(match first_named_bindings {
                        Some(first_named_bindings) => factory
                            .update_named_imports(
                                TransformNode::new(file, first_named_bindings),
                                sorted_list,
                            )
                            .expect("named imports"),
                        None => factory
                            .create_named_imports(file, sorted_list)
                            .expect("named imports"),
                    })
                };
                if let (Some(new_named_imports), Some(first_named_bindings)) =
                    (new_named_imports, first_named_bindings)
                {
                    let (pos, end) = {
                        let record = tracker.syntax(source).node(first_named_bindings);
                        (record.pos, record.end)
                    };
                    if !is_synthesized(pos) && !range_is_on_single_line(source, pos, end) {
                        tracker.set_emit_flags(new_named_imports, EmitFlags::MULTI_LINE);
                    }
                }
                if is_type_only && new_default_import.is_some() && new_named_imports.is_some() {
                    let phase_modifier = import_clause(tracker.syntax(source), import_decl.node())
                        .and_then(|(_, clause)| clause.phase_modifier);
                    let default_clause = tracker
                        .arena()
                        .factory()
                        .create_import_clause(file, phase_modifier, new_default_import, None)
                        .expect("an import clause");
                    coalesced_imports.push(update_import_declaration(
                        tracker,
                        import_decl,
                        Some(default_clause),
                    ));
                    let named_decl = first_named_import.unwrap_or(import_decl);
                    let named_phase_modifier =
                        import_clause(tracker.syntax(source), named_decl.node())
                            .and_then(|(_, clause)| clause.phase_modifier);
                    let named_clause = tracker
                        .arena()
                        .factory()
                        .create_import_clause(file, named_phase_modifier, None, new_named_imports)
                        .expect("an import clause");
                    coalesced_imports.push(update_import_declaration(
                        tracker,
                        named_decl,
                        Some(named_clause),
                    ));
                } else {
                    let (clause_id, clause) =
                        import_clause(tracker.syntax(source), import_decl.node())
                            .expect("an import clause");
                    let new_clause = tracker
                        .arena()
                        .factory()
                        .update_import_clause(
                            TransformNode::new(file, clause_id),
                            clause.phase_modifier,
                            new_default_import,
                            new_named_imports,
                        )
                        .expect("an import clause");
                    coalesced_imports.push(update_import_declaration(
                        tracker,
                        import_decl,
                        Some(new_clause),
                    ));
                }
            }
        }
        coalesced_imports
    }

    /// tsgo `organizeExportsWorker`.
    fn organize_exports_worker(&self, old_export_decls: &[NodeId], tracker: &mut Tracker<'a>) {
        if old_export_decls.is_empty() {
            return;
        }
        let source = self.source;
        let new_export_decls = coalesce_exports_worker(
            source,
            old_export_decls,
            self.comparer.specifier_comparer(),
            self.comparer.module_specifier,
            tracker,
        );
        let first = old_export_decls[0];
        if new_export_decls.is_empty() {
            tracker.delete_node_range(
                source,
                first,
                old_export_decls[old_export_decls.len() - 1],
                LeadingTriviaOption::Exclude,
                TrailingTriviaOption::Include,
            );
            return;
        }
        for &export in &new_export_decls {
            let flags = tracker.emit_flags(export) | EmitFlags::NO_LEADING_COMMENTS;
            tracker.set_emit_flags(export, flags);
        }
        let options = NodeOptions {
            leading_trivia_option: LeadingTriviaOption::Exclude,
            trailing_trivia_option: TrailingTriviaOption::Include,
            suffix: "\n".to_owned(),
            ..NodeOptions::default()
        };
        tracker.replace_node_with_nodes(source, first, new_export_decls, Some(options));
        for &decl in &old_export_decls[1..] {
            tracker.delete(source, decl);
        }
    }
}

/// The sorted specifiers' list of a combined import: tsgo's new list, with
/// the first named imports' range when their list has a trailing comma (a
/// tsgo list has one when it ends after its last element).
fn sorted_specifier_list<'a>(
    tracker: &mut Tracker<'a>,
    source: &'a SourceFile,
    file: tsc_emitter::TransformSourceId,
    specifiers: Vec<TransformNode>,
    first_named_bindings: Option<NodeId>,
) -> TransformNodeArray {
    let original_range = first_named_bindings.and_then(|bindings| {
        let arena = tracker.syntax(source);
        let NodeData::NamedImports(named_imports) = &arena.node(bindings).data else {
            return None;
        };
        let record = arena.node_array(named_imports.elements?);
        record
            .has_trailing_comma
            .then_some((record.pos, record.end))
    });
    let last_end = specifiers.last().map(|last| {
        let arena = tracker.syntax(source);
        raw_position(arena.node(last.node()).end)
    });
    let mut factory = tracker.arena().factory();
    match original_range {
        Some((pos, end)) => {
            let has_trailing_comma = last_end.is_some_and(|last_end| last_end < raw_position(end));
            let list = factory
                .create_node_array_with_trailing_comma(file, specifiers, has_trailing_comma)
                .expect("a list");
            factory
                .set_node_array_text_range(list, pos, end)
                .expect("a list");
            list
        }
        None => factory.create_node_array(file, specifiers).expect("a list"),
    }
}

/// tsgo `PositionIsSynthesized`.
fn is_synthesized(pos: u32) -> bool {
    pos == u32::MAX
}

/// A position as tsgo's int (-1 for none).
fn raw_position(position: u32) -> i64 {
    if is_synthesized(position) {
        -1
    } else {
        i64::from(position)
    }
}

/// The module specifier of an import or export declaration.
fn module_specifier(arena: &NodeArena, declaration: NodeId) -> Option<NodeId> {
    match &arena.node(declaration).data {
        NodeData::ImportDeclaration(data) => data.module_specifier,
        NodeData::ExportDeclaration(data) => data.module_specifier,
        _ => None,
    }
}

/// An import declaration's clause.
fn import_clause(arena: &NodeArena, declaration: NodeId) -> Option<(NodeId, ImportClauseData)> {
    let NodeData::ImportDeclaration(data) = &arena.node(declaration).data else {
        return None;
    };
    let clause = data.import_clause?;
    match &arena.node(clause).data {
        NodeData::ImportClause(data) => Some((clause, data.clone())),
        _ => None,
    }
}

/// tsgo `factory.UpdateImportDeclaration(decl, decl.Modifiers(), clause,
/// decl.ModuleSpecifier, decl.Attributes)`.
fn update_import_declaration(
    tracker: &mut Tracker<'_>,
    declaration: TransformNode,
    clause: Option<TransformNode>,
) -> TransformNode {
    let file = declaration.source();
    let NodeData::ImportDeclaration(data) = tracker
        .arena()
        .node(declaration)
        .expect("an import declaration")
        .data
        .clone()
    else {
        unreachable!("an import declaration");
    };
    tracker
        .arena()
        .factory()
        .update_import_declaration(
            declaration,
            data.modifiers
                .map(|list| TransformNodeArray::new(file, list)),
            clause,
            TransformNode::new(file, data.module_specifier.expect("a module specifier")),
            data.attributes
                .map(|attributes| TransformNode::new(file, attributes)),
        )
        .expect("an import declaration")
}

/// tsgo `groupByModuleSpecifier`: in the order each specifier first
/// appears.
fn group_by_module_specifier(
    arena: &NodeArena,
    imports: &[TransformNode],
) -> Vec<Vec<TransformNode>> {
    let mut groups: Vec<(String, Vec<TransformNode>)> = Vec::new();
    for &import in imports {
        let specifier = get_external_module_name(arena, module_specifier(arena, import.node()));
        match groups.iter_mut().find(|(known, _)| *known == specifier) {
            Some((_, group)) => group.push(import),
            None => groups.push((specifier, vec![import])),
        }
    }
    groups.into_iter().map(|(_, group)| group).collect()
}

/// tsgo `getPossibleSymbolReferenceNodes` over the whole file.
fn possible_symbol_reference_nodes(source: &SourceFile, symbol_name: &str) -> Vec<NodeId> {
    let navigator = Navigator::new(source);
    possible_symbol_reference_positions(source, symbol_name)
        .into_iter()
        .filter_map(
            |position| match navigator.touching_property_name(position as u32) {
                Found::Node(view) => view.node().filter(|&node| node != source.root),
                // A token tsgo creates is not an identifier.
                Found::Token(_) => None,
            },
        )
        .collect()
}

/// tsgo `getPossibleSymbolReferencePositions` over the whole file: each
/// occurrence of the name not inside a longer identifier.
fn possible_symbol_reference_positions(source: &SourceFile, symbol_name: &str) -> Vec<usize> {
    let mut positions = Vec::new();
    // Be resilient in the face of a symbol with no name or zero length name
    if symbol_name.is_empty() {
        return positions;
    }
    let text = source.text();
    let bytes = text.as_bytes();
    let mut search_from = 0;
    while let Some(index) = text[search_from..].find(symbol_name) {
        let position = search_from + index;
        // We found a match.  Make sure it's not part of a larger word (i.e. the char
        // before and after it have to be a non-identifier char).
        let end_position = position + symbol_name.len();
        // tsgo reads the bytes around the match as characters.
        if (position == 0 || !tsc_syntax::is_identifier_part(char::from(bytes[position - 1])))
            && (end_position == text.len()
                || !tsc_syntax::is_identifier_part(char::from(bytes[end_position])))
        {
            // Found a real match.  Keep searching.
            positions.push(position);
        }
        let start_index = position + symbol_name.len() + 1;
        if start_index > text.len() {
            break;
        }
        // A match starts at a character, so the search resumes at the
        // first one from there.
        search_from = start_index;
        while !text.is_char_boundary(search_from) {
            search_from += 1;
        }
    }
    positions
}

/// tsgo `hasModuleDeclarationMatchingSpecifier`: the file augments the
/// module the specifier names.
fn has_module_declaration_matching_specifier(
    source: &SourceFile,
    specifier: Option<NodeId>,
) -> bool {
    let Some(NodeData::StringLiteral(specifier)) =
        specifier.map(|specifier| &source.arena.node(specifier).data)
    else {
        return false;
    };
    collect_external_module_references(source)
        .module_augmentations
        .iter()
        .any(|&name| {
            matches!(
                &source.arena.node(name).data,
                NodeData::StringLiteral(name) if name.text == specifier.text
            )
        })
}

/// tsgo `getImportAttributesKey`: the attributes' keyword and sorted
/// entries.
fn get_import_attributes_key(arena: &NodeArena, attributes: Option<NodeId>) -> String {
    let Some(NodeData::ImportAttributes(attributes)) =
        attributes.map(|attributes| &arena.node(attributes).data)
    else {
        return String::new();
    };
    let mut key = format!("{:?} ", attributes.token);
    let mut entries: Vec<(String, String)> = attributes
        .elements
        .map(|list| arena.node_array(list).nodes)
        .unwrap_or_default()
        .iter()
        .filter_map(|&attribute| {
            let NodeData::ImportAttribute(attribute) = &arena.node(attribute).data else {
                return None;
            };
            let name = attribute
                .name
                .map_or_else(String::new, |name| node_name_text(arena, name));
            let value =
                attribute
                    .value
                    .map_or_else(String::new, |value| match &arena.node(value).data {
                        NodeData::StringLiteral(literal) => {
                            format!("\"{}\"", literal.text.to_string_lossy())
                        }
                        NodeData::NoSubstitutionTemplateLiteral(literal) => {
                            format!("\"{}\"", literal.text.to_string_lossy())
                        }
                        _ => node_name_text(arena, value),
                    });
            Some((name, value))
        })
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, value) in entries {
        key.push_str(&name);
        key.push(':');
        key.push_str(&value);
        key.push(' ');
    }
    key
}

/// tsgo `groupByNewlineContiguous`: the declarations split where a blank
/// line separates two.
fn group_by_newline_contiguous(source: &SourceFile, decls: &[NodeId]) -> Vec<Vec<NodeId>> {
    let mut groups = Vec::new();
    let mut current_group: Vec<NodeId> = Vec::new();
    for &decl in decls {
        if !current_group.is_empty() && is_new_group(source, decl) {
            groups.push(std::mem::take(&mut current_group));
        }
        current_group.push(decl);
    }
    if !current_group.is_empty() {
        groups.push(current_group);
    }
    groups
}

/// tsgo `isNewGroup`: two line breaks among the trivia before the
/// declaration.
fn is_new_group(source: &SourceFile, decl: NodeId) -> bool {
    let full_start = source.arena.node(decl).pos;
    if is_synthesized(full_start) {
        return false;
    }
    let full_start = full_start as usize;
    let text = source.text();
    if full_start >= text.len() {
        return false;
    }
    let start_pos = skip_trivia(text, full_start);
    if start_pos <= full_start {
        return false;
    }
    let mut scanner = TokenScanner::with_trivia(source, full_start);
    let mut number_of_new_lines = 0;
    while scanner.token_start() < start_pos {
        if scanner.scan() == SyntaxKind::NewLineTrivia {
            number_of_new_lines += 1;
            if number_of_new_lines >= 2 {
                return true;
            }
        }
    }
    false
}

/// tsgo `importGroup`.
#[derive(Default)]
struct ImportGroup {
    default_imports: Vec<TransformNode>,
    namespace_imports: Vec<TransformNode>,
    named_imports: Vec<TransformNode>,
}

impl ImportGroup {
    fn is_empty(&self) -> bool {
        self.default_imports.is_empty()
            && self.namespace_imports.is_empty()
            && self.named_imports.is_empty()
    }
}

/// tsgo `categorizedImports`.
#[derive(Default)]
struct CategorizedImports {
    import_without_clause: Option<TransformNode>,
    source_phase_imports: Vec<TransformNode>,
    type_only_imports: ImportGroup,
    regular_imports: ImportGroup,
}

/// tsgo `getCategorizedImports`.
fn get_categorized_imports(
    arena: &NodeArena,
    import_decls: &[TransformNode],
) -> CategorizedImports {
    let mut categorized = CategorizedImports::default();
    for &import_decl in import_decls {
        let Some((_, clause)) = import_clause(arena, import_decl.node()) else {
            if categorized.import_without_clause.is_none() {
                categorized.import_without_clause = Some(import_decl);
            }
            continue;
        };
        if clause.phase_modifier == Some(SyntaxKind::SourceKeyword) {
            categorized.source_phase_imports.push(import_decl);
            continue;
        }
        let group = if clause.is_type_only {
            &mut categorized.type_only_imports
        } else {
            &mut categorized.regular_imports
        };
        if clause.name.is_some() {
            group.default_imports.push(import_decl);
        }
        if let Some(named_bindings) = clause.named_bindings {
            match arena.node(named_bindings).kind {
                SyntaxKind::NamespaceImport => group.namespace_imports.push(import_decl),
                SyntaxKind::NamedImports => group.named_imports.push(import_decl),
                _ => {}
            }
        }
    }
    categorized
}

/// tsgo `getNewImportSpecifiers`: the specifiers of the named imports, an
/// `a as a` as `a`.
fn get_new_import_specifiers<'a>(
    tracker: &mut Tracker<'a>,
    source: &'a SourceFile,
    named_imports: &[TransformNode],
) -> Vec<TransformNode> {
    let mut result = Vec::new();
    for &named_import in named_imports {
        let file = named_import.source();
        let elements: Vec<(NodeId, Option<(bool, NodeId)>)> = {
            let arena = tracker.syntax(source);
            try_get_named_binding_elements(arena, named_import.node())
                .iter()
                .map(|&element| {
                    let normalized = match &arena.node(element).data {
                        NodeData::ImportSpecifier(specifier) => {
                            match (specifier.property_name, specifier.name) {
                                (Some(property_name), Some(name))
                                    if node_name_text(arena, property_name)
                                        == node_name_text(arena, name) =>
                                {
                                    Some((specifier.is_type_only, name))
                                }
                                _ => None,
                            }
                        }
                        _ => None,
                    };
                    (element, normalized)
                })
                .collect()
        };
        for (element, normalized) in elements {
            result.push(match normalized {
                Some((is_type_only, name)) => tracker
                    .arena()
                    .factory()
                    .update_import_specifier(
                        TransformNode::new(file, element),
                        is_type_only,
                        None,
                        TransformNode::new(file, name),
                    )
                    .expect("an import specifier"),
                None => TransformNode::new(file, element),
            });
        }
    }
    result
}

/// tsgo `tryGetNamedBindingElements`.
fn try_get_named_binding_elements(arena: &NodeArena, named_import: NodeId) -> &[NodeId] {
    import_clause(arena, named_import)
        .and_then(|(_, clause)| clause.named_bindings)
        .and_then(|bindings| match &arena.node(bindings).data {
            NodeData::NamedImports(named_imports) => named_imports.elements,
            _ => None,
        })
        .map(|list| arena.node_array(list).nodes)
        .unwrap_or_default()
}

/// tsgo `getTopLevelExportGroups`: the runs of export declarations (an
/// `export … from` alone) split at blank lines.
fn get_top_level_export_groups(source: &SourceFile) -> Vec<Vec<NodeId>> {
    let statements = statements(source, source.root);
    let is_export =
        |index: usize| source.arena.node(statements[index]).kind == SyntaxKind::ExportDeclaration;
    let mut top_level_export_groups: Vec<Vec<NodeId>> = Vec::new();
    let mut index = 0;
    let mut group_index = 0;
    while index < statements.len() {
        if is_export(index) {
            if group_index >= top_level_export_groups.len() {
                top_level_export_groups.push(Vec::new());
            }
            if module_specifier(&source.arena, statements[index]).is_some() {
                top_level_export_groups[group_index].push(statements[index]);
                index += 1;
            } else {
                while index < statements.len() && is_export(index) {
                    top_level_export_groups[group_index].push(statements[index]);
                    index += 1;
                }
                group_index += 1;
            }
        } else {
            index += 1;
            if group_index < top_level_export_groups.len()
                && !top_level_export_groups[group_index].is_empty()
            {
                group_index += 1;
            }
        }
    }
    top_level_export_groups
        .iter()
        .flat_map(|export_group| group_by_newline_contiguous(source, export_group))
        .collect()
}

/// tsgo `coalesceExportsWorker`: the exports of each module (in the order
/// of their specifiers, the local ones last) combined.
fn coalesce_exports_worker<'a>(
    source: &'a SourceFile,
    export_group: &[NodeId],
    specifier_comparer: (StringComparer, OrganizeImportsTypeOrder),
    module_specifier_comparer: StringComparer,
    tracker: &mut Tracker<'a>,
) -> Vec<TransformNode> {
    let arena = &source.arena;
    let file = tracker.source(source);
    let mut exports_by_module_specifier: Vec<(String, Vec<NodeId>)> = Vec::new();
    for &export_decl in export_group {
        let specifier = module_specifier(arena, export_decl)
            .map_or_else(String::new, |specifier| node_name_text(arena, specifier));
        match exports_by_module_specifier
            .iter_mut()
            .find(|(known, _)| *known == specifier)
        {
            Some((_, group)) => group.push(export_decl),
            None => exports_by_module_specifier.push((specifier, vec![export_decl])),
        }
    }
    exports_by_module_specifier.sort_by(|(a, _), (b, _)| match (a.is_empty(), b.is_empty()) {
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        _ => module_specifier_comparer.compare(a, b),
    });
    let mut coalesced_exports = Vec::new();
    for (_, group) in exports_by_module_specifier {
        // tsgo `getCategorizedExports`.
        let mut export_without_clause = None;
        let mut named_exports = Vec::new();
        let mut type_only_exports = Vec::new();
        for &export_decl in &group {
            let NodeData::ExportDeclaration(export) = &arena.node(export_decl).data else {
                continue;
            };
            if export.export_clause.is_none() {
                export_without_clause.get_or_insert(export_decl);
            } else if export.is_type_only {
                type_only_exports.push(export_decl);
            } else {
                named_exports.push(export_decl);
            }
        }
        if let Some(export) = export_without_clause {
            coalesced_exports.push(TransformNode::new(file, export));
        }
        for sub_group in [named_exports, type_only_exports] {
            if sub_group.is_empty() {
                continue;
            }
            let mut new_export_specifiers: Vec<NodeId> = Vec::new();
            for &export_decl in &sub_group {
                let NodeData::ExportDeclaration(export) = &arena.node(export_decl).data else {
                    continue;
                };
                if let Some(NodeData::NamedExports(named_exports)) =
                    export.export_clause.map(|clause| &arena.node(clause).data)
                {
                    new_export_specifiers.extend_from_slice(
                        named_exports
                            .elements
                            .map(|list| arena.node_array(list).nodes)
                            .unwrap_or_default(),
                    );
                }
            }
            new_export_specifiers.sort_by(|&a, &b| {
                compare_import_or_export_specifiers(
                    arena,
                    a,
                    b,
                    specifier_comparer.0,
                    specifier_comparer.1,
                )
            });
            let NodeData::ExportDeclaration(export_decl) = arena.node(sub_group[0]).data.clone()
            else {
                continue;
            };
            let updated_export_clause = match export_decl.export_clause {
                Some(clause) if arena.node(clause).kind == SyntaxKind::NamedExports => {
                    let mut factory = tracker.arena().factory();
                    let sorted_list = factory
                        .create_node_array(
                            file,
                            new_export_specifiers
                                .iter()
                                .map(|&specifier| TransformNode::new(file, specifier))
                                .collect(),
                        )
                        .expect("a list");
                    let updated = factory
                        .update_named_exports(TransformNode::new(file, clause), sorted_list)
                        .expect("named exports");
                    let original = arena.node(clause);
                    if !is_synthesized(original.pos)
                        && !range_is_on_single_line(source, original.pos, original.end)
                    {
                        tracker.set_emit_flags(updated, EmitFlags::MULTI_LINE);
                    }
                    Some(updated)
                }
                clause => clause.map(|clause| TransformNode::new(file, clause)),
            };
            coalesced_exports.push(
                tracker
                    .arena()
                    .factory()
                    .update_export_declaration(
                        TransformNode::new(file, sub_group[0]),
                        export_decl
                            .modifiers
                            .map(|list| TransformNodeArray::new(file, list)),
                        export_decl.is_type_only,
                        updated_export_clause,
                        export_decl
                            .module_specifier
                            .map(|specifier| TransformNode::new(file, specifier)),
                        export_decl
                            .attributes
                            .map(|attributes| TransformNode::new(file, attributes)),
                    )
                    .expect("an export declaration"),
            );
        }
    }
    coalesced_exports
}

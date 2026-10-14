//! tsgo `findallreferences.go`: the search for a node's references — the
//! special searches (type keywords, `import.meta`, labels, `this`, `super`,
//! string literals, modules) and the symbol search over the program's files
//! (`refState`).

use std::collections::{HashMap, HashSet};

use tsc_binder::node_util;
use tsc_checker::state::CheckResult;
use tsc_syntax::{NodeData, NodeId, SourceFile, SyntaxKind};
use tsc_types::{
    CheckFlags, ContextFlags, EscapedName, InternalSymbolName, ModifierFlags, SymbolFlags,
    SymbolId, TypeFlags,
};

use super::import_tracker::{
    find_module_references, get_export_info, get_import_or_export_symbol,
    is_external_module_symbol, is_source_file_with_global_exports, symbol_name,
    try_get_ambient_module_name_from_symbol_name, ExportInfo, ExportKind, ImpExpKind,
    ImportTracker, ModuleReference,
};
use super::{
    Definition, DefinitionKind, EntryKind, RefOptions, ReferenceEntry, ReferenceUse,
    SymbolAndEntries,
};
use crate::astnav::Navigator;
use crate::meaning::SemanticMeaning;
use crate::request::Request;
use crate::tree::View;
use crate::utilities::{
    containing_object_literal_element, reference_at_position, touching, Touching,
};

/// tsgo `getReferencedSymbolsForNode`.
pub(crate) fn get_referenced_symbols_for_node(
    request: &mut Request<'_, '_>,
    position: u32,
    node: Touching,
    files: &[usize],
    options: RefOptions,
) -> CheckResult<Vec<SymbolAndEntries>> {
    let file_set: HashSet<usize> = files.iter().copied().collect();
    let node = if matches!(
        options.use_,
        ReferenceUse::References | ReferenceUse::Rename
    ) {
        request.adjusted_location(node, options.use_ == ReferenceUse::Rename)
    } else {
        node
    };

    if let Touching::Node(id) = node {
        if request.node_kind(id) == SyntaxKind::SourceFile {
            let file = request.state.binder.file_index_of_node(id);
            let Some(referenced) =
                reference_at_position(request.state, request.prepared, file, position)
                    .and_then(|reference| reference.file)
            else {
                return Ok(Vec::new());
            };
            let root = request.state.binder.source(referenced).root;
            if let Some(module_symbol) = request.state.binder.node_symbol(root) {
                let module_symbol = request.state.get_merged_symbol(module_symbol);
                return get_referenced_symbols_for_module(
                    request,
                    module_symbol,
                    false,
                    files,
                    &file_set,
                );
            }
            // tsgo's `getReferencesForNonModule` is not implemented.
            return Ok(vec![SymbolAndEntries {
                definition: Some(Definition {
                    kind: DefinitionKind::TripleSlashReference,
                    symbol: None,
                    node: None,
                }),
                references: Vec::new(),
            }]);
        }
    }

    if !options.implementations {
        if let Some(special) = get_referenced_symbols_special(request, node, files)? {
            return Ok(special);
        }
    }

    // constructors should use the class symbol, detected by name, if present
    let symbol_node = match node {
        Touching::Node(id) if request.node_kind(id) == SyntaxKind::Constructor => request
            .parent(node)
            .and_then(|class| request.name_of(class))
            .map_or(node, Touching::Node),
        _ => node,
    };
    let Some(symbol) = request.symbol_at(symbol_node)? else {
        // String literal might be a property (and thus have a symbol), so do this here rather than in getReferencedSymbolsSpecial.
        if !options.implementations && is_string_literal_like(request.kind(node)) {
            if let Touching::Node(literal) = node {
                return get_references_for_string_literal(request, literal, files);
            }
        }
        return Ok(Vec::new());
    };

    if request.state.binder.symbol(symbol).escaped_name == InternalSymbolName::EXPORT_EQUALS {
        let Some(parent) = request.state.binder.symbol(symbol).parent else {
            return Ok(Vec::new());
        };
        return get_referenced_symbols_for_module(request, parent, false, files, &file_set);
    }

    let module_references = get_referenced_symbols_for_module_if_declared_by_source_file(
        request,
        Some(symbol),
        files,
        &file_set,
        options,
    )?;
    if let Some(module_references) = &module_references {
        if !request
            .state
            .symbol_flags(symbol)
            .intersects(SymbolFlags::TRANSIENT)
        {
            return Ok(module_references.clone());
        }
    }

    let aliased_symbol =
        get_merged_aliased_symbol_of_namespace_export_declaration(request, node, symbol)?;
    let module_references_of_export_target =
        get_referenced_symbols_for_module_if_declared_by_source_file(
            request,
            aliased_symbol,
            files,
            &file_set,
            options,
        )?;

    let references =
        get_referenced_symbols_for_symbol(request, symbol, Some(node), files, &file_set, options)?;
    Ok(merge_references(
        request,
        vec![
            module_references.unwrap_or_default(),
            references,
            module_references_of_export_target.unwrap_or_default(),
        ],
    ))
}

/// tsgo `mergeReferences`: the symbols' references, those of a symbol seen
/// before sorted into its entry by file and range.
fn merge_references(
    request: &Request<'_, '_>,
    references_to_merge: Vec<Vec<SymbolAndEntries>>,
) -> Vec<SymbolAndEntries> {
    let mut result: Vec<SymbolAndEntries> = Vec::new();
    for references in references_to_merge {
        if references.is_empty() {
            continue;
        }
        if result.is_empty() {
            result = references;
            continue;
        }
        for entry in references {
            let Some(symbol) = entry
                .definition
                .as_ref()
                .filter(|definition| definition.kind == DefinitionKind::Symbol)
                .map(|definition| definition.symbol)
            else {
                result.push(entry);
                continue;
            };
            let Some(index) = result.iter().position(|reference| {
                reference.definition.as_ref().is_some_and(|definition| {
                    definition.kind == DefinitionKind::Symbol && definition.symbol == symbol
                })
            }) else {
                result.push(entry);
                continue;
            };
            let mut sorted = result[index].references.clone();
            sorted.extend(entry.references);
            sorted.sort_by(|entry1, entry2| {
                entry1.file.cmp(&entry2.file).then_with(|| {
                    let range1 = request.location_of_entry(entry1).range;
                    let range2 = request.location_of_entry(entry2).range;
                    (
                        range1.start.line,
                        range1.start.character,
                        range1.end.line,
                        range1.end.character,
                    )
                        .cmp(&(
                            range2.start.line,
                            range2.start.character,
                            range2.end.line,
                            range2.end.character,
                        ))
                })
            });
            result[index].references = sorted;
        }
    }
    result
}

/// tsgo `getReferencesForStringLiteral`.
fn get_references_for_string_literal(
    request: &mut Request<'_, '_>,
    node: NodeId,
    files: &[usize],
) -> CheckResult<Vec<SymbolAndEntries>> {
    let ty = request.contextual_type_from_parent_or_ancestor_type_node(node)?;
    let text = literal_text(request, node);
    let mut references = Vec::new();
    for &file in files {
        let source = request.state.binder.source(file);
        for reference in get_possible_symbol_reference_nodes(source, &text, None) {
            let Touching::Node(reference) = reference else {
                continue;
            };
            if !is_string_literal_like(request.node_kind(reference))
                || literal_text(request, reference) != text
            {
                continue;
            }
            if let Some(ty) = ty {
                let reference_type =
                    request.contextual_type_from_parent_or_ancestor_type_node(reference)?;
                if ty != request.state.tables.intrinsics.string
                    && (Some(ty) == reference_type
                        || is_string_literal_property_reference(request, reference)?)
                {
                    references.push(request.new_node_entry_with_kind(
                        Touching::Node(reference),
                        EntryKind::StringLiteral,
                    ));
                }
            } else {
                if request.node_kind(reference) == SyntaxKind::NoSubstitutionTemplateLiteral
                    && !range_is_on_single_line(source, reference)
                {
                    continue;
                }
                references.push(
                    request.new_node_entry_with_kind(
                        Touching::Node(reference),
                        EntryKind::StringLiteral,
                    ),
                );
            }
        }
    }
    Ok(vec![SymbolAndEntries {
        definition: Some(Definition {
            kind: DefinitionKind::String,
            symbol: None,
            node: Some(Touching::Node(node)),
        }),
        references,
    }])
}

/// tsgo `isStringLiteralPropertyReference`.
fn is_string_literal_property_reference(
    request: &mut Request<'_, '_>,
    node: NodeId,
) -> CheckResult<bool> {
    let Some(parent) = request.state.binder.node_record(node).parent else {
        return Ok(false);
    };
    if request.node_kind(parent) != SyntaxKind::PropertySignature {
        return Ok(false);
    }
    let Some(grandparent) = request.state.binder.node_record(parent).parent else {
        return Ok(false);
    };
    let ty = request.state.get_type_at_location(grandparent)?;
    let text = literal_text(request, node);
    Ok(request
        .state
        .get_property_of_type_full(ty, &text)?
        .is_some())
}

/// tsgo `getReferencedSymbolsForModuleIfDeclaredBySourceFile`: none when the
/// symbol is not a module declared by a file.
fn get_referenced_symbols_for_module_if_declared_by_source_file(
    request: &mut Request<'_, '_>,
    symbol: Option<SymbolId>,
    files: &[usize],
    file_set: &HashSet<usize>,
    options: RefOptions,
) -> CheckResult<Option<Vec<SymbolAndEntries>>> {
    let Some(symbol) = symbol else {
        return Ok(None);
    };
    let declarations = request.declarations_of(symbol);
    if !request
        .state
        .symbol_flags(symbol)
        .intersects(SymbolFlags::MODULE)
        || declarations.is_empty()
    {
        return Ok(None);
    }
    let Some(module_source_file) = declarations
        .iter()
        .copied()
        .find(|&declaration| request.node_kind(declaration) == SyntaxKind::SourceFile)
    else {
        return Ok(None);
    };
    let export_equals = request
        .state
        .symbol_exports(symbol)
        .get(EscapedName::internal(InternalSymbolName::EXPORT_EQUALS))
        .copied();
    // If exportEquals != nil, we're about to add references to `import("mod")` anyway, so don't double-count them.
    let module_references = get_referenced_symbols_for_module(
        request,
        symbol,
        export_equals.is_some(),
        files,
        file_set,
    )?;
    let Some(export_equals) = export_equals.filter(|&export_equals| {
        request
            .state
            .symbol_flags(export_equals)
            .intersects(SymbolFlags::ALIAS)
            && file_set.contains(&request.state.binder.file_index_of_node(module_source_file))
    }) else {
        return Ok(Some(module_references));
    };
    let symbol = request.state.resolve_alias(export_equals)?;
    let references =
        get_referenced_symbols_for_symbol(request, symbol, None, files, file_set, options)?;
    Ok(Some(merge_references(
        request,
        vec![module_references, references],
    )))
}

/// tsgo `getReferencedSymbolsSpecial`: none when the node is not special.
fn get_referenced_symbols_special(
    request: &mut Request<'_, '_>,
    node: Touching,
    files: &[usize],
) -> CheckResult<Option<Vec<SymbolAndEntries>>> {
    let kind = request.kind(node);
    let parent = request.parent(node);
    if is_type_keyword(kind) {
        // A void expression (i.e., `void foo()`) is not special, but the `void` type is.
        if kind == SyntaxKind::VoidKeyword
            && parent.is_some_and(|parent| request.node_kind(parent) == SyntaxKind::VoidExpression)
        {
            return Ok(None);
        }
        // A modifier readonly (like on a property declaration) is not special;
        // a readonly type keyword (like `readonly string[]`) is.
        if kind == SyntaxKind::ReadonlyKeyword && !is_readonly_type_operator(request, node) {
            return Ok(None);
        }
        // Likewise, when we *are* looking for a special keyword, make sure we
        // *don't* include readonly member modifiers.
        return Ok(get_all_references_for_keyword(
            request,
            files,
            kind,
            kind == SyntaxKind::ReadonlyKeyword,
        ));
    }

    if let Some(parent) = parent {
        if request.is_import_meta(parent)
            && request.name_of(parent).map(Touching::Node) == Some(node)
        {
            return Ok(get_all_references_for_import_meta(request, files));
        }
        if kind == SyntaxKind::StaticKeyword
            && request.node_kind(parent) == SyntaxKind::ClassStaticBlockDeclaration
        {
            return Ok(Some(vec![SymbolAndEntries {
                definition: Some(Definition {
                    kind: DefinitionKind::Keyword,
                    symbol: None,
                    node: Some(node),
                }),
                references: vec![request.new_node_entry(node)],
            }]));
        }
    }

    if let Touching::Node(id) = node {
        // Labels
        if request.is_jump_statement_target(id) {
            // if we have a label definition, look within its statement for references, if not, then
            // the label is undefined and we have no results..
            let parent = parent.expect("a jump statement");
            if let Some(label_definition) = request.target_label(parent, id) {
                let statement = request
                    .state
                    .binder
                    .node_record(label_definition)
                    .parent
                    .expect("a labeled statement");
                return Ok(Some(get_label_references_in_node(
                    request,
                    statement,
                    label_definition,
                )));
            }
            return Ok(None);
        }
        if is_label_of_labeled_statement(request, id) {
            // it is a label definition and not a target, search within the parent labeledStatement
            let statement = parent.expect("a labeled statement");
            return Ok(Some(get_label_references_in_node(request, statement, id)));
        }
    }

    if is_this(request, node) {
        return get_references_for_this_keyword(request, node, files);
    }

    if kind == SyntaxKind::SuperKeyword {
        return get_references_for_super_keyword(request, node);
    }

    Ok(None)
}

/// tsgo `getLabelReferencesInNode`.
fn get_label_references_in_node(
    request: &Request<'_, '_>,
    container: NodeId,
    target_label: NodeId,
) -> Vec<SymbolAndEntries> {
    let source = request.state.binder.source_of_node(container);
    let label_name = node_util::id_text(source, target_label)
        .unwrap_or_default()
        .to_owned();
    let mut references = Vec::new();
    for node in get_possible_symbol_reference_nodes(source, &label_name, Some(container)) {
        // Only pick labels that are either the target label, or have a target that is the target label
        let Touching::Node(id) = node else {
            continue;
        };
        if id == target_label
            || request.is_jump_statement_target(id)
                && request.target_label(id, id) == Some(target_label)
        {
            references.push(request.new_node_entry(node));
        }
    }
    vec![SymbolAndEntries::new(
        DefinitionKind::Label,
        Some(Touching::Node(target_label)),
        None,
        references,
    )]
}

/// tsgo `getReferencesForThisKeyword`.
fn get_references_for_this_keyword(
    request: &mut Request<'_, '_>,
    this_or_super_keyword: Touching,
    files: &[usize],
) -> CheckResult<Option<Vec<SymbolAndEntries>>> {
    let Touching::Node(keyword) = this_or_super_keyword else {
        return Ok(None);
    };
    let keyword_source = request.state.binder.source_of_node(keyword);
    let Some(mut search_space_node) = node_util::get_this_container(keyword_source, keyword, false)
    else {
        return Ok(None);
    };

    // Whether 'this' occurs in a static context within a class.
    let mut static_flag = ModifierFlags::STATIC;

    match request.node_kind(search_space_node) {
        SyntaxKind::MethodDeclaration
        | SyntaxKind::MethodSignature
        | SyntaxKind::PropertyDeclaration
        | SyntaxKind::PropertySignature
        | SyntaxKind::Constructor
        | SyntaxKind::GetAccessor
        | SyntaxKind::SetAccessor => {
            static_flag = static_flag & modifier_flags(request, search_space_node);
            // re-assign to be the owning class (or the owning object literal)
            search_space_node = request
                .state
                .binder
                .node_record(search_space_node)
                .parent
                .expect("a member's parent");
        }
        SyntaxKind::SourceFile => {
            if is_external_module(request, search_space_node) || is_parameter_name(request, keyword)
            {
                return Ok(None);
            }
        }
        // Computed properties in classes are not handled here because references to this are illegal,
        // so there is no point finding references to them.
        SyntaxKind::FunctionDeclaration | SyntaxKind::FunctionExpression => {}
        _ => return Ok(None),
    }

    let search_space_kind = request.node_kind(search_space_node);
    let files_to_search: Vec<usize> = if search_space_kind == SyntaxKind::SourceFile {
        files.to_vec()
    } else {
        vec![request.state.binder.file_index_of_node(search_space_node)]
    };
    let search_space_symbol = request.state.binder.node_symbol(search_space_node);
    let mut references = Vec::new();
    for file in files_to_search {
        let source = request.state.binder.source(file);
        let container = if search_space_kind == SyntaxKind::SourceFile {
            source.root
        } else {
            search_space_node
        };
        for node in get_possible_symbol_reference_nodes(source, "this", Some(container)) {
            if !is_this(request, node) {
                continue;
            }
            let Touching::Node(id) = node else {
                continue;
            };
            let Some(container) = node_util::get_this_container(source, id, false) else {
                continue;
            };
            if !crate::utilities::can_have_symbol(request.node_kind(container)) {
                continue;
            }
            let container_symbol = request.state.binder.node_symbol(container);
            let matches = match search_space_kind {
                SyntaxKind::FunctionExpression | SyntaxKind::FunctionDeclaration => {
                    search_space_symbol == container_symbol
                }
                SyntaxKind::MethodDeclaration | SyntaxKind::MethodSignature => {
                    node_util::is_object_literal_method(
                        request.state.binder.source_of_node(search_space_node),
                        search_space_node,
                    ) && search_space_symbol == container_symbol
                }
                SyntaxKind::ClassExpression
                | SyntaxKind::ClassDeclaration
                | SyntaxKind::ObjectLiteralExpression => {
                    // Make sure the container belongs to the same class/object literals
                    // and has the appropriate static modifier from the original container.
                    let container_parent = request.state.binder.node_record(container).parent;
                    container_parent.is_some_and(|container_parent| {
                        crate::utilities::can_have_symbol(request.node_kind(container_parent))
                            && search_space_symbol
                                == request.state.binder.node_symbol(container_parent)
                    }) && is_static(request, container) == (static_flag != ModifierFlags::NONE)
                }
                SyntaxKind::SourceFile => {
                    request.node_kind(container) == SyntaxKind::SourceFile
                        && !is_external_module(request, container)
                        && !is_parameter_name(request, id)
                }
                _ => false,
            };
            if matches {
                references.push(request.new_node_entry(node));
            }
        }
    }

    let this_parameter = references
        .iter()
        .find_map(|reference| {
            let node = reference.node?;
            (request
                .parent(node)
                .is_some_and(|parent| request.node_kind(parent) == SyntaxKind::Parameter))
            .then_some(node)
        })
        .unwrap_or(this_or_super_keyword);
    Ok(Some(vec![SymbolAndEntries::new(
        DefinitionKind::This,
        Some(this_parameter),
        search_space_symbol,
        references,
    )]))
}

/// tsgo `getReferencesForSuperKeyword`.
fn get_references_for_super_keyword(
    request: &mut Request<'_, '_>,
    super_keyword: Touching,
) -> CheckResult<Option<Vec<SymbolAndEntries>>> {
    let Touching::Node(keyword) = super_keyword else {
        return Ok(None);
    };
    let Some(mut search_space_node) = request.state.get_super_container(keyword, false) else {
        return Ok(None);
    };
    // Whether 'super' occurs in a static context within a class.
    let mut static_flag = ModifierFlags::STATIC;

    match request.node_kind(search_space_node) {
        SyntaxKind::PropertyDeclaration
        | SyntaxKind::PropertySignature
        | SyntaxKind::MethodDeclaration
        | SyntaxKind::MethodSignature
        | SyntaxKind::Constructor
        | SyntaxKind::GetAccessor
        | SyntaxKind::SetAccessor => {
            static_flag = static_flag & modifier_flags(request, search_space_node);
            // re-assign to be the owning class
            search_space_node = request
                .state
                .binder
                .node_record(search_space_node)
                .parent
                .expect("a member's parent");
        }
        _ => return Ok(None),
    }

    let source = request.state.binder.source_of_node(search_space_node);
    let search_space_symbol = request.state.binder.node_symbol(search_space_node);
    let mut references = Vec::new();
    for node in get_possible_symbol_reference_nodes(source, "super", Some(search_space_node)) {
        if request.kind(node) != SyntaxKind::SuperKeyword {
            continue;
        }
        let Touching::Node(id) = node else {
            continue;
        };
        let container = request.state.get_super_container(id, false);

        // If we have a 'super' container, we must have an enclosing class.
        // Now make sure the owning class is the same as the search-space
        // and has the same static qualifier as the original 'super's owner.
        if let Some(container) = container {
            if is_static(request, container) == (static_flag != ModifierFlags::NONE)
                && request
                    .state
                    .binder
                    .node_record(container)
                    .parent
                    .and_then(|parent| request.state.binder.node_symbol(parent))
                    == search_space_symbol
            {
                references.push(request.new_node_entry(node));
            }
        }
    }

    Ok(Some(vec![SymbolAndEntries::new(
        DefinitionKind::Symbol,
        None,
        search_space_symbol,
        references,
    )]))
}

/// tsgo `getAllReferencesForImportMeta`.
fn get_all_references_for_import_meta(
    request: &Request<'_, '_>,
    files: &[usize],
) -> Option<Vec<SymbolAndEntries>> {
    let mut references = Vec::new();
    for &file in files {
        let source = request.state.binder.source(file);
        for node in get_possible_symbol_reference_nodes(source, "meta", Some(source.root)) {
            let Some(parent) = request.parent(node) else {
                continue;
            };
            if request.is_import_meta(parent) {
                references.push(request.new_node_entry(Touching::Node(parent)));
            }
        }
    }
    let first = references.first()?.node;
    Some(vec![SymbolAndEntries {
        definition: Some(Definition {
            kind: DefinitionKind::Keyword,
            symbol: None,
            node: first,
        }),
        references,
    }])
}

/// tsgo `getAllReferencesForKeyword`.
fn get_all_references_for_keyword(
    request: &Request<'_, '_>,
    files: &[usize],
    keyword_kind: SyntaxKind,
    filter_read_only_type_operator: bool,
) -> Option<Vec<SymbolAndEntries>> {
    let keyword = tsc_syntax::tokens::token_to_string(keyword_kind)?;
    let mut references = Vec::new();
    for &file in files {
        let source = request.state.binder.source(file);
        for reference_location in
            get_possible_symbol_reference_nodes(source, keyword, Some(source.root))
        {
            if request.kind(reference_location) == keyword_kind
                && (!filter_read_only_type_operator
                    || is_readonly_type_operator(request, reference_location))
            {
                references.push(request.new_node_entry(reference_location));
            }
        }
    }
    let first = references.first()?.node;
    Some(vec![SymbolAndEntries::new(
        DefinitionKind::Keyword,
        first,
        None,
        references,
    )])
}

/// tsgo `getPossibleSymbolReferenceNodes`: the nodes at the name's
/// occurrences.
pub(super) fn get_possible_symbol_reference_nodes(
    source: &SourceFile,
    symbol_name: &str,
    container: Option<NodeId>,
) -> Vec<Touching> {
    let navigator = Navigator::new(source);
    get_possible_symbol_reference_positions(source, symbol_name, container)
        .into_iter()
        .filter_map(|position| touching(navigator.touching_property_name(position as u32)))
        .filter(|node| *node != Touching::Node(source.root))
        .collect()
}

/// tsgo `getPossibleSymbolReferencePositions`: the positions of the name
/// that are not within a longer word. As in tsgo, the first occurrence is
/// found from the container's position but taken as a position of the file,
/// and a byte before or after is read as a character.
fn get_possible_symbol_reference_positions(
    source: &SourceFile,
    symbol_name: &str,
    container: Option<NodeId>,
) -> Vec<usize> {
    let mut positions = Vec::new();
    // Be resilient in the face of a symbol with no name or zero length name
    if symbol_name.is_empty() {
        return positions;
    }
    let text = source.text().as_bytes();
    let name = symbol_name.as_bytes();
    let container = container.unwrap_or(source.root);
    let record = source.arena.node(container);
    let end_pos = record.end as usize;
    let mut position = index_of(&text[(record.pos as usize).min(text.len())..], name);
    while let Some(found) = position {
        if found >= end_pos {
            break;
        }
        // We found a match.  Make sure it's not part of a larger word (i.e. the char
        // before and after it have to be a non-identifier char).
        let end_position = found + name.len();
        if (found == 0 || !tsc_syntax::is_identifier_part(char::from(text[found - 1])))
            && (end_position == text.len()
                || !tsc_syntax::is_identifier_part(char::from(text[end_position])))
        {
            // Found a real match.  Keep searching.
            positions.push(found);
        }
        let start_index = found + name.len() + 1;
        if start_index > text.len() {
            break;
        }
        position = index_of(&text[start_index..], name).map(|index| start_index + index);
    }
    positions
}

/// Go `strings.Index` over bytes.
fn index_of(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// tsgo `getMergedAliasedSymbolOfNamespaceExportDeclaration`: at `export as
/// namespace N`, the merged symbol of the module when it differs.
fn get_merged_aliased_symbol_of_namespace_export_declaration(
    request: &mut Request<'_, '_>,
    node: Touching,
    symbol: SymbolId,
) -> CheckResult<Option<SymbolId>> {
    let Some(parent) = request.parent(node) else {
        return Ok(None);
    };
    if request.node_kind(parent) != SyntaxKind::NamespaceExportDeclaration {
        return Ok(None);
    }
    let aliased_symbol = request.state.resolve_alias(symbol)?;
    if aliased_symbol == request.state.unknown_symbol {
        return Ok(None);
    }
    let target_symbol = request.state.get_merged_symbol(aliased_symbol);
    Ok((aliased_symbol != target_symbol).then_some(target_symbol))
}

/// tsgo `getReferencedSymbolsForModule`: the imports and `<reference>`s of a
/// module, its `declare module` names and its `export =` declarations.
fn get_referenced_symbols_for_module(
    request: &mut Request<'_, '_>,
    symbol: SymbolId,
    exclude_import_type_of_export_equals: bool,
    files: &[usize],
    file_set: &HashSet<usize>,
) -> CheckResult<Vec<SymbolAndEntries>> {
    let module_references = find_module_references(request, files, symbol)?;
    let mut references = Vec::new();
    for reference in module_references {
        match reference {
            ModuleReference::Import { literal } => {
                let parent = request.state.binder.node_record(literal).parent;
                if let Some(parent) =
                    parent.filter(|&parent| request.node_kind(parent) == SyntaxKind::LiteralType)
                {
                    if let Some(import_type) = request.state.binder.node_record(parent).parent {
                        if let NodeData::ImportType(data) =
                            &request.state.binder.node_record(import_type).data
                        {
                            if exclude_import_type_of_export_equals && data.qualifier.is_none() {
                                continue;
                            }
                        }
                    }
                }
                // import("foo") with no qualifier will reference the `export =` of the module, which may be referenced anyway.
                references.push(request.new_node_entry(Touching::Node(literal)));
            }
            ModuleReference::Implicit { file, specifier } => {
                // For implicit references (e.g., JSX runtime imports), return the first JSX node,
                // the first statement, or the whole file
                let source = request.state.binder.source(file);
                let mut range_node = None;
                // Skip the JSX search for tslib imports
                if specifier != "tslib" {
                    range_node = find_first_jsx_node(source, source.root);
                }
                let range_node = range_node
                    .or_else(|| {
                        crate::references::statements(source, source.root)
                            .first()
                            .copied()
                    })
                    .unwrap_or(source.root);
                references.push(request.new_node_entry(Touching::Node(range_node)));
            }
            ModuleReference::Reference { file, range } => references.push(ReferenceEntry {
                kind: EntryKind::Range,
                node: None,
                context: None,
                file,
                range,
            }),
        }
    }

    // Add references to the module declarations themselves
    for declaration in request.declarations_of(symbol) {
        // Don't include the source file itself. (This may not be ideal behavior, but awkward to include an entire file as a reference.)
        // A declaration of another kind may be merged with something (e.g. a class merged with a namespace).
        if request.node_kind(declaration) == SyntaxKind::ModuleDeclaration
            && file_set.contains(&request.state.binder.file_index_of_node(declaration))
        {
            if let Some(name) = request.name_of(declaration) {
                references.push(request.new_node_entry(Touching::Node(name)));
            }
        }
    }

    // Handle export equals declarations
    let exported = request
        .state
        .symbol_exports(symbol)
        .get(EscapedName::internal(InternalSymbolName::EXPORT_EQUALS))
        .copied();
    if let Some(exported) = exported {
        for declaration in request.declarations_of(exported) {
            let file = request.state.binder.file_index_of_node(declaration);
            if !file_set.contains(&file) {
                continue;
            }
            let node = match &request.state.binder.node_record(declaration).data {
                // At `module.exports = ...`, reference node is `module`
                NodeData::BinaryExpression(binary)
                    if binary.left.is_some_and(|left| {
                        request.node_kind(left) == SyntaxKind::PropertyAccessExpression
                    }) =>
                {
                    let left = binary.left.expect("a left side");
                    match &request.state.binder.node_record(left).data {
                        NodeData::PropertyAccessExpression(access) => {
                            access.expression.map(Touching::Node)
                        }
                        _ => None,
                    }
                }
                // Find the export keyword
                NodeData::ExportAssignment(_) => {
                    let source = request.state.binder.source(file);
                    Navigator::new(source)
                        .child_of_kind(View::Node(declaration), SyntaxKind::ExportKeyword)
                        .and_then(touching)
                }
                _ => Some(Touching::Node(
                    node_util::get_name_of_declaration(
                        request.state.binder.source(file),
                        declaration,
                    )
                    .unwrap_or(declaration),
                )),
            };
            if let Some(node) = node {
                references.push(request.new_node_entry(node));
            }
        }
    }

    if references.is_empty() {
        return Ok(Vec::new());
    }
    Ok(vec![SymbolAndEntries {
        definition: Some(Definition {
            kind: DefinitionKind::Symbol,
            symbol: Some(symbol),
            node: None,
        }),
        references,
    }])
}

/// tsgo `findFirstJsxNode`: the first JSX element, self-closing element or
/// fragment of a file.
fn find_first_jsx_node(source: &SourceFile, node: NodeId) -> Option<NodeId> {
    if matches!(
        source.arena.node(node).kind,
        SyntaxKind::JsxElement | SyntaxKind::JsxSelfClosingElement | SyntaxKind::JsxFragment
    ) {
        return Some(node);
    }
    let mut children = Vec::new();
    tsc_syntax::for_each_child(&source.arena, source.arena.node(node), |child| {
        children.push(child);
        false
    });
    children
        .into_iter()
        .find_map(|child| find_first_jsx_node(source, child))
}

/// tsgo `getSpecialSearchKind`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SpecialSearchKind {
    None,
    Constructor,
    Class,
}

fn get_special_search_kind(request: &Request<'_, '_>, node: Option<Touching>) -> SpecialSearchKind {
    let Some(node) = node else {
        return SpecialSearchKind::None;
    };
    match request.kind(node) {
        SyntaxKind::Constructor | SyntaxKind::ConstructorKeyword => SpecialSearchKind::Constructor,
        SyntaxKind::Identifier
            if request
                .parent(node)
                .is_some_and(|parent| is_class_like(request.node_kind(parent))) =>
        {
            SpecialSearchKind::Class
        }
        _ => SpecialSearchKind::None,
    }
}

/// tsgo `getReferencedSymbolsForSymbol`: the core find-all-references
/// algorithm for a normal symbol.
fn get_referenced_symbols_for_symbol(
    request: &mut Request<'_, '_>,
    original_symbol: SymbolId,
    node: Option<Touching>,
    files: &[usize],
    file_set: &HashSet<usize>,
    options: RefOptions,
) -> CheckResult<Vec<SymbolAndEntries>> {
    let symbol = skip_past_export_or_import_specifier_or_union(
        request,
        original_symbol,
        node,
        !is_for_rename_with_prefix_and_suffix_text(options),
    )?
    .unwrap_or(original_symbol);

    // Compute the meaning from the location and the symbol it references
    let search_meaning = if options.use_ != ReferenceUse::Rename {
        get_intersecting_meaning_from_declarations(request, node, symbol, SemanticMeaning::ALL)
    } else {
        SemanticMeaning::ALL
    };
    let special_search_kind = get_special_search_kind(request, node);
    let mut state = RefState {
        request,
        files,
        file_set,
        special_search_kind,
        search_meaning,
        options,
        result: Vec::new(),
        inherits_from_cache: HashMap::new(),
        seen_containing_type_references: HashSet::new(),
        seen_re_export_rhs: HashSet::new(),
        import_tracker: None,
        symbol_to_references: HashMap::new(),
        file_to_seen_symbols: HashMap::new(),
        name_tables: HashMap::new(),
    };

    let export_specifier = if is_for_rename_with_prefix_and_suffix_text(options) {
        state
            .request
            .declarations_of(symbol)
            .into_iter()
            .find(|&declaration| {
                state.request.node_kind(declaration) == SyntaxKind::ExportSpecifier
            })
    } else {
        None
    };
    if let Some(export_specifier) = export_specifier {
        // When renaming at an export specifier, rename the export and not the thing being exported.
        let search =
            state.create_search(node, original_symbol, ImpExpKind::Unknown, None, Vec::new())?;
        let name = state
            .request
            .name_of(export_specifier)
            .expect("an export specifier's name");
        state.get_references_at_export_specifier(
            name,
            symbol,
            export_specifier,
            &search,
            true,
            true,
        )?;
    } else if let Some(node) = node.filter(|&node| {
        state.request.kind(node) == SyntaxKind::DefaultKeyword
            && state.request.state.binder.symbol(symbol).escaped_name == InternalSymbolName::DEFAULT
            && state.request.state.binder.symbol(symbol).parent.is_some()
    }) {
        state.add_reference(node, symbol, EntryKind::Node)?;
        let parent = state
            .request
            .state
            .binder
            .symbol(symbol)
            .parent
            .expect("a parent");
        state.search_for_imports_of_export(
            Some(node),
            symbol,
            ExportInfo {
                exporting_module_symbol: parent,
                export_kind: ExportKind::Default,
            },
        )?;
    } else {
        let search_symbols = state.populate_search_symbol_set(
            symbol,
            node,
            options.use_ == ReferenceUse::Rename,
            options.use_aliases_for_rename,
            options.implementations,
        )?;
        let search =
            state.create_search(node, symbol, ImpExpKind::Unknown, None, search_symbols)?;
        state.get_references_in_container_or_files(symbol, &search)?;
    }

    Ok(state.result)
}

/// tsgo `isForRenameWithPrefixAndSuffixText`.
fn is_for_rename_with_prefix_and_suffix_text(options: RefOptions) -> bool {
    options.use_ == ReferenceUse::Rename && options.use_aliases_for_rename
}

/// tsgo `skipPastExportOrImportSpecifierOrUnion`.
fn skip_past_export_or_import_specifier_or_union(
    request: &mut Request<'_, '_>,
    symbol: SymbolId,
    node: Option<Touching>,
    use_local_symbol_for_export_specifier: bool,
) -> CheckResult<Option<SymbolId>> {
    let Some(node) = node else {
        return Ok(None);
    };
    let parent = request.parent(node);
    if let (Some(parent), Touching::Node(id)) = (parent, node) {
        if request.node_kind(parent) == SyntaxKind::ExportSpecifier
            && use_local_symbol_for_export_specifier
        {
            return get_local_symbol_for_export_specifier(request, id, symbol, parent).map(Some);
        }
    }
    // If the symbol is declared as part of a declaration like `{ type: "a" } | { type: "b" }`, use the property on the union type to get more references.
    for declaration in request.declarations_of(symbol) {
        // A declaration without a parent is a file: the UMD module and global merge and CJS module end exports symbols.
        let Some(declaration_parent) = request.state.binder.node_record(declaration).parent else {
            continue;
        };
        if request.node_kind(declaration_parent) == SyntaxKind::TypeLiteral {
            if let Some(union) = request
                .state
                .binder
                .node_record(declaration_parent)
                .parent
                .filter(|&union| request.node_kind(union) == SyntaxKind::UnionType)
            {
                let ty = request.state.get_type_from_type_node(union)?;
                let name = request.state.binder.symbol(symbol).escaped_name;
                if let Some(property) = request.state.get_property_of_type_full(ty, name)? {
                    return Ok(Some(property));
                }
            }
        }
    }
    Ok(None)
}

/// tsgo `getLocalSymbolForExportSpecifier`.
fn get_local_symbol_for_export_specifier(
    request: &mut Request<'_, '_>,
    reference_location: NodeId,
    reference_symbol: SymbolId,
    export_specifier: NodeId,
) -> CheckResult<SymbolId> {
    if is_export_specifier_alias(request, reference_location, export_specifier) {
        if let Some(symbol) = request
            .state
            .get_export_specifier_local_target_symbol(export_specifier)?
        {
            return Ok(symbol);
        }
    }
    Ok(reference_symbol)
}

/// tsgo `isExportSpecifierAlias`.
fn is_export_specifier_alias(
    request: &Request<'_, '_>,
    reference_location: NodeId,
    export_specifier: NodeId,
) -> bool {
    let NodeData::ExportSpecifier(data) = &request.state.binder.node_record(export_specifier).data
    else {
        return false;
    };
    if let Some(property_name) = data.property_name {
        // Given `export { foo as bar } [from "someModule"]`: It's an alias at `foo`, but at `bar` it's a new symbol.
        return property_name == reference_location;
    }
    // `export { foo } from "foo"` is a re-export.
    // `export { foo };` is not a re-export, it creates an alias for the local variable `foo`.
    export_declaration_module_specifier(request, export_specifier).is_none()
}

/// The module specifier of an export specifier's declaration.
fn export_declaration_module_specifier(
    request: &Request<'_, '_>,
    export_specifier: NodeId,
) -> Option<NodeId> {
    let named_exports = request.state.binder.node_record(export_specifier).parent?;
    let declaration = request.state.binder.node_record(named_exports).parent?;
    match &request.state.binder.node_record(declaration).data {
        NodeData::ExportDeclaration(data) => data.module_specifier,
        _ => None,
    }
}

/// tsgo `getIntersectingMeaningFromDeclarations`.
pub(super) fn get_intersecting_meaning_from_declarations(
    request: &Request<'_, '_>,
    node: Option<Touching>,
    symbol: SymbolId,
    default_meaning: SemanticMeaning,
) -> SemanticMeaning {
    let Some(node) = node else {
        return default_meaning;
    };
    let mut meaning = request.meaning_from_location(node);
    let declarations = request.declarations_of(symbol);
    if declarations.is_empty() {
        return meaning;
    }
    let iteration = |mut meaning: SemanticMeaning| {
        for &declaration in &declarations {
            let declaration_meaning = request.meaning_from_declaration(declaration);
            if declaration_meaning.0 & meaning.0 != 0 {
                meaning = meaning.union(declaration_meaning);
            }
        }
        meaning
    };
    let mut last_iteration_meaning = meaning;
    meaning = iteration(meaning);
    // The result is order-sensitive, for instance if initialMeaning == Namespace, and declarations = [class, instantiated module]
    // we need to consider both as the initialMeaning intersects with the module in the namespace space, and the module
    // intersects with the class in the value space.
    // To achieve that we will keep iterating until the result stabilizes.
    while meaning != last_iteration_meaning {
        last_iteration_meaning = meaning;
        meaning = iteration(meaning);
    }
    meaning
}

/// tsgo `refSearch`: the symbol searched for (replaced when an alias of it
/// is found).
struct RefSearch {
    /// If coming from an export, we will not recursively search for the imported symbol (since that's where we came from).
    coming_from: ImpExpKind,
    symbol: SymbolId,
    text: String,
    /// Only set if `options.implementations` is true. These are the symbols checked to get the implementations of a property access.
    parents: Vec<SymbolId>,
    all_search_symbols: Vec<SymbolId>,
}

impl RefSearch {
    /// Whether a symbol is in the search set.
    /// Do not compare directly to `symbol` because there may be related symbols to search for. See `populateSearchSymbolSet`.
    fn includes(&self, symbol: SymbolId) -> bool {
        self.all_search_symbols.contains(&symbol)
    }
}

/// tsgo `refState`.
struct RefState<'r, 's, 'a, 'f> {
    request: &'r mut Request<'s, 'a>,
    files: &'f [usize],
    file_set: &'f HashSet<usize>,
    special_search_kind: SpecialSearchKind,
    search_meaning: SemanticMeaning,
    options: RefOptions,
    result: Vec<SymbolAndEntries>,
    inherits_from_cache: HashMap<(SymbolId, SymbolId), bool>,
    seen_containing_type_references: HashSet<NodeId>,
    seen_re_export_rhs: HashSet<NodeId>,
    import_tracker: Option<ImportTracker>,
    symbol_to_references: HashMap<SymbolId, usize>,
    file_to_seen_symbols: HashMap<usize, HashSet<SymbolId>>,
    /// tsgo caches a file's name table on the file.
    name_tables: HashMap<usize, HashSet<String>>,
}

/// How `forEachRelatedSymbol` reports a related symbol: tsgo's callbacks of
/// `populateSearchSymbolSet` and `getRelatedSymbol`.
enum RelatedSymbols<'s> {
    /// Collect each symbol, its root or its base (`populateSearchSymbolSet`).
    Populate {
        symbol: SymbolId,
        result: Vec<SymbolId>,
    },
    /// The symbol of the search the reference is related by
    /// (`getRelatedSymbol`).
    Find {
        search: &'s RefSearch,
        reference_symbol: SymbolId,
    },
}

impl RefState<'_, '_, '_, '_> {
    fn includes_source_file(&self, file: usize) -> bool {
        self.file_set.contains(&file)
    }

    /// tsgo `getImportSearches`.
    fn get_import_searches(
        &mut self,
        export_symbol: SymbolId,
        export_info: ExportInfo,
    ) -> CheckResult<super::import_tracker::ImportsResult> {
        if self.import_tracker.is_none() {
            self.import_tracker = Some(ImportTracker::new(self.request, self.files)?);
        }
        let tracker = self.import_tracker.as_ref().expect("an import tracker");
        tracker.imports(
            self.request,
            self.files,
            self.file_set,
            export_symbol,
            export_info,
            self.options.use_ == ReferenceUse::Rename,
        )
    }

    /// tsgo `createSearch`.
    fn create_search(
        &mut self,
        location: Option<Touching>,
        symbol: SymbolId,
        coming_from: ImpExpKind,
        text: Option<&str>,
        mut all_search_symbols: Vec<SymbolId>,
    ) -> CheckResult<RefSearch> {
        // Note: if this is an external module symbol, the name doesn't include quotes.
        // Note: getLocalSymbolForExportDefault handles `export default class C {}`, but not `export default C` or `export { C as default }`.
        // The other two forms seem to be handled downstream (e.g. in `skipPastExportOrImportSpecifier`), so special-casing the first form
        // here appears to be intentional).
        let text = match text {
            Some(text) => text.to_owned(),
            None => {
                let named = self
                    .request
                    .state
                    .local_symbol_for_export_default(symbol)
                    .or_else(|| get_non_module_symbol_of_merged_module_symbol(self.request, symbol))
                    .unwrap_or(symbol);
                let symbol_name = ast_symbol_name(self.request, named);
                try_get_ambient_module_name_from_symbol_name(&symbol_name).unwrap_or(symbol_name)
            }
        };
        if all_search_symbols.is_empty() {
            all_search_symbols = vec![symbol];
        }
        let parents = match location {
            Some(location) if self.options.implementations => {
                get_parent_symbols_of_property_access(self.request, location, symbol)?
            }
            _ => Vec::new(),
        };
        Ok(RefSearch {
            coming_from,
            symbol,
            text,
            parents,
            all_search_symbols,
        })
    }

    /// tsgo `referenceAdder`: add an entry to the search symbol's
    /// references.
    fn add_entry(&mut self, search_symbol: SymbolId, node: Touching, kind: EntryKind) {
        let index = match self.symbol_to_references.get(&search_symbol) {
            Some(&index) => index,
            None => {
                self.result.push(SymbolAndEntries::new(
                    DefinitionKind::Symbol,
                    None,
                    Some(search_symbol),
                    Vec::new(),
                ));
                let index = self.result.len() - 1;
                self.symbol_to_references.insert(search_symbol, index);
                index
            }
        };
        let entry = self.request.new_node_entry_with_kind(node, kind);
        self.result[index].references.push(entry);
    }

    /// tsgo `referenceAdder` without a reference: the symbol's entry.
    fn ensure_entries(&mut self, search_symbol: SymbolId) {
        if !self.symbol_to_references.contains_key(&search_symbol) {
            self.result.push(SymbolAndEntries::new(
                DefinitionKind::Symbol,
                None,
                Some(search_symbol),
                Vec::new(),
            ));
            self.symbol_to_references
                .insert(search_symbol, self.result.len() - 1);
        }
    }

    /// tsgo `addReference`.
    fn add_reference(
        &mut self,
        reference_location: Touching,
        symbol: SymbolId,
        kind: EntryKind,
    ) -> CheckResult<()> {
        // if rename symbol from default export anonymous function, for example `export default function() {}`, we do not need to add reference
        if self.options.use_ == ReferenceUse::Rename
            && self.request.kind(reference_location) == SyntaxKind::DefaultKeyword
        {
            return Ok(());
        }
        self.ensure_entries(symbol);
        if self.options.implementations {
            let mut nodes = Vec::new();
            self.add_implementation_references(reference_location, &mut nodes)?;
            for node in nodes {
                self.add_entry(symbol, node, kind);
            }
        } else {
            self.add_entry(symbol, reference_location, kind);
        }
        Ok(())
    }

    /// tsgo `addImplementationReferences`: the implementations a reference
    /// stands for.
    fn add_implementation_references(
        &mut self,
        ref_node: Touching,
        add: &mut Vec<Touching>,
    ) -> CheckResult<()> {
        let Touching::Node(id) = ref_node else {
            return Ok(());
        };
        // Check if we found a function/propertyAssignment/method with an implementation or initializer
        if self.request.is_declaration_name(id)
            && self
                .request
                .parent(ref_node)
                .is_some_and(|parent| is_implementation(self.request, parent))
        {
            add.push(ref_node);
            return Ok(());
        }

        if self.request.node_kind(id) != SyntaxKind::Identifier {
            return Ok(());
        }

        let parent = self
            .request
            .parent(ref_node)
            .expect("an identifier's parent");
        if self.request.node_kind(parent) == SyntaxKind::ShorthandPropertyAssignment {
            // Go ahead and dereference the shorthand assignment by going to its definition
            get_reference_entries_for_shorthand_property_assignment(self.request, id, add)?;
        }

        // Check if the node is within an extends or implements clause
        if let Some(containing_node) = get_containing_node_if_in_heritage_clause(self.request, id) {
            add.push(Touching::Node(containing_node));
            return Ok(());
        }

        // If we got a type reference, try and see if the reference applies to any expressions that can implement an interface
        // Find the first node whose parent isn't a type node -- i.e., the highest type node.
        let type_node = self.request.find_ancestor(Some(id), |request, node| {
            let Some(parent) = request.state.binder.node_record(node).parent else {
                return true;
            };
            let kind = request.node_kind(parent);
            kind != SyntaxKind::QualifiedName
                && !crate::completions::is_type_node_kind(kind)
                && !crate::completions::is_type_element(kind)
        });
        let Some(type_node) = type_node else {
            return Ok(());
        };
        let Some(type_having_node) = self.request.state.binder.node_record(type_node).parent else {
            return Ok(());
        };
        let Some(type_of_type_having_node) = node_type(self.request, type_having_node) else {
            return Ok(());
        };
        if type_of_type_having_node == type_node
            && self
                .seen_containing_type_references
                .insert(type_having_node)
        {
            let source = self.request.state.binder.source_of_node(type_having_node);
            let mut add_if_implementation = |request: &Request<'_, '_>, expression: NodeId| {
                if is_implementation_expression(request, expression) {
                    add.push(Touching::Node(expression));
                }
            };
            if let Some(initializer) = initializer_of(self.request, type_having_node) {
                add_if_implementation(self.request, initializer);
            } else if crate::utilities::is_function_like_kind(
                self.request.node_kind(type_having_node),
            ) {
                if let Some(body) = node_util::body_of(source, type_having_node) {
                    if self.request.node_kind(body) == SyntaxKind::Block {
                        let mut returned = Vec::new();
                        node_util::for_each_return_statement(source, body, &mut |statement| {
                            if let NodeData::ReturnStatement(data) =
                                &source.arena.node(statement).data
                            {
                                if let Some(expression) = data.expression {
                                    returned.push(expression);
                                }
                            }
                            false
                        });
                        for expression in returned {
                            add_if_implementation(self.request, expression);
                        }
                    } else {
                        add_if_implementation(self.request, body);
                    }
                }
            } else if let NodeData::TypeAssertionExpression(data) =
                &source.arena.node(type_having_node).data
            {
                if let Some(expression) = data.expression {
                    add_if_implementation(self.request, expression);
                }
            } else if let NodeData::AsExpression(data) = &source.arena.node(type_having_node).data {
                if let Some(expression) = data.expression {
                    add_if_implementation(self.request, expression);
                }
            } else if let NodeData::SatisfiesExpression(data) =
                &source.arena.node(type_having_node).data
            {
                if let Some(expression) = data.expression {
                    add_if_implementation(self.request, expression);
                }
            }
        }
        Ok(())
    }

    /// tsgo `getReferencesInContainerOrFiles`.
    fn get_references_in_container_or_files(
        &mut self,
        symbol: SymbolId,
        search: &RefSearch,
    ) -> CheckResult<()> {
        // Try to get the smallest valid scope that we can limit our search to;
        // otherwise we'll need to search globally (i.e. include each file).
        if let Some(scope) = get_symbol_scope(self.request, symbol) {
            let file = self.request.state.binder.file_index_of_node(scope);
            let add_references_here = self.request.node_kind(scope) != SyntaxKind::SourceFile
                || self.files.contains(&file);
            self.get_references_in_container(scope, file, search, add_references_here)?;
        } else {
            // Global search
            let files = self.files;
            for &file in files {
                self.search_for_name(file, search)?;
            }
        }
        Ok(())
    }

    /// tsgo `getReferencesInSourceFile`.
    fn get_references_in_source_file(
        &mut self,
        file: usize,
        search: &RefSearch,
        add_references_here: bool,
    ) -> CheckResult<()> {
        let root = self.request.state.binder.source(file).root;
        self.get_references_in_container(root, file, search, add_references_here)
    }

    /// tsgo `getReferencesInContainer`: search within the container for
    /// references for a search value.
    fn get_references_in_container(
        &mut self,
        container: NodeId,
        file: usize,
        search: &RefSearch,
        add_references_here: bool,
    ) -> CheckResult<()> {
        if !self.mark_searched_symbols(file, &search.all_search_symbols) {
            return Ok(());
        }
        let source = self.request.state.binder.source(file);
        for position in
            get_possible_symbol_reference_positions(source, &search.text, Some(container))
        {
            self.get_references_at_location(file, position, search, add_references_here)?;
        }
        Ok(())
    }

    /// tsgo `markSearchedSymbols`: whether any of the symbols is new for the
    /// file.
    fn mark_searched_symbols(&mut self, file: usize, symbols: &[SymbolId]) -> bool {
        let seen_symbols = self.file_to_seen_symbols.entry(file).or_default();
        let mut any_new_symbols = false;
        for &symbol in symbols {
            if seen_symbols.insert(symbol) {
                any_new_symbols = true;
            }
        }
        any_new_symbols
    }

    /// tsgo `getReferencesAtLocation`.
    fn get_references_at_location(
        &mut self,
        file: usize,
        position: usize,
        search: &RefSearch,
        add_references_here: bool,
    ) -> CheckResult<()> {
        let source = self.request.state.binder.source(file);
        let Some(reference_location) =
            touching(Navigator::new(source).touching_property_name(position as u32))
        else {
            return Ok(());
        };

        if !is_valid_reference_position(self.request, reference_location, &search.text) {
            // This wasn't the start of a token.  Check to see if it might be a
            // match in a comment or string if that's what the caller is asking
            // for. (tsgo does not implement the search in comments and strings.)
            return Ok(());
        }

        if self.request.meaning_from_location(reference_location).0 & self.search_meaning.0 == 0 {
            return Ok(());
        }

        let Some(mut reference_symbol) = self.request.symbol_at(reference_location)? else {
            return Ok(());
        };

        let parent = self
            .request
            .parent(reference_location)
            .expect("a reference's parent");
        if let (NodeData::ImportSpecifier(data), Touching::Node(id)) = (
            &self.request.state.binder.node_record(parent).data,
            reference_location,
        ) {
            if data.property_name == Some(id) {
                // This is added through `singleReferences` in ImportsResult. If we happen to see it again, don't add it again.
                return Ok(());
            }
        }

        if self.request.node_kind(parent) == SyntaxKind::ExportSpecifier {
            if let Touching::Node(id) = reference_location {
                self.get_references_at_export_specifier(
                    id,
                    reference_symbol,
                    parent,
                    search,
                    add_references_here,
                    false,
                )?;
            }
            return Ok(());
        }

        let Some((related_symbol, related_symbol_kind)) =
            self.get_related_symbol(search, reference_symbol, reference_location)?
        else {
            self.get_reference_for_shorthand_property(reference_symbol, search)?;
            return Ok(());
        };

        match self.special_search_kind {
            SpecialSearchKind::None => {
                if add_references_here {
                    self.add_reference(reference_location, related_symbol, related_symbol_kind)?;
                }
            }
            SpecialSearchKind::Constructor => {
                self.add_constructor_references(
                    reference_location,
                    related_symbol,
                    search,
                    add_references_here,
                )?;
            }
            SpecialSearchKind::Class => {
                self.add_class_static_this_references(
                    reference_location,
                    related_symbol,
                    search,
                    add_references_here,
                )?;
            }
        }

        // Use the parent symbol if the location is commonjs require syntax on javascript files only.
        if let Touching::Node(id) = reference_location {
            if self.request.state.is_in_js_file(id)
                && self.request.node_kind(parent) == SyntaxKind::BindingElement
            {
                let declaration = self
                    .request
                    .state
                    .binder
                    .node_record(parent)
                    .parent
                    .and_then(|pattern| self.request.state.binder.node_record(pattern).parent);
                if declaration.is_some_and(|declaration| {
                    crate::utilities::is_variable_declaration_initialized_to_bare_or_accessed_require(
                        self.request.state.binder.source_of_node(declaration),
                        declaration,
                    )
                }) {
                    // The parent will not have a symbol if it's an ObjectBindingPattern (when destructuring is used).  In
                    // this case, just skip it, since the bound identifiers are not an alias of the import.
                    let Some(parent_symbol) = self.request.state.binder.node_symbol(parent) else {
                        return Ok(());
                    };
                    reference_symbol = parent_symbol;
                }
            }
        }

        self.get_import_or_export_references(reference_location, reference_symbol, search)
    }

    /// tsgo `addConstructorReferences`.
    fn add_constructor_references(
        &mut self,
        reference_location: Touching,
        symbol: SymbolId,
        search: &RefSearch,
        add_references_here: bool,
    ) -> CheckResult<()> {
        if is_new_expression_target(self.request, reference_location) && add_references_here {
            self.add_reference(reference_location, symbol, EntryKind::Node)?;
        }

        let parent = self
            .request
            .parent(reference_location)
            .expect("a reference's parent");
        if is_class_like(self.request.node_kind(parent)) {
            // This is the class declaration containing the constructor.
            let mut nodes = Vec::new();
            find_own_constructor_references(self.request, search.symbol, &mut nodes);
            for node in nodes {
                self.add_entry(search.symbol, node, EntryKind::Node);
            }
        } else if let Some(class_extending) =
            try_get_class_by_extending_identifier(self.request, reference_location)
        {
            // If this class appears in `extends C`, then the extending class' "super" calls are references.
            let mut nodes = Vec::new();
            find_super_constructor_accesses(self.request, class_extending, &mut nodes);
            for node in nodes {
                self.add_entry(search.symbol, node, EntryKind::Node);
            }
            self.find_inherited_constructor_references(class_extending)?;
        }
        Ok(())
    }

    /// tsgo `addClassStaticThisReferences`.
    fn add_class_static_this_references(
        &mut self,
        reference_location: Touching,
        symbol: SymbolId,
        search: &RefSearch,
        add_references_here: bool,
    ) -> CheckResult<()> {
        if add_references_here {
            self.add_reference(reference_location, symbol, EntryKind::Node)?;
        }

        let Some(class_like) = self.request.parent(reference_location) else {
            return Ok(());
        };
        if self.options.use_ == ReferenceUse::Rename
            || !is_class_like(self.request.node_kind(class_like))
        {
            return Ok(());
        }

        self.ensure_entries(search.symbol);
        let source = self.request.state.binder.source_of_node(class_like);
        let members = class_members(self.request, class_like);
        for member in members {
            if !(is_method_or_accessor(self.request.node_kind(member))
                && node_util::has_syntactic_modifier(source, member, ModifierFlags::STATIC))
            {
                continue;
            }
            let Some(body) = node_util::body_of(source, member) else {
                continue;
            };
            let mut this_keywords = Vec::new();
            collect_static_this_keywords(source, body, &mut this_keywords);
            for this_keyword in this_keywords {
                self.add_entry(search.symbol, Touching::Node(this_keyword), EntryKind::Node);
            }
        }
        Ok(())
    }

    /// tsgo `findInheritedConstructorReferences`.
    fn find_inherited_constructor_references(
        &mut self,
        class_declaration: NodeId,
    ) -> CheckResult<()> {
        if has_own_constructor(self.request, class_declaration) {
            return Ok(());
        }
        let Some(class_symbol) = self.request.state.binder.node_symbol(class_declaration) else {
            return Ok(());
        };
        let search =
            self.create_search(None, class_symbol, ImpExpKind::Unknown, None, Vec::new())?;
        self.get_references_in_container_or_files(class_symbol, &search)
    }

    /// tsgo `getImportOrExportReferences`.
    fn get_import_or_export_references(
        &mut self,
        reference_location: Touching,
        reference_symbol: SymbolId,
        search: &RefSearch,
    ) -> CheckResult<()> {
        let Some(import_or_export) = get_import_or_export_symbol(
            self.request,
            reference_location,
            reference_symbol,
            search.coming_from == ImpExpKind::Export,
        )?
        else {
            return Ok(());
        };
        if import_or_export.kind == ImpExpKind::Import {
            if !is_for_rename_with_prefix_and_suffix_text(self.options) {
                self.search_for_imported_symbol(import_or_export.symbol)?;
            }
        } else if let Some(export_info) = import_or_export.export_info {
            self.search_for_imports_of_export(
                Some(reference_location),
                import_or_export.symbol,
                export_info,
            )?;
        }
        Ok(())
    }

    /// tsgo `markSeenReExportRHS`.
    fn mark_seen_re_export_rhs(&mut self, node: NodeId) -> bool {
        self.seen_re_export_rhs.insert(node)
    }

    /// tsgo `getReferencesAtExportSpecifier`.
    fn get_references_at_export_specifier(
        &mut self,
        reference_location: NodeId,
        reference_symbol: SymbolId,
        export_specifier: NodeId,
        search: &RefSearch,
        add_references_here: bool,
        always_get_references: bool,
    ) -> CheckResult<()> {
        let NodeData::ExportSpecifier(data) = self
            .request
            .state
            .binder
            .node_record(export_specifier)
            .data
            .clone()
        else {
            return Ok(());
        };
        let module_specifier = export_declaration_module_specifier(self.request, export_specifier);
        let property_name = data.property_name;
        let Some(name) = data.name else {
            return Ok(());
        };
        let local_symbol = get_local_symbol_for_export_specifier(
            self.request,
            reference_location,
            reference_symbol,
            export_specifier,
        )?;

        if !always_get_references && !search.includes(local_symbol) {
            return Ok(());
        }

        let source = self.request.state.binder.source_of_node(export_specifier);
        let add_ref = |state: &mut Self| -> CheckResult<()> {
            if add_references_here {
                state.add_reference(
                    Touching::Node(reference_location),
                    local_symbol,
                    EntryKind::Node,
                )?;
            }
            Ok(())
        };

        match property_name {
            None => {
                // Don't rename at `export { default } from "m";`. (but do continue to search for imports of the re-export)
                if !(self.options.use_ == ReferenceUse::Rename
                    && node_util::module_export_name_is_default(source, name))
                {
                    add_ref(self)?;
                }
            }
            Some(property_name) if reference_location == property_name => {
                // For `export { foo as bar } from "baz"`, "`foo`" will be added from the singleReferences for import searches of the original export.
                // For `export { foo as bar };`, where `foo` is a local, so add it now.
                if module_specifier.is_none() {
                    add_ref(self)?;
                }

                if add_references_here
                    && self.options.use_ != ReferenceUse::Rename
                    && self.mark_seen_re_export_rhs(name)
                {
                    let export_symbol = self
                        .request
                        .state
                        .binder
                        .node_symbol(export_specifier)
                        .expect("exportSpecifier.Symbol() should not be nil");
                    self.add_reference(Touching::Node(name), export_symbol, EntryKind::Node)?;
                }
            }
            Some(_) => {
                if self.mark_seen_re_export_rhs(reference_location) {
                    add_ref(self)?;
                }
            }
        }

        // For `export { foo as bar }`, rename `foo`, but not `bar`.
        if !is_for_rename_with_prefix_and_suffix_text(self.options) || always_get_references {
            let is_default_export =
                node_util::module_export_name_is_default(source, reference_location)
                    || node_util::module_export_name_is_default(source, name);
            let export_kind = if is_default_export {
                ExportKind::Default
            } else {
                ExportKind::Named
            };
            let export_symbol = self
                .request
                .state
                .binder
                .node_symbol(export_specifier)
                .expect("exportSpecifier.Symbol() should not be nil");
            if let Some(export_info) = get_export_info(self.request, export_symbol, export_kind) {
                self.search_for_imports_of_export(
                    Some(Touching::Node(reference_location)),
                    export_symbol,
                    export_info,
                )?;
            }
        }

        // At `export { x } from "foo"`, also search for the imported symbol `"foo".x`.
        if search.coming_from != ImpExpKind::Export
            && module_specifier.is_some()
            && property_name.is_none()
            && !is_for_rename_with_prefix_and_suffix_text(self.options)
        {
            if let Some(imported) = self
                .request
                .state
                .get_export_specifier_local_target_symbol(export_specifier)?
            {
                self.search_for_imported_symbol(imported)?;
            }
        }
        Ok(())
    }

    /// tsgo `searchForImportedSymbol`: go to the symbol we imported from and
    /// find references for it.
    fn search_for_imported_symbol(&mut self, symbol: SymbolId) -> CheckResult<()> {
        for declaration in self.request.declarations_of(symbol) {
            let exporting_file = self.request.state.binder.file_index_of_node(declaration);
            // Need to search in the file even if it's not in the search-file set, because it might export the symbol.
            let search = self.create_search(
                Some(Touching::Node(declaration)),
                symbol,
                ImpExpKind::Import,
                None,
                Vec::new(),
            )?;
            let add_references_here = self.includes_source_file(exporting_file);
            self.get_references_in_source_file(exporting_file, &search, add_references_here)?;
        }
        Ok(())
    }

    /// tsgo `searchForImportsOfExport`: search for all imports of a given
    /// exported symbol using `getImportSearches`.
    fn search_for_imports_of_export(
        &mut self,
        export_location: Option<Touching>,
        export_symbol: SymbolId,
        export_info: ExportInfo,
    ) -> CheckResult<()> {
        let result = self.get_import_searches(export_symbol, export_info)?;

        // For `import { foo as bar }` just add the reference to `foo`, and don't otherwise search in the file.
        if !result.single_references.is_empty() {
            self.ensure_entries(export_symbol);
            for single_reference in result.single_references {
                if self.should_add_single_reference(single_reference) {
                    self.add_entry(
                        export_symbol,
                        Touching::Node(single_reference),
                        EntryKind::Node,
                    );
                }
            }
        }

        // For each import, find all references to that import in its source file.
        for (import_location, import_symbol) in result.import_searches {
            let file = self
                .request
                .state
                .binder
                .file_index_of_node(import_location);
            let search = self.create_search(
                Some(Touching::Node(import_location)),
                import_symbol,
                ImpExpKind::Export,
                None,
                Vec::new(),
            )?;
            self.get_references_in_source_file(file, &search, true)?;
        }

        if !result.indirect_users.is_empty() {
            let indirect_search = match export_info.export_kind {
                ExportKind::Named => Some(self.create_search(
                    export_location,
                    export_symbol,
                    ImpExpKind::Export,
                    None,
                    Vec::new(),
                )?),
                // Search for a property access to '.default'. This can't be renamed.
                ExportKind::Default if self.options.use_ != ReferenceUse::Rename => {
                    Some(self.create_search(
                        export_location,
                        export_symbol,
                        ImpExpKind::Export,
                        Some("default"),
                        Vec::new(),
                    )?)
                }
                _ => None,
            };
            if let Some(indirect_search) = indirect_search {
                for indirect_user in result.indirect_users {
                    self.search_for_name(indirect_user, &indirect_search)?;
                }
            }
        }
        Ok(())
    }

    /// tsgo `shouldAddSingleReference`.
    fn should_add_single_reference(&self, single_reference: NodeId) -> bool {
        if !self.has_matching_meaning(Touching::Node(single_reference)) {
            return false;
        }
        if self.options.use_ != ReferenceUse::Rename {
            return true;
        }
        let parent = self
            .request
            .state
            .binder
            .node_record(single_reference)
            .parent;
        let parent_is_specifier = parent.is_some_and(|parent| {
            matches!(
                self.request.node_kind(parent),
                SyntaxKind::ImportSpecifier | SyntaxKind::ExportSpecifier
            )
        });
        // Don't rename an import type `import("./module-name")` when renaming `name` in `export = name;`
        if self.request.node_kind(single_reference) != SyntaxKind::Identifier
            && !parent_is_specifier
        {
            return false;
        }
        // At `default` in `import { default as x }` or `export { default as x }`, do add a reference, but do not rename.
        !(parent_is_specifier
            && node_util::module_export_name_is_default(
                self.request.state.binder.source_of_node(single_reference),
                single_reference,
            ))
    }

    /// tsgo `hasMatchingMeaning`.
    fn has_matching_meaning(&self, reference_location: Touching) -> bool {
        self.request.meaning_from_location(reference_location).0 & self.search_meaning.0 != 0
    }

    /// tsgo `getReferenceForShorthandProperty`.
    fn get_reference_for_shorthand_property(
        &mut self,
        reference_symbol: SymbolId,
        search: &RefSearch,
    ) -> CheckResult<()> {
        if self
            .request
            .state
            .symbol_flags(reference_symbol)
            .intersects(SymbolFlags::TRANSIENT)
        {
            return Ok(());
        }
        let Some(value_declaration) = self
            .request
            .state
            .binder
            .symbol(reference_symbol)
            .value_declaration
        else {
            return Ok(());
        };
        let shorthand_value_symbol = self
            .request
            .state
            .get_shorthand_assignment_value_symbol(value_declaration)?;
        let name = node_util::get_name_of_declaration(
            self.request.state.binder.source_of_node(value_declaration),
            value_declaration,
        );

        // Because in short-hand property assignment, an identifier which stored as name of the short-hand property assignment
        // has two meanings: property name and property value. Therefore when we do findAllReference at the position where
        // an identifier is declared, the language service should return the position of the variable declaration as well as
        // the position in short-hand property assignment excluding property accessing. However, if we do findAllReference at the
        // position of property accessing, the referenceEntry of such position will be handled in the first case.
        if let (Some(name), Some(shorthand_value_symbol)) = (name, shorthand_value_symbol) {
            if search.includes(shorthand_value_symbol) {
                self.add_reference(
                    Touching::Node(name),
                    shorthand_value_symbol,
                    EntryKind::Node,
                )?;
            }
        }
        Ok(())
    }

    /// tsgo `populateSearchSymbolSet`.
    fn populate_search_symbol_set(
        &mut self,
        symbol: SymbolId,
        location: Option<Touching>,
        is_for_rename: bool,
        provide_prefix_and_suffix_text: bool,
        implementations: bool,
    ) -> CheckResult<Vec<SymbolId>> {
        let Some(location) = location else {
            return Ok(vec![symbol]);
        };
        let mut related = RelatedSymbols::Populate {
            symbol,
            result: Vec::new(),
        };
        self.for_each_related_symbol(
            symbol,
            location,
            is_for_rename,
            !(is_for_rename && provide_prefix_and_suffix_text),
            &mut related,
            // when try to find implementation, implementations is true, and not allowed to find base class
            Some(!implementations),
        )?;
        let RelatedSymbols::Populate { result, .. } = related else {
            unreachable!("populating");
        };
        Ok(result)
    }

    /// tsgo `getRelatedSymbol`.
    fn get_related_symbol(
        &mut self,
        search: &RefSearch,
        reference_symbol: SymbolId,
        reference_location: Touching,
    ) -> CheckResult<Option<(SymbolId, EntryKind)>> {
        let mut related = RelatedSymbols::Find {
            search,
            reference_symbol,
        };
        self.for_each_related_symbol(
            reference_symbol,
            reference_location,
            false, // isForRenamePopulateSearchSymbolSet
            self.options.use_ != ReferenceUse::Rename || self.options.use_aliases_for_rename, // onlyIncludeBindingElementAtReferenceLocation
            &mut related,
            None,
        )
    }

    /// The callback of `forEachRelatedSymbol`.
    fn related_callback(
        &self,
        related: &mut RelatedSymbols<'_>,
        symbol: SymbolId,
        root_symbol: Option<SymbolId>,
        base_symbol: Option<SymbolId>,
    ) -> Option<SymbolId> {
        match related {
            RelatedSymbols::Populate {
                symbol: searched,
                result,
            } => {
                let mut base_symbol = base_symbol;
                // static method/property and instance method/property might have the same name. Only include static or only include instance.
                if let Some(base) = base_symbol {
                    if is_static_symbol(self.request, *searched)
                        != is_static_symbol(self.request, base)
                    {
                        base_symbol = None;
                    }
                }
                result.push(base_symbol.or(root_symbol).unwrap_or(symbol));
                None
            }
            RelatedSymbols::Find {
                search,
                reference_symbol,
            } => {
                let mut base_symbol = base_symbol;
                // check whether the symbol used to search itself is just the searched one.
                if let Some(base) = base_symbol {
                    // static method/property and instance method/property might have the same name. Only check static or only check instance.
                    if is_static_symbol(self.request, *reference_symbol)
                        != is_static_symbol(self.request, base)
                    {
                        base_symbol = None;
                    }
                }
                let search_symbol = base_symbol.or(root_symbol).unwrap_or(symbol);
                if search.includes(search_symbol) {
                    if let Some(root_symbol) = root_symbol {
                        if !self
                            .request
                            .state
                            .symbol_check_flags(symbol)
                            .intersects(CheckFlags::SYNTHETIC)
                        {
                            return Some(root_symbol);
                        }
                    }
                    return Some(symbol);
                }
                // For a base type, use the symbol for the derived type. For a synthetic (e.g. union) property, use the union symbol.
                None
            }
        }
    }

    /// The `allowBaseTypes` of `forEachRelatedSymbol`: `allow` for
    /// `populateSearchSymbolSet`, else whether the root's parent inherits
    /// from a parent of the search.
    fn allow_base_types(
        &mut self,
        related: &RelatedSymbols<'_>,
        allow: Option<bool>,
        root_symbol: SymbolId,
    ) -> CheckResult<bool> {
        if let Some(allow) = allow {
            return Ok(allow);
        }
        let RelatedSymbols::Find { search, .. } = related else {
            return Ok(true);
        };
        if search.parents.is_empty() {
            return Ok(true);
        }
        let Some(root_parent) = self.request.state.binder.symbol(root_symbol).parent else {
            return Ok(false);
        };
        for &parent in &search.parents {
            if self.explicitly_inherits_from(root_parent, parent)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// tsgo `forEachRelatedSymbol`'s `fromRoot`.
    fn related_from_root(
        &mut self,
        symbol: SymbolId,
        related: &mut RelatedSymbols<'_>,
        allow: Option<bool>,
    ) -> CheckResult<Option<SymbolId>> {
        // If this is a union property:
        //   - In populateSearchSymbolsSet we will add all the symbols from all its source symbols in all unioned types.
        //   - In findRelatedSymbol, we will just use the union symbol if any source symbol is included in the search.
        // If the symbol is an instantiation from a another symbol (e.g. widened symbol):
        //   - In populateSearchSymbolsSet, add the root the list
        //   - In findRelatedSymbol, return the source symbol if that is in the search. (Do not return the instantiation symbol.)
        for root_symbol in self.request.state.get_root_symbols(symbol)? {
            if let Some(result) = self.related_callback(related, symbol, Some(root_symbol), None) {
                return Ok(Some(result));
            }
            // Add symbol of properties/methods of the same name in base classes and implemented interfaces definitions
            let root_parent = self.request.state.binder.symbol(root_symbol).parent;
            if let Some(root_parent) = root_parent {
                if self
                    .request
                    .state
                    .symbol_flags(root_parent)
                    .intersects(SymbolFlags::CLASS | SymbolFlags::INTERFACE)
                    && self.allow_base_types(related, allow, root_symbol)?
                {
                    let name = self.request.state.binder.symbol(root_symbol).escaped_name;
                    let bases =
                        get_property_symbols_from_base_types(self.request, root_parent, name)?;
                    for base in bases {
                        if let Some(result) =
                            self.related_callback(related, symbol, Some(root_symbol), Some(base))
                        {
                            return Ok(Some(result));
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    /// tsgo `forEachRelatedSymbol`.
    fn for_each_related_symbol(
        &mut self,
        symbol: SymbolId,
        location: Touching,
        is_for_rename_populate_search_symbol_set: bool,
        only_include_binding_element_at_reference_location: bool,
        related: &mut RelatedSymbols<'_>,
        allow: Option<bool>,
    ) -> CheckResult<Option<(SymbolId, EntryKind)>> {
        let location_node = match location {
            Touching::Node(id) => Some(id),
            Touching::Token { .. } => None,
        };
        if let Some(containing_object_literal_element) =
            location_node.and_then(|id| containing_object_literal_element(self.request.state, id))
        {
            let location_id = location_node.expect("a node");
            /* Because in short-hand property assignment, location has two meaning : property name and as value of the property
             * When we do findAllReference at the position of the short-hand property assignment, we would want to have references to position of
             * property name and variable declaration of the identifier.
             * Like in below example, when querying for all references for an identifier 'name', of the property assignment, the language service
             * should show both 'name' in 'obj' and 'name' in variable declaration
             *      const name = "Foo";
             *      const obj = { name };
             * In order to do that, we will populate the search set with the value symbol of the identifier as a value of the property assignment
             * so that when matching with potential reference symbol, both symbols from property declaration and variable declaration
             * will be included correctly.
             */
            let location_parent = self
                .request
                .state
                .binder
                .node_record(location_id)
                .parent
                .expect("a parent");
            let shorthand_value_symbol = self
                .request
                .state
                .get_shorthand_assignment_value_symbol(location_parent)?;
            // gets the local symbol
            if let Some(shorthand_value_symbol) = shorthand_value_symbol {
                if is_for_rename_populate_search_symbol_set {
                    // When renaming 'x' in `const o = { x }`, just rename the local variable, not the property.
                    return Ok(self
                        .related_callback(related, shorthand_value_symbol, None, None)
                        .map(|symbol| (symbol, EntryKind::SearchedLocalFoundProperty)));
                }
            }
            // If the location is in a context sensitive location (i.e. in an object literal) try
            // to get a contextual type for it, and add the property symbol from the contextual
            // type to the search set
            let object = self
                .request
                .state
                .binder
                .node_record(containing_object_literal_element)
                .parent
                .expect("an object literal");
            if let Some(contextual_type) = self
                .request
                .state
                .language_service_contextual_type(object, ContextFlags::NONE)?
            {
                let symbols = self
                    .request
                    .state
                    .get_property_symbols_from_contextual_type(
                        containing_object_literal_element,
                        contextual_type,
                        true,
                    )?;
                for symbol in symbols {
                    if let Some(result) = self.related_from_root(symbol, related, allow)? {
                        return Ok(Some((result, EntryKind::SearchedPropertyFoundLocal)));
                    }
                }
            }
            // If the location is name of property symbol from object literal destructuring pattern
            // Search the property symbol
            //      for ( { property: p2 } of elems) { }
            if let Some(property_symbol) = self
                .request
                .state
                .get_property_symbol_of_destructuring_assignment(location_id)?
            {
                if let Some(result) = self.related_callback(related, property_symbol, None, None) {
                    return Ok(Some((result, EntryKind::SearchedPropertyFoundLocal)));
                }
            }
            if let Some(shorthand_value_symbol) = shorthand_value_symbol {
                if let Some(result) =
                    self.related_callback(related, shorthand_value_symbol, None, None)
                {
                    return Ok(Some((result, EntryKind::SearchedLocalFoundProperty)));
                }
            }
        }

        if let Some(aliased_symbol) = get_merged_aliased_symbol_of_namespace_export_declaration(
            self.request,
            location,
            symbol,
        )? {
            // In case of UMD module and global merging, search for global as well
            if let Some(result) = self.related_callback(related, aliased_symbol, None, None) {
                return Ok(Some((result, EntryKind::Node)));
            }
        }

        if let Some(result) = self.related_from_root(symbol, related, allow)? {
            return Ok(Some((result, EntryKind::Node)));
        }

        let value_declaration = self.request.state.binder.symbol(symbol).value_declaration;
        if let Some(value_declaration) = value_declaration {
            let source = self.request.state.binder.source_of_node(value_declaration);
            let is_parameter_property = self
                .request
                .state
                .binder
                .node_record(value_declaration)
                .parent
                .is_some_and(|parent| {
                    node_util::is_parameter_property_declaration(source, value_declaration, parent)
                });
            if is_parameter_property {
                let name = self.request.state.binder.symbol(symbol).escaped_name;
                let Some((parameter, property)) = self
                    .request
                    .state
                    .get_symbols_of_parameter_property_declaration(value_declaration, name)?
                else {
                    return Ok(None);
                };
                let other = if self
                    .request
                    .state
                    .symbol_flags(symbol)
                    .intersects(SymbolFlags::FUNCTION_SCOPED_VARIABLE)
                {
                    property
                } else {
                    parameter
                };
                return Ok(self
                    .related_from_root(other, related, allow)?
                    .map(|result| (result, EntryKind::Node)));
            }
        }

        let export_specifier =
            self.request
                .declarations_of(symbol)
                .into_iter()
                .find(|&declaration| {
                    self.request.node_kind(declaration) == SyntaxKind::ExportSpecifier
                });
        if let Some(export_specifier) = export_specifier {
            let has_property_name = matches!(
                &self.request.state.binder.node_record(export_specifier).data,
                NodeData::ExportSpecifier(data) if data.property_name.is_some()
            );
            if !is_for_rename_populate_search_symbol_set || !has_property_name {
                if let Some(local_symbol) = self
                    .request
                    .state
                    .get_export_specifier_local_target_symbol(export_specifier)?
                {
                    if let Some(result) = self.related_callback(related, local_symbol, None, None) {
                        return Ok(Some((result, EntryKind::Node)));
                    }
                }
            }
        }

        // symbolAtLocation for a binding element is the local symbol. See if the search symbol is the property.
        // Don't do this when populating search set for a rename when prefix and suffix text will be provided -- just rename the local.
        if !is_for_rename_populate_search_symbol_set {
            let binding_element_property_symbol =
                if only_include_binding_element_at_reference_location {
                    let Some(parent) = self.request.parent(location) else {
                        return Ok(None);
                    };
                    if !is_object_binding_element_without_property_name(self.request, parent) {
                        return Ok(None);
                    }
                    get_property_symbol_from_binding_element(self.request, parent)?
                } else {
                    get_property_symbol_of_object_binding_pattern_without_property_name(
                        self.request,
                        symbol,
                    )?
                };
            let Some(binding_element_property_symbol) = binding_element_property_symbol else {
                return Ok(None);
            };
            return Ok(self
                .related_from_root(binding_element_property_symbol, related, allow)?
                .map(|result| (result, EntryKind::SearchedPropertyFoundLocal)));
        }

        // due to the above assert and the arguments at the uses of this function,
        // (onlyIncludeBindingElementAtReferenceLocation <=> !providePrefixAndSuffixTextForRename) holds
        let include_original_symbol_of_binding_element =
            only_include_binding_element_at_reference_location;
        if include_original_symbol_of_binding_element {
            if let Some(binding_element_property_symbol) =
                get_property_symbol_of_object_binding_pattern_without_property_name(
                    self.request,
                    symbol,
                )?
            {
                return Ok(self
                    .related_from_root(binding_element_property_symbol, related, allow)?
                    .map(|result| (result, EntryKind::SearchedPropertyFoundLocal)));
            }
        }
        Ok(None)
    }

    /// tsgo `searchForName`: search for all occurrences of an identifier in a
    /// source file (and filter out the ones that match).
    fn search_for_name(&mut self, file: usize, search: &RefSearch) -> CheckResult<()> {
        if !self.name_tables.contains_key(&file) {
            let source = self.request.state.binder.source(file);
            let names = self.request.name_table(source).into_keys().collect();
            self.name_tables.insert(file, names);
        }
        if self.name_tables[&file].contains(&search.text) {
            self.get_references_in_source_file(file, search, true)?;
        }
        Ok(())
    }

    /// tsgo `explicitlyInheritsFrom`.
    fn explicitly_inherits_from(
        &mut self,
        symbol: SymbolId,
        parent: SymbolId,
    ) -> CheckResult<bool> {
        if symbol == parent {
            return Ok(true);
        }
        // Check cache first
        let key = (symbol, parent);
        if let Some(&cached) = self.inherits_from_cache.get(&key) {
            return Ok(cached);
        }
        // Set to false initially to prevent infinite recursion
        self.inherits_from_cache.insert(key, false);
        let mut inherits = false;
        'declarations: for declaration in self.request.declarations_of(symbol) {
            let source = self.request.state.binder.source_of_node(declaration);
            for type_reference in node_util::get_all_super_type_nodes(source, declaration) {
                let ty = self.request.state.get_type_at_location(type_reference)?;
                if let Some(type_symbol) = self.request.state.tables.type_of(ty).symbol {
                    if self.explicitly_inherits_from(type_symbol, parent)? {
                        inherits = true;
                        break 'declarations;
                    }
                }
            }
        }
        // Update cache with the actual result
        self.inherits_from_cache.insert(key, inherits);
        Ok(inherits)
    }
}

/// tsgo `getReferenceEntriesForShorthandPropertyAssignment`.
fn get_reference_entries_for_shorthand_property_assignment(
    request: &mut Request<'_, '_>,
    node: NodeId,
    add: &mut Vec<Touching>,
) -> CheckResult<()> {
    let Some(ref_symbol) = request.state.get_symbol_at_location(node)? else {
        return Ok(());
    };
    let Some(value_declaration) = request.state.binder.symbol(ref_symbol).value_declaration else {
        return Ok(());
    };
    let Some(shorthand_symbol) = request
        .state
        .get_shorthand_assignment_value_symbol(value_declaration)?
    else {
        return Ok(());
    };
    for declaration in request.declarations_of(shorthand_symbol) {
        if request.meaning_from_declaration(declaration).0 & SemanticMeaning::VALUE.0 != 0 {
            add.push(Touching::Node(declaration));
        }
    }
    Ok(())
}

/// tsgo `getSymbolScope`: the node the symbol's references are limited to,
/// none when they can be anywhere.
fn get_symbol_scope(request: &Request<'_, '_>, symbol: SymbolId) -> Option<NodeId> {
    // If this is the symbol of a named function expression or named class expression,
    // then named references are limited to its own scope.
    let data = request.state.binder.symbol(symbol);
    if let Some(value_declaration) = data.value_declaration {
        if matches!(
            request.node_kind(value_declaration),
            SyntaxKind::FunctionExpression | SyntaxKind::ClassExpression
        ) {
            return Some(value_declaration);
        }
    }

    let declarations = request.declarations_of(symbol);
    if declarations.is_empty() {
        return None;
    }

    let flags = request.state.symbol_flags(symbol);
    // If this is private property or method, the scope is the containing class
    if flags.intersects(SymbolFlags::PROPERTY | SymbolFlags::METHOD) {
        let private_declaration = declarations.iter().copied().find(|&declaration| {
            modifier_flags(request, declaration).intersects(ModifierFlags::PRIVATE)
                || is_private_identifier_class_element_declaration(request, declaration)
        });
        if let Some(private_declaration) = private_declaration {
            return request.find_ancestor(Some(private_declaration), |request, node| {
                request.node_kind(node) == SyntaxKind::ClassDeclaration
            });
        }
        // Else this is a public property and could be accessed from anywhere.
        return None;
    }

    // If symbol is of object binding pattern element without property name we would want to
    // look for property too and that could be anywhere
    if declarations
        .iter()
        .any(|&declaration| is_object_binding_element_without_property_name(request, declaration))
    {
        return None;
    }

    /*
        If the symbol has a parent, it's globally visible unless:
        - It's a private property (handled above).
        - It's a type parameter.
        - The parent is an external module: then we should only search in the module (and recurse on the export later).
        - But if the parent has `export as namespace`, the symbol is globally visible through that namespace.
    */
    let parent = data.parent;
    let exposed_by_parent = parent.is_some() && !flags.intersects(SymbolFlags::TYPE_PARAMETER);
    if let Some(parent) = parent.filter(|_| exposed_by_parent) {
        let parent_value_declaration = request.state.binder.symbol(parent).value_declaration;
        if !is_external_module_symbol(request, parent)
            || is_source_file_with_global_exports(request, parent_value_declaration)
        {
            return None;
        }
    }

    let mut scope: Option<NodeId> = None;
    for declaration in declarations {
        let container = request.container_node(Touching::Node(declaration));
        if scope.is_some() && scope != container {
            // Different declarations have different containers, bail out
            return None;
        }
        let container = container?;
        if request.node_kind(container) == SyntaxKind::SourceFile
            && !request.state.is_external_or_common_js_module_file(
                request.state.binder.file_index_of_node(container),
            )
        {
            // This is a global variable and not an external module, any declaration defined
            // within this scope is visible outside the file
            return None;
        }
        scope = Some(container);
    }

    // If symbol.parent, this means we are in an export of an external module. (Otherwise we would have returned `undefined` above.)
    // For an export of a module, we may be in a declaration file, and it may be accessed elsewhere. E.g.:
    //     declare module "a" { export type T = number; }
    //     declare module "b" { import { T } from "a"; export const x: T; }
    // So we must search the whole source file. (Because we will mark the source file as seen, we we won't return to it when searching for imports.)
    if exposed_by_parent {
        return scope.map(|scope| request.state.binder.source_of_node(scope).root);
    }
    scope
}

/// tsgo `isValidReferencePosition` (lengths are Go's byte lengths).
fn is_valid_reference_position(
    request: &Request<'_, '_>,
    node: Touching,
    search_symbol_name: &str,
) -> bool {
    let id = match node {
        Touching::Node(id) => id,
        Touching::Token { kind, .. } => {
            return kind == SyntaxKind::DefaultKeyword
                && "default".len() == search_symbol_name.len();
        }
    };
    let source = request.state.binder.source_of_node(id);
    match request.node_kind(id) {
        SyntaxKind::PrivateIdentifier | SyntaxKind::Identifier => {
            node_util::id_text(source, id).unwrap_or_default().len() == search_symbol_name.len()
        }
        SyntaxKind::NoSubstitutionTemplateLiteral | SyntaxKind::StringLiteral => {
            literal_text(request, id).len() == search_symbol_name.len()
                && (request.is_literal_name_of_property_declaration_or_index_access(id)
                    || is_name_of_module_declaration(request, id)
                    || is_expression_of_external_module_import_equals_declaration(request, id)
                    || is_second_argument_of_bindable_object_define_property_call(request, id)
                    || request.parent(node).is_some_and(|parent| {
                        matches!(
                            request.node_kind(parent),
                            SyntaxKind::ImportSpecifier | SyntaxKind::ExportSpecifier
                        )
                    }))
        }
        SyntaxKind::NumericLiteral => {
            request.is_literal_name_of_property_declaration_or_index_access(id)
                && literal_text(request, id).len() == search_symbol_name.len()
        }
        SyntaxKind::DefaultKeyword => "default".len() == search_symbol_name.len(),
        _ => false,
    }
}

/// tsgo `isNameOfModuleDeclaration`.
fn is_name_of_module_declaration(request: &Request<'_, '_>, node: NodeId) -> bool {
    let Some(parent) = request.state.binder.node_record(node).parent else {
        return false;
    };
    request.node_kind(parent) == SyntaxKind::ModuleDeclaration
        && request.name_of(parent) == Some(node)
}

/// tsgo `isExpressionOfExternalModuleImportEqualsDeclaration`.
fn is_expression_of_external_module_import_equals_declaration(
    request: &Request<'_, '_>,
    node: NodeId,
) -> bool {
    let Some(declaration) = request
        .state
        .binder
        .node_record(node)
        .parent
        .and_then(|parent| request.state.binder.node_record(parent).parent)
    else {
        return false;
    };
    request.node_kind(declaration) == SyntaxKind::ImportEqualsDeclaration
        && super::import_tracker::external_module_import_equals_expression(request, declaration)
            == Some(node)
}

/// `ast.IsCallExpression(node.Parent) && ast.IsBindableObjectDefinePropertyCall(node.Parent) &&
/// node.Parent.Arguments()[1] == node`.
fn is_second_argument_of_bindable_object_define_property_call(
    request: &Request<'_, '_>,
    node: NodeId,
) -> bool {
    let Some(call) = request.state.binder.node_record(node).parent else {
        return false;
    };
    let NodeData::CallExpression(data) = &request.state.binder.node_record(call).data else {
        return false;
    };
    let source = request.state.binder.source_of_node(call);
    tsc_binder::assignment::is_bindable_object_define_property_call(source, call)
        && data
            .arguments
            .and_then(|arguments| source.arena.node_array(arguments).nodes.get(1).copied())
            == Some(node)
}

/// tsgo `getParentSymbolsOfPropertyAccess`: at the name of a property
/// access, the classes and interfaces of the object's type.
fn get_parent_symbols_of_property_access(
    request: &mut Request<'_, '_>,
    location: Touching,
    symbol: SymbolId,
) -> CheckResult<Vec<SymbolId>> {
    let Touching::Node(id) = location else {
        return Ok(Vec::new());
    };
    if !request.is_right_side_of_property_access(id) {
        return Ok(Vec::new());
    }
    let parent = request
        .state
        .binder
        .node_record(id)
        .parent
        .expect("a property access");
    let NodeData::PropertyAccessExpression(data) = &request.state.binder.node_record(parent).data
    else {
        return Ok(Vec::new());
    };
    let Some(expression) = data.expression else {
        return Ok(Vec::new());
    };
    let lhs_type = request.state.get_type_at_location(expression)?;
    let possible_types = if request
        .state
        .tables
        .flags_of(lhs_type)
        .intersects(TypeFlags::UNION_OR_INTERSECTION)
    {
        request.state.get_types_of_type(lhs_type)
    } else if request.state.tables.type_of(lhs_type).symbol
        != request.state.binder.symbol(symbol).parent
    {
        vec![lhs_type]
    } else {
        Vec::new()
    };
    Ok(possible_types
        .into_iter()
        .filter_map(|ty| {
            request
                .state
                .tables
                .type_of(ty)
                .symbol
                .filter(|&type_symbol| {
                    request
                        .state
                        .symbol_flags(type_symbol)
                        .intersects(SymbolFlags::CLASS | SymbolFlags::INTERFACE)
                })
        })
        .collect())
}

/// tsgo `getPropertySymbolsFromBaseTypes`: the properties named so of the
/// base types (each root symbol), in order.
fn get_property_symbols_from_base_types(
    request: &mut Request<'_, '_>,
    symbol: SymbolId,
    property_name: EscapedName,
) -> CheckResult<Vec<SymbolId>> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    property_symbols_from_base_types_worker(
        request,
        symbol,
        property_name,
        &mut seen,
        &mut result,
    )?;
    Ok(result)
}

fn property_symbols_from_base_types_worker(
    request: &mut Request<'_, '_>,
    symbol: SymbolId,
    property_name: EscapedName,
    seen: &mut HashSet<SymbolId>,
    result: &mut Vec<SymbolId>,
) -> CheckResult<()> {
    // Use `addToSeen` to ensure we don't infinitely recurse in this situation:
    //      interface C extends C {
    //          /*findRef*/propName: string;
    //      }
    if !request
        .state
        .symbol_flags(symbol)
        .intersects(SymbolFlags::CLASS | SymbolFlags::INTERFACE)
        || !seen.insert(symbol)
    {
        return Ok(());
    }
    for declaration in request.declarations_of(symbol) {
        let source = request.state.binder.source_of_node(declaration);
        for type_reference in node_util::get_all_super_type_nodes(source, declaration) {
            let property_type = request.state.get_type_at_location(type_reference)?;
            let Some(type_symbol) = request.state.tables.type_of(property_type).symbol else {
                continue;
            };
            // Visit the typeReference as well to see if it directly or indirectly uses that property
            if let Some(property_symbol) = request
                .state
                .get_property_of_type_full(property_type, property_name)?
            {
                result.extend(request.state.get_root_symbols(property_symbol)?);
            }
            property_symbols_from_base_types_worker(
                request,
                type_symbol,
                property_name,
                seen,
                result,
            )?;
        }
    }
    Ok(())
}

/// tsgo `isStaticSymbol`.
fn is_static_symbol(request: &Request<'_, '_>, symbol: SymbolId) -> bool {
    request
        .state
        .binder
        .symbol(symbol)
        .value_declaration
        .is_some_and(|declaration| {
            modifier_flags(request, declaration).intersects(ModifierFlags::STATIC)
        })
}

/// tsgo `getPropertySymbolFromBindingElement`.
fn get_property_symbol_from_binding_element(
    request: &mut Request<'_, '_>,
    binding_element: NodeId,
) -> CheckResult<Option<SymbolId>> {
    let Some(pattern) = request.state.binder.node_record(binding_element).parent else {
        return Ok(None);
    };
    let type_of_pattern = request.state.get_type_at_location(pattern)?;
    let Some(name) = request.name_of(binding_element) else {
        return Ok(None);
    };
    let text = node_util::id_text(request.state.binder.source_of_node(name), name)
        .unwrap_or_default()
        .to_owned();
    request
        .state
        .get_property_of_type_full(type_of_pattern, &text)
}

/// tsgo `getPropertySymbolOfObjectBindingPatternWithoutPropertyName`.
pub(super) fn get_property_symbol_of_object_binding_pattern_without_property_name(
    request: &mut Request<'_, '_>,
    symbol: SymbolId,
) -> CheckResult<Option<SymbolId>> {
    let binding_element = request
        .declarations_of(symbol)
        .into_iter()
        .find(|&declaration| request.node_kind(declaration) == SyntaxKind::BindingElement);
    match binding_element {
        Some(binding_element)
            if is_object_binding_element_without_property_name(request, binding_element) =>
        {
            get_property_symbol_from_binding_element(request, binding_element)
        }
        _ => Ok(None),
    }
}

/// tsgo `isObjectBindingElementWithoutPropertyName`.
fn is_object_binding_element_without_property_name(
    request: &Request<'_, '_>,
    binding_element: NodeId,
) -> bool {
    let NodeData::BindingElement(data) = &request.state.binder.node_record(binding_element).data
    else {
        return false;
    };
    request
        .state
        .binder
        .node_record(binding_element)
        .parent
        .is_some_and(|pattern| request.node_kind(pattern) == SyntaxKind::ObjectBindingPattern)
        && data
            .name
            .is_some_and(|name| request.node_kind(name) == SyntaxKind::Identifier)
        && data.property_name.is_none()
}

/// tsgo `getContainingNodeIfInHeritageClause`.
fn get_containing_node_if_in_heritage_clause(
    request: &Request<'_, '_>,
    node: NodeId,
) -> Option<NodeId> {
    let mut node = node;
    loop {
        match request.node_kind(node) {
            SyntaxKind::Identifier
            | SyntaxKind::QualifiedName
            | SyntaxKind::PropertyAccessExpression => {
                node = request.state.binder.node_record(node).parent?;
            }
            SyntaxKind::ExpressionWithTypeArguments | SyntaxKind::TypeReference => {
                let clause = request.state.binder.node_record(node).parent?;
                if request.node_kind(clause) != SyntaxKind::HeritageClause {
                    return None;
                }
                let declaration = request.state.binder.node_record(clause).parent?;
                let kind = request.node_kind(declaration);
                return (is_class_like(kind) || kind == SyntaxKind::InterfaceDeclaration)
                    .then_some(declaration);
            }
            _ => return None,
        }
    }
}

/// tsgo `isImplementation`.
fn is_implementation(request: &Request<'_, '_>, node: NodeId) -> bool {
    let source = request.state.binder.source_of_node(node);
    let kind = request.node_kind(node);
    if tsc_types::NodeFlags::from_bits(request.state.binder.node_record(node).flags)
        .intersects(tsc_types::NodeFlags::AMBIENT)
    {
        return !matches!(
            kind,
            SyntaxKind::InterfaceDeclaration | SyntaxKind::TypeAliasDeclaration
        );
    }
    if is_variable_like(kind) {
        return initializer_of(request, node).is_some();
    }
    if is_function_like_declaration(kind) {
        return node_util::body_of(source, node).is_some();
    }
    is_class_like(kind)
        || matches!(
            kind,
            SyntaxKind::ModuleDeclaration | SyntaxKind::EnumDeclaration
        )
}

/// tsgo `isImplementationExpression`.
fn is_implementation_expression(request: &Request<'_, '_>, node: NodeId) -> bool {
    match &request.state.binder.node_record(node).data {
        NodeData::ParenthesizedExpression(data) => data
            .expression
            .is_some_and(|expression| is_implementation_expression(request, expression)),
        NodeData::ArrowFunction(_)
        | NodeData::FunctionExpression(_)
        | NodeData::ObjectLiteralExpression(_)
        | NodeData::ClassExpression(_)
        | NodeData::ArrayLiteralExpression(_) => true,
        _ => false,
    }
}

/// tsgo `getNonModuleSymbolOfMergedModuleSymbol`.
fn get_non_module_symbol_of_merged_module_symbol(
    request: &Request<'_, '_>,
    symbol: SymbolId,
) -> Option<SymbolId> {
    let declarations = request.declarations_of(symbol);
    if declarations.is_empty()
        || !request
            .state
            .symbol_flags(symbol)
            .intersects(SymbolFlags::MODULE | SymbolFlags::TRANSIENT)
    {
        return None;
    }
    let declaration = declarations.into_iter().find(|&declaration| {
        !matches!(
            request.node_kind(declaration),
            SyntaxKind::SourceFile | SyntaxKind::ModuleDeclaration
        )
    })?;
    request.state.binder.node_symbol(declaration)
}

/// tsgo `ast.SymbolName`: a private class element's written name, else
/// the symbol's name.
fn ast_symbol_name(request: &Request<'_, '_>, symbol: SymbolId) -> String {
    if let Some(value_declaration) = request.state.binder.symbol(symbol).value_declaration {
        if is_private_identifier_class_element_declaration(request, value_declaration) {
            let name = request.name_of(value_declaration).expect("a private name");
            return node_util::id_text(request.state.binder.source_of_node(name), name)
                .unwrap_or_default()
                .to_owned();
        }
    }
    symbol_name(request, symbol)
}

/// tsgo `ast.IsPrivateIdentifierClassElementDeclaration`.
fn is_private_identifier_class_element_declaration(
    request: &Request<'_, '_>,
    node: NodeId,
) -> bool {
    matches!(
        request.node_kind(node),
        SyntaxKind::PropertyDeclaration
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
    ) && request
        .name_of(node)
        .is_some_and(|name| request.node_kind(name) == SyntaxKind::PrivateIdentifier)
}

/// tsgo `Node.ModifierFlags` (tsgo's modifier list holds the JSDoc
/// modifiers it reparses in JavaScript).
fn modifier_flags(request: &Request<'_, '_>, node: NodeId) -> ModifierFlags {
    node_util::get_effective_modifier_flags(request.state.binder.source_of_node(node), node)
}

/// tsgo `ast.IsStatic`.
fn is_static(request: &Request<'_, '_>, node: NodeId) -> bool {
    let kind = request.node_kind(node);
    crate::utilities::is_class_element(kind)
        && modifier_flags(request, node).intersects(ModifierFlags::STATIC)
        || kind == SyntaxKind::ClassStaticBlockDeclaration
}

/// tsgo `ast.IsExternalModule` of a file node.
fn is_external_module(request: &Request<'_, '_>, file: NodeId) -> bool {
    request
        .state
        .binder
        .source_of_node(file)
        .external_module_indicator
        .is_some()
}

/// tsgo `isParameterName` (getReferencesForThisKeyword).
fn is_parameter_name(request: &Request<'_, '_>, node: NodeId) -> bool {
    request.node_kind(node) == SyntaxKind::Identifier
        && request
            .state
            .binder
            .node_record(node)
            .parent
            .is_some_and(|parent| {
                request.node_kind(parent) == SyntaxKind::Parameter
                    && request.name_of(parent) == Some(node)
            })
}

/// tsgo `isThis`.
fn is_this(request: &Request<'_, '_>, node: Touching) -> bool {
    match request.kind(node) {
        // case ast.KindThisType: TODO: GH#9267
        SyntaxKind::ThisKeyword => true,
        // 'this' as a parameter
        SyntaxKind::Identifier => {
            let Touching::Node(id) = node else {
                return false;
            };
            node_util::id_text(request.state.binder.source_of_node(id), id) == Some("this")
                && request
                    .parent(node)
                    .is_some_and(|parent| request.node_kind(parent) == SyntaxKind::Parameter)
        }
        _ => false,
    }
}

/// tsgo `isLabelOfLabeledStatement`.
fn is_label_of_labeled_statement(request: &Request<'_, '_>, node: NodeId) -> bool {
    request.node_kind(node) == SyntaxKind::Identifier
        && request
            .state
            .binder
            .node_record(node)
            .parent
            .is_some_and(|parent| {
                matches!(
                    &request.state.binder.node_record(parent).data,
                    NodeData::LabeledStatement(data) if data.label == Some(node)
                )
            })
}

/// tsgo `isReadonlyTypeOperator`.
fn is_readonly_type_operator(request: &Request<'_, '_>, node: Touching) -> bool {
    request.kind(node) == SyntaxKind::ReadonlyKeyword
        && request.parent(node).is_some_and(|parent| {
            matches!(
                &request.state.binder.node_record(parent).data,
                NodeData::TypeOperator(data) if data.operator == SyntaxKind::ReadonlyKeyword
            )
        })
}

/// tsgo `isTypeKeyword`.
fn is_type_keyword(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::AnyKeyword
            | SyntaxKind::AssertsKeyword
            | SyntaxKind::BigIntKeyword
            | SyntaxKind::BooleanKeyword
            | SyntaxKind::FalseKeyword
            | SyntaxKind::InferKeyword
            | SyntaxKind::KeyOfKeyword
            | SyntaxKind::NeverKeyword
            | SyntaxKind::NullKeyword
            | SyntaxKind::NumberKeyword
            | SyntaxKind::ObjectKeyword
            | SyntaxKind::ReadonlyKeyword
            | SyntaxKind::StringKeyword
            | SyntaxKind::SymbolKeyword
            | SyntaxKind::TypeOfKeyword
            | SyntaxKind::TrueKeyword
            | SyntaxKind::VoidKeyword
            | SyntaxKind::UndefinedKeyword
            | SyntaxKind::UniqueKeyword
            | SyntaxKind::UnknownKeyword
    )
}

/// tsgo `ast.IsStringLiteralLike`.
fn is_string_literal_like(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::StringLiteral | SyntaxKind::NoSubstitutionTemplateLiteral
    )
}

/// tsgo `ast.IsClassLike`.
fn is_class_like(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
    )
}

/// tsgo `isMethodOrAccessor` (findallreferences.go).
fn is_method_or_accessor(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::MethodDeclaration | SyntaxKind::GetAccessor | SyntaxKind::SetAccessor
    )
}

/// tsgo `ast.IsVariableLike`.
fn is_variable_like(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::BindingElement
            | SyntaxKind::EnumMember
            | SyntaxKind::Parameter
            | SyntaxKind::PropertyAssignment
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::PropertySignature
            | SyntaxKind::ShorthandPropertyAssignment
            | SyntaxKind::VariableDeclaration
    )
}

/// tsgo `ast.IsFunctionLikeDeclaration`.
fn is_function_like_declaration(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ArrowFunction
            | SyntaxKind::Constructor
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::FunctionExpression
            | SyntaxKind::GetAccessor
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::SetAccessor
    )
}

/// tsgo `Node.Initializer` of the kinds `ast.HasInitializer` accepts.
fn initializer_of(request: &Request<'_, '_>, node: NodeId) -> Option<NodeId> {
    match &request.state.binder.node_record(node).data {
        NodeData::VariableDeclaration(data) => data.initializer,
        NodeData::Parameter(data) => data.initializer,
        NodeData::BindingElement(data) => data.initializer,
        NodeData::PropertyDeclaration(data) => data.initializer,
        NodeData::PropertyAssignment(data) => data.initializer,
        NodeData::EnumMember(data) => data.initializer,
        NodeData::ForStatement(data) => data.initializer,
        NodeData::ForInStatement(data) => data.initializer,
        NodeData::ForOfStatement(data) => data.initializer,
        NodeData::JsxAttribute(data) => data.initializer,
        _ => None,
    }
}

/// tsgo `Node.Type`: the checker's (annotations and JSDoc types), and the
/// type of an assertion, a `satisfies` expression or a type alias.
fn node_type(request: &Request<'_, '_>, node: NodeId) -> Option<NodeId> {
    match &request.state.binder.node_record(node).data {
        NodeData::AsExpression(data) => data.r#type,
        NodeData::SatisfiesExpression(data) => data.r#type,
        NodeData::TypeAssertionExpression(data) => data.r#type,
        NodeData::TypeAliasDeclaration(data) => data.r#type,
        _ => request.state.type_node_of(node),
    }
}

/// A string or numeric literal's text.
fn literal_text(request: &Request<'_, '_>, node: NodeId) -> String {
    match &request.state.binder.node_record(node).data {
        NodeData::StringLiteral(data) => data.text.to_string_lossy().into_owned(),
        NodeData::NoSubstitutionTemplateLiteral(data) => data.text.to_string_lossy().into_owned(),
        NodeData::NumericLiteral(data) => data.text.clone(),
        _ => String::new(),
    }
}

/// tsgo `printer.RangeIsOnSingleLine` of a node's range.
fn range_is_on_single_line(source: &SourceFile, node: NodeId) -> bool {
    let record = source.arena.node(node);
    tsc_emitter::range_is_on_single_line(source, record.pos, record.end)
}

/// tsgo `ast.IsNewExpressionTarget(node, false, false)`.
fn is_new_expression_target(request: &Request<'_, '_>, node: Touching) -> bool {
    is_callee(request, node, SyntaxKind::NewExpression)
}

/// tsgo `isCalleeWorker` without element access or outer expressions: the
/// node (past a property access it is the name of) is the expression of a
/// call or `new` expression of `kind`.
fn is_callee(request: &Request<'_, '_>, node: Touching, kind: SyntaxKind) -> bool {
    let Touching::Node(id) = node else {
        return false;
    };
    let target = if request.is_right_side_of_property_access(id) {
        request
            .state
            .binder
            .node_record(id)
            .parent
            .expect("a property access")
    } else {
        id
    };
    let Some(parent) = request.state.binder.node_record(target).parent else {
        return false;
    };
    if request.node_kind(parent) != kind {
        return false;
    }
    let expression = match &request.state.binder.node_record(parent).data {
        NodeData::CallExpression(data) => data.expression,
        NodeData::NewExpression(data) => data.expression,
        _ => None,
    };
    expression == Some(target)
}

/// tsgo `tryGetClassByExtendingIdentifier`.
fn try_get_class_by_extending_identifier(
    request: &Request<'_, '_>,
    node: Touching,
) -> Option<NodeId> {
    let Touching::Node(id) = node else {
        return None;
    };
    let climbed = if request.is_right_side_of_property_access(id) {
        request.state.binder.node_record(id).parent?
    } else {
        id
    };
    let element = request.state.binder.node_record(climbed).parent?;
    if request.node_kind(element) != SyntaxKind::ExpressionWithTypeArguments {
        return None;
    }
    let clause = request.state.binder.node_record(element).parent?;
    let NodeData::HeritageClause(data) = &request.state.binder.node_record(clause).data else {
        return None;
    };
    let class = request.state.binder.node_record(clause).parent?;
    (is_class_like(request.node_kind(class)) && data.token != SyntaxKind::ImplementsKeyword)
        .then_some(class)
}

/// tsgo `getClassConstructorSymbol`.
fn get_class_constructor_symbol(
    request: &Request<'_, '_>,
    class_symbol: SymbolId,
) -> Option<SymbolId> {
    request
        .state
        .binder
        .symbol(class_symbol)
        .members()
        .get(EscapedName::internal(InternalSymbolName::CONSTRUCTOR))
        .copied()
}

/// tsgo `hasOwnConstructor`.
fn has_own_constructor(request: &Request<'_, '_>, class_declaration: NodeId) -> bool {
    request
        .state
        .binder
        .node_symbol(class_declaration)
        .and_then(|symbol| get_class_constructor_symbol(request, symbol))
        .is_some()
}

/// tsgo `findOwnConstructorReferences`: the class's `constructor` keywords
/// and the `new this` of its static methods.
fn find_own_constructor_references(
    request: &Request<'_, '_>,
    class_symbol: SymbolId,
    add: &mut Vec<Touching>,
) {
    if let Some(constructor_symbol) = get_class_constructor_symbol(request, class_symbol) {
        for declaration in request.declarations_of(constructor_symbol) {
            if request.node_kind(declaration) == SyntaxKind::Constructor {
                // tsgo scans with the reference's file, which is the class's.
                let source = request.state.binder.source_of_node(declaration);
                if let Some(keyword) = Navigator::new(source)
                    .child_of_kind(View::Node(declaration), SyntaxKind::ConstructorKeyword)
                    .and_then(touching)
                {
                    add.push(keyword);
                }
            }
        }
    }

    let exports = request.state.symbol_exports(class_symbol);
    for (_, &member) in exports.iter() {
        let Some(declaration) = request.state.binder.symbol(member).value_declaration else {
            continue;
        };
        if request.node_kind(declaration) != SyntaxKind::MethodDeclaration {
            continue;
        }
        let member_source = request.state.binder.source_of_node(declaration);
        if let Some(body) = node_util::body_of(member_source, declaration) {
            let mut this_keywords = Vec::new();
            for_each_descendant_of_kind(
                member_source,
                body,
                SyntaxKind::ThisKeyword,
                &mut this_keywords,
            );
            for this_keyword in this_keywords {
                if is_new_expression_target(request, Touching::Node(this_keyword)) {
                    add.push(Touching::Node(this_keyword));
                }
            }
        }
    }
}

/// tsgo `findSuperConstructorAccesses`: the `super(...)` calls of the
/// class's constructor.
fn find_super_constructor_accesses(
    request: &Request<'_, '_>,
    class_declaration: NodeId,
    add: &mut Vec<Touching>,
) {
    let Some(constructor_symbol) = request
        .state
        .binder
        .node_symbol(class_declaration)
        .and_then(|symbol| get_class_constructor_symbol(request, symbol))
    else {
        return;
    };
    for declaration in request.declarations_of(constructor_symbol) {
        if request.node_kind(declaration) != SyntaxKind::Constructor {
            continue;
        }
        let source = request.state.binder.source_of_node(declaration);
        if let Some(body) = node_util::body_of(source, declaration) {
            let mut super_keywords = Vec::new();
            for_each_descendant_of_kind(
                source,
                body,
                SyntaxKind::SuperKeyword,
                &mut super_keywords,
            );
            for node in super_keywords {
                if is_callee(request, Touching::Node(node), SyntaxKind::CallExpression) {
                    add.push(Touching::Node(node));
                }
            }
        }
    }
}

/// tsgo `forEachDescendantOfKind`.
fn for_each_descendant_of_kind(
    source: &SourceFile,
    node: NodeId,
    kind: SyntaxKind,
    found: &mut Vec<NodeId>,
) {
    let mut children = Vec::new();
    tsc_syntax::for_each_child(&source.arena, source.arena.node(node), |child| {
        children.push(child);
        false
    });
    for child in children {
        if source.arena.node(child).kind == kind {
            found.push(child);
        }
        for_each_descendant_of_kind(source, child, kind, found);
    }
}

/// The `this` keywords of a static member's body, not of nested functions
/// or classes (tsgo `addClassStaticThisReferences`).
fn collect_static_this_keywords(source: &SourceFile, node: NodeId, found: &mut Vec<NodeId>) {
    let kind = source.arena.node(node).kind;
    if kind == SyntaxKind::ThisKeyword {
        found.push(node);
    } else if !crate::utilities::is_function_like_kind(kind) && !is_class_like(kind) {
        let mut children = Vec::new();
        tsc_syntax::for_each_child(&source.arena, source.arena.node(node), |child| {
            children.push(child);
            false
        });
        for child in children {
            collect_static_this_keywords(source, child, found);
        }
    }
}

/// tsgo `classLike.Members()`.
fn class_members(request: &Request<'_, '_>, class_like: NodeId) -> Vec<NodeId> {
    let source = request.state.binder.source_of_node(class_like);
    let members = match &source.arena.node(class_like).data {
        NodeData::ClassDeclaration(data) => data.members,
        NodeData::ClassExpression(data) => data.members,
        _ => None,
    };
    members
        .map(|members| source.arena.node_array(members).nodes.to_vec())
        .unwrap_or_default()
}

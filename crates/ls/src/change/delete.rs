//! tsgo `ls/change/delete.go`: deleting a node with what goes with it (a
//! list's separator, an import's declaration, a lone variable's statement).

use tsc_syntax::{skip_trivia_ex, NodeData, NodeId, SkipTriviaOptions, SourceFile, SyntaxKind};

use super::{line_start_of, LeadingTriviaOption, Tracker, TrailingTriviaOption};
use crate::astnav::{Found, Navigator};
use crate::format::{self, File};
use crate::tree::View;

impl<'a> Tracker<'a> {
    /// tsgo `finishDeleteDeclarations`: each queued node not inside another
    /// one deleted, then the comma before a deleted last list item.
    pub(super) fn finish_delete_declarations(&mut self) {
        let deleted = self.deleted_nodes.clone();
        let mut deleted_in_lists = Vec::new();
        for &(index, node) in &deleted {
            let file = self.files[index].0;
            // Skip if this node is contained within another deleted node
            let contained = deleted.iter().any(|&(other_index, other)| {
                other_index == index
                    && other != node
                    && range_contains_range_exclusive(file, other, node)
            });
            if contained {
                continue;
            }
            delete_declaration(self, &mut deleted_in_lists, file, node);
        }
        // Handle trailing commas for last elements in lists
        for &(index, node) in &deleted_in_lists {
            let file = self.files[index].0;
            let formatting = File::new(file);
            let Some(list) =
                format::get_containing_list(&formatting, Found::Node(View::Node(node)))
            else {
                continue;
            };
            let nodes = file.arena.node_array(list.id).nodes;
            if nodes.last() != Some(&node) {
                continue;
            }
            let last_non_deleted = (0..nodes.len() - 1)
                .rev()
                .find(|&item| !deleted_in_lists.contains(&(index, nodes[item])));
            if let Some(item) = last_non_deleted {
                let start = file.arena.node(nodes[item]).end as usize;
                let end = self.start_position_to_delete_node_in_list(file, nodes[item + 1]);
                self.replace_range_with_text(file, (start, end), String::new());
            }
        }
    }

    /// tsgo `Delete` of a `type` or `function` keyword (tsgo's
    /// `deleteDeclaration`): the keyword and the white space after it. A
    /// token is no node here, so it goes now rather than with the queued
    /// nodes.
    pub(crate) fn delete_keyword(&mut self, file: &'a SourceFile, keyword: Found) {
        let navigator = Navigator::new(file);
        let start = self.get_adjusted_start_position_of(
            file,
            keyword,
            &navigator,
            LeadingTriviaOption::Exclude,
            false,
        );
        let end = self.get_adjusted_end_position_of(
            file,
            navigator.end(keyword) as usize,
            TrailingTriviaOption::Include,
        );
        self.replace_range_with_text(file, (start, end), String::new());
    }

    /// tsgo `startPositionToDeleteNodeInList`: the first position in the
    /// node's leading trivia that is not white space.
    fn start_position_to_delete_node_in_list(&self, file: &SourceFile, node: NodeId) -> usize {
        let start =
            self.get_adjusted_start_position(file, node, LeadingTriviaOption::IncludeAll, false);
        skip_trivia_ex(
            file.text(),
            start,
            SkipTriviaOptions {
                stop_at_comments: true,
                ..Default::default()
            },
        )
    }

    /// tsgo `endPositionToDeleteNodeInList`.
    fn end_position_to_delete_node_in_list(
        &self,
        file: &SourceFile,
        node: NodeId,
        previous: Option<NodeId>,
        next: NodeId,
    ) -> usize {
        let end = self.start_position_to_delete_node_in_list(file, next);
        let navigator = Navigator::new(file);
        let start_of = |node: Found| navigator.start_of_node(node, false) as usize;
        if previous.is_none()
            || positions_are_on_same_line(
                file,
                self.get_adjusted_end_position(file, node, TrailingTriviaOption::Include),
                end,
            )
        {
            return end;
        }
        let token = navigator.preceding_token(start_of(Found::Node(View::Node(next))) as u32);
        if is_separator(file, &navigator, node, token) {
            let previous = previous.expect("checked above");
            let previous_token =
                navigator.preceding_token(start_of(Found::Node(View::Node(node))) as u32);
            if is_separator(file, &navigator, previous, previous_token) {
                let token = token.expect("a separator");
                let previous_token = previous_token.expect("a separator");
                let pos = skip_trivia_ex(
                    file.text(),
                    navigator.end(token) as usize,
                    SkipTriviaOptions {
                        stop_after_line_break: true,
                        stop_at_comments: true,
                        ..Default::default()
                    },
                );
                let bytes = file.text().as_bytes();
                if positions_are_on_same_line(file, start_of(previous_token), start_of(token)) {
                    if pos > 0 && matches!(bytes[pos - 1], b'\n' | b'\r') {
                        return pos - 1;
                    }
                    return pos;
                }
                if bytes
                    .get(pos)
                    .is_some_and(|&byte| matches!(byte, b'\n' | b'\r'))
                {
                    return pos;
                }
            }
        }
        end
    }
}

/// tsgo `deleteDeclaration`.
fn delete_declaration<'a>(
    tracker: &mut Tracker<'a>,
    deleted_in_lists: &mut Vec<(usize, NodeId)>,
    file: &'a SourceFile,
    node: NodeId,
) {
    let record = file.arena.node(node);
    let navigator = Navigator::new(file);
    match record.kind {
        SyntaxKind::Parameter => {
            let function = record.parent.expect("a parameter has a parent");
            let lone_arrow_parameter = match &file.arena.node(function).data {
                NodeData::ArrowFunction(data) => {
                    data.parameters
                        .is_some_and(|list| file.arena.node_array(list).nodes.len() == 1)
                        && navigator
                            .child_of_kind(View::Node(function), SyntaxKind::OpenParenToken)
                            .is_none()
                }
                _ => false,
            };
            if lone_arrow_parameter {
                // Lambdas with exactly one parameter are special because, after removal, there
                // must be an empty parameter list (i.e. `()`) and this won't necessarily be the
                // case if the parameter is simply removed (e.g. in `x => 1`).
                let range = tracker.get_adjusted_range(
                    file,
                    node,
                    node,
                    LeadingTriviaOption::IncludeAll,
                    TrailingTriviaOption::Include,
                );
                tracker.replace_range_with_text(file, range, "()".to_owned());
            } else {
                delete_node_in_list(tracker, deleted_in_lists, file, node);
            }
        }
        SyntaxKind::ImportDeclaration | SyntaxKind::ImportEqualsDeclaration => {
            let imports = crate::references::collect_external_module_references(file).imports;
            let first_import_statement = statements(file).iter().copied().find(|&statement| {
                matches!(
                    file.arena.node(statement).kind,
                    SyntaxKind::ImportDeclaration | SyntaxKind::ImportEqualsDeclaration
                )
            });
            let is_first_import = imports
                .first()
                .is_some_and(|&import| file.arena.node(import).parent == Some(node))
                || first_import_statement == Some(node);
            // For first import, leave header comment in place, otherwise only delete JSDoc comments
            let leading = if is_first_import {
                LeadingTriviaOption::Exclude
            } else if has_jsdoc_nodes(file, node) {
                LeadingTriviaOption::JSDoc
            } else {
                LeadingTriviaOption::StartLine
            };
            delete_node(tracker, file, node, leading, TrailingTriviaOption::Include);
        }
        SyntaxKind::BindingElement => {
            let pattern = record.parent.expect("a binding element has a parent");
            let preserve_comma = match &file.arena.node(pattern).data {
                NodeData::ArrayBindingPattern(data) => data
                    .elements
                    .is_some_and(|list| file.arena.node_array(list).nodes.last() != Some(&node)),
                _ => false,
            };
            if preserve_comma {
                delete_node(
                    tracker,
                    file,
                    node,
                    LeadingTriviaOption::IncludeAll,
                    TrailingTriviaOption::Exclude,
                );
            } else {
                delete_node_in_list(tracker, deleted_in_lists, file, node);
            }
        }
        SyntaxKind::VariableDeclaration => {
            delete_variable_declaration(tracker, deleted_in_lists, file, node);
        }
        SyntaxKind::TypeParameter => delete_node_in_list(tracker, deleted_in_lists, file, node),
        SyntaxKind::ImportSpecifier => {
            let named_imports = record.parent.expect("an import specifier has a parent");
            let lone = match &file.arena.node(named_imports).data {
                NodeData::NamedImports(data) => data
                    .elements
                    .is_some_and(|list| file.arena.node_array(list).nodes.len() == 1),
                _ => false,
            };
            if lone {
                delete_import_binding(tracker, file, named_imports);
            } else {
                delete_node_in_list(tracker, deleted_in_lists, file, node);
            }
        }
        SyntaxKind::NamespaceImport => delete_import_binding(tracker, file, node),
        SyntaxKind::SemicolonToken => delete_node(
            tracker,
            file,
            node,
            LeadingTriviaOption::IncludeAll,
            TrailingTriviaOption::Exclude,
        ),
        // For type keyword in import clauses, we need to delete the keyword and any trailing space
        // The trailing space is part of the next token's leading trivia, so we include it
        SyntaxKind::TypeKeyword | SyntaxKind::FunctionKeyword => delete_node(
            tracker,
            file,
            node,
            LeadingTriviaOption::Exclude,
            TrailingTriviaOption::Include,
        ),
        SyntaxKind::ClassDeclaration | SyntaxKind::FunctionDeclaration => {
            let leading = if has_jsdoc_nodes(file, node) {
                LeadingTriviaOption::JSDoc
            } else {
                LeadingTriviaOption::StartLine
            };
            delete_node(tracker, file, node, leading, TrailingTriviaOption::Include);
        }
        _ => match record.parent {
            // a misbehaving client can reach here with the SourceFile node
            None => delete_node(
                tracker,
                file,
                node,
                LeadingTriviaOption::IncludeAll,
                TrailingTriviaOption::Include,
            ),
            Some(parent) => match &file.arena.node(parent).data {
                NodeData::ImportClause(data) if data.name == Some(node) => {
                    delete_default_import(tracker, file, parent);
                }
                NodeData::CallExpression(data)
                    if data
                        .arguments
                        .is_some_and(|list| file.arena.node_array(list).nodes.contains(&node)) =>
                {
                    delete_node_in_list(tracker, deleted_in_lists, file, node);
                }
                _ => delete_node(
                    tracker,
                    file,
                    node,
                    LeadingTriviaOption::IncludeAll,
                    TrailingTriviaOption::Include,
                ),
            },
        },
    }
}

fn statements(file: &SourceFile) -> &[NodeId] {
    match &file.arena.node(file.root).data {
        NodeData::SourceFile(data) => data
            .statements
            .map(|list| file.arena.node_array(list).nodes)
            .unwrap_or_default(),
        _ => &[],
    }
}

/// tsgo `deleteDefaultImport`.
fn delete_default_import<'a>(
    tracker: &mut Tracker<'a>,
    file: &'a SourceFile,
    import_clause: NodeId,
) {
    let NodeData::ImportClause(clause) = &file.arena.node(import_clause).data else {
        return;
    };
    let declaration = file.arena.node(import_clause).parent;
    let (Some(name), Some(_)) = (clause.name, clause.named_bindings) else {
        // Delete the whole import
        if let Some(declaration) = declaration {
            delete_node(
                tracker,
                file,
                declaration,
                LeadingTriviaOption::IncludeAll,
                TrailingTriviaOption::Include,
            );
        }
        return;
    };
    // import |d,| * as ns from './file'
    let navigator = Navigator::new(file);
    let start = navigator.start_of_node(Found::Node(View::Node(name)), false) as usize;
    let next_token = navigator.token_at(file.arena.node(name).end);
    if navigator.kind(next_token) == SyntaxKind::CommaToken {
        // shift first non-whitespace position after comma to the start position of the node
        let end = skip_trivia_ex(
            file.text(),
            navigator.end(next_token) as usize,
            SkipTriviaOptions {
                stop_at_comments: true,
                ..Default::default()
            },
        );
        tracker.replace_range_with_text(file, (start, end), String::new());
    } else {
        delete_node(
            tracker,
            file,
            name,
            LeadingTriviaOption::IncludeAll,
            TrailingTriviaOption::Include,
        );
    }
}

/// tsgo `deleteImportBinding`.
fn delete_import_binding<'a>(tracker: &mut Tracker<'a>, file: &'a SourceFile, node: NodeId) {
    let Some(clause) = file.arena.node(node).parent else {
        return;
    };
    let has_name = matches!(
        &file.arena.node(clause).data,
        NodeData::ImportClause(data) if data.name.is_some()
    );
    if has_name {
        // Delete named imports while preserving the default import
        // import d|, * as ns| from './file'
        // import d|, { a }| from './file'
        let navigator = Navigator::new(file);
        let previous_token = navigator.token_at(file.arena.node(node).pos.saturating_sub(1));
        let start = navigator.start_of_node(previous_token, false) as usize;
        tracker.replace_range_with_text(
            file,
            (start, file.arena.node(node).end as usize),
            String::new(),
        );
    } else {
        // Delete the entire import declaration
        // |import * as ns from './file'|
        // |import { a } from './file'|
        let mut ancestor = Some(node);
        while let Some(current) = ancestor {
            if file.arena.node(current).kind == SyntaxKind::ImportDeclaration {
                delete_node(
                    tracker,
                    file,
                    current,
                    LeadingTriviaOption::IncludeAll,
                    TrailingTriviaOption::Include,
                );
                return;
            }
            ancestor = file.arena.node(current).parent;
        }
    }
}

/// tsgo `deleteVariableDeclaration`.
fn delete_variable_declaration<'a>(
    tracker: &mut Tracker<'a>,
    deleted_in_lists: &mut Vec<(usize, NodeId)>,
    file: &'a SourceFile,
    node: NodeId,
) {
    let Some(parent) = file.arena.node(node).parent else {
        return;
    };
    if file.arena.node(parent).kind == SyntaxKind::CatchClause {
        // TODO: There's currently no unused diagnostic for this, could be a suggestion
        let navigator = Navigator::new(file);
        let open = navigator.child_of_kind(View::Node(parent), SyntaxKind::OpenParenToken);
        let close = navigator.child_of_kind(View::Node(parent), SyntaxKind::CloseParenToken);
        if let (Some(open), Some(close)) = (open, close) {
            // tsgo `DeleteNodeRange(openParen, closeParen, IncludeAll, Include)`.
            let start = tracker.get_adjusted_start_position_of(
                file,
                open,
                &navigator,
                LeadingTriviaOption::IncludeAll,
                false,
            );
            let end = tracker.get_adjusted_end_position_of(
                file,
                navigator.end(close) as usize,
                TrailingTriviaOption::Include,
            );
            tracker.replace_range_with_text(file, (start, end), String::new());
        }
        return;
    }
    let lone = match &file.arena.node(parent).data {
        NodeData::VariableDeclarationList(data) => data
            .declarations
            .is_some_and(|list| file.arena.node_array(list).nodes.len() == 1),
        _ => false,
    };
    if !lone {
        delete_node_in_list(tracker, deleted_in_lists, file, node);
        return;
    }
    let Some(grandparent) = file.arena.node(parent).parent else {
        return;
    };
    match file.arena.node(grandparent).kind {
        SyntaxKind::ForOfStatement | SyntaxKind::ForInStatement => {
            let source = tracker.source(file);
            let mut factory = tracker.arena().factory();
            let Ok(elements) = factory.create_node_array(source, Vec::new()) else {
                return;
            };
            let Ok(object) = factory.create_node(
                source,
                NodeData::ObjectLiteralExpression(tsc_syntax::nodes::ObjectLiteralExpressionData {
                    properties: Some(elements.array()),
                }),
                tsc_emitter::TransformFlags::NONE,
            ) else {
                return;
            };
            tracker.replace_node(file, node, object, None);
        }
        SyntaxKind::ForStatement => delete_node(
            tracker,
            file,
            parent,
            LeadingTriviaOption::IncludeAll,
            TrailingTriviaOption::Include,
        ),
        SyntaxKind::VariableStatement => {
            let leading = if has_jsdoc_nodes(file, grandparent) {
                LeadingTriviaOption::JSDoc
            } else {
                LeadingTriviaOption::StartLine
            };
            delete_node(
                tracker,
                file,
                grandparent,
                leading,
                TrailingTriviaOption::Include,
            );
        }
        // tsgo fails here ("Unexpected grandparent kind").
        _ => {}
    }
}

/// tsgo `deleteNode`: deletes comments too.
fn delete_node<'a>(
    tracker: &mut Tracker<'a>,
    file: &'a SourceFile,
    node: NodeId,
    leading: LeadingTriviaOption,
    trailing: TrailingTriviaOption,
) {
    let start = tracker.get_adjusted_start_position(file, node, leading, false);
    let end = tracker.get_adjusted_end_position(file, node, trailing);
    tracker.replace_range_with_text(file, (start, end), String::new());
}

/// tsgo `deleteNodeInList`: the node and the comma after it (a deleted last
/// item's comma before it goes in `finish_delete_declarations`).
fn delete_node_in_list<'a>(
    tracker: &mut Tracker<'a>,
    deleted_in_lists: &mut Vec<(usize, NodeId)>,
    file: &'a SourceFile,
    node: NodeId,
) {
    let formatting = File::new(file);
    let Some(list) = format::get_containing_list(&formatting, Found::Node(View::Node(node))) else {
        return;
    };
    let nodes = file.arena.node_array(list.id).nodes;
    let Some(index) = nodes.iter().position(|&item| item == node) else {
        return;
    };
    if nodes.len() == 1 {
        delete_node(
            tracker,
            file,
            node,
            LeadingTriviaOption::IncludeAll,
            TrailingTriviaOption::Include,
        );
        return;
    }
    // Note: We will only delete a comma *after* a node. This will leave a trailing comma if we delete the last node.
    // That's handled in the end by finishTrailingCommaAfterDeletingNodesInList.
    let file_index = tracker.file_index(file);
    deleted_in_lists.push((file_index, node));
    let start = tracker.start_position_to_delete_node_in_list(file, node);
    let end = if index == nodes.len() - 1 {
        tracker.get_adjusted_end_position(file, node, TrailingTriviaOption::None)
    } else {
        let previous = index.checked_sub(1).map(|previous| nodes[previous]);
        tracker.end_position_to_delete_node_in_list(file, node, previous, nodes[index + 1])
    };
    tracker.replace_range_with_text(file, (start, end), String::new());
}

/// tsgo `rangeContainsRangeExclusive`.
fn range_contains_range_exclusive(file: &SourceFile, outer: NodeId, inner: NodeId) -> bool {
    let outer = file.arena.node(outer);
    let inner = file.arena.node(inner);
    outer.pos < inner.pos && inner.end < outer.end
}

/// tsgo `isSeparator`: a comma, or a semicolon in an object literal.
pub(super) fn is_separator(
    file: &SourceFile,
    navigator: &Navigator<'_>,
    node: NodeId,
    candidate: Option<Found>,
) -> bool {
    let Some(candidate) = candidate else {
        return false;
    };
    let Some(parent) = file.arena.node(node).parent else {
        return false;
    };
    match navigator.kind(candidate) {
        SyntaxKind::CommaToken => true,
        SyntaxKind::SemicolonToken => {
            file.arena.node(parent).kind == SyntaxKind::ObjectLiteralExpression
        }
        _ => false,
    }
}

/// tsgo `positionsAreOnSameLine`.
pub(super) fn positions_are_on_same_line(file: &SourceFile, first: usize, second: usize) -> bool {
    line_start_of(file, first) == line_start_of(file, second)
}

/// tsgo `hasJSDocNodes`.
fn has_jsdoc_nodes(file: &SourceFile, node: NodeId) -> bool {
    file.arena
        .node(node)
        .js_doc
        .is_some_and(|list| !file.arena.node_array(list).nodes.is_empty())
}

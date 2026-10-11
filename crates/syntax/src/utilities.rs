//! tsgo `ast` utilities (internal/ast/utilities.go) the crates share over a
//! parsed tree.

use crate::{NodeArena, NodeData, NodeId, SyntaxKind};

/// tsgo `ast.IsImportCall`: `import(...)`, `import.defer(...)` or
/// `import.source(...)`.
pub fn is_import_call(arena: &NodeArena, node: NodeId) -> bool {
    let NodeData::CallExpression(call) = &arena.node(node).data else {
        return false;
    };
    call.expression.is_some_and(|callee| {
        arena.node(callee).kind == SyntaxKind::ImportKeyword
            || is_import_phase_meta_property(arena, callee)
    })
}

/// tsgo `ast.IsImportPhaseMetaProperty`: `import.defer` or `import.source`.
pub fn is_import_phase_meta_property(arena: &NodeArena, node: NodeId) -> bool {
    is_import_defer_meta_property(arena, node) || is_import_source_meta_property(arena, node)
}

/// tsgo `ast.IsImportDeferMetaProperty`.
pub fn is_import_defer_meta_property(arena: &NodeArena, node: NodeId) -> bool {
    is_import_meta_property(arena, node, "defer")
}

/// tsgo `ast.IsImportSourceMetaProperty`.
pub fn is_import_source_meta_property(arena: &NodeArena, node: NodeId) -> bool {
    is_import_meta_property(arena, node, "source")
}

fn is_import_meta_property(arena: &NodeArena, node: NodeId, name: &str) -> bool {
    matches!(&arena.node(node).data, NodeData::MetaProperty(meta)
        if meta.keyword_token == SyntaxKind::ImportKeyword
            && meta.name.is_some_and(|id| matches!(&arena.node(id).data,
                NodeData::Identifier(identifier) if identifier.escaped_text == name)))
}

/// tsgo `ast.IsSourcePhaseImport`: an import declaration with the `source`
/// phase, or an `import.source(...)` call.
pub fn is_source_phase_import(arena: &NodeArena, node: NodeId) -> bool {
    if let NodeData::ImportDeclaration(declaration) = &arena.node(node).data {
        return declaration.import_clause.is_some_and(|clause| {
            matches!(&arena.node(clause).data, NodeData::ImportClause(clause)
                if clause.phase_modifier == Some(SyntaxKind::SourceKeyword))
        });
    }
    is_source_phase_import_call(arena, node)
}

/// tsgo `ast.IsSourcePhaseImportCall`.
pub fn is_source_phase_import_call(arena: &NodeArena, node: NodeId) -> bool {
    matches!(&arena.node(node).data, NodeData::CallExpression(call)
        if call.expression.is_some_and(|callee| is_import_source_meta_property(arena, callee)))
}

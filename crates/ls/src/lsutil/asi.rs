//! tsgo `ls/lsutil/asi.go`: whether a position or node ends where automatic
//! semicolon insertion could end a statement.

use tsc_syntax::SyntaxKind;

use crate::astnav::{Found, Navigator};
use crate::utilities::is_function_like_kind;

/// tsgo `PositionIsASICandidate`: the ancestor of `context` (itself
/// included) that ends at `pos` and may be an ASI candidate is one.
pub(crate) fn position_is_asi_candidate(
    navigator: &Navigator<'_>,
    line_starts: &[usize],
    pos: usize,
    context: Found,
) -> bool {
    let mut ancestor = Some(context);
    while let Some(node) = ancestor {
        if navigator.end(node) as usize != pos {
            return false;
        }
        if syntax_may_be_asi_candidate(navigator.kind(node)) {
            return node_is_asi_candidate(navigator, line_starts, node);
        }
        ancestor = navigator.parent(node);
    }
    false
}

/// tsgo `SyntaxMayBeASICandidate`.
pub(crate) fn syntax_may_be_asi_candidate(kind: SyntaxKind) -> bool {
    syntax_requires_trailing_comma_or_semicolon_or_asi(kind)
        || syntax_requires_trailing_function_block_or_semicolon_or_asi(kind)
        || syntax_requires_trailing_module_block_or_semicolon_or_asi(kind)
        || syntax_requires_trailing_semicolon_or_asi(kind)
}

/// tsgo `SyntaxRequiresTrailingCommaOrSemicolonOrASI`.
fn syntax_requires_trailing_comma_or_semicolon_or_asi(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::CallSignature
            | SyntaxKind::ConstructSignature
            | SyntaxKind::IndexSignature
            | SyntaxKind::PropertySignature
            | SyntaxKind::MethodSignature
    )
}

/// tsgo `SyntaxRequiresTrailingFunctionBlockOrSemicolonOrASI`.
fn syntax_requires_trailing_function_block_or_semicolon_or_asi(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::FunctionDeclaration
            | SyntaxKind::Constructor
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
    )
}

/// tsgo `SyntaxRequiresTrailingModuleBlockOrSemicolonOrASI`.
fn syntax_requires_trailing_module_block_or_semicolon_or_asi(kind: SyntaxKind) -> bool {
    kind == SyntaxKind::ModuleDeclaration
}

/// tsgo `SyntaxRequiresTrailingSemicolonOrASI`.
fn syntax_requires_trailing_semicolon_or_asi(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::VariableStatement
            | SyntaxKind::ExpressionStatement
            | SyntaxKind::DoStatement
            | SyntaxKind::ContinueStatement
            | SyntaxKind::BreakStatement
            | SyntaxKind::ReturnStatement
            | SyntaxKind::ThrowStatement
            | SyntaxKind::DebuggerStatement
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::ImportDeclaration
            | SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::ExportDeclaration
            | SyntaxKind::NamespaceExportDeclaration
            | SyntaxKind::ExportAssignment
    )
}

/// tsgo `NodeIsASICandidate`.
pub(crate) fn node_is_asi_candidate(
    navigator: &Navigator<'_>,
    line_starts: &[usize],
    node: Found,
) -> bool {
    let kind = navigator.kind(node);
    let Found::Node(view) = node else {
        return false;
    };
    let last_token = navigator.last_token(view);
    if last_token.is_some_and(|last| navigator.kind(last) == SyntaxKind::SemicolonToken) {
        return false;
    }

    if syntax_requires_trailing_comma_or_semicolon_or_asi(kind) {
        if last_token.is_some_and(|last| navigator.kind(last) == SyntaxKind::CommaToken) {
            return false;
        }
    } else if syntax_requires_trailing_module_block_or_semicolon_or_asi(kind) {
        if navigator
            .last_child(view)
            .is_some_and(|last| navigator.kind(last) == SyntaxKind::ModuleBlock)
        {
            return false;
        }
    } else if syntax_requires_trailing_function_block_or_semicolon_or_asi(kind) {
        if navigator
            .last_child(view)
            .is_some_and(|last| is_function_block(navigator, last))
        {
            return false;
        }
    } else if !syntax_requires_trailing_semicolon_or_asi(kind) {
        return false;
    }

    // See comment in parser's `parseDoStatement`
    if kind == SyntaxKind::DoStatement {
        return true;
    }

    let top_node = crate::tree::View::Node(navigator.file().root);
    let Some(next_token) = navigator.next_token(node, top_node) else {
        return true;
    };
    if navigator.kind(next_token) == SyntaxKind::CloseBraceToken {
        return true;
    }

    let line_of = |pos: u32| match line_starts.binary_search(&(pos as usize)) {
        Ok(line) => line,
        Err(next) => next.saturating_sub(1),
    };
    let start_line = line_of(navigator.end(node));
    let end_line = line_of(navigator.start_of_node(next_token, false));
    start_line != end_line
}

/// tsgo `ast.IsFunctionBlock`.
fn is_function_block(navigator: &Navigator<'_>, node: Found) -> bool {
    navigator.kind(node) == SyntaxKind::Block
        && navigator
            .parent(node)
            .is_some_and(|parent| is_function_like_kind(navigator.kind(parent)))
}

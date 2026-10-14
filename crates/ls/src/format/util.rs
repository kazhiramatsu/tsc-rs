//! tsgo `format/util.go`.

use tsc_syntax::SyntaxKind;

use super::file::{File, List, Node};

/// tsgo `rangeIsOnOneLine`.
pub(super) fn range_is_on_one_line(file: &File<'_>, pos: usize, end: usize) -> bool {
    file.line_of(pos) == file.line_of(end)
}

/// tsgo `getOpenTokenForList`: the token that opens `list` of `node`.
pub(super) fn get_open_token_for_list(file: &File<'_>, node: Node, list: List) -> SyntaxKind {
    let list = Some(list.id);
    match file.kind(node) {
        SyntaxKind::Constructor
        | SyntaxKind::FunctionDeclaration
        | SyntaxKind::FunctionExpression
        | SyntaxKind::MethodDeclaration
        | SyntaxKind::MethodSignature
        | SyntaxKind::ArrowFunction
        | SyntaxKind::CallSignature
        | SyntaxKind::ConstructSignature
        | SyntaxKind::FunctionType
        | SyntaxKind::ConstructorType
        | SyntaxKind::GetAccessor
        | SyntaxKind::SetAccessor => {
            if file.type_parameter_list(node) == list {
                return SyntaxKind::LessThanToken;
            } else if file.parameter_list(node) == list {
                return SyntaxKind::OpenParenToken;
            }
        }
        SyntaxKind::CallExpression | SyntaxKind::NewExpression => {
            if file.type_argument_list(node) == list {
                return SyntaxKind::LessThanToken;
            } else if file.argument_list(node) == list {
                return SyntaxKind::OpenParenToken;
            }
        }
        SyntaxKind::ClassDeclaration
        | SyntaxKind::ClassExpression
        | SyntaxKind::InterfaceDeclaration
        | SyntaxKind::TypeAliasDeclaration => {
            if file.type_parameter_list(node) == list {
                return SyntaxKind::LessThanToken;
            }
        }
        SyntaxKind::TypeReference
        | SyntaxKind::TaggedTemplateExpression
        | SyntaxKind::TypeQuery
        | SyntaxKind::ExpressionWithTypeArguments
        | SyntaxKind::ImportType => {
            if file.type_argument_list(node) == list {
                return SyntaxKind::LessThanToken;
            }
        }
        SyntaxKind::TypeLiteral => return SyntaxKind::OpenBraceToken,
        _ => {}
    }
    SyntaxKind::Unknown
}

/// tsgo `getCloseTokenForOpenToken`.
pub(super) fn get_close_token_for_open_token(kind: SyntaxKind) -> SyntaxKind {
    // TODO: matches strada - seems like it could handle more pairs of braces, though? [] notably missing
    match kind {
        SyntaxKind::OpenParenToken => SyntaxKind::CloseParenToken,
        SyntaxKind::LessThanToken => SyntaxKind::GreaterThanToken,
        SyntaxKind::OpenBraceToken => SyntaxKind::CloseBraceToken,
        _ => SyntaxKind::Unknown,
    }
}

/// tsgo `GetLineStartPositionForPosition`.
pub(crate) fn get_line_start_position_for_position(file: &File<'_>, position: usize) -> usize {
    file.line_starts()[file.line_of(position)]
}

/// tsgo `findImmediatelyPrecedingTokenOfKind`: the token of
/// `expected_token_kind` that ends at `end` (typed where expected, not in a
/// comment).
pub(super) fn find_immediately_preceding_token_of_kind(
    file: &File<'_>,
    end: usize,
    expected_token_kind: SyntaxKind,
) -> Option<Node> {
    file.find_preceding_token(end).filter(|&preceding| {
        file.kind(preceding) == expected_token_kind && file.end(preceding) == end
    })
}

/// tsgo `findOutermostNodeWithinListLevel`: the highest node enclosing
/// `node` at the same list level as `node` and whose end does not exceed
/// `node.end`.
///
/// Consider typing the following
/// ```text
/// let x = 1;
/// while (true) {
/// }
/// ```
/// Upon typing the closing curly, we want to format the entire
/// `while`-statement, but not the preceding variable declaration.
pub(super) fn find_outermost_node_within_list_level(
    file: &File<'_>,
    node: Option<Node>,
) -> Option<Node> {
    let node = node?;
    let mut current = node;
    while let Some(parent) = file.parent(current) {
        if file.end(parent) != file.end(node) || is_list_element(file, parent, current) {
            break;
        }
        current = parent;
    }
    Some(current)
}

/// tsgo `isListElement`: whether `node` is an element of a list of `parent`
/// (a class's members, say).
fn is_list_element(file: &File<'_>, parent: Node, node: Node) -> bool {
    let contained_by = |list: Option<_>| {
        list.is_some_and(|list| {
            let (pos, end) = file.list_range(list);
            pos <= file.pos(node) && file.end(node) <= end
        })
    };
    match file.kind(parent) {
        SyntaxKind::ClassDeclaration | SyntaxKind::InterfaceDeclaration => {
            contained_by(file.member_list(parent))
        }
        SyntaxKind::ModuleDeclaration => file.module_body(parent).is_some_and(|body| {
            file.kind(body) == SyntaxKind::ModuleBlock && contained_by(file.statement_list(body))
        }),
        SyntaxKind::SourceFile | SyntaxKind::Block | SyntaxKind::ModuleBlock => {
            contained_by(file.statement_list(parent))
        }
        SyntaxKind::CatchClause => file
            .catch_block(parent)
            .is_some_and(|block| contained_by(file.statement_list(block))),
        _ => false,
    }
}

/// tsgo `isMemberListElement`.
pub(super) fn is_member_list_element(file: &File<'_>, parent: Node, node: Node) -> bool {
    match file.kind(parent) {
        SyntaxKind::ClassDeclaration
        | SyntaxKind::ClassExpression
        | SyntaxKind::InterfaceDeclaration
        | SyntaxKind::EnumDeclaration
        | SyntaxKind::TypeLiteral
        | SyntaxKind::MappedType => file.member_list(parent).is_some_and(|list| {
            let (pos, end) = file.list_range(list);
            pos <= file.pos(node) && file.end(node) <= end
        }),
        _ => false,
    }
}

//! tsgo `ls/symbols.go`: the protocol kind of a declaration (the document
//! and workspace symbols come with L6 and L7e).

use tsc_binder::assignment::{get_assignment_declaration_kind, AssignmentDeclarationKind};
use tsc_binder::node_util;
use tsc_lsproto::SymbolKind;
use tsc_syntax::{NodeId, SyntaxKind};
use tsc_types::ModifierFlags;

use crate::request::Request;

/// tsgo `getSymbolKindFromNode`.
pub(crate) fn symbol_kind_from_node(request: &Request<'_, '_>, node: NodeId) -> SymbolKind {
    let source = request.state.binder.source_of_node(node);
    match request.node_kind(node) {
        SyntaxKind::SourceFile => {
            if source.external_module_indicator.is_some() {
                SymbolKind::MODULE
            } else {
                SymbolKind::FILE
            }
        }
        SyntaxKind::ModuleDeclaration => SymbolKind::NAMESPACE,
        SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression => SymbolKind::CLASS,
        SyntaxKind::InterfaceDeclaration => SymbolKind::INTERFACE,
        SyntaxKind::TypeAliasDeclaration
        | SyntaxKind::JSDocTypedefTag
        | SyntaxKind::JSDocCallbackTag => SymbolKind::CLASS,
        SyntaxKind::EnumDeclaration => SymbolKind::ENUM,
        SyntaxKind::VariableDeclaration => SymbolKind::VARIABLE,
        SyntaxKind::ArrowFunction
        | SyntaxKind::FunctionDeclaration
        | SyntaxKind::FunctionExpression => SymbolKind::FUNCTION,
        SyntaxKind::GetAccessor | SyntaxKind::SetAccessor => SymbolKind::PROPERTY,
        SyntaxKind::MethodDeclaration | SyntaxKind::MethodSignature => SymbolKind::METHOD,
        SyntaxKind::PropertyDeclaration
        | SyntaxKind::PropertySignature
        | SyntaxKind::PropertyAssignment
        | SyntaxKind::ShorthandPropertyAssignment
        | SyntaxKind::SpreadAssignment
        | SyntaxKind::IndexSignature => SymbolKind::PROPERTY,
        SyntaxKind::CallSignature => SymbolKind::METHOD,
        SyntaxKind::ConstructSignature => SymbolKind::CONSTRUCTOR,
        SyntaxKind::Constructor | SyntaxKind::ClassStaticBlockDeclaration => {
            SymbolKind::CONSTRUCTOR
        }
        SyntaxKind::TypeParameter => SymbolKind::TYPE_PARAMETER,
        SyntaxKind::EnumMember => SymbolKind::ENUM_MEMBER,
        SyntaxKind::Parameter => {
            if node_util::has_syntactic_modifier(
                source,
                node,
                ModifierFlags::PARAMETER_PROPERTY_MODIFIER,
            ) {
                SymbolKind::PROPERTY
            } else {
                SymbolKind::VARIABLE
            }
        }
        SyntaxKind::BinaryExpression | SyntaxKind::CallExpression => {
            match get_assignment_declaration_kind(source, node) {
                AssignmentDeclarationKind::ThisProperty
                | AssignmentDeclarationKind::Property
                | AssignmentDeclarationKind::ObjectDefinePropertyValue => SymbolKind::PROPERTY,
                _ => SymbolKind::VARIABLE,
            }
        }
        // String literals used as property names (e.g., in Object.defineProperty)
        SyntaxKind::StringLiteral
        | SyntaxKind::NoSubstitutionTemplateLiteral
        | SyntaxKind::NumericLiteral => SymbolKind::PROPERTY,
        _ => SymbolKind::VARIABLE,
    }
}

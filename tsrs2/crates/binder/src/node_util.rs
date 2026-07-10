//! tsc utility ports the binder needs: modifier flags, declaration
//! names, dynamic-name predicates, and error spans. Anchors are into
//! the vendored `_tsc.js`; JS-only branches (assignment declarations,
//! JSDoc) are carved out to stage 3.4 / JSDoc parsing and return the
//! TS-only result, each marked with a `JS-only:` comment.

use crate::symbols::{escape_leading_underscores, unescape_leading_underscores};
use tsrs2_syntax::{NodeArrayId, NodeData, NodeId, SourceFile, SyntaxKind};
use tsrs2_types::{ModifierFlags, NodeFlags};

pub fn node_flags(source: &SourceFile, id: NodeId) -> NodeFlags {
    NodeFlags::from_bits(source.arena.node(id).flags)
}

pub fn parent_of(source: &SourceFile, id: NodeId) -> Option<NodeId> {
    source.arena.node(id).parent
}

pub fn kind_of(source: &SourceFile, id: NodeId) -> SyntaxKind {
    source.arena.node(id).kind
}

/// tsc `node.modifiers` dynamic access: the modifiers array of any kind
/// that can carry one (canHaveModifiers proxy — exactly the generated
/// Data structs with a modifiers field).
pub fn modifiers_of(source: &SourceFile, id: NodeId) -> Option<NodeArrayId> {
    match &source.arena.node(id).data {
        NodeData::ArrowFunction(data) => data.modifiers,
        NodeData::ClassDeclaration(data) => data.modifiers,
        NodeData::ClassExpression(data) => data.modifiers,
        NodeData::ClassStaticBlockDeclaration(data) => data.modifiers,
        NodeData::Constructor(data) => data.modifiers,
        NodeData::ConstructorType(data) => data.modifiers,
        NodeData::EnumDeclaration(data) => data.modifiers,
        NodeData::ExportAssignment(data) => data.modifiers,
        NodeData::ExportDeclaration(data) => data.modifiers,
        NodeData::FunctionDeclaration(data) => data.modifiers,
        NodeData::FunctionExpression(data) => data.modifiers,
        NodeData::FunctionType(data) => data.modifiers,
        NodeData::GetAccessor(data) => data.modifiers,
        NodeData::ImportDeclaration(data) => data.modifiers,
        NodeData::ImportEqualsDeclaration(data) => data.modifiers,
        NodeData::IndexSignature(data) => data.modifiers,
        NodeData::InterfaceDeclaration(data) => data.modifiers,
        NodeData::MethodDeclaration(data) => data.modifiers,
        NodeData::MethodSignature(data) => data.modifiers,
        NodeData::MissingDeclaration(data) => data.modifiers,
        NodeData::ModuleDeclaration(data) => data.modifiers,
        NodeData::NamespaceExportDeclaration(data) => data.modifiers,
        NodeData::Parameter(data) => data.modifiers,
        NodeData::PropertyAssignment(data) => data.modifiers,
        NodeData::PropertyDeclaration(data) => data.modifiers,
        NodeData::PropertySignature(data) => data.modifiers,
        NodeData::SetAccessor(data) => data.modifiers,
        NodeData::ShorthandPropertyAssignment(data) => data.modifiers,
        NodeData::TypeAliasDeclaration(data) => data.modifiers,
        NodeData::TypeParameter(data) => data.modifiers,
        NodeData::VariableStatement(data) => data.modifiers,
        _ => None,
    }
}

/// tsc-port: modifierToFlag @6.0.3
/// tsc-hash: 770fd67655828664335271fc87c68ad1ef1aea9f151a86d5d826eb087cb2bfa0
/// tsc-span: _tsc.js:17035-17073
pub fn modifier_to_flag(token: SyntaxKind) -> ModifierFlags {
    match token {
        SyntaxKind::StaticKeyword => ModifierFlags::STATIC,
        SyntaxKind::PublicKeyword => ModifierFlags::PUBLIC,
        SyntaxKind::ProtectedKeyword => ModifierFlags::PROTECTED,
        SyntaxKind::PrivateKeyword => ModifierFlags::PRIVATE,
        SyntaxKind::AbstractKeyword => ModifierFlags::ABSTRACT,
        SyntaxKind::AccessorKeyword => ModifierFlags::ACCESSOR,
        SyntaxKind::ExportKeyword => ModifierFlags::EXPORT,
        SyntaxKind::DeclareKeyword => ModifierFlags::AMBIENT,
        SyntaxKind::ConstKeyword => ModifierFlags::CONST,
        SyntaxKind::DefaultKeyword => ModifierFlags::DEFAULT,
        SyntaxKind::AsyncKeyword => ModifierFlags::ASYNC,
        SyntaxKind::ReadonlyKeyword => ModifierFlags::READONLY,
        SyntaxKind::OverrideKeyword => ModifierFlags::OVERRIDE,
        SyntaxKind::InKeyword => ModifierFlags::IN,
        SyntaxKind::OutKeyword => ModifierFlags::OUT,
        SyntaxKind::Decorator => ModifierFlags::DECORATOR,
        _ => ModifierFlags::NONE,
    }
}

/// tsc-port: modifiersToFlags @6.0.3
/// tsc-hash: 61423bd0b76ca73e8f19fde199b31e595db0c92cc24ca6fafa079d5a98e596cc
/// tsc-span: _tsc.js:17026-17034
pub fn modifiers_to_flags(source: &SourceFile, modifiers: Option<NodeArrayId>) -> ModifierFlags {
    let mut flags = ModifierFlags::NONE;
    if let Some(modifiers) = modifiers {
        for &modifier in &source.arena.node_array(modifiers).nodes {
            flags |= modifier_to_flag(source.arena.node(modifier).kind);
        }
    }
    flags
}

/// tsc-port: getSyntacticModifierFlagsNoCache @6.0.3
/// tsc-hash: 1b7cb8845b02b8a88b15a70b861ff5cfa0e1959d0f056d0316c204da7dcf7644
/// tsc-span: _tsc.js:17019-17025
///
/// JS-only: bit 4096 on an Identifier is tsc's repurposed
/// IdentifierIsInJSDocNamespace flag (the generated NodeFlags names the
/// bit HasAsyncFunctions); it never appears while JSDoc parsing is
/// unported, the check is kept for shape.
pub fn get_syntactic_modifier_flags_no_cache(source: &SourceFile, id: NodeId) -> ModifierFlags {
    let mut flags = modifiers_to_flags(source, modifiers_of(source, id));
    let node_flags = node_flags(source, id);
    if node_flags.intersects(NodeFlags::NESTED_NAMESPACE)
        || kind_of(source, id) == SyntaxKind::Identifier
            && node_flags.intersects(NodeFlags::from_bits(4096))
    {
        flags |= ModifierFlags::EXPORT;
    }
    flags
}

/// tsc getSyntacticModifierFlags: the modifierFlagsCache is a pure
/// memoization — recompute instead.
pub fn get_syntactic_modifier_flags(source: &SourceFile, id: NodeId) -> ModifierFlags {
    let kind = kind_of(source, id);
    if kind as u16 >= SyntaxKind::FirstToken as u16 && kind as u16 <= SyntaxKind::LastToken as u16 {
        return ModifierFlags::NONE;
    }
    get_syntactic_modifier_flags_no_cache(source, id)
}

/// tsc-port: hasSyntacticModifier @6.0.3
/// tsc-hash: 7dd8709f25333f4dd5745735d9a1e5b97b6b1a5b056f1f34bd9c06517a93112a
/// tsc-span: _tsc.js:16931-16933
pub fn has_syntactic_modifier(source: &SourceFile, id: NodeId, flags: ModifierFlags) -> bool {
    get_syntactic_modifier_flags(source, id).intersects(flags)
}

/// tsc-port: walkUpBindingElementsAndPatterns @6.0.3
/// tsc-hash: 7c894ff81a7f38a64b44750d844db8129770e2a52a31216617f0a1f7b1ef68a2
/// tsc-span: _tsc.js:11315-11321
fn walk_up_binding_elements_and_patterns(source: &SourceFile, binding: NodeId) -> Option<NodeId> {
    let mut node = parent_of(source, binding)?;
    while let Some(parent) = parent_of(source, node) {
        if kind_of(source, parent) == SyntaxKind::BindingElement {
            node = parent_of(source, parent)?;
        } else {
            break;
        }
    }
    parent_of(source, node)
}

/// tsc-port: getCombinedFlags @6.0.3
/// tsc-hash: a6c964170d4df0cc4be460454fb5933c6f1dd557cd9cdca86648e2ba4b17b44b
/// tsc-span: _tsc.js:11322-11338
fn get_combined_flags(
    source: &SourceFile,
    id: NodeId,
    get_flags: impl Fn(&SourceFile, NodeId) -> ModifierFlags,
) -> ModifierFlags {
    let mut node = Some(id);
    if kind_of(source, id) == SyntaxKind::BindingElement {
        node = walk_up_binding_elements_and_patterns(source, id);
    }
    let Some(mut current) = node else {
        return ModifierFlags::NONE;
    };
    let mut flags = get_flags(source, current);
    if kind_of(source, current) == SyntaxKind::VariableDeclaration {
        match parent_of(source, current) {
            Some(parent) => current = parent,
            None => return flags,
        }
    }
    if kind_of(source, current) == SyntaxKind::VariableDeclarationList {
        flags |= get_flags(source, current);
        match parent_of(source, current) {
            Some(parent) => current = parent,
            None => return flags,
        }
    }
    if kind_of(source, current) == SyntaxKind::VariableStatement {
        flags |= get_flags(source, current);
    }
    flags
}

/// tsc getCombinedModifierFlags — via getEffectiveModifierFlags, which
/// only differs from the syntactic flags through JSDoc modifier tags
/// (unported: JSDoc parsing absent), so the syntactic flags are exact.
pub fn get_combined_modifier_flags(source: &SourceFile, id: NodeId) -> ModifierFlags {
    get_combined_flags(source, id, get_syntactic_modifier_flags)
}

/// tsc `declaration.name` dynamic access: the name field of any kind
/// that has one (the getNonAssignedNameOfDeclaration default arm).
pub fn name_field_of(source: &SourceFile, id: NodeId) -> Option<NodeId> {
    match &source.arena.node(id).data {
        NodeData::BindingElement(data) => data.name,
        NodeData::ClassDeclaration(data) => data.name,
        NodeData::ClassExpression(data) => data.name,
        NodeData::Constructor(data) => data.name,
        NodeData::EnumDeclaration(data) => data.name,
        NodeData::EnumMember(data) => data.name,
        NodeData::ExportSpecifier(data) => data.name,
        NodeData::FunctionDeclaration(data) => data.name,
        NodeData::FunctionExpression(data) => data.name,
        NodeData::GetAccessor(data) => data.name,
        NodeData::ImportAttribute(data) => data.name,
        NodeData::ImportClause(data) => data.name,
        NodeData::ImportEqualsDeclaration(data) => data.name,
        NodeData::ImportSpecifier(data) => data.name,
        NodeData::InterfaceDeclaration(data) => data.name,
        NodeData::JsxAttribute(data) => data.name,
        NodeData::JsxNamespacedName(data) => data.name,
        NodeData::MetaProperty(data) => data.name,
        NodeData::MethodDeclaration(data) => data.name,
        NodeData::MethodSignature(data) => data.name,
        NodeData::ModuleDeclaration(data) => data.name,
        NodeData::NamedTupleMember(data) => data.name,
        NodeData::NamespaceExport(data) => data.name,
        NodeData::NamespaceExportDeclaration(data) => data.name,
        NodeData::NamespaceImport(data) => data.name,
        NodeData::Parameter(data) => data.name,
        NodeData::PropertyAccessExpression(data) => data.name,
        NodeData::PropertyAssignment(data) => data.name,
        NodeData::PropertyDeclaration(data) => data.name,
        NodeData::PropertySignature(data) => data.name,
        NodeData::SetAccessor(data) => data.name,
        NodeData::ShorthandPropertyAssignment(data) => data.name,
        NodeData::TypeAliasDeclaration(data) => data.name,
        NodeData::TypeParameter(data) => data.name,
        NodeData::VariableDeclaration(data) => data.name,
        _ => None,
    }
}

/// tsc-port: getNonAssignedNameOfDeclaration @6.0.3
/// tsc-hash: 382ebe3aca3c5b65c264f1177b6f9ed47454cdc918c228f8469456fb504d617b
/// tsc-span: _tsc.js:11517-11561
///
/// JS-only: the CallExpression/BinaryExpression assignment-declaration
/// arms and the bindable static ElementAccessExpression arm resolve via
/// getAssignmentDeclarationKind — stage 3.4's JS subsystem; they return
/// None here. JSDoc tag arms await JSDoc parsing.
pub fn get_non_assigned_name_of_declaration(source: &SourceFile, id: NodeId) -> Option<NodeId> {
    match kind_of(source, id) {
        SyntaxKind::Identifier => Some(id),
        SyntaxKind::CallExpression | SyntaxKind::BinaryExpression => None,
        SyntaxKind::ExportAssignment => match &source.arena.node(id).data {
            NodeData::ExportAssignment(data) => data
                .expression
                .filter(|&expression| kind_of(source, expression) == SyntaxKind::Identifier),
            _ => None,
        },
        SyntaxKind::ElementAccessExpression => None,
        _ => name_field_of(source, id),
    }
}

/// tsc-port: getAssignedName @6.0.3
/// tsc-hash: 16b3160d83ab91d6d9c08811b7d3ef66bce1a84ccdde30ee5cac09babbc2bab7
/// tsc-span: _tsc.js:11566-11580
///
/// JS-only: the access-expression left side resolves through
/// getElementOrPropertyAccessArgumentExpressionOrName (stage 3.4).
fn get_assigned_name(source: &SourceFile, id: NodeId) -> Option<NodeId> {
    let parent = parent_of(source, id)?;
    match &source.arena.node(parent).data {
        NodeData::PropertyAssignment(data) => data.name,
        NodeData::BindingElement(data) => data.name,
        NodeData::BinaryExpression(data) => {
            if data.right == Some(id) {
                let left = data.left?;
                if kind_of(source, left) == SyntaxKind::Identifier {
                    return Some(left);
                }
            }
            None
        }
        NodeData::VariableDeclaration(data) => data
            .name
            .filter(|&name| kind_of(source, name) == SyntaxKind::Identifier),
        _ => None,
    }
}

/// tsc-port: getNameOfDeclaration @6.0.3
/// tsc-hash: 5d3aafbdab871f0fe6f088a4904cd11e6b44e467e0cca8ad0c215b3f899b570b
/// tsc-span: _tsc.js:11562-11565
pub fn get_name_of_declaration(source: &SourceFile, id: NodeId) -> Option<NodeId> {
    get_non_assigned_name_of_declaration(source, id).or_else(|| {
        match kind_of(source, id) {
            SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::ClassExpression => get_assigned_name(source, id),
            _ => None,
        }
    })
}

/// tsc-port: nodeIsMissing @6.0.3
/// tsc-hash: 36954b70e1f42b497b6ff78e99e881c951ebfa5bd2d1341256d28a4a78a645ea
/// tsc-span: _tsc.js:12910-12915
pub fn node_is_missing(source: &SourceFile, id: Option<NodeId>) -> bool {
    let Some(id) = id else { return true };
    let node = source.arena.node(id);
    node.pos == node.end && node.kind != SyntaxKind::EndOfFileToken
}

/// tsc moduleExportNameIsDefault (_tsc.js 13032).
pub fn module_export_name_is_default(source: &SourceFile, name: NodeId) -> bool {
    match &source.arena.node(name).data {
        NodeData::StringLiteral(data) => data.text == "default",
        NodeData::Identifier(data) => data.escaped_text == "default",
        _ => false,
    }
}

/// tsc isAmbientModule (_tsc.js 13713).
pub fn is_ambient_module(source: &SourceFile, id: NodeId) -> bool {
    match &source.arena.node(id).data {
        NodeData::ModuleDeclaration(data) => match data.name {
            Some(name) => {
                kind_of(source, name) == SyntaxKind::StringLiteral
                    || is_global_scope_augmentation(source, id)
            }
            None => is_global_scope_augmentation(source, id),
        },
        _ => false,
    }
}

/// tsc isGlobalScopeAugmentation (_tsc.js 13734).
pub fn is_global_scope_augmentation(source: &SourceFile, id: NodeId) -> bool {
    node_flags(source, id).intersects(NodeFlags::GLOBAL_AUGMENTATION)
}

/// tsc skipParentheses with OuterExpressionKinds.Parentheses: only
/// ParenthesizedExpression unwraps (JSDocTypeAssertion is JS-only).
fn skip_parentheses(source: &SourceFile, mut id: NodeId) -> NodeId {
    while let NodeData::ParenthesizedExpression(data) = &source.arena.node(id).data {
        match data.expression {
            Some(expression) => id = expression,
            None => break,
        }
    }
    id
}

/// tsc isStringOrNumericLiteralLike (_tsc.js 15844).
pub fn is_string_or_numeric_literal_like(source: &SourceFile, id: NodeId) -> bool {
    matches!(
        kind_of(source, id),
        SyntaxKind::StringLiteral
            | SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::NumericLiteral
    )
}

/// tsc isSignedNumericLiteral (_tsc.js 15847).
pub fn is_signed_numeric_literal(source: &SourceFile, id: NodeId) -> bool {
    match &source.arena.node(id).data {
        NodeData::PrefixUnaryExpression(data) => {
            matches!(
                data.operator,
                SyntaxKind::PlusToken | SyntaxKind::MinusToken
            ) && data
                .operand
                .is_some_and(|operand| kind_of(source, operand) == SyntaxKind::NumericLiteral)
        }
        _ => false,
    }
}

/// tsc-port: hasDynamicName @6.0.3
/// tsc-hash: d126787bc1b36621098ed5255c26d1e27abe5bf6dbc55570657aa03f95a588bb
/// tsc-span: _tsc.js:15850-15853
pub fn has_dynamic_name(source: &SourceFile, declaration: NodeId) -> bool {
    match get_name_of_declaration(source, declaration) {
        Some(name) => is_dynamic_name(source, name),
        None => false,
    }
}

/// tsc-port: isDynamicName @6.0.3
/// tsc-hash: 7014a94f7e8ee40358469f32712ba44f9dadcab04d99dc7e54a8965ea989c1cb
/// tsc-span: _tsc.js:15854-15861
pub fn is_dynamic_name(source: &SourceFile, name: NodeId) -> bool {
    let expr = match &source.arena.node(name).data {
        NodeData::ComputedPropertyName(data) => data.expression,
        NodeData::ElementAccessExpression(data) => {
            data.argument_expression.map(|argument| skip_parentheses(source, argument))
        }
        _ => return false,
    };
    let Some(expr) = expr else { return false };
    !is_string_or_numeric_literal_like(source, expr) && !is_signed_numeric_literal(source, expr)
}

/// tsc isPropertyNameLiteral (_tsc.js 15888).
pub fn is_property_name_literal(source: &SourceFile, id: NodeId) -> bool {
    matches!(
        kind_of(source, id),
        SyntaxKind::Identifier
            | SyntaxKind::StringLiteral
            | SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::NumericLiteral
    )
}

/// The `text` payload of literal-like name nodes.
pub fn literal_text_of(source: &SourceFile, id: NodeId) -> Option<&str> {
    match &source.arena.node(id).data {
        NodeData::StringLiteral(data) => Some(&data.text),
        NodeData::NumericLiteral(data) => Some(&data.text),
        NodeData::BigIntLiteral(data) => Some(&data.text),
        NodeData::NoSubstitutionTemplateLiteral(data) => Some(&data.text),
        NodeData::Identifier(data) => Some(&data.text),
        NodeData::PrivateIdentifier(data) => Some(&data.text),
        _ => None,
    }
}

/// tsc idText: unescapeLeadingUnderscores(node.escapedText).
pub fn id_text(source: &SourceFile, id: NodeId) -> Option<&str> {
    match &source.arena.node(id).data {
        NodeData::Identifier(data) => Some(unescape_leading_underscores(&data.escaped_text)),
        NodeData::PrivateIdentifier(data) => {
            Some(unescape_leading_underscores(&data.escaped_text))
        }
        _ => None,
    }
}

/// tsc getTextOfIdentifierOrLiteral (_tsc.js 15899).
pub fn get_text_of_identifier_or_literal(source: &SourceFile, id: NodeId) -> Option<String> {
    if let Some(text) = id_text(source, id) {
        return Some(text.to_owned());
    }
    if let NodeData::JsxNamespacedName(_) = &source.arena.node(id).data {
        return get_text_of_jsx_namespaced_name(source, id);
    }
    literal_text_of(source, id).map(str::to_owned)
}

/// tsc getEscapedTextOfIdentifierOrLiteral (_tsc.js 15902).
pub fn get_escaped_text_of_identifier_or_literal(source: &SourceFile, id: NodeId) -> Option<String> {
    match &source.arena.node(id).data {
        NodeData::Identifier(data) => Some(data.escaped_text.clone()),
        NodeData::PrivateIdentifier(data) => Some(data.escaped_text.clone()),
        NodeData::JsxNamespacedName(_) => get_escaped_text_of_jsx_namespaced_name(source, id),
        _ => literal_text_of(source, id).map(escape_leading_underscores),
    }
}

/// tsc getEscapedTextOfJsxNamespacedName (_tsc.js 19342):
/// `${namespace.escapedText}:${idText(name)}`.
pub fn get_escaped_text_of_jsx_namespaced_name(
    source: &SourceFile,
    id: NodeId,
) -> Option<String> {
    let NodeData::JsxNamespacedName(data) = &source.arena.node(id).data else {
        return None;
    };
    let namespace = data.namespace?;
    let name = data.name?;
    let NodeData::Identifier(namespace_data) = &source.arena.node(namespace).data else {
        return None;
    };
    Some(format!(
        "{}:{}",
        namespace_data.escaped_text,
        id_text(source, name)?
    ))
}

fn get_text_of_jsx_namespaced_name(source: &SourceFile, id: NodeId) -> Option<String> {
    let NodeData::JsxNamespacedName(data) = &source.arena.node(id).data else {
        return None;
    };
    Some(format!(
        "{}:{}",
        id_text(source, data.namespace?)?,
        id_text(source, data.name?)?
    ))
}

/// tsc getContainingClass (_tsc.js 14441): nearest ClassDeclaration or
/// ClassExpression strictly above the node.
pub fn get_containing_class(source: &SourceFile, id: NodeId) -> Option<NodeId> {
    let mut current = parent_of(source, id);
    while let Some(node) = current {
        if matches!(
            kind_of(source, node),
            SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
        ) {
            return Some(node);
        }
        current = parent_of(source, node);
    }
    None
}

/// tsc declarationNameToString (_tsc.js 13854): missing names print
/// "(Missing)", everything else its trivia-skipped source text.
pub fn declaration_name_to_string(source: &SourceFile, name: Option<NodeId>) -> String {
    match name {
        None => "(Missing)".to_owned(),
        Some(name) => {
            let node = source.arena.node(name);
            if node.end == node.pos {
                return "(Missing)".to_owned();
            }
            let start = tsrs2_syntax::skip_trivia(&source.text, node.pos as usize);
            source.text[start..node.end as usize].to_owned()
        }
    }
}

/// tsc-port: getSpanOfTokenAtPosition @6.0.3
/// tsc-hash: 1389c564d97e9dbaa92975ba813b8d14180a73654923d1bc6d0817ac357009b0
/// tsc-span: _tsc.js:13983-13997
pub fn get_span_of_token_at_position(source: &SourceFile, pos: usize) -> (usize, usize) {
    let tokens = tsrs2_syntax::scan_tokens(&source.text[pos..], source.language_variant);
    match tokens.first() {
        Some(token) => (pos + token.start as usize, pos + token.end as usize),
        None => (pos, pos),
    }
}

/// tsc-port: getErrorSpanForNode @6.0.3
/// tsc-hash: 2d2ca68c825de352e44893a3a69b54b87090a276ae158a59d05c5e3ebfec35dd
/// tsc-span: _tsc.js:14023-14115
///
/// Byte offsets; callers convert to UTF-16 at diagnostic creation.
/// The JSDocSatisfiesTag arm awaits JSDoc parsing.
pub fn get_error_span_for_node(source: &SourceFile, id: NodeId) -> (usize, usize) {
    let node = source.arena.node(id);
    let mut error_node = Some(id);
    match &node.data {
        NodeData::SourceFile(_) => {
            let pos = tsrs2_syntax::skip_trivia(&source.text, 0);
            if pos == source.text.len() {
                return (0, 0);
            }
            return get_span_of_token_at_position(source, pos);
        }
        NodeData::ArrowFunction(data) => {
            return get_error_span_for_arrow_function(source, id, data.body);
        }
        NodeData::CaseClause(data) => {
            let start = tsrs2_syntax::skip_trivia(&source.text, node.pos as usize);
            return case_clause_span(source, node.end as usize, start, data.statements);
        }
        NodeData::DefaultClause(data) => {
            let start = tsrs2_syntax::skip_trivia(&source.text, node.pos as usize);
            return case_clause_span(source, node.end as usize, start, data.statements);
        }
        NodeData::ReturnStatement(_) | NodeData::YieldExpression(_) => {
            let pos = tsrs2_syntax::skip_trivia(&source.text, node.pos as usize);
            return get_span_of_token_at_position(source, pos);
        }
        NodeData::SatisfiesExpression(data) => {
            if let Some(expression) = data.expression {
                let pos = tsrs2_syntax::skip_trivia(
                    &source.text,
                    source.arena.node(expression).end as usize,
                );
                return get_span_of_token_at_position(source, pos);
            }
        }
        NodeData::Constructor(_) => {
            let start = tsrs2_syntax::skip_trivia(&source.text, node.pos as usize);
            let tokens =
                tsrs2_syntax::scan_tokens(&source.text[start..], source.language_variant);
            for token in &tokens {
                if token.kind == SyntaxKind::ConstructorKeyword {
                    return (start, start + token.end as usize);
                }
            }
            return (start, node.end as usize);
        }
        _ => {
            if matches!(
                node.kind,
                SyntaxKind::VariableDeclaration
                    | SyntaxKind::BindingElement
                    | SyntaxKind::ClassDeclaration
                    | SyntaxKind::ClassExpression
                    | SyntaxKind::InterfaceDeclaration
                    | SyntaxKind::ModuleDeclaration
                    | SyntaxKind::EnumDeclaration
                    | SyntaxKind::EnumMember
                    | SyntaxKind::FunctionDeclaration
                    | SyntaxKind::FunctionExpression
                    | SyntaxKind::MethodDeclaration
                    | SyntaxKind::GetAccessor
                    | SyntaxKind::SetAccessor
                    | SyntaxKind::TypeAliasDeclaration
                    | SyntaxKind::PropertyDeclaration
                    | SyntaxKind::PropertySignature
                    | SyntaxKind::NamespaceImport
            ) {
                error_node = name_field_of(source, id);
            }
        }
    }

    let Some(error_node) = error_node else {
        return get_span_of_token_at_position(source, node.pos as usize);
    };
    let error = source.arena.node(error_node);
    let is_missing = node_is_missing(source, Some(error_node));
    let pos = if is_missing || node.kind == SyntaxKind::JsxText {
        error.pos as usize
    } else {
        tsrs2_syntax::skip_trivia(&source.text, error.pos as usize)
    };
    (pos, error.end as usize)
}

fn case_clause_span(
    source: &SourceFile,
    node_end: usize,
    start: usize,
    statements: Option<NodeArrayId>,
) -> (usize, usize) {
    let end = statements
        .map(|statements| &source.arena.node_array(statements).nodes)
        .and_then(|nodes| nodes.first())
        .map(|&first| source.arena.node(first).pos as usize)
        .unwrap_or(node_end);
    (start, end)
}

/// tsc-port: getErrorSpanForArrowFunction @6.0.3
/// tsc-hash: 5d0186cc48fbcd0de233dc4fa9c9fafda71af0d8607ddb9c7675cc32f8f4c3ab
/// tsc-span: _tsc.js:14012-14022
///
/// A multi-line arrow body clamps the span to the first line.
fn get_error_span_for_arrow_function(
    source: &SourceFile,
    id: NodeId,
    body: Option<NodeId>,
) -> (usize, usize) {
    let node = source.arena.node(id);
    let pos = tsrs2_syntax::skip_trivia(&source.text, node.pos as usize);
    if let Some(body) = body {
        if kind_of(source, body) == SyntaxKind::Block {
            let body_node = source.arena.node(body);
            let start_line = line_of(source, body_node.pos as usize);
            let end_line = line_of(source, body_node.end as usize);
            if start_line < end_line {
                return (pos, get_end_line_position(source, start_line) + 1);
            }
        }
    }
    (pos, node.end as usize)
}

/// tsc isFunctionLikeKind (_tsc.js 12018) + isFunctionLikeDeclarationKind.
pub fn is_function_like_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::MethodSignature
            | SyntaxKind::CallSignature
            | SyntaxKind::ConstructSignature
            | SyntaxKind::IndexSignature
            | SyntaxKind::FunctionType
            | SyntaxKind::ConstructorType
    ) || is_function_like_declaration_kind(kind)
}

/// tsc isFunctionLikeDeclarationKind (_tsc.js 12004).
pub fn is_function_like_declaration_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::FunctionDeclaration
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::Constructor
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction
    )
}

/// tsc-port: isObjectLiteralOrClassExpressionMethodOrAccessor @6.0.3
/// tsc-hash: 7770ac43c5e3642345bac7cdb81f934a594f6bdbf4fa5b3729e44cd09d441f42
/// tsc-span: _tsc.js:14410-14412
pub fn is_object_literal_or_class_expression_method_or_accessor(
    source: &SourceFile,
    id: NodeId,
) -> bool {
    matches!(
        kind_of(source, id),
        SyntaxKind::MethodDeclaration | SyntaxKind::GetAccessor | SyntaxKind::SetAccessor
    ) && parent_of(source, id).is_some_and(|parent| {
        matches!(
            kind_of(source, parent),
            SyntaxKind::ObjectLiteralExpression | SyntaxKind::ClassExpression
        )
    })
}

/// tsc-port: getImmediatelyInvokedFunctionExpression @6.0.3
/// tsc-hash: 376116f5822935a0b930eb122871cf6a305990b92a0e03ce126f83914ed84686
/// tsc-span: _tsc.js:14595-14607
pub fn get_immediately_invoked_function_expression(
    source: &SourceFile,
    func: NodeId,
) -> Option<NodeId> {
    if !matches!(
        kind_of(source, func),
        SyntaxKind::FunctionExpression | SyntaxKind::ArrowFunction
    ) {
        return None;
    }
    let mut prev = func;
    let mut parent = parent_of(source, func)?;
    while kind_of(source, parent) == SyntaxKind::ParenthesizedExpression {
        prev = parent;
        parent = parent_of(source, parent)?;
    }
    match &source.arena.node(parent).data {
        NodeData::CallExpression(data) if data.expression == Some(prev) => Some(parent),
        _ => None,
    }
}

/// tsc `node.body` dynamic access for the container kinds bindContainer
/// inspects.
pub fn body_of(source: &SourceFile, id: NodeId) -> Option<NodeId> {
    match &source.arena.node(id).data {
        NodeData::FunctionDeclaration(data) => data.body,
        NodeData::FunctionExpression(data) => data.body,
        NodeData::ArrowFunction(data) => data.body,
        NodeData::MethodDeclaration(data) => data.body,
        NodeData::Constructor(data) => data.body,
        NodeData::GetAccessor(data) => data.body,
        NodeData::SetAccessor(data) => data.body,
        NodeData::ClassStaticBlockDeclaration(data) => data.body,
        NodeData::ModuleDeclaration(data) => data.body,
        _ => None,
    }
}

/// tsc `node.asteriskToken` access (bindContainer's IIFE test).
pub fn asterisk_token_of(source: &SourceFile, id: NodeId) -> Option<NodeId> {
    match &source.arena.node(id).data {
        NodeData::FunctionDeclaration(data) => data.asterisk_token,
        NodeData::FunctionExpression(data) => data.asterisk_token,
        NodeData::MethodDeclaration(data) => data.asterisk_token,
        NodeData::YieldExpression(data) => data.asterisk_token,
        _ => None,
    }
}

/// tsc `.statements` of SourceFile/Block/ModuleBlock.
pub fn statements_of(source: &SourceFile, id: NodeId) -> Option<NodeArrayId> {
    match &source.arena.node(id).data {
        NodeData::SourceFile(data) => data.statements,
        NodeData::Block(data) => data.statements,
        NodeData::ModuleBlock(data) => data.statements,
        _ => None,
    }
}

/// tsc-port: nodeHasName @6.0.3
/// tsc-hash: debb0bab240f8237c96d8122ec3051f641818c1535af8434b059f114f8881fdc
/// tsc-span: _tsc.js:11502-11510
pub fn node_has_name(source: &SourceFile, statement: NodeId, name: NodeId) -> bool {
    let target = id_text(source, name);
    if let Some(statement_name) = name_field_of(source, statement) {
        if kind_of(source, statement_name) == SyntaxKind::Identifier
            && id_text(source, statement_name) == target
        {
            return true;
        }
    }
    if let NodeData::VariableStatement(data) = &source.arena.node(statement).data {
        if let Some(list) = data.declaration_list {
            if let NodeData::VariableDeclarationList(list) = &source.arena.node(list).data {
                if let Some(declarations) = list.declarations {
                    return source
                        .arena
                        .node_array(declarations)
                        .nodes
                        .iter()
                        .any(|&declaration| node_has_name(source, declaration, name));
                }
            }
        }
    }
    false
}

/// tsc-port: isModuleAugmentationExternal @6.0.3
/// tsc-hash: aaba1aaac0a2bbde6e1923580f5a67a0d947ee79784eba560143f321455d79ec
/// tsc-span: _tsc.js:13740-13748
pub fn is_module_augmentation_external(source: &SourceFile, node: NodeId) -> bool {
    let Some(parent) = parent_of(source, node) else {
        return false;
    };
    match kind_of(source, parent) {
        SyntaxKind::SourceFile => source.external_module_indicator.is_some(),
        SyntaxKind::ModuleBlock => {
            let Some(grand) = parent_of(source, parent) else {
                return false;
            };
            is_ambient_module(source, grand)
                && parent_of(source, grand).is_some_and(|great| {
                    kind_of(source, great) == SyntaxKind::SourceFile
                        && source.external_module_indicator.is_none()
                })
        }
        _ => false,
    }
}

/// tsc-port: tryParsePattern @6.0.3
/// tsc-hash: 160377a6950ca7abc1ca3c322c9fc416f93bb49df6d452eba4e70d5703fcd5bc
/// tsc-span: _tsc.js:18773-18781
///
/// None ⇒ more than one `*`; Whole ⇒ no star; Wildcard ⇒ one star.
pub enum ParsedPattern {
    Whole(String),
    Wildcard { prefix: String, suffix: String },
}

pub fn try_parse_pattern(pattern: &str) -> Option<ParsedPattern> {
    match pattern.find('*') {
        None => Some(ParsedPattern::Whole(pattern.to_owned())),
        Some(index) => {
            if pattern[index + 1..].contains('*') {
                None
            } else {
                Some(ParsedPattern::Wildcard {
                    prefix: pattern[..index].to_owned(),
                    suffix: pattern[index + 1..].to_owned(),
                })
            }
        }
    }
}

fn line_of(source: &SourceFile, pos: usize) -> usize {
    let starts = &source.line_map.line_starts;
    match starts.binary_search(&(pos as u32)) {
        Ok(line) => line,
        Err(insert) => insert.saturating_sub(1),
    }
}

/// tsc getEndLinePosition (_tsc.js 12890): the last non-line-break
/// position on `line`.
fn get_end_line_position(source: &SourceFile, line: usize) -> usize {
    let starts = &source.line_map.line_starts;
    if line + 1 == starts.len() {
        return source.text.len().saturating_sub(1);
    }
    let start = starts[line] as usize;
    let mut pos = (starts[line + 1] as usize).saturating_sub(1);
    while pos >= start {
        if source.text.is_char_boundary(pos) {
            let ch = source.text[pos..].chars().next();
            match ch {
                Some('\n') | Some('\r') | Some('\u{2028}') | Some('\u{2029}') => {}
                _ => break,
            }
            if pos == 0 {
                break;
            }
            pos -= 1;
        } else {
            pos -= 1;
        }
    }
    pos
}

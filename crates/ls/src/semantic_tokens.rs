//! tsgo `ls/semantictokens.go`: a file's semantic tokens
//! (`textDocument/semanticTokens/full` and `/range`) — each identifier the
//! checker resolves to a symbol it can classify, with the symbol's
//! modifiers, encoded in the client's legend.

use std::borrow::Cow;

use tsc_binder::node_util;
use tsc_checker::program::ProgramFileId;
use tsc_checker::state::{CheckResult, SignatureKind};
use tsc_lsproto::{
    DocumentUri, Range, ResolvedSemanticTokensClientCapabilities, SemanticTokenModifier,
    SemanticTokenType, SemanticTokens, SemanticTokensLegend, SemanticTokensOrNull,
    SemanticTokensResponse,
};
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{ModifierFlags, NodeFlags, SymbolFlags, TypeFlags, TypeId};

use crate::language_service::{LanguageService, LsError};
use crate::meaning::SemanticMeaning;
use crate::request::Request;
use crate::utilities::{script_of, Touching};

/// tsgo `tokenTypes`, in their encoding order.
pub const TOKEN_TYPES: [SemanticTokenType; 23] = [
    SemanticTokenType::NAMESPACE,
    SemanticTokenType::CLASS,
    SemanticTokenType::ENUM,
    SemanticTokenType::INTERFACE,
    SemanticTokenType::STRUCT,
    SemanticTokenType::TYPE_PARAMETER,
    SemanticTokenType::TYPE,
    SemanticTokenType::PARAMETER,
    SemanticTokenType::VARIABLE,
    SemanticTokenType::PROPERTY,
    SemanticTokenType::ENUM_MEMBER,
    SemanticTokenType::DECORATOR,
    SemanticTokenType::EVENT,
    SemanticTokenType::FUNCTION,
    SemanticTokenType::METHOD,
    SemanticTokenType::MACRO,
    SemanticTokenType::LABEL,
    SemanticTokenType::COMMENT,
    SemanticTokenType::STRING,
    SemanticTokenType::KEYWORD,
    SemanticTokenType::NUMBER,
    SemanticTokenType::REGEXP,
    SemanticTokenType::OPERATOR,
];

/// tsgo `tokenModifiers`, in their encoding order (`local` is tsgo's own).
pub const TOKEN_MODIFIERS: [SemanticTokenModifier; 11] = [
    SemanticTokenModifier::DECLARATION,
    SemanticTokenModifier::DEFINITION,
    SemanticTokenModifier::READONLY,
    SemanticTokenModifier::STATIC,
    SemanticTokenModifier::DEPRECATED,
    SemanticTokenModifier::ABSTRACT,
    SemanticTokenModifier::ASYNC,
    SemanticTokenModifier::MODIFICATION,
    SemanticTokenModifier::DOCUMENTATION,
    SemanticTokenModifier::DEFAULT_LIBRARY,
    SemanticTokenModifier(Cow::Borrowed("local")),
];

/// tsgo `SemanticTokensLegend`: the token types and modifiers the client
/// supports, in tsgo's order.
pub fn semantic_tokens_legend(
    client: &ResolvedSemanticTokensClientCapabilities,
) -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TOKEN_TYPES
            .iter()
            .filter(|kind| {
                client
                    .token_types
                    .iter()
                    .any(|supported| *supported == *kind.0)
            })
            .map(|kind| kind.0.to_string())
            .collect(),
        token_modifiers: TOKEN_MODIFIERS
            .iter()
            .filter(|modifier| {
                client
                    .token_modifiers
                    .iter()
                    .any(|supported| *supported == *modifier.0)
            })
            .map(|modifier| modifier.0.to_string())
            .collect(),
    }
}

/// tsgo's token types (indices of [`TOKEN_TYPES`]).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum TokenType {
    Namespace = 0,
    Class = 1,
    Enum = 2,
    Interface = 3,
    TypeParameter = 5,
    Type = 6,
    Parameter = 7,
    Variable = 8,
    Property = 9,
    EnumMember = 10,
    Function = 13,
    Method = 14,
}

/// tsgo's token modifiers (bits of [`TOKEN_MODIFIERS`]' indices).
const DECLARATION: u32 = 1 << 0;
const READONLY: u32 = 1 << 2;
const STATIC: u32 = 1 << 3;
const ASYNC: u32 = 1 << 6;
const DEFAULT_LIBRARY: u32 = 1 << 9;
const LOCAL: u32 = 1 << 10;

/// tsgo `semanticToken`: an identifier, its type and its modifiers.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct SemanticToken {
    node: NodeId,
    token_type: TokenType,
    modifiers: u32,
}

impl LanguageService<'_> {
    /// tsgo `ProvideSemanticTokens` (a file without a content mapper: one
    /// projection).
    pub fn provide_semantic_tokens(
        &mut self,
        uri: &DocumentUri,
    ) -> Result<SemanticTokensResponse, LsError> {
        let file = self.file(uri);
        self.semantic_tokens_in(file, None)
    }

    /// tsgo `ProvideSemanticTokensRange`: the tokens of the nodes that
    /// overlap the range.
    pub fn provide_semantic_tokens_range(
        &mut self,
        uri: &DocumentUri,
        range: &Range,
    ) -> Result<SemanticTokensResponse, LsError> {
        let file = self.file(uri);
        self.semantic_tokens_in(file, Some(range))
    }

    fn semantic_tokens_in(
        &mut self,
        file: usize,
        range: Option<&Range>,
    ) -> Result<SemanticTokensResponse, LsError> {
        let capabilities = self.capabilities.text_document.semantic_tokens.clone();
        let encoded = self
            .with_request_at(file, 0, false, |request, _| {
                let source = request.state.binder.source(file);
                let script = script_of(source);
                let (start, end) = match range {
                    Some(range) => (
                        request.converters.from_lsp_position(&script, &range.start),
                        request.converters.from_lsp_position(&script, &range.end),
                    ),
                    None => {
                        let root = source.arena.node(source.root);
                        (root.pos as usize, root.end as usize)
                    }
                };
                let mut tokens = request.collect_semantic_tokens(file, start, end)?;
                let mut seen = std::collections::HashSet::new();
                tokens.retain(|token| seen.insert(*token));
                Ok(request.encode_semantic_tokens(file, tokens, &capabilities))
            })?
            .unwrap_or_default();
        if encoded.is_empty() {
            return Ok(SemanticTokensOrNull::Null);
        }
        Ok(SemanticTokensOrNull::SemanticTokens(SemanticTokens {
            result_id: None,
            data: encoded,
        }))
    }
}

impl Request<'_, '_> {
    /// tsgo `collectSemanticTokensInRange`: the identifiers (not in a JSX
    /// element's tags or attributes, an import clause, or `Infinity` and
    /// `NaN`) of the nodes overlapping the span, classified by their
    /// symbols.
    fn collect_semantic_tokens(
        &mut self,
        file: usize,
        span_start: usize,
        span_end: usize,
    ) -> CheckResult<Vec<SemanticToken>> {
        let source = self.state.binder.source(file);
        let mut tokens = Vec::new();
        let mut stack = vec![(source.root, false)];
        while let Some((node, in_jsx_parent)) = stack.pop() {
            let source = self.state.binder.source(file);
            let record = source.arena.node(node);
            if record.pos as usize >= span_end || (record.end as usize) <= span_start {
                continue;
            }
            let in_jsx_element = match record.kind {
                SyntaxKind::JsxElement | SyntaxKind::JsxSelfClosingElement => true,
                SyntaxKind::JsxExpression => false,
                _ => in_jsx_parent,
            };
            let text = match &record.data {
                NodeData::Identifier(data) => Some(data.escaped_text),
                NodeData::PrivateIdentifier(data) => Some(data.escaped_text),
                _ => None,
            };
            if let Some(text) = text {
                if !text.is_empty()
                    && !in_jsx_element
                    && !self.is_in_import_clause(node)
                    && text != "Infinity"
                    && text != "NaN"
                {
                    if let Some(token) = self.semantic_token_of(file, node)? {
                        tokens.push(token);
                    }
                }
            }
            let source = self.state.binder.source(file);
            let mut children = Vec::new();
            tsc_syntax::for_each_child(&source.arena, source.arena.node(node), |child| {
                children.push(child);
                false
            });
            for child in children.into_iter().rev() {
                stack.push((child, in_jsx_element));
            }
        }
        Ok(tokens)
    }

    /// The token of an identifier whose (aliased) symbol classifies: its
    /// type (a parameter on the right of a dot is a property; a variable,
    /// property or parameter by its type's signatures) and modifiers.
    fn semantic_token_of(
        &mut self,
        file: usize,
        node: NodeId,
    ) -> CheckResult<Option<SemanticToken>> {
        let Some(mut symbol) = self.state.get_symbol_at_location(node)? else {
            return Ok(None);
        };
        if self
            .state
            .symbol_flags(symbol)
            .intersects(SymbolFlags::ALIAS)
        {
            symbol = self.state.get_aliased_symbol(symbol)?;
        }
        let meaning = self.meaning_from_location(Touching::Node(node));
        let Some(mut token_type) = self.classify_symbol(symbol, meaning) else {
            return Ok(None);
        };
        let mut modifiers = 0;
        if let Some(parent) = self.parent_of(node) {
            let parent_is_declaration = self.node_kind(parent) == SyntaxKind::BindingElement
                || token_from_declaration_mapping(self.node_kind(parent)) == Some(token_type);
            if parent_is_declaration && self.name_of(parent) == Some(node) {
                modifiers |= DECLARATION;
            }
        }
        // A parameter property on the right of a dot is a property.
        if token_type == TokenType::Parameter
            && self.is_right_side_of_qualified_name_or_property_access(node)
        {
            token_type = TokenType::Property;
        }
        token_type = self.reclassify_by_type(node, token_type)?;
        let record = self.state.binder.symbol(symbol);
        let value_declaration = record.value_declaration;
        let declarations: Vec<NodeId> = record.declarations.iter().copied().collect();
        if let Some(declaration) = value_declaration {
            let source = self.state.binder.source_of_node(declaration);
            let modifier_flags = combined_modifier_flags(source, declaration);
            let node_flags = node_util::get_combined_node_flags(source, declaration);
            if modifier_flags.intersects(ModifierFlags::STATIC) {
                modifiers |= STATIC;
            }
            if modifier_flags.intersects(ModifierFlags::ASYNC) {
                modifiers |= ASYNC;
            }
            if token_type != TokenType::Class
                && token_type != TokenType::Interface
                && (modifier_flags.intersects(ModifierFlags::READONLY)
                    || node_flags.intersects(NodeFlags::CONST)
                    || self
                        .state
                        .symbol_flags(symbol)
                        .intersects(SymbolFlags::ENUM_MEMBER))
            {
                modifiers |= READONLY;
            }
            if matches!(token_type, TokenType::Variable | TokenType::Function)
                && self.is_local_declaration(declaration, file)
            {
                modifiers |= LOCAL;
            }
            if self.is_in_default_library(declaration) {
                modifiers |= DEFAULT_LIBRARY;
            }
        } else if declarations
            .iter()
            .any(|&declaration| self.is_in_default_library(declaration))
        {
            modifiers |= DEFAULT_LIBRARY;
        }
        Ok(Some(SemanticToken {
            node,
            token_type,
            modifiers,
        }))
    }

    /// tsgo `classifySymbol`: by the symbol's flags, else by the kind of its
    /// value declaration (or first declaration; a binding element's
    /// variable or parameter).
    fn classify_symbol(
        &self,
        symbol: tsc_binder::SymbolId,
        meaning: SemanticMeaning,
    ) -> Option<TokenType> {
        let flags = self.state.symbol_flags(symbol);
        if flags.intersects(SymbolFlags::CLASS) {
            return Some(TokenType::Class);
        }
        if flags.intersects(SymbolFlags::ENUM) {
            return Some(TokenType::Enum);
        }
        if flags.intersects(SymbolFlags::TYPE_ALIAS) {
            return Some(TokenType::Type);
        }
        if flags.intersects(SymbolFlags::INTERFACE) && meaning.0 & SemanticMeaning::TYPE.0 != 0 {
            return Some(TokenType::Interface);
        }
        if flags.intersects(SymbolFlags::TYPE_PARAMETER) {
            return Some(TokenType::TypeParameter);
        }
        let record = self.state.binder.symbol(symbol);
        let mut declaration = record
            .value_declaration
            .or_else(|| record.declarations.first().copied())?;
        if self.node_kind(declaration) == SyntaxKind::BindingElement {
            declaration = self.declaration_for_binding_element(declaration);
        }
        token_from_declaration_mapping(self.node_kind(declaration))
    }

    /// tsgo `reclassifyByType`: a variable, property or parameter whose type
    /// (or a union member) constructs is a class (not a parameter); one that
    /// is called (without properties, or as a call's callee) a function or
    /// method.
    fn reclassify_by_type(
        &mut self,
        node: NodeId,
        token_type: TokenType,
    ) -> CheckResult<TokenType> {
        if !matches!(
            token_type,
            TokenType::Variable | TokenType::Property | TokenType::Parameter
        ) {
            return Ok(token_type);
        }
        let ty = self.state.get_type_at_location(node)?;
        if token_type != TokenType::Parameter
            && self.type_test(ty, |request, member| {
                Ok(!request
                    .state
                    .get_signatures_of_type(member, SignatureKind::Construct)?
                    .is_empty())
            })?
        {
            return Ok(TokenType::Class);
        }
        let has_call_signatures = self.type_test(ty, |request, member| {
            Ok(!request
                .state
                .get_signatures_of_type(member, SignatureKind::Call)?
                .is_empty())
        })?;
        if has_call_signatures {
            let has_no_properties = !self.type_test(ty, |request, member| {
                Ok(request
                    .state
                    .tables
                    .flags_of(member)
                    .intersects(TypeFlags::OBJECT)
                    && !request.state.get_properties_of_type(member)?.is_empty())
            })?;
            if has_no_properties || self.is_expression_in_call_expression(node) {
                return Ok(if token_type == TokenType::Property {
                    TokenType::Method
                } else {
                    TokenType::Function
                });
            }
        }
        Ok(token_type)
    }

    /// tsgo `reclassifyByType`'s `test`: the type, or one of its union's
    /// members.
    fn type_test(
        &mut self,
        ty: TypeId,
        mut condition: impl FnMut(&mut Self, TypeId) -> CheckResult<bool>,
    ) -> CheckResult<bool> {
        if condition(self, ty)? {
            return Ok(true);
        }
        if self.state.tables.flags_of(ty).intersects(TypeFlags::UNION) {
            for member in self.state.get_types_of_type(ty) {
                if condition(self, member)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// tsgo `isLocalDeclaration`: a variable in a function, block or catch
    /// clause, or a function not at the top of its file, of this file.
    fn is_local_declaration(&self, declaration: NodeId, file: usize) -> bool {
        let declaration = if self.node_kind(declaration) == SyntaxKind::BindingElement {
            self.declaration_for_binding_element(declaration)
        } else {
            declaration
        };
        let in_file = self.state.binder.file_index_of_node(declaration) == file;
        match self.node_kind(declaration) {
            SyntaxKind::VariableDeclaration => {
                let Some(parent) = self.parent_of(declaration) else {
                    return false;
                };
                if self.node_kind(parent) == SyntaxKind::CatchClause {
                    return in_file;
                }
                if self.node_kind(parent) == SyntaxKind::VariableDeclarationList {
                    if let Some(grandparent) = self.parent_of(parent) {
                        let great_grandparent = self.parent_of(grandparent);
                        return (great_grandparent
                            .is_none_or(|node| self.node_kind(node) != SyntaxKind::SourceFile)
                            || self.node_kind(grandparent) == SyntaxKind::CatchClause)
                            && in_file;
                    }
                }
                false
            }
            SyntaxKind::FunctionDeclaration => {
                self.parent_of(declaration)
                    .is_some_and(|parent| self.node_kind(parent) != SyntaxKind::SourceFile)
                    && in_file
            }
            _ => false,
        }
    }

    /// tsgo `getDeclarationForBindingElement`: the variable or parameter a
    /// (nested) binding element binds.
    fn declaration_for_binding_element(&self, mut element: NodeId) -> NodeId {
        loop {
            let Some(parent) = self.parent_of(element) else {
                return element;
            };
            if !matches!(
                self.node_kind(parent),
                SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern
            ) {
                return element;
            }
            let Some(grandparent) = self.parent_of(parent) else {
                return element;
            };
            if self.node_kind(grandparent) == SyntaxKind::BindingElement {
                element = grandparent;
                continue;
            }
            return grandparent;
        }
    }

    /// tsgo `isInImportClause`.
    fn is_in_import_clause(&self, node: NodeId) -> bool {
        self.parent_of(node).is_some_and(|parent| {
            matches!(
                self.node_kind(parent),
                SyntaxKind::ImportClause
                    | SyntaxKind::ImportSpecifier
                    | SyntaxKind::NamespaceImport
            )
        })
    }

    /// tsgo `isExpressionInCallExpression`: the name (after its dots) is a
    /// call's callee.
    fn is_expression_in_call_expression(&self, mut node: NodeId) -> bool {
        while self.is_right_side_of_qualified_name_or_property_access(node) {
            match self.parent_of(node) {
                Some(parent) => node = parent,
                None => return false,
            }
        }
        self.parent_of(node).is_some_and(|parent| {
            matches!(
                &self.state.binder.node_record(parent).data,
                NodeData::CallExpression(call) if call.expression == Some(node)
            )
        })
    }

    /// tsgo `program.IsSourceFileDefaultLibrary` of the node's file.
    fn is_in_default_library(&self, node: NodeId) -> bool {
        let file = self.state.binder.file_index_of_node(node);
        self.state
            .binder
            .file_facts(ProgramFileId::from_raw(file as u32))
            .is_default_library()
    }

    /// tsgo `encodeSemanticTokens` after `sortSemanticTokens`: by start, each
    /// start once, as the deltas, length, type and modifiers in the client's
    /// legend (a type it lacks drops the token).
    fn encode_semantic_tokens(
        &self,
        file: usize,
        mut tokens: Vec<SemanticToken>,
        capabilities: &ResolvedSemanticTokensClientCapabilities,
    ) -> Vec<u32> {
        let source = self.state.binder.source(file);
        let script = script_of(source);
        let range_of = |token: &SemanticToken| {
            let record = source.arena.node(token.node);
            let start = tsc_syntax::skip_trivia(source.text(), record.pos as usize);
            self.converters
                .to_lsp_range(&script, start..record.end as usize)
        };
        let mut keyed: Vec<(Range, SemanticToken)> = tokens
            .drain(..)
            .map(|token| (range_of(&token), token))
            .collect();
        keyed.sort_by_key(|(range, token)| {
            (
                range.start.line,
                range.start.character,
                source.arena.node(token.node).pos,
            )
        });
        let mut type_mapping = [None; TOKEN_TYPES.len()];
        let mut index = 0;
        for (server, kind) in TOKEN_TYPES.iter().enumerate() {
            if capabilities
                .token_types
                .iter()
                .any(|supported| *supported == *kind.0)
            {
                type_mapping[server] = Some(index);
                index += 1;
            }
        }
        let mut modifier_mapping = [None; TOKEN_MODIFIERS.len()];
        let mut bit = 0;
        for (server, modifier) in TOKEN_MODIFIERS.iter().enumerate() {
            if capabilities
                .token_modifiers
                .iter()
                .any(|supported| *supported == *modifier.0)
            {
                modifier_mapping[server] = Some(bit);
                bit += 1;
            }
        }
        let mut encoded = Vec::with_capacity(keyed.len() * 5);
        let (mut previous_line, mut previous_character) = (0, 0);
        for (range, token) in keyed {
            let Some(client_type) = type_mapping[token.token_type as usize] else {
                continue;
            };
            let mut client_modifiers = 0u32;
            for (server, client) in modifier_mapping.iter().enumerate() {
                if token.modifiers & (1 << server) != 0 {
                    if let Some(client) = client {
                        client_modifiers |= 1 << client;
                    }
                }
            }
            let (line, character) = (range.start.line, range.start.character);
            // One token per start.
            if !encoded.is_empty() && line == previous_line && character == previous_character {
                continue;
            }
            let delta_line = line - previous_line;
            let delta_character = if delta_line == 0 {
                character - previous_character
            } else {
                character
            };
            encoded.extend([
                delta_line,
                delta_character,
                range.end.character - range.start.character,
                client_type,
                client_modifiers,
            ]);
            previous_line = line;
            previous_character = character;
        }
        encoded
    }
}

/// tsgo `tokenFromDeclarationMapping`.
fn token_from_declaration_mapping(kind: SyntaxKind) -> Option<TokenType> {
    Some(match kind {
        SyntaxKind::VariableDeclaration => TokenType::Variable,
        SyntaxKind::Parameter => TokenType::Parameter,
        SyntaxKind::PropertyDeclaration => TokenType::Property,
        SyntaxKind::ModuleDeclaration => TokenType::Namespace,
        SyntaxKind::EnumDeclaration => TokenType::Enum,
        SyntaxKind::EnumMember => TokenType::EnumMember,
        SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression => TokenType::Class,
        SyntaxKind::MethodDeclaration | SyntaxKind::MethodSignature => TokenType::Method,
        SyntaxKind::FunctionDeclaration | SyntaxKind::FunctionExpression => TokenType::Function,
        SyntaxKind::GetAccessor
        | SyntaxKind::SetAccessor
        | SyntaxKind::PropertySignature
        | SyntaxKind::PropertyAssignment
        | SyntaxKind::ShorthandPropertyAssignment => TokenType::Property,
        SyntaxKind::InterfaceDeclaration => TokenType::Interface,
        SyntaxKind::TypeAliasDeclaration => TokenType::Type,
        SyntaxKind::TypeParameter => TokenType::TypeParameter,
        _ => return None,
    })
}

/// tsgo `ast.GetCombinedModifierFlags` with tsgo's modifier lists: a
/// declaration's (its binding pattern's root's) modifiers, with its variable
/// list's and statement's.
fn combined_modifier_flags(source: &tsc_syntax::SourceFile, declaration: NodeId) -> ModifierFlags {
    let mut node = node_util::get_root_declaration(source, declaration);
    let mut flags = tsgo_modifier_flags(source, node);
    if source.arena.node(node).kind == SyntaxKind::VariableDeclaration {
        match source.arena.node(node).parent {
            Some(parent) => node = parent,
            None => return flags,
        }
    }
    if source.arena.node(node).kind == SyntaxKind::VariableDeclarationList {
        flags |= tsgo_modifier_flags(source, node);
        match source.arena.node(node).parent {
            Some(parent) => node = parent,
            None => return flags,
        }
    }
    if source.arena.node(node).kind == SyntaxKind::VariableStatement {
        flags |= tsgo_modifier_flags(source, node);
    }
    flags
}

/// tsgo `Node.ModifierFlags()`: the node's modifiers, with (in a JavaScript
/// file) the modifiers tsgo's parser reparses from its last JSDoc's
/// `@readonly`, `@private`, `@public`, `@protected` and `@override` tags on a
/// property, a constructor, an assignment, or a method or accessor parsed
/// outside every object literal (tsgo's `PCObjectLiteralMembers` context).
fn tsgo_modifier_flags(source: &tsc_syntax::SourceFile, node: NodeId) -> ModifierFlags {
    let mut flags = node_util::get_syntactic_modifier_flags(source, node);
    let root = source.arena.node(source.root);
    if !NodeFlags::from_bits(root.flags).contains(NodeFlags::JAVA_SCRIPT_FILE) {
        return flags;
    }
    let record = source.arena.node(node);
    let reparsed = match record.kind {
        SyntaxKind::PropertyDeclaration
        | SyntaxKind::Constructor
        | SyntaxKind::BinaryExpression => true,
        SyntaxKind::MethodDeclaration | SyntaxKind::GetAccessor | SyntaxKind::SetAccessor => {
            let mut ancestor = record.parent;
            let mut in_object_literal = false;
            while let Some(current) = ancestor {
                if source.arena.node(current).kind == SyntaxKind::ObjectLiteralExpression {
                    in_object_literal = true;
                    break;
                }
                ancestor = source.arena.node(current).parent;
            }
            !in_object_literal
        }
        _ => false,
    };
    if !reparsed {
        return flags;
    }
    // The tags of the node's (or its expression statement's) JSDoc.
    let host = match record.parent {
        Some(parent)
            if record.kind == SyntaxKind::BinaryExpression
                && source.arena.node(parent).kind == SyntaxKind::ExpressionStatement =>
        {
            parent
        }
        _ => node,
    };
    let Some(js_doc) = source.arena.node(host).js_doc else {
        return flags;
    };
    let comments = source.arena.node_array(js_doc).nodes;
    // tsgo reparses the hosted tags of the last JSDoc comment only.
    if let Some(&last) = comments.last() {
        if let NodeData::JSDoc(data) = &source.arena.node(last).data {
            if let Some(tags) = data.tags {
                for &tag in source.arena.node_array(tags).nodes {
                    flags |= match source.arena.node(tag).kind {
                        SyntaxKind::JSDocReadonlyTag => ModifierFlags::READONLY,
                        SyntaxKind::JSDocPrivateTag => ModifierFlags::PRIVATE,
                        SyntaxKind::JSDocPublicTag => ModifierFlags::PUBLIC,
                        SyntaxKind::JSDocProtectedTag => ModifierFlags::PROTECTED,
                        SyntaxKind::JSDocOverrideTag => ModifierFlags::OVERRIDE,
                        _ => ModifierFlags::NONE,
                    };
                }
            }
        }
    }
    flags
}

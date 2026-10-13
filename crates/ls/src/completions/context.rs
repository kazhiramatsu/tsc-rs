//! tsgo's completion context: the tokens around the position and what they
//! say about the location (`completions.go`, `utilities.go`,
//! `lsutil/completednode.go`).

use tsc_binder::node_util;
use tsc_checker::state::{CheckResult, SignatureId, SignatureKind};
use tsc_syntax::{
    identifier_to_keyword_kind, Node, NodeArrayId, NodeData, NodeId, SourceFile, SyntaxKind,
    TokenScanner,
};

use crate::astnav::{Found, Navigator, Token};
use crate::request::Request;
use crate::tree::View;
use crate::utilities::{is_class_element, is_function_like_kind, touching, Touching};

use super::keywords::{is_class_member_completion_keyword, is_parameter_property_modifier};

/// A completion request: the checker, the file and the position.
pub(crate) struct Completions<'r, 's, 'a> {
    pub(crate) request: &'r mut Request<'s, 'a>,
    pub(crate) file: usize,
    pub(crate) source: &'a SourceFile,
    pub(crate) navigator: Navigator<'a>,
    pub(crate) position: u32,
    line_starts: Vec<usize>,
}

impl<'r, 's, 'a> Completions<'r, 's, 'a> {
    pub(crate) fn new(request: &'r mut Request<'s, 'a>, file: usize, position: u32) -> Self {
        let source = request.state.binder.source(file);
        Self {
            request,
            file,
            source,
            navigator: Navigator::new(source),
            position,
            line_starts: tsc_emitter::source_mapper::compute_ecma_line_starts(source.text()),
        }
    }

    pub(crate) fn text(&self) -> &'a str {
        self.source.text()
    }

    /// A node of any file of the program.
    pub(crate) fn record(&self, node: NodeId) -> &'a Node {
        self.request.state.binder.node_record(node)
    }

    /// The file of a node of the program.
    pub(crate) fn source_of(&self, node: NodeId) -> &'a SourceFile {
        self.request.state.binder.source_of_node(node)
    }

    pub(crate) fn node_kind(&self, node: NodeId) -> SyntaxKind {
        self.record(node).kind
    }

    pub(crate) fn data(&self, node: NodeId) -> &'a NodeData {
        &self.record(node).data
    }

    pub(crate) fn parent_of(&self, node: NodeId) -> Option<NodeId> {
        self.record(node).parent
    }

    pub(crate) fn kind(&self, token: Touching) -> SyntaxKind {
        match token {
            Touching::Node(node) => self.node_kind(node),
            Touching::Token { kind, .. } => kind,
        }
    }

    pub(crate) fn parent(&self, token: Touching) -> Option<NodeId> {
        match token {
            Touching::Node(node) => self.parent_of(node),
            Touching::Token { parent, .. } => Some(parent),
        }
    }

    pub(crate) fn pos(&self, token: Touching) -> u32 {
        match token {
            Touching::Node(node) => self.record(node).pos,
            Touching::Token { pos, .. } => pos,
        }
    }

    pub(crate) fn end(&self, token: Touching) -> u32 {
        match token {
            Touching::Node(node) => self.record(node).end,
            Touching::Token { end, .. } => end,
        }
    }

    pub(crate) fn node_list(&self, list: Option<NodeArrayId>) -> &'a [NodeId] {
        list.map_or(&[], |list| self.request.state.binder.node_array(list).nodes)
    }

    /// A node list's range.
    pub(crate) fn list_range(&self, list: NodeArrayId) -> (u32, u32) {
        let array = self.request.state.binder.node_array(list);
        (array.pos, array.end)
    }

    /// The navigator's view of a token.
    pub(crate) fn found(&self, token: Touching) -> Found {
        match token {
            Touching::Node(node) => Found::Node(View::Node(node)),
            Touching::Token {
                kind,
                pos,
                end,
                parent,
            } => Found::Token(Token {
                kind,
                pos,
                end,
                parent: View::Node(parent),
            }),
        }
    }

    /// tsgo `GetStartOfNode(node, file, false)`.
    pub(crate) fn start(&self, token: Touching) -> u32 {
        self.navigator.start_of_node(self.found(token), false)
    }

    pub(crate) fn node_start(&self, node: NodeId) -> u32 {
        self.start(Touching::Node(node))
    }

    /// tsgo `FindPrecedingToken`.
    pub(crate) fn preceding_token(&self, position: u32) -> Option<Touching> {
        self.navigator.preceding_token(position).and_then(touching)
    }

    /// An identifier's (or private identifier's) text.
    pub(crate) fn identifier_text(&self, token: Touching) -> Option<&'a str> {
        match token {
            Touching::Node(node) => node_util::id_text(self.source_of(node), node),
            Touching::Token { .. } => None,
        }
    }

    /// tsgo `keywordForNode`: an identifier's keyword, else the token's
    /// kind.
    pub(crate) fn keyword_for_node(&self, token: Touching) -> SyntaxKind {
        if self.kind(token) == SyntaxKind::Identifier {
            return self
                .identifier_text(token)
                .and_then(identifier_to_keyword_kind)
                .unwrap_or(SyntaxKind::Unknown);
        }
        self.kind(token)
    }

    /// tsgo `getLineOfPosition` (`GetECMALineOfPosition`).
    pub(crate) fn line_of(&self, position: u32) -> usize {
        match self.line_starts.binary_search(&(position as usize)) {
            Ok(line) => line,
            Err(insertion) => insertion.saturating_sub(1),
        }
    }

    /// tsgo `format.GetLineStartPositionForPosition`.
    pub(crate) fn line_start_of(&self, position: u32) -> u32 {
        self.line_starts[self.line_of(position)] as u32
    }

    /// tsgo `getLineEndOfPosition`.
    pub(crate) fn line_end_of(&self, position: u32) -> u32 {
        let line = self.line_of(position);
        let text = self.text();
        let last = if line + 1 >= self.line_starts.len() {
            self.record(self.source.root).end as usize
        } else {
            self.line_starts[line + 1] - 1
        };
        let bytes = text.as_bytes();
        if last > 0 && last < bytes.len() && bytes[last] == b'\n' && bytes[last - 1] == b'\r' {
            return (last - 1) as u32;
        }
        last as u32
    }

    /// tsgo `ast.FindAncestor` from `node`.
    pub(crate) fn find_ancestor(
        &self,
        mut node: Option<NodeId>,
        test: impl Fn(&Self, NodeId) -> bool,
    ) -> Option<NodeId> {
        while let Some(current) = node {
            if test(self, current) {
                return Some(current);
            }
            node = self.parent_of(current);
        }
        None
    }

    pub(crate) fn name_of(&self, node: NodeId) -> Option<NodeId> {
        node_util::name_field_of(self.source_of(node), node)
    }

    /// tsgo `Node.Expression`.
    pub(crate) fn expression_of(&self, node: NodeId) -> Option<NodeId> {
        match self.data(node) {
            NodeData::PropertyAccessExpression(data) => data.expression,
            NodeData::ElementAccessExpression(data) => data.expression,
            NodeData::CallExpression(data) => data.expression,
            NodeData::NewExpression(data) => data.expression,
            NodeData::NonNullExpression(data) => data.expression,
            NodeData::ParenthesizedExpression(data) => data.expression,
            NodeData::ExpressionStatement(data) => data.expression,
            NodeData::TypeOfExpression(data) => data.expression,
            NodeData::DeleteExpression(data) => data.expression,
            NodeData::VoidExpression(data) => data.expression,
            NodeData::AwaitExpression(data) => data.expression,
            NodeData::YieldExpression(data) => data.expression,
            NodeData::SpreadElement(data) => data.expression,
            NodeData::SpreadAssignment(data) => data.expression,
            NodeData::SwitchStatement(data) => data.expression,
            NodeData::CaseClause(data) => data.expression,
            NodeData::IfStatement(data) => data.expression,
            NodeData::WhileStatement(data) => data.expression,
            NodeData::DoStatement(data) => data.expression,
            NodeData::ForInStatement(data) => data.expression,
            NodeData::ForOfStatement(data) => data.expression,
            NodeData::WithStatement(data) => data.expression,
            NodeData::ReturnStatement(data) => data.expression,
            NodeData::ThrowStatement(data) => data.expression,
            NodeData::ExpressionWithTypeArguments(data) => data.expression,
            NodeData::TemplateSpan(data) => data.expression,
            NodeData::Decorator(data) => data.expression,
            NodeData::ComputedPropertyName(data) => data.expression,
            NodeData::JsxExpression(data) => data.expression,
            NodeData::JsxSpreadAttribute(data) => data.expression,
            NodeData::ExportAssignment(data) => data.expression,
            NodeData::ExternalModuleReference(data) => data.expression,
            NodeData::TypeAssertionExpression(data) => data.expression,
            NodeData::AsExpression(data) => data.expression,
            NodeData::SatisfiesExpression(data) => data.expression,
            NodeData::PartiallyEmittedExpression(data) => data.expression,
            _ => None,
        }
    }

    /// tsgo `Node.Initializer`.
    pub(crate) fn initializer_of(&self, node: NodeId) -> Option<NodeId> {
        match self.data(node) {
            NodeData::VariableDeclaration(data) => data.initializer,
            NodeData::Parameter(data) => data.initializer,
            NodeData::BindingElement(data) => data.initializer,
            NodeData::PropertyDeclaration(data) => data.initializer,
            NodeData::PropertyAssignment(data) => data.initializer,
            NodeData::EnumMember(data) => data.initializer,
            NodeData::JsxAttribute(data) => data.initializer,
            NodeData::ForStatement(data) => data.initializer,
            NodeData::ForInStatement(data) => data.initializer,
            NodeData::ForOfStatement(data) => data.initializer,
            _ => None,
        }
    }

    /// tsgo `Node.Type`: the type annotation (or return type) of a node.
    pub(crate) fn type_of(&self, node: NodeId) -> Option<NodeId> {
        self.request.state.type_annotation_of(node)
    }

    /// tsgo `Node.Body`.
    pub(crate) fn body_of(&self, node: NodeId) -> Option<NodeId> {
        node_util::body_of(self.source_of(node), node)
    }

    /// tsgo `Node.TypeParameterList` of a class.
    fn class_type_parameters(&self, node: NodeId) -> Option<NodeArrayId> {
        match self.data(node) {
            NodeData::ClassDeclaration(data) => data.type_parameters,
            NodeData::ClassExpression(data) => data.type_parameters,
            _ => None,
        }
    }

    /// tsgo `Node.ParameterList`.
    pub(crate) fn parameter_list_of(&self, node: NodeId) -> Option<NodeArrayId> {
        match self.data(node) {
            NodeData::FunctionDeclaration(data) => data.parameters,
            NodeData::FunctionExpression(data) => data.parameters,
            NodeData::ArrowFunction(data) => data.parameters,
            NodeData::MethodDeclaration(data) => data.parameters,
            NodeData::MethodSignature(data) => data.parameters,
            NodeData::Constructor(data) => data.parameters,
            NodeData::GetAccessor(data) => data.parameters,
            NodeData::SetAccessor(data) => data.parameters,
            NodeData::CallSignature(data) => data.parameters,
            NodeData::ConstructSignature(data) => data.parameters,
            NodeData::IndexSignature(data) => data.parameters,
            NodeData::FunctionType(data) => data.parameters,
            NodeData::ConstructorType(data) => data.parameters,
            NodeData::JSDocSignature(data) => data.parameters,
            _ => None,
        }
    }

    /// tsgo `Node.TypeParameterList`.
    pub(crate) fn type_parameter_list_of(&self, node: NodeId) -> Option<NodeArrayId> {
        match self.data(node) {
            NodeData::FunctionDeclaration(data) => data.type_parameters,
            NodeData::FunctionExpression(data) => data.type_parameters,
            NodeData::ArrowFunction(data) => data.type_parameters,
            NodeData::MethodDeclaration(data) => data.type_parameters,
            NodeData::MethodSignature(data) => data.type_parameters,
            NodeData::Constructor(data) => data.type_parameters,
            NodeData::GetAccessor(data) => data.type_parameters,
            NodeData::SetAccessor(data) => data.type_parameters,
            NodeData::CallSignature(data) => data.type_parameters,
            NodeData::ConstructSignature(data) => data.type_parameters,
            NodeData::FunctionType(data) => data.type_parameters,
            NodeData::ConstructorType(data) => data.type_parameters,
            NodeData::ClassDeclaration(data) => data.type_parameters,
            NodeData::ClassExpression(data) => data.type_parameters,
            NodeData::InterfaceDeclaration(data) => data.type_parameters,
            NodeData::TypeAliasDeclaration(data) => data.type_parameters,
            NodeData::JSDocTemplateTag(data) => data.type_parameters,
            _ => None,
        }
    }

    /// tsgo `Node.ModuleSpecifier`.
    pub(crate) fn module_specifier_of(&self, node: NodeId) -> Option<NodeId> {
        match self.data(node) {
            NodeData::ImportDeclaration(data) => data.module_specifier,
            NodeData::ExportDeclaration(data) => data.module_specifier,
            NodeData::JSDocImportTag(data) => data.module_specifier,
            _ => None,
        }
    }

    /// tsgo `ast.IsDeclarationName`.
    pub(crate) fn is_declaration_name(&self, token: Touching) -> bool {
        match token {
            Touching::Node(node) => self.request.is_declaration_name(node),
            Touching::Token { .. } => false,
        }
    }

    /// tsgo `ast.NodeIsMissing`.
    pub(crate) fn node_is_missing(&self, node: Option<NodeId>) -> bool {
        node.is_none_or(|node| node_util::node_is_missing(self.source_of(node), Some(node)))
    }

    /// tsgo `astnav.FindChildOfKind`.
    pub(crate) fn child_of_kind(&self, node: NodeId, kind: SyntaxKind) -> Option<Touching> {
        self.navigator
            .child_of_kind(View::Node(node), kind)
            .and_then(touching)
    }

    /// tsgo `hasChildOfKind`.
    fn has_child_of_kind(&self, node: NodeId, kind: SyntaxKind) -> bool {
        self.child_of_kind(node, kind).is_some()
    }

    /// tsgo `lsutil.GetLastToken`.
    pub(crate) fn last_token(&self, node: NodeId) -> Option<Touching> {
        self.navigator
            .last_token(View::Node(node))
            .and_then(touching)
    }

    /// tsgo `lsutil.GetFirstToken`.
    pub(crate) fn first_token(&self, node: NodeId) -> Option<Touching> {
        self.navigator
            .first_token(View::Node(node))
            .and_then(touching)
    }

    /// tsgo `lsutil.GetLastChild`.
    pub(crate) fn last_child(&self, node: NodeId) -> Option<Touching> {
        self.navigator
            .last_child(View::Node(node))
            .and_then(touching)
    }

    /// tsgo `getRelevantTokens`: the token the completion follows (the one
    /// before an identifier or keyword being written) and the previous
    /// token.
    pub(crate) fn relevant_tokens(&self, position: u32) -> (Option<Touching>, Option<Touching>) {
        let previous = self.preceding_token(position);
        if let Some(previous) = previous {
            let kind = self.kind(previous);
            if position <= self.end(previous)
                && (matches!(kind, SyntaxKind::Identifier | SyntaxKind::PrivateIdentifier)
                    || crate::astnav::is_keyword_kind(kind))
            {
                return (self.preceding_token(self.pos(previous)), Some(previous));
            }
        }
        (previous, previous)
    }

    /// tsgo `getRangeOfEnclosingComment`: the comment the position is in.
    pub(crate) fn enclosing_comment(
        &self,
        position: u32,
        preceding: Option<Touching>,
        at_position: Touching,
    ) -> Option<(u32, u32)> {
        let mut token = at_position;
        let js_doc = self.find_ancestor(
            match token {
                Touching::Node(node) => Some(node),
                Touching::Token { parent, .. } => Some(parent),
            },
            |this, node| this.node_kind(node) == SyntaxKind::JSDoc,
        );
        if let Some(js_doc) = js_doc {
            token = Touching::Node(self.parent_of(js_doc).expect("a JSDoc comment's host"));
        }
        let start = self.start(token);
        if start <= position && position < self.end(token) {
            return None;
        }
        let text = self.text();
        let mut ranges = Vec::new();
        if let Some(preceding) = preceding {
            ranges.extend(tsc_syntax::comment_ranges(
                text,
                self.end(preceding) as usize,
                true,
            ));
        }
        if self.kind(token) != SyntaxKind::JsxText {
            ranges.extend(tsc_syntax::comment_ranges(
                text,
                self.pos(token) as usize,
                false,
            ));
        }
        let position = position as usize;
        ranges
            .into_iter()
            .find(|&(pos, end)| {
                let single_line = text[pos..].starts_with("//");
                (pos < position && position < end)
                    || position == end && (single_line || position == text.len())
            })
            .map(|(pos, end)| (pos as u32, end as u32))
    }

    /// tsgo `IsInString`.
    pub(crate) fn is_in_string(&self, position: u32, previous: Option<Touching>) -> bool {
        let Some(previous) = previous else {
            return false;
        };
        if !is_string_text_containing_kind(self.kind(previous)) {
            return false;
        }
        let start = self.start(previous);
        let end = self.end(previous);
        if start < position && position < end {
            return true;
        }
        position == end && self.is_unterminated_literal(previous)
    }

    /// tsgo `ast.IsUnterminatedLiteral`: the token read again, as tsc-rs's
    /// tree keeps no unterminated flag on a string or template literal.
    pub(crate) fn is_unterminated_literal(&self, token: Touching) -> bool {
        let kind = self.kind(token);
        if !(is_literal_kind(kind) || is_template_literal_kind(kind)) {
            return false;
        }
        let mut scanner = TokenScanner::new(self.source, self.pos(token) as usize);
        if matches!(kind, SyntaxKind::TemplateMiddle | SyntaxKind::TemplateTail) {
            scanner.re_scan_template_token(false);
        }
        scanner.token() == kind && scanner.is_unterminated()
    }

    /// tsgo `isValidTrigger`.
    pub(crate) fn is_valid_trigger(
        &self,
        trigger: &str,
        context: Option<Touching>,
        position: u32,
    ) -> bool {
        match trigger {
            "." | "@" => true,
            "\"" | "'" | "`" => context.is_some_and(|context| {
                matches!(
                    self.kind(context),
                    SyntaxKind::StringLiteral
                        | SyntaxKind::NoSubstitutionTemplateLiteral
                        | SyntaxKind::TemplateExpression
                        | SyntaxKind::TaggedTemplateExpression
                ) && position == self.start(context) + 1
            }),
            "#" => context.is_some_and(|context| {
                self.kind(context) == SyntaxKind::PrivateIdentifier
                    && self
                        .find_ancestor(self.parent(context), |this, node| {
                            is_class_like(this.node_kind(node))
                        })
                        .is_some()
            }),
            "<" => context.is_some_and(|context| {
                self.kind(context) == SyntaxKind::LessThanToken && {
                    let parent = self.parent(context).expect("a token's parent");
                    self.node_kind(parent) != SyntaxKind::BinaryExpression
                        || self.binary_expression_may_be_open_tag(parent)
                }
            }),
            "/" => context.is_some_and(|context| {
                if is_string_literal_like(self.kind(context)) {
                    return self.try_get_import_from_module_specifier(context).is_some();
                }
                self.kind(context) == SyntaxKind::LessThanSlashToken
                    && self.parent(context).is_some_and(|parent| {
                        self.node_kind(parent) == SyntaxKind::JsxClosingElement
                    })
            }),
            " " => context.is_some_and(|context| {
                self.kind(context) == SyntaxKind::ImportKeyword
                    && self
                        .parent(context)
                        .is_some_and(|parent| self.node_kind(parent) == SyntaxKind::SourceFile)
            }),
            "*" => self.is_potentially_valid_js_doc_snippet_completion_position(position),
            _ => panic!("Unknown trigger character: {trigger}"),
        }
    }

    /// tsgo `binaryExpressionMayBeOpenTag`.
    pub(crate) fn binary_expression_may_be_open_tag(&self, binary: NodeId) -> bool {
        let NodeData::BinaryExpression(data) = self.data(binary) else {
            return false;
        };
        self.node_is_missing(data.left)
    }

    /// tsgo `ast.TryGetImportFromModuleSpecifier`.
    fn try_get_import_from_module_specifier(&self, token: Touching) -> Option<NodeId> {
        let Touching::Node(node) = token else {
            return None;
        };
        let parent = self.parent_of(node)?;
        match self.data(parent) {
            NodeData::ImportDeclaration(data) if data.module_specifier == Some(node) => {
                Some(parent)
            }
            NodeData::ExportDeclaration(data) if data.module_specifier == Some(node) => {
                Some(parent)
            }
            NodeData::JSDocImportTag(data) if data.module_specifier == Some(node) => Some(parent),
            NodeData::ExternalModuleReference(_) => {
                let grandparent = self.parent_of(parent)?;
                (self.node_kind(grandparent) == SyntaxKind::ImportEqualsDeclaration)
                    .then_some(grandparent)
            }
            NodeData::CallExpression(data) => {
                let first = self.node_list(data.arguments).first().copied();
                let is_import_call = data.expression.is_some_and(|expression| {
                    self.node_kind(expression) == SyntaxKind::ImportKeyword
                });
                let is_require = data.expression.is_some_and(|expression| {
                    self.identifier_text(Touching::Node(expression)) == Some("require")
                });
                ((is_import_call || is_require) && first == Some(node)).then_some(parent)
            }
            NodeData::LiteralType(_) => {
                let grandparent = self.parent_of(parent)?;
                (self.node_kind(grandparent) == SyntaxKind::ImportType).then_some(grandparent)
            }
            _ => None,
        }
    }

    /// tsgo `isPotentiallyValidJSDocSnippetCompletionPosition`; JSDoc
    /// snippets come with the JSDoc completions.
    fn is_potentially_valid_js_doc_snippet_completion_position(&self, _position: u32) -> bool {
        false
    }
}

/// tsgo `ast.IsClassLike`.
pub(crate) fn is_class_like(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
    )
}

/// tsgo `ast.IsTypeElement`.
pub(crate) fn is_type_element(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ConstructSignature
            | SyntaxKind::CallSignature
            | SyntaxKind::PropertySignature
            | SyntaxKind::MethodSignature
            | SyntaxKind::IndexSignature
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::NotEmittedTypeElement
    )
}

/// tsgo `ast.IsObjectTypeDeclaration`.
pub(crate) fn is_object_type_declaration(kind: SyntaxKind) -> bool {
    is_class_like(kind)
        || matches!(
            kind,
            SyntaxKind::InterfaceDeclaration | SyntaxKind::TypeLiteral
        )
}

/// tsgo `ast.IsStringTextContainingNode`.
pub(crate) fn is_string_text_containing_kind(kind: SyntaxKind) -> bool {
    kind == SyntaxKind::StringLiteral || is_template_literal_kind(kind)
}

/// tsgo `ast.IsTemplateLiteralKind`.
pub(crate) fn is_template_literal_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::TemplateHead
            | SyntaxKind::TemplateMiddle
            | SyntaxKind::TemplateTail
    )
}

/// tsgo `ast.IsLiteralKind`.
pub(crate) fn is_literal_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::NumericLiteral
            | SyntaxKind::BigIntLiteral
            | SyntaxKind::StringLiteral
            | SyntaxKind::JsxText
            | SyntaxKind::JsxTextAllWhiteSpaces
            | SyntaxKind::RegularExpressionLiteral
            | SyntaxKind::NoSubstitutionTemplateLiteral
    )
}

/// tsgo `ast.IsStringLiteralLike`.
pub(crate) fn is_string_literal_like(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::StringLiteral | SyntaxKind::NoSubstitutionTemplateLiteral
    )
}

/// tsgo `isFunctionLikeButNotConstructor`.
fn is_function_like_but_not_constructor(kind: SyntaxKind) -> bool {
    is_function_like_kind(kind) && kind != SyntaxKind::Constructor
}

/// tsgo `ast.IsTypeNodeKind`.
pub(crate) fn is_type_node_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::AnyKeyword
            | SyntaxKind::UnknownKeyword
            | SyntaxKind::NumberKeyword
            | SyntaxKind::BigIntKeyword
            | SyntaxKind::ObjectKeyword
            | SyntaxKind::BooleanKeyword
            | SyntaxKind::StringKeyword
            | SyntaxKind::SymbolKeyword
            | SyntaxKind::VoidKeyword
            | SyntaxKind::UndefinedKeyword
            | SyntaxKind::NeverKeyword
            | SyntaxKind::IntrinsicKeyword
            | SyntaxKind::ExpressionWithTypeArguments
            | SyntaxKind::JSDocAllType
            | SyntaxKind::JSDocNullableType
            | SyntaxKind::JSDocNonNullableType
            | SyntaxKind::JSDocOptionalType
            | SyntaxKind::JSDocVariadicType
    ) || (SyntaxKind::FirstTypeNode.value() <= kind.value()
        && kind.value() <= SyntaxKind::LastTypeNode.value())
}

impl Completions<'_, '_, '_> {
    /// tsgo `isCompletionListBlocker`.
    pub(crate) fn is_completion_list_blocker(
        &mut self,
        context: Touching,
        previous: Option<Touching>,
        location: Touching,
        position: u32,
    ) -> CheckResult<bool> {
        Ok(
            self.is_in_string_or_regular_expression_or_template_literal(context, position)
                || self.is_solely_identifier_definition_location(context, previous, position)?
                || self.is_dot_of_numeric_literal(context)
                || self.is_in_jsx_text(context, location)
                || self.kind(context) == SyntaxKind::BigIntLiteral,
        )
    }

    /// tsgo `isInStringOrRegularExpressionOrTemplateLiteral`.
    fn is_in_string_or_regular_expression_or_template_literal(
        &self,
        context: Touching,
        position: u32,
    ) -> bool {
        let kind = self.kind(context);
        (kind == SyntaxKind::RegularExpressionLiteral || is_string_text_containing_kind(kind))
            && self.pos(context) < position
            && position < self.end(context)
            || position == self.end(context)
                && (self.is_unterminated_literal(context)
                    || kind == SyntaxKind::RegularExpressionLiteral)
    }

    /// tsgo `isSolelyIdentifierDefinitionLocation`: whether the position
    /// surely defines a new name.
    fn is_solely_identifier_definition_location(
        &mut self,
        context: Touching,
        previous: Option<Touching>,
        position: u32,
    ) -> CheckResult<bool> {
        let parent = self.parent(context).expect("a context token's parent");
        let containing_kind = self.node_kind(parent);
        match self.kind(context) {
            SyntaxKind::CommaToken => {
                return Ok(containing_kind == SyntaxKind::VariableDeclaration
                    || (containing_kind == SyntaxKind::VariableDeclarationList
                        && !self.is_possibly_type_argument_position(Some(context))?)
                    || containing_kind == SyntaxKind::VariableStatement
                    || containing_kind == SyntaxKind::EnumDeclaration
                    || is_function_like_but_not_constructor(containing_kind)
                    || containing_kind == SyntaxKind::InterfaceDeclaration
                    || containing_kind == SyntaxKind::ArrayBindingPattern
                    || containing_kind == SyntaxKind::TypeAliasDeclaration
                    || (is_class_like(containing_kind)
                        && self
                            .class_type_parameters(parent)
                            .is_some_and(|list| self.list_range(list).1 >= self.pos(context))));
            }
            SyntaxKind::DotToken | SyntaxKind::OpenBracketToken => {
                return Ok(containing_kind == SyntaxKind::ArrayBindingPattern);
            }
            SyntaxKind::ColonToken => return Ok(containing_kind == SyntaxKind::BindingElement),
            SyntaxKind::OpenParenToken => {
                return Ok(containing_kind == SyntaxKind::CatchClause
                    || is_function_like_but_not_constructor(containing_kind));
            }
            SyntaxKind::OpenBraceToken => return Ok(containing_kind == SyntaxKind::EnumDeclaration),
            SyntaxKind::LessThanToken => {
                return Ok(matches!(
                    containing_kind,
                    SyntaxKind::ClassDeclaration
                        | SyntaxKind::ClassExpression
                        | SyntaxKind::InterfaceDeclaration
                        | SyntaxKind::TypeAliasDeclaration
                ) || is_function_like_kind(containing_kind));
            }
            SyntaxKind::StaticKeyword => {
                return Ok(containing_kind == SyntaxKind::PropertyDeclaration
                    && !self
                        .parent_of(parent)
                        .is_some_and(|grandparent| is_class_like(self.node_kind(grandparent))));
            }
            SyntaxKind::DotDotDotToken => {
                return Ok(containing_kind == SyntaxKind::Parameter
                    || self.parent_of(parent).is_some_and(|grandparent| {
                        self.node_kind(grandparent) == SyntaxKind::ArrayBindingPattern
                    }));
            }
            SyntaxKind::PublicKeyword
            | SyntaxKind::PrivateKeyword
            | SyntaxKind::ProtectedKeyword => {
                return Ok(containing_kind == SyntaxKind::Parameter
                    && !self.parent_of(parent).is_some_and(|grandparent| {
                        self.node_kind(grandparent) == SyntaxKind::Constructor
                    }));
            }
            SyntaxKind::AsKeyword => {
                return Ok(matches!(
                    containing_kind,
                    SyntaxKind::ImportSpecifier
                        | SyntaxKind::ExportSpecifier
                        | SyntaxKind::NamespaceImport
                ));
            }
            SyntaxKind::GetKeyword | SyntaxKind::SetKeyword => {
                return Ok(!self.is_from_object_type_declaration(context));
            }
            SyntaxKind::Identifier => {
                if matches!(
                    containing_kind,
                    SyntaxKind::ImportSpecifier | SyntaxKind::ExportSpecifier
                ) && Touching::Node(self.name_of(parent).unwrap_or(parent)) == context
                    && self.identifier_text(context) == Some("type")
                {
                    // import { type | }
                    return Ok(false);
                }
                let ancestor_variable_declaration = self
                    .find_ancestor(Some(parent), |this, node| {
                        this.node_kind(node) == SyntaxKind::VariableDeclaration
                    });
                if ancestor_variable_declaration.is_some()
                    && self.line_end_of(self.end(context)) < position
                {
                    // let a
                    // |
                    return Ok(false);
                }
            }
            SyntaxKind::ClassKeyword
            | SyntaxKind::EnumKeyword
            | SyntaxKind::InterfaceKeyword
            | SyntaxKind::FunctionKeyword
            | SyntaxKind::VarKeyword
            | SyntaxKind::ImportKeyword
            | SyntaxKind::LetKeyword
            | SyntaxKind::ConstKeyword
            | SyntaxKind::InferKeyword => return Ok(true),
            SyntaxKind::TypeKeyword => {
                // import { type foo| }
                return Ok(containing_kind != SyntaxKind::ImportSpecifier);
            }
            SyntaxKind::AsteriskToken => {
                return Ok(is_function_like_kind(containing_kind)
                    && containing_kind != SyntaxKind::MethodDeclaration);
            }
            _ => {}
        }

        let token_kind = self.keyword_for_node(context);
        // After a keyword of a class member, completions are there.
        if is_class_member_completion_keyword(token_kind)
            && self.is_from_object_type_declaration(context)
        {
            return Ok(false);
        }
        if self.is_constructor_parameter_completion(context) {
            // A constructor parameter completes only at a modifier, or at a
            // parameter's name not being written (`constructor(a |`).
            if self.kind(context) != SyntaxKind::Identifier
                || is_parameter_property_modifier(token_kind)
                || self.is_currently_editing_node(context, position)
            {
                return Ok(false);
            }
        }
        // A keyword may have been read as an identifier.
        match token_kind {
            SyntaxKind::AbstractKeyword
            | SyntaxKind::ClassKeyword
            | SyntaxKind::DeclareKeyword
            | SyntaxKind::EnumKeyword
            | SyntaxKind::FunctionKeyword
            | SyntaxKind::InterfaceKeyword
            | SyntaxKind::LetKeyword
            | SyntaxKind::PrivateKeyword
            | SyntaxKind::ProtectedKeyword
            | SyntaxKind::PublicKeyword
            | SyntaxKind::StaticKeyword
            | SyntaxKind::VarKeyword => return Ok(true),
            SyntaxKind::AsyncKeyword => {
                return Ok(self.node_kind(parent) == SyntaxKind::PropertyDeclaration);
            }
            _ => {}
        }

        // In a class with no `constructor` yet, a completion asked for at
        // white space is not blocked.
        let ancestor_class_like = self.find_ancestor(Some(parent), |this, node| {
            is_class_like(this.node_kind(node))
        });
        if ancestor_class_like.is_some()
            && Some(context) == previous
            && self.is_previous_property_declaration_terminated(context, position)
        {
            return Ok(false);
        }

        let ancestor_property_declaration = self.find_ancestor(Some(parent), |this, node| {
            this.node_kind(node) == SyntaxKind::PropertyDeclaration
        });
        // `constructor` typed after a property declaration.
        if let (Some(property), Some(previous)) = (ancestor_property_declaration, previous) {
            let previous_grandparent = self
                .parent(previous)
                .and_then(|previous_parent| self.parent_of(previous_parent));
            if Some(context) != Some(previous)
                && previous_grandparent.is_some_and(|node| is_class_like(self.node_kind(node)))
                && position <= self.end(previous)
            {
                if self.is_previous_property_declaration_terminated(context, self.end(previous)) {
                    return Ok(false);
                } else if self.kind(context) != SyntaxKind::EqualsToken
                    && (self.initializer_of(property).is_some() || self.type_of(property).is_some())
                {
                    // `class C { blah = c/**/ }` completes, but not
                    // `class C { blah = somewhat c/**/ }` or
                    // `class C { blah: SomeType c/**/ }`.
                    return Ok(true);
                }
            }
        }
        if token_kind == SyntaxKind::ConstKeyword {
            return Ok(true);
        }
        let parent_kind = self.node_kind(parent);
        Ok(self.is_declaration_name(context)
            && parent_kind != SyntaxKind::ShorthandPropertyAssignment
            && parent_kind != SyntaxKind::JsxAttribute
            // Past the name in `class C /**/`, `interface I /**/` or
            // `<T /**/>`, `extends` may come; `class C ex/**/` is the case
            // where the context token is not the previous token.
            && !((is_class_like(parent_kind)
                || matches!(
                    parent_kind,
                    SyntaxKind::InterfaceDeclaration | SyntaxKind::TypeParameter
                ))
                && (Some(context) != previous
                    || previous.is_some_and(|previous| position > self.end(previous)))))
    }

    /// tsgo `isPreviousPropertyDeclarationTerminated`.
    fn is_previous_property_declaration_terminated(
        &self,
        context: Touching,
        position: u32,
    ) -> bool {
        let kind = self.kind(context);
        kind != SyntaxKind::EqualsToken
            && (kind == SyntaxKind::SemicolonToken
                || self.line_of(self.end(context)) != self.line_of(position))
    }

    /// tsgo `isDotOfNumericLiteral`.
    fn is_dot_of_numeric_literal(&self, context: Touching) -> bool {
        self.kind(context) == SyntaxKind::NumericLiteral
            && self.text()[self.pos(context) as usize..self.end(context) as usize].ends_with('.')
    }

    /// tsgo `isInJsxText`.
    fn is_in_jsx_text(&self, context: Touching, location: Touching) -> bool {
        match self.kind(context) {
            SyntaxKind::JsxText => true,
            SyntaxKind::GreaterThanToken => {
                let Some(parent) = self.parent(context) else {
                    return false;
                };
                // <Component<string> /**/ />
                if location == Touching::Node(parent)
                    && matches!(
                        self.node_kind(parent),
                        SyntaxKind::JsxOpeningElement | SyntaxKind::JsxSelfClosingElement
                    )
                {
                    return false;
                }
                match self.node_kind(parent) {
                    // <div>/**/
                    SyntaxKind::JsxOpeningElement => self
                        .parent(location)
                        .is_none_or(|node| self.node_kind(node) != SyntaxKind::JsxOpeningElement),
                    SyntaxKind::JsxClosingElement | SyntaxKind::JsxSelfClosingElement => self
                        .parent_of(parent)
                        .is_some_and(|node| self.node_kind(node) == SyntaxKind::JsxElement),
                    _ => false,
                }
            }
            _ => false,
        }
    }

    /// tsgo `isCurrentlyEditingNode`.
    pub(crate) fn is_currently_editing_node(&self, node: Touching, position: u32) -> bool {
        self.start(node) <= position && position <= self.end(node)
    }

    /// tsgo `isFromObjectTypeDeclaration`.
    pub(crate) fn is_from_object_type_declaration(&self, node: Touching) -> bool {
        let Some(parent) = self.parent(node) else {
            return false;
        };
        let parent_kind = self.node_kind(parent);
        (is_class_element(parent_kind) || is_type_element(parent_kind))
            && self
                .parent_of(parent)
                .is_some_and(|grandparent| is_object_type_declaration(self.node_kind(grandparent)))
    }

    /// tsgo `isConstructorParameterCompletion`.
    pub(crate) fn is_constructor_parameter_completion(&self, node: Touching) -> bool {
        let Some(parent) = self.parent(node) else {
            return false;
        };
        self.node_kind(parent) == SyntaxKind::Parameter
            && self
                .parent_of(parent)
                .is_some_and(|grandparent| self.node_kind(grandparent) == SyntaxKind::Constructor)
            && (is_parameter_property_modifier(self.kind(node)) || self.is_declaration_name(node))
    }

    /// tsgo `tryGetConstructorLikeCompletionContainer`.
    pub(crate) fn try_get_constructor_like_completion_container(
        &self,
        context: Option<Touching>,
    ) -> Option<NodeId> {
        let context = context?;
        let parent = self.parent(context)?;
        match self.kind(context) {
            SyntaxKind::OpenParenToken | SyntaxKind::CommaToken => {
                (self.node_kind(parent) == SyntaxKind::Constructor).then_some(parent)
            }
            _ => {
                if self.is_constructor_parameter_completion(context) {
                    self.parent_of(parent)
                } else {
                    None
                }
            }
        }
    }

    /// tsgo `tryGetFunctionLikeBodyCompletionContainer`: the function whose
    /// body holds the context token, below any class.
    pub(crate) fn try_get_function_like_body_completion_container(
        &self,
        context: Option<Touching>,
    ) -> Option<NodeId> {
        // tsgo's FindAncestorOrQuit from the token itself, the previous
        // node compared with each function's body.
        let mut previous: Option<Touching> = None;
        let mut current = context;
        while let Some(node) = current {
            if let Touching::Node(id) = node {
                let kind = self.node_kind(id);
                if is_class_like(kind) {
                    return None;
                }
                if node_util::is_function_like_declaration_kind(kind)
                    && previous == self.body_of(id).map(Touching::Node)
                {
                    return Some(id);
                }
            }
            previous = Some(node);
            current = self.parent(node).map(Touching::Node);
        }
        None
    }

    /// tsgo `computeCommitCharactersAndIsNewIdentifier`.
    pub(crate) fn compute_commit_characters_and_is_new_identifier(
        &self,
        context: Option<Touching>,
        position: u32,
    ) -> (bool, Vec<String>) {
        let all = || {
            super::ALL_COMMIT_CHARACTERS
                .iter()
                .map(|&c| c.to_owned())
                .collect()
        };
        let no_comma = || {
            super::NO_COMMA_COMMIT_CHARACTERS
                .iter()
                .map(|&c| c.to_owned())
                .collect()
        };
        let Some(context) = context else {
            return (false, all());
        };
        let parent = self.parent(context).expect("a context token's parent");
        let containing_kind = self.node_kind(parent);
        let token_kind = self.keyword_for_node(context);
        // The call's expression on another line: `func\n(a, |`.
        let call_on_other_line = || {
            self.expression_of(parent).is_some_and(|expression| {
                self.line_of(self.record(expression).end) != self.line_of(position)
            })
        };
        match token_kind {
            SyntaxKind::CommaToken => match containing_kind {
                SyntaxKind::CallExpression | SyntaxKind::NewExpression => {
                    if call_on_other_line() {
                        return (true, no_comma());
                    }
                    return (true, all());
                }
                SyntaxKind::BinaryExpression => return (true, no_comma()),
                SyntaxKind::Constructor
                | SyntaxKind::FunctionType
                | SyntaxKind::ObjectLiteralExpression => return (true, Vec::new()),
                SyntaxKind::ArrayLiteralExpression => return (true, all()),
                _ => return (false, all()),
            },
            SyntaxKind::OpenParenToken => match containing_kind {
                SyntaxKind::CallExpression | SyntaxKind::NewExpression => {
                    if call_on_other_line() {
                        return (true, no_comma());
                    }
                    return (true, all());
                }
                SyntaxKind::ParenthesizedExpression => return (true, no_comma()),
                SyntaxKind::Constructor | SyntaxKind::ParenthesizedType => {
                    return (true, Vec::new());
                }
                _ => return (false, all()),
            },
            SyntaxKind::OpenBracketToken => {
                return match containing_kind {
                    SyntaxKind::ArrayLiteralExpression
                    | SyntaxKind::IndexSignature
                    | SyntaxKind::TupleType
                    | SyntaxKind::ComputedPropertyName => (true, all()),
                    _ => (false, all()),
                };
            }
            SyntaxKind::ModuleKeyword
            | SyntaxKind::NamespaceKeyword
            | SyntaxKind::ImportKeyword => {
                return (true, Vec::new());
            }
            SyntaxKind::DotToken => {
                return match containing_kind {
                    SyntaxKind::ModuleDeclaration => (true, Vec::new()),
                    _ => (false, all()),
                };
            }
            SyntaxKind::OpenBraceToken => {
                return match containing_kind {
                    SyntaxKind::ClassDeclaration | SyntaxKind::ObjectLiteralExpression => {
                        (true, Vec::new())
                    }
                    _ => (false, all()),
                };
            }
            SyntaxKind::EqualsToken => {
                return match containing_kind {
                    SyntaxKind::VariableDeclaration | SyntaxKind::BinaryExpression => (true, all()),
                    _ => (false, all()),
                };
            }
            SyntaxKind::TemplateHead => {
                return (containing_kind == SyntaxKind::TemplateExpression, all());
            }
            SyntaxKind::TemplateMiddle => {
                return (containing_kind == SyntaxKind::TemplateSpan, all());
            }
            SyntaxKind::AsyncKeyword => {
                if matches!(
                    containing_kind,
                    SyntaxKind::MethodDeclaration | SyntaxKind::ShorthandPropertyAssignment
                ) {
                    return (true, Vec::new());
                }
                return (false, all());
            }
            SyntaxKind::AsteriskToken => {
                if containing_kind == SyntaxKind::MethodDeclaration {
                    return (true, Vec::new());
                }
                return (false, all());
            }
            _ => {}
        }
        if is_class_member_completion_keyword(token_kind) {
            return (true, Vec::new());
        }
        (false, all())
    }

    /// tsgo `getScopeNode`: the first node that embraces the position.
    pub(crate) fn scope_node(&self, initial: Option<Touching>, position: u32) -> Option<NodeId> {
        let mut scope = initial?;
        loop {
            if self.position_belongs_to_node(scope, position) {
                return match scope {
                    Touching::Node(node) => Some(node),
                    Touching::Token { parent, .. } => Some(parent),
                };
            }
            scope = Touching::Node(self.parent(scope)?);
        }
    }

    /// tsgo `lsutil.PositionBelongsToNode`.
    fn position_belongs_to_node(&self, candidate: Touching, position: u32) -> bool {
        assert!(
            self.pos(candidate) <= position,
            "Expected candidate.pos <= position"
        );
        position < self.end(candidate)
            || match candidate {
                Touching::Node(node) => !self.is_completed_node(Some(node)),
                Touching::Token { pos, end, .. } => pos == end,
            }
    }

    /// tsgo `lsutil.IsCompletedNode`.
    pub(crate) fn is_completed_node(&self, node: Option<NodeId>) -> bool {
        let Some(node) = node else {
            return false;
        };
        let record = self.record(node);
        if record.pos == record.end && record.kind != SyntaxKind::EndOfFileToken {
            return false;
        }
        match (record.kind, &record.data) {
            (
                SyntaxKind::ClassDeclaration
                | SyntaxKind::InterfaceDeclaration
                | SyntaxKind::EnumDeclaration
                | SyntaxKind::ObjectLiteralExpression
                | SyntaxKind::ObjectBindingPattern
                | SyntaxKind::TypeLiteral
                | SyntaxKind::Block
                | SyntaxKind::ModuleBlock
                | SyntaxKind::CaseBlock
                | SyntaxKind::NamedImports
                | SyntaxKind::NamedExports,
                _,
            ) => self
                .navigator
                .node_ends_with(View::Node(node), SyntaxKind::CloseBraceToken),
            (SyntaxKind::CatchClause, NodeData::CatchClause(data)) => {
                self.is_completed_node(data.block)
            }
            (SyntaxKind::NewExpression, NodeData::NewExpression(data))
                if data.arguments.is_none() =>
            {
                true
            }
            (
                SyntaxKind::NewExpression
                | SyntaxKind::CallExpression
                | SyntaxKind::ParenthesizedExpression
                | SyntaxKind::ParenthesizedType,
                _,
            ) => self
                .navigator
                .node_ends_with(View::Node(node), SyntaxKind::CloseParenToken),
            (SyntaxKind::FunctionType | SyntaxKind::ConstructorType, _) => {
                self.is_completed_node(self.type_of(node))
            }
            (
                SyntaxKind::Constructor
                | SyntaxKind::GetAccessor
                | SyntaxKind::SetAccessor
                | SyntaxKind::FunctionDeclaration
                | SyntaxKind::FunctionExpression
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::MethodSignature
                | SyntaxKind::ConstructSignature
                | SyntaxKind::CallSignature
                | SyntaxKind::ArrowFunction,
                _,
            ) => {
                if let Some(body) = self.body_of(node) {
                    return self.is_completed_node(Some(body));
                }
                if let Some(ty) = self.type_of(node) {
                    return self.is_completed_node(Some(ty));
                }
                // An unclosed type parameter list still has its `)`.
                self.has_child_of_kind(node, SyntaxKind::CloseParenToken)
            }
            (SyntaxKind::ModuleDeclaration, NodeData::ModuleDeclaration(data)) => {
                data.body.is_some() && self.is_completed_node(data.body)
            }
            (SyntaxKind::IfStatement, NodeData::IfStatement(data)) => match data.else_statement {
                Some(else_statement) => self.is_completed_node(Some(else_statement)),
                None => self.is_completed_node(data.then_statement),
            },
            (SyntaxKind::ExpressionStatement, NodeData::ExpressionStatement(data)) => {
                self.is_completed_node(data.expression)
                    || self.has_child_of_kind(node, SyntaxKind::SemicolonToken)
            }
            (
                SyntaxKind::ArrayLiteralExpression
                | SyntaxKind::ArrayBindingPattern
                | SyntaxKind::ElementAccessExpression
                | SyntaxKind::ComputedPropertyName
                | SyntaxKind::TupleType,
                _,
            ) => self
                .navigator
                .node_ends_with(View::Node(node), SyntaxKind::CloseBracketToken),
            (SyntaxKind::IndexSignature, _) => match self.type_of(node) {
                Some(ty) => self.is_completed_node(Some(ty)),
                None => self.has_child_of_kind(node, SyntaxKind::CloseBracketToken),
            },
            // A case or default clause has no terminator, so it is never
            // completed.
            (SyntaxKind::CaseClause | SyntaxKind::DefaultClause, _) => false,
            (
                SyntaxKind::ForStatement
                | SyntaxKind::ForInStatement
                | SyntaxKind::ForOfStatement
                | SyntaxKind::WhileStatement,
                _,
            ) => self.is_completed_node(self.statement_of(node)),
            (SyntaxKind::DoStatement, _) => {
                // Roughly: with its `while`, a do statement needs its `)`.
                if self.has_child_of_kind(node, SyntaxKind::WhileKeyword) {
                    return self
                        .navigator
                        .node_ends_with(View::Node(node), SyntaxKind::CloseParenToken);
                }
                self.is_completed_node(self.statement_of(node))
            }
            (SyntaxKind::TypeQuery, NodeData::TypeQuery(data)) => {
                self.is_completed_node(data.expr_name)
            }
            (
                SyntaxKind::TypeOfExpression
                | SyntaxKind::DeleteExpression
                | SyntaxKind::VoidExpression
                | SyntaxKind::YieldExpression
                | SyntaxKind::SpreadElement,
                _,
            ) => self.is_completed_node(self.expression_of(node)),
            (SyntaxKind::TaggedTemplateExpression, NodeData::TaggedTemplateExpression(data)) => {
                self.is_completed_node(data.template)
            }
            (SyntaxKind::TemplateExpression, NodeData::TemplateExpression(data)) => {
                let Some(spans) = data.template_spans else {
                    return false;
                };
                self.is_completed_node(self.node_list(Some(spans)).last().copied())
            }
            (SyntaxKind::TemplateSpan, NodeData::TemplateSpan(data)) => {
                !self.node_is_missing(data.literal)
            }
            (SyntaxKind::ExportDeclaration | SyntaxKind::ImportDeclaration, _) => {
                !self.node_is_missing(self.module_specifier_of(node))
            }
            (SyntaxKind::PrefixUnaryExpression, NodeData::PrefixUnaryExpression(data)) => {
                self.is_completed_node(data.operand)
            }
            (SyntaxKind::BinaryExpression, NodeData::BinaryExpression(data)) => {
                self.is_completed_node(data.right)
            }
            (SyntaxKind::ConditionalExpression, NodeData::ConditionalExpression(data)) => {
                self.is_completed_node(data.when_false)
            }
            _ => true,
        }
    }

    /// tsgo `Node.Statement` of a loop.
    fn statement_of(&self, node: NodeId) -> Option<NodeId> {
        match self.data(node) {
            NodeData::ForStatement(data) => data.statement,
            NodeData::ForInStatement(data) => data.statement,
            NodeData::ForOfStatement(data) => data.statement,
            NodeData::WhileStatement(data) => data.statement,
            NodeData::DoStatement(data) => data.statement,
            _ => None,
        }
    }

    /// tsgo `isContextTokenValueLocation`.
    pub(crate) fn is_context_token_value_location(&self, context: Option<Touching>) -> bool {
        let Some(context) = context else {
            return false;
        };
        let parent_kind = self.parent(context).map(|parent| self.node_kind(parent));
        match self.kind(context) {
            SyntaxKind::TypeOfKeyword => matches!(
                parent_kind,
                Some(SyntaxKind::TypeQuery | SyntaxKind::TypeOfExpression)
            ),
            SyntaxKind::AssertsKeyword => parent_kind == Some(SyntaxKind::TypePredicate),
            _ => false,
        }
    }

    /// tsgo `isContextTokenTypeLocation`.
    pub(crate) fn is_context_token_type_location(&self, context: Option<Touching>) -> bool {
        let Some(context) = context else {
            return false;
        };
        let Some(parent) = self.parent(context) else {
            return false;
        };
        let parent_kind = self.node_kind(parent);
        match self.kind(context) {
            SyntaxKind::ColonToken => {
                matches!(
                    parent_kind,
                    SyntaxKind::PropertyDeclaration
                        | SyntaxKind::PropertySignature
                        | SyntaxKind::Parameter
                        | SyntaxKind::VariableDeclaration
                ) || is_function_like_kind(parent_kind)
            }
            SyntaxKind::EqualsToken => matches!(
                parent_kind,
                SyntaxKind::TypeAliasDeclaration | SyntaxKind::TypeParameter
            ),
            SyntaxKind::AsKeyword => parent_kind == SyntaxKind::AsExpression,
            SyntaxKind::LessThanToken => matches!(
                parent_kind,
                SyntaxKind::TypeReference | SyntaxKind::TypeAssertionExpression
            ),
            SyntaxKind::ExtendsKeyword => parent_kind == SyntaxKind::TypeParameter,
            SyntaxKind::SatisfiesKeyword => parent_kind == SyntaxKind::SatisfiesExpression,
            SyntaxKind::OpenBracketToken | SyntaxKind::CommaToken => {
                parent_kind == SyntaxKind::TupleType
            }
            _ => false,
        }
    }

    /// tsgo `isPossiblyTypeArgumentPosition`.
    pub(crate) fn is_possibly_type_argument_position(
        &mut self,
        token: Option<Touching>,
    ) -> CheckResult<bool> {
        let Some((called, type_argument_count)) = self.possible_type_arguments_info(token) else {
            return Ok(false);
        };
        Ok(self.request.state.is_part_of_type_node(called)
            || !self
                .possible_generic_signatures(called, type_argument_count)?
                .is_empty()
            || self.is_possibly_type_argument_position(Some(Touching::Node(called)))?)
    }

    /// tsgo `getPossibleTypeArgumentsInfo`: for `f <` that may start type
    /// arguments, the identifier called and the type arguments before.
    fn possible_type_arguments_info(&self, token: Option<Touching>) -> Option<(NodeId, usize)> {
        // A file with no `<` has no type arguments.
        if !self.text().contains('<') {
            return None;
        }
        let mut token = token;
        let mut remaining_less_than_tokens = 0;
        let mut type_argument_count = 0;
        while let Some(current) = token {
            match self.kind(current) {
                SyntaxKind::LessThanToken => {
                    // The beginning of the type arguments.
                    let mut preceding = self.preceding_token(self.pos(current));
                    if let Some(found) = preceding {
                        if self.kind(found) == SyntaxKind::QuestionDotToken {
                            preceding = self.preceding_token(self.pos(found));
                        }
                    }
                    let called = preceding?;
                    let Touching::Node(called_node) = called else {
                        return None;
                    };
                    if self.kind(called) != SyntaxKind::Identifier {
                        return None;
                    }
                    if remaining_less_than_tokens == 0 {
                        if self.is_declaration_name(called) {
                            return None;
                        }
                        return Some((called_node, type_argument_count));
                    }
                    remaining_less_than_tokens -= 1;
                    token = Some(called);
                }
                SyntaxKind::GreaterThanGreaterThanGreaterThanToken => {
                    remaining_less_than_tokens += 3;
                }
                SyntaxKind::GreaterThanGreaterThanToken => remaining_less_than_tokens += 2,
                SyntaxKind::GreaterThanToken => remaining_less_than_tokens += 1,
                SyntaxKind::CloseBraceToken => {
                    // An object type: to the matching `{`.
                    token =
                        Some(self.preceding_matching_token(current, SyntaxKind::OpenBraceToken)?);
                }
                SyntaxKind::CloseParenToken => {
                    token =
                        Some(self.preceding_matching_token(current, SyntaxKind::OpenParenToken)?);
                }
                SyntaxKind::CloseBracketToken => {
                    token =
                        Some(self.preceding_matching_token(current, SyntaxKind::OpenBracketToken)?);
                }
                // A type name's tokens.
                SyntaxKind::CommaToken => type_argument_count += 1,
                SyntaxKind::EqualsGreaterThanToken
                | SyntaxKind::Identifier
                | SyntaxKind::StringLiteral
                | SyntaxKind::NumericLiteral
                | SyntaxKind::BigIntLiteral
                | SyntaxKind::TrueKeyword
                | SyntaxKind::FalseKeyword
                | SyntaxKind::TypeOfKeyword
                | SyntaxKind::ExtendsKeyword
                | SyntaxKind::KeyOfKeyword
                | SyntaxKind::DotToken
                | SyntaxKind::BarToken
                | SyntaxKind::QuestionToken
                | SyntaxKind::ColonToken => {}
                kind => {
                    if !is_type_node_kind(kind) {
                        // A token no type has.
                        return None;
                    }
                }
            }
            let current = token.expect("a token");
            token = self.preceding_token(self.pos(current));
        }
        None
    }

    /// tsgo `findPrecedingMatchingToken`.
    fn preceding_matching_token(&self, token: Touching, matching: SyntaxKind) -> Option<Touching> {
        let close_text =
            tsc_syntax::tokens::token_to_string(self.kind(token)).expect("a close token");
        let matching_text = tsc_syntax::tokens::token_to_string(matching).expect("an open token");
        let text = self.text();
        // A text search first: comments can fool it, but it often answers.
        let best_guess = text.rfind(matching_text)?;
        if text
            .rfind(close_text)
            .is_none_or(|close| close < best_guess)
        {
            if let Some(at_guess) = self.preceding_token(best_guess as u32 + 1) {
                if self.kind(at_guess) == matching {
                    return Some(at_guess);
                }
            }
        }
        let token_kind = self.kind(token);
        let mut remaining_matching_tokens = 0;
        let mut token = token;
        loop {
            token = self.preceding_token(self.pos(token))?;
            let kind = self.kind(token);
            if kind == matching {
                if remaining_matching_tokens == 0 {
                    return Some(token);
                }
                remaining_matching_tokens -= 1;
            } else if kind == token_kind {
                remaining_matching_tokens += 1;
            }
        }
    }

    /// tsgo `getPossibleGenericSignatures`.
    fn possible_generic_signatures(
        &mut self,
        called: NodeId,
        type_argument_count: usize,
    ) -> CheckResult<Vec<SignatureId>> {
        let parent = self
            .parent_of(called)
            .expect("a called identifier's parent");
        let optional_chain = node_util::is_optional_chain(self.source, parent);
        let optional_chain_root = node_util::is_optional_chain_root(self.source, parent);
        let kind = if self.node_kind(parent) == SyntaxKind::NewExpression {
            SignatureKind::Construct
        } else {
            SignatureKind::Call
        };
        let state = &mut *self.request.state;
        let mut ty = state.get_type_at_location(called)?;
        if optional_chain {
            ty = if optional_chain_root {
                state.get_non_nullable_type(ty)?
            } else {
                state.get_non_optional_type(ty)
            };
        }
        let signatures = state.get_signatures_of_type(ty, kind)?;
        let mut generic = Vec::new();
        for signature in signatures {
            let data = state.signature_of(signature);
            // tsgo gives a class's construct signatures (declared,
            // default or inherited) the class's local type parameters,
            // an empty list when it has none, never nil as tsc's (and
            // tsc-rs's) undefined is.
            let class_signature = data.declaration.is_some_and(|declaration| {
                state.binder.node_record(declaration).kind == SyntaxKind::Constructor
            }) || data.flags.intersects(tsc_types::SignatureFlags::CONSTRUCT)
                && data
                    .resolved_return_type
                    .resolved()
                    .is_some_and(|return_type| {
                        state
                            .tables
                            .object_flags_of(return_type)
                            .intersects(tsc_types::ObjectFlags::CLASS)
                    });
            let count = match &data.type_parameters {
                Some(parameters) => Some(parameters.len()),
                None if class_signature => Some(0),
                None => None,
            };
            if count.is_some_and(|count| count >= type_argument_count) {
                generic.push(signature);
            }
        }
        Ok(generic)
    }
}

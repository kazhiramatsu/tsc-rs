//! The source file as tsgo's formatter reads it: tsgo's tree over tsc-rs's
//! (token navigation's [`Found`]: a node view, or a token the tree does not
//! keep), the file's ECMAScript lines and the accessors of tsgo's `Node`
//! the formatter asks.

use tsc_syntax::{NodeArrayId, NodeData, NodeId, SourceFile, SyntaxKind};
use tsc_types::NodeFlags;

use crate::astnav::{Found, Navigator};
use crate::tree::{Elements, Property, View};

/// A node of tsgo's tree, or a token.
pub(crate) type Node = Found;

/// A NodeList of tsgo's tree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct List {
    pub(crate) id: NodeArrayId,
    elements: Elements,
}

/// A child tsgo's `VisitEachChild` visits: a node (each modifier is one),
/// or a NodeList it takes as a whole.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Child {
    Node(Node),
    List(List),
}

/// A source file for the formatter.
pub(crate) struct File<'a> {
    pub(crate) source: &'a SourceFile,
    pub(crate) navigator: Navigator<'a>,
    line_starts: Vec<usize>,
}

impl<'a> File<'a> {
    pub(crate) fn new(source: &'a SourceFile) -> Self {
        Self {
            source,
            navigator: Navigator::new(source),
            line_starts: tsc_emitter::source_mapper::compute_ecma_line_starts(source.text()),
        }
    }

    pub(crate) fn text(&self) -> &'a str {
        self.source.text()
    }

    pub(crate) fn root(&self) -> Node {
        Found::Node(View::Node(self.source.root))
    }

    pub(crate) fn kind(&self, node: Node) -> SyntaxKind {
        self.navigator.kind(node)
    }

    pub(crate) fn pos(&self, node: Node) -> usize {
        self.navigator.pos(node) as usize
    }

    pub(crate) fn end(&self, node: Node) -> usize {
        self.navigator.end(node) as usize
    }

    pub(crate) fn parent(&self, node: Node) -> Option<Node> {
        self.navigator.parent(node)
    }

    /// The tsc-rs node of a node view.
    pub(crate) fn id(&self, node: Node) -> Option<NodeId> {
        match node {
            Found::Node(
                View::Node(id) | View::HeritageTypeReference(id) | View::QualifiedName(id),
            ) => Some(id),
            _ => None,
        }
    }

    fn data(&self, node: Node) -> Option<&'a NodeData> {
        match node {
            Found::Node(View::Node(id)) => Some(&self.source.arena.node(id).data),
            _ => None,
        }
    }

    fn flags(&self, node: Node) -> NodeFlags {
        self.id(node).map_or(NodeFlags::from_bits(0), |id| {
            NodeFlags::from_bits(self.source.arena.node(id).flags)
        })
    }

    /// tsgo `NodeFlagsReparsed`: of the views, only the implicit `export`
    /// of a nested namespace.
    pub(crate) fn is_reparsed(&self, node: Node) -> bool {
        matches!(node, Found::Node(View::ImplicitExport(_)))
    }

    /// tsgo `NodeFlagsThisNodeHasError`.
    pub(crate) fn this_node_has_error(&self, node: Node) -> bool {
        self.flags(node).contains(NodeFlags::THIS_NODE_HAS_ERROR)
    }

    /// tsgo `NodeIsMissing`.
    pub(crate) fn node_is_missing(&self, node: Node) -> bool {
        self.pos(node) == self.end(node) && self.kind(node) != SyntaxKind::EndOfFileToken
    }

    /// tsgo `GetTokenPosOfNode(node, file, false)`.
    pub(crate) fn token_start(&self, node: Node) -> usize {
        self.navigator.start_of_node(node, false) as usize
    }

    /// tsgo `withTokenStart`: the node's range from its first token.
    pub(crate) fn with_token_start(&self, node: Node) -> (usize, usize) {
        (self.token_start(node), self.end(node))
    }

    /// tsgo `GetECMALineStarts`.
    pub(crate) fn line_starts(&self) -> &[usize] {
        &self.line_starts
    }

    /// tsgo `GetECMALineOfPosition`.
    pub(crate) fn line_of(&self, pos: usize) -> usize {
        match self.line_starts.binary_search(&pos) {
            Ok(line) => line,
            Err(next) => next.saturating_sub(1),
        }
    }

    /// tsgo `GetECMALineAndByteOffsetOfPosition`.
    pub(crate) fn line_and_offset(&self, pos: usize) -> (usize, usize) {
        let line = self.line_of(pos);
        (line, pos - self.line_starts[line])
    }

    /// tsgo `GetECMAEndLinePosition`: the position before the line's break
    /// or the end of the text (-1 for an empty first line).
    pub(crate) fn end_line_position(&self, line: usize) -> isize {
        let text = self.text();
        let mut pos = self.line_starts[line];
        loop {
            let (ch, size) = decode_rune(text, pos);
            if size == 0 || tsc_syntax::is_line_break(ch) {
                return pos as isize - 1;
            }
            pos += size;
        }
    }

    /// tsgo `file.Diagnostics()`: the byte ranges of the parse errors.
    pub(crate) fn parse_error_ranges(&self) -> Vec<(usize, usize)> {
        let positions = self.source.positions();
        let text_len = self.text().len() as u32;
        let byte = |position: u32| positions.utf16_to_byte(position).unwrap_or(text_len) as usize;
        self.source
            .parse_diagnostics
            .iter()
            .map(|diagnostic| {
                let start = diagnostic.start.unwrap_or(0);
                let end = start.saturating_add(diagnostic.length.unwrap_or(0));
                (byte(start), byte(end))
            })
            .collect()
    }

    /// tsgo `astnav.FindChildOfKind`.
    pub(crate) fn find_child_of_kind(&self, node: Node, kind: SyntaxKind) -> Option<Node> {
        let Found::Node(view) = node else {
            return None;
        };
        self.navigator.child_of_kind(view, kind)
    }

    /// tsgo `astnav.FindPrecedingToken`.
    pub(crate) fn find_preceding_token(&self, position: usize) -> Option<Node> {
        self.navigator.preceding_token(position as u32)
    }

    /// tsgo `astnav.FindPrecedingTokenEx` from the file, JSDoc excluded or
    /// not.
    pub(crate) fn find_preceding_token_excluding_js_doc(&self, position: usize) -> Option<Node> {
        self.navigator
            .preceding_token_from(position as u32, None, true)
    }

    /// tsgo `astnav.FindNextToken`.
    pub(crate) fn find_next_token(&self, previous: Node, parent: Node) -> Option<Node> {
        let Found::Node(parent) = parent else {
            return None;
        };
        self.navigator.next_token(previous, parent)
    }

    /// tsgo `astnav.GetTokenAtPosition`.
    pub(crate) fn token_at_position(&self, position: usize) -> Node {
        self.navigator.token_at(position as u32)
    }

    /// tsgo `lsutil.GetFirstToken`.
    pub(crate) fn first_token(&self, node: Node) -> Option<Node> {
        let Found::Node(view) = node else {
            return None;
        };
        self.navigator.first_token(view)
    }

    /// The children tsgo's `VisitEachChild` visits, in order: each
    /// modifier, and each NodeList as a whole.
    pub(crate) fn children(&self, node: Node) -> Vec<Child> {
        let mut children = Vec::new();
        let Found::Node(view) = node else {
            return children;
        };
        let tree = self.navigator.tree();
        match view {
            View::Node(id) => {
                tree.properties(tree.node(id), |property| match property {
                    Property::Node(Some(child)) => {
                        children.push(Child::Node(Found::Node(View::Node(child))));
                    }
                    Property::Modifiers(Some(list)) | Property::Slice(Some(list)) => {
                        children.extend(
                            self.source
                                .arena
                                .node_array(list)
                                .nodes
                                .iter()
                                .map(|&modifier| Child::Node(Found::Node(View::Node(modifier)))),
                        );
                    }
                    Property::List(Some(list)) => children.push(Child::List(List {
                        id: list,
                        elements: Elements::Plain,
                    })),
                    Property::HeritageTypes(Some(list)) => children.push(Child::List(List {
                        id: list,
                        elements: Elements::HeritageTypes,
                    })),
                    // The implicit `export` of a nested namespace (reparsed)
                    // and JSDoc.
                    _ => {}
                });
            }
            View::HeritageTypeReference(id) => {
                let NodeData::ExpressionWithTypeArguments(element) = &tree.node(id).data else {
                    unreachable!("a heritage element")
                };
                if let Some(expression) = element.expression {
                    children.push(Child::Node(Found::Node(tree.entity_name(expression))));
                }
                if let Some(arguments) = element.type_arguments {
                    children.push(Child::List(List {
                        id: arguments,
                        elements: Elements::Plain,
                    }));
                }
            }
            View::QualifiedName(id) => {
                let NodeData::PropertyAccessExpression(access) = &tree.node(id).data else {
                    unreachable!("a property access")
                };
                if let Some(expression) = access.expression {
                    children.push(Child::Node(Found::Node(tree.entity_name(expression))));
                }
                if let Some(name) = access.name {
                    children.push(Child::Node(Found::Node(View::Node(name))));
                }
            }
            _ => {}
        }
        children
    }

    /// tsgo `ForEachChild`: the children one by one.
    pub(crate) fn each_child(&self, node: Node) -> Vec<Node> {
        let mut nodes = Vec::new();
        for child in self.children(node) {
            match child {
                Child::Node(node) => nodes.push(node),
                Child::List(list) => nodes.extend(self.list_nodes(list)),
            }
        }
        nodes
    }

    pub(crate) fn list_nodes(&self, list: List) -> Vec<Node> {
        let tree = self.navigator.tree();
        self.source
            .arena
            .node_array(list.id)
            .nodes
            .iter()
            .map(|&element| {
                Found::Node(match list.elements {
                    Elements::HeritageTypes => tree.heritage_element(element),
                    _ => View::Node(element),
                })
            })
            .collect()
    }

    /// A NodeList's position and end.
    pub(crate) fn list_range(&self, list: NodeArrayId) -> (usize, usize) {
        let array = self.source.arena.node_array(list);
        (array.pos as usize, array.end as usize)
    }

    pub(crate) fn list(&self, id: NodeArrayId) -> List {
        List {
            id,
            elements: Elements::Plain,
        }
    }

    /// The node `id` as tsgo's tree has it.
    pub(crate) fn node_of(&self, id: Option<NodeId>) -> Option<Node> {
        id.map(|id| Found::Node(self.navigator.tree().view_of(id)))
    }

    /// tsgo `Node.TypeParameterList`.
    pub(crate) fn type_parameter_list(&self, node: Node) -> Option<NodeArrayId> {
        match self.data(node)? {
            NodeData::ClassDeclaration(data) => data.type_parameters,
            NodeData::ClassExpression(data) => data.type_parameters,
            NodeData::InterfaceDeclaration(data) => data.type_parameters,
            NodeData::TypeAliasDeclaration(data) => data.type_parameters,
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
            NodeData::IndexSignature(data) => data.type_parameters,
            NodeData::FunctionType(data) => data.type_parameters,
            NodeData::ConstructorType(data) => data.type_parameters,
            _ => None,
        }
    }

    /// tsgo `Node.ParameterList`.
    pub(crate) fn parameter_list(&self, node: Node) -> Option<NodeArrayId> {
        match self.data(node)? {
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
            _ => None,
        }
    }

    /// tsgo `Node.TypeArgumentList`.
    pub(crate) fn type_argument_list(&self, node: Node) -> Option<NodeArrayId> {
        if let Found::Node(View::HeritageTypeReference(id)) = node {
            let NodeData::ExpressionWithTypeArguments(element) = &self.source.arena.node(id).data
            else {
                unreachable!("a heritage element")
            };
            return element.type_arguments;
        }
        match self.data(node)? {
            NodeData::CallExpression(data) => data.type_arguments,
            NodeData::NewExpression(data) => data.type_arguments,
            NodeData::TaggedTemplateExpression(data) => data.type_arguments,
            NodeData::TypeReference(data) => data.type_arguments,
            NodeData::ExpressionWithTypeArguments(data) => data.type_arguments,
            NodeData::ImportType(data) => data.type_arguments,
            NodeData::TypeQuery(data) => data.type_arguments,
            NodeData::JsxOpeningElement(data) => data.type_arguments,
            NodeData::JsxSelfClosingElement(data) => data.type_arguments,
            _ => None,
        }
    }

    /// tsgo `Node.ArgumentList`.
    pub(crate) fn argument_list(&self, node: Node) -> Option<NodeArrayId> {
        match self.data(node)? {
            NodeData::CallExpression(data) => data.arguments,
            NodeData::NewExpression(data) => data.arguments,
            _ => None,
        }
    }

    /// tsgo `Node.Arguments`.
    pub(crate) fn arguments(&self, node: Node) -> Vec<Node> {
        self.argument_list(node)
            .map(|list| self.list_nodes(self.list(list)))
            .unwrap_or_default()
    }

    /// tsgo `Node.MemberList`.
    pub(crate) fn member_list(&self, node: Node) -> Option<NodeArrayId> {
        match self.data(node)? {
            NodeData::ClassDeclaration(data) => data.members,
            NodeData::ClassExpression(data) => data.members,
            NodeData::InterfaceDeclaration(data) => data.members,
            NodeData::EnumDeclaration(data) => data.members,
            NodeData::TypeLiteral(data) => data.members,
            NodeData::MappedType(data) => data.members,
            _ => None,
        }
    }

    /// tsgo `Node.PropertyList`.
    pub(crate) fn property_list(&self, node: Node) -> Option<NodeArrayId> {
        match self.data(node)? {
            NodeData::ObjectLiteralExpression(data) => data.properties,
            NodeData::JsxAttributes(data) => data.properties,
            _ => None,
        }
    }

    /// tsgo `Node.ElementList`.
    pub(crate) fn element_list(&self, node: Node) -> Option<NodeArrayId> {
        match self.data(node)? {
            NodeData::NamedImports(data) => data.elements,
            NodeData::NamedExports(data) => data.elements,
            NodeData::ObjectBindingPattern(data) => data.elements,
            NodeData::ArrayBindingPattern(data) => data.elements,
            NodeData::ArrayLiteralExpression(data) => data.elements,
            NodeData::TupleType(data) => data.elements,
            _ => None,
        }
    }

    /// tsgo `Node.StatementList`.
    pub(crate) fn statement_list(&self, node: Node) -> Option<NodeArrayId> {
        match self.data(node)? {
            NodeData::SourceFile(data) => data.statements,
            NodeData::Block(data) => data.statements,
            NodeData::ModuleBlock(data) => data.statements,
            NodeData::CaseClause(data) => data.statements,
            NodeData::DefaultClause(data) => data.statements,
            _ => None,
        }
    }

    /// The declarations of a VariableDeclarationList.
    pub(crate) fn declarations(&self, node: Node) -> Option<NodeArrayId> {
        match self.data(node)? {
            NodeData::VariableDeclarationList(data) => data.declarations,
            _ => None,
        }
    }

    /// tsgo `Node.Body` of a module declaration.
    pub(crate) fn module_body(&self, node: Node) -> Option<Node> {
        match self.data(node)? {
            NodeData::ModuleDeclaration(data) => self.node_of(data.body),
            _ => None,
        }
    }

    /// The block of a catch clause.
    pub(crate) fn catch_block(&self, node: Node) -> Option<Node> {
        match self.data(node)? {
            NodeData::CatchClause(data) => self.node_of(data.block),
            _ => None,
        }
    }

    /// tsgo `Node.Expression` of the nodes the formatter asks: a yield, a
    /// property access, a call.
    pub(crate) fn expression(&self, node: Node) -> Option<Node> {
        if let Found::Node(View::QualifiedName(id)) = node {
            let NodeData::PropertyAccessExpression(access) = &self.source.arena.node(id).data
            else {
                unreachable!("a property access")
            };
            return access
                .expression
                .map(|left| Found::Node(self.navigator.tree().entity_name(left)));
        }
        match self.data(node)? {
            NodeData::YieldExpression(data) => self.node_of(data.expression),
            NodeData::PropertyAccessExpression(data) => self.node_of(data.expression),
            NodeData::CallExpression(data) => self.node_of(data.expression),
            NodeData::NewExpression(data) => self.node_of(data.expression),
            _ => None,
        }
    }

    /// tsgo `Node.Initializer` of a property declaration or a JSX attribute.
    pub(crate) fn initializer(&self, node: Node) -> Option<Node> {
        match self.data(node)? {
            NodeData::PropertyDeclaration(data) => self.node_of(data.initializer),
            NodeData::JsxAttribute(data) => self.node_of(data.initializer),
            _ => None,
        }
    }

    /// tsgo `Node.Type` of a property signature.
    pub(crate) fn property_signature_type(&self, node: Node) -> Option<Node> {
        match self.data(node)? {
            NodeData::PropertySignature(data) => self.node_of(data.r#type),
            _ => None,
        }
    }

    /// tsgo `HasQuestionToken` of a property declaration.
    pub(crate) fn property_declaration_has_question_token(&self, node: Node) -> bool {
        matches!(self.data(node), Some(NodeData::PropertyDeclaration(data)) if data.question_token.is_some())
    }

    /// tsgo `Node.Text` of a numeric literal.
    pub(crate) fn numeric_literal_text(&self, node: Node) -> &'a str {
        match self.data(node) {
            Some(NodeData::NumericLiteral(data)) => &data.text,
            _ => "",
        }
    }

    /// The operator token's kind of a binary expression.
    pub(crate) fn operator_kind(&self, node: Node) -> Option<SyntaxKind> {
        match self.data(node)? {
            NodeData::BinaryExpression(data) => data
                .operator_token
                .map(|token| self.source.arena.node(token).kind),
            _ => None,
        }
    }

    /// The else statement of an if statement.
    pub(crate) fn else_statement(&self, node: Node) -> Option<Node> {
        match self.data(node)? {
            NodeData::IfStatement(data) => self.node_of(data.else_statement),
            _ => None,
        }
    }

    /// The condition, true branch and false branch of a conditional
    /// expression.
    pub(crate) fn conditional_parts(&self, node: Node) -> Option<(Node, Node, Node)> {
        match self.data(node)? {
            NodeData::ConditionalExpression(data) => Some((
                self.node_of(data.condition)?,
                self.node_of(data.when_true)?,
                self.node_of(data.when_false)?,
            )),
            _ => None,
        }
    }

    /// The named bindings of an import clause.
    pub(crate) fn named_bindings(&self, node: Node) -> Option<Node> {
        match self.data(node)? {
            NodeData::ImportClause(data) => self.node_of(data.named_bindings),
            _ => None,
        }
    }

    /// Whether a method declaration has an asterisk.
    pub(crate) fn method_has_asterisk(&self, node: Node) -> bool {
        matches!(self.data(node), Some(NodeData::MethodDeclaration(data)) if data.asterisk_token.is_some())
    }

    /// tsgo `Node.TagName` of a JSX opening, closing or self-closing
    /// element.
    pub(crate) fn tag_name(&self, node: Node) -> Option<Node> {
        match self.data(node)? {
            NodeData::JsxOpeningElement(data) => self.node_of(data.tag_name),
            NodeData::JsxClosingElement(data) => self.node_of(data.tag_name),
            NodeData::JsxSelfClosingElement(data) => self.node_of(data.tag_name),
            _ => None,
        }
    }

    /// tsgo `Node.ModifierNodes`: the modifiers and decorators.
    pub(crate) fn modifier_nodes(&self, node: Node) -> Vec<Node> {
        let Some(id) = self
            .id(node)
            .filter(|_| matches!(node, Found::Node(View::Node(_))))
        else {
            return Vec::new();
        };
        tsc_binder::node_util::modifiers_of(self.source, id)
            .map(|list| {
                self.source
                    .arena
                    .node_array(list)
                    .nodes
                    .iter()
                    .map(|&modifier| Found::Node(View::Node(modifier)))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// tsgo `HasDecorators`.
    pub(crate) fn has_decorators(&self, node: Node) -> bool {
        self.modifier_nodes(node)
            .iter()
            .any(|&modifier| self.kind(modifier) == SyntaxKind::Decorator)
    }

    /// tsgo `GetNameOfDeclaration`.
    pub(crate) fn name_of_declaration(&self, node: Node) -> Option<Node> {
        let id = self
            .id(node)
            .filter(|_| matches!(node, Found::Node(View::Node(_))))?;
        tsc_binder::node_util::get_name_of_declaration(self.source, id)
            .map(|name| Found::Node(View::Node(name)))
    }

    /// tsgo `IsDeclaration`.
    pub(crate) fn is_declaration(&self, node: Node) -> bool {
        let kind = self.kind(node);
        if kind == SyntaxKind::TypeParameter {
            return self.parent(node).is_some();
        }
        matches!(node, Found::Node(_)) && crate::utilities::has_declaration_data(kind)
    }
}

/// tsgo `utf8.DecodeRuneInString(text[pos:])`: the character at a byte
/// position and its size, U+FFFD of size 1 inside a character, and size 0
/// at the end.
pub(crate) fn decode_rune(text: &str, pos: usize) -> (char, usize) {
    if pos >= text.len() {
        return ('\u{FFFD}', 0);
    }
    if !text.is_char_boundary(pos) {
        return ('\u{FFFD}', 1);
    }
    let ch = text[pos..].chars().next().expect("a character");
    (ch, ch.len_utf8())
}

/// tsgo `stringutil.IsWhiteSpaceSingleLine`.
pub(crate) fn is_white_space_single_line(ch: char) -> bool {
    tsc_syntax::is_whitespace_like(ch) && !tsc_syntax::is_line_break(ch)
}

/// tsgo `IsTokenKind`.
pub(crate) fn is_token_kind(kind: SyntaxKind) -> bool {
    kind <= SyntaxKind::LastToken
}

/// tsgo `IsKeywordKind`.
pub(crate) fn is_keyword_kind(kind: SyntaxKind) -> bool {
    (SyntaxKind::FirstKeyword..=SyntaxKind::LastKeyword).contains(&kind)
}

/// tsgo `IsTrivia`.
pub(crate) fn is_trivia(kind: SyntaxKind) -> bool {
    (SyntaxKind::FirstTriviaToken..=SyntaxKind::LastTriviaToken).contains(&kind)
}

pub(crate) use tsc_binder::node_util::is_modifier_kind;

/// tsgo `CanHaveModifiers`.
pub(crate) fn can_have_modifiers(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::TypeParameter
            | SyntaxKind::Parameter
            | SyntaxKind::PropertySignature
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::MethodSignature
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::Constructor
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::IndexSignature
            | SyntaxKind::ConstructorType
            | SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::ClassExpression
            | SyntaxKind::VariableStatement
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::ClassDeclaration
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::EnumDeclaration
            | SyntaxKind::ModuleDeclaration
            | SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::ImportDeclaration
            | SyntaxKind::ExportAssignment
            | SyntaxKind::ExportDeclaration
    )
}

/// tsgo `IsExpression` (by kind).
pub(crate) fn is_expression_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ConditionalExpression
            | SyntaxKind::YieldExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::BinaryExpression
            | SyntaxKind::SpreadElement
            | SyntaxKind::AsExpression
            | SyntaxKind::OmittedExpression
            | SyntaxKind::PartiallyEmittedExpression
            | SyntaxKind::SatisfiesExpression
            | SyntaxKind::PrefixUnaryExpression
            | SyntaxKind::PostfixUnaryExpression
            | SyntaxKind::DeleteExpression
            | SyntaxKind::TypeOfExpression
            | SyntaxKind::VoidExpression
            | SyntaxKind::AwaitExpression
            | SyntaxKind::TypeAssertionExpression
            | SyntaxKind::PropertyAccessExpression
            | SyntaxKind::ElementAccessExpression
            | SyntaxKind::NewExpression
            | SyntaxKind::CallExpression
            | SyntaxKind::JsxElement
            | SyntaxKind::JsxSelfClosingElement
            | SyntaxKind::JsxFragment
            | SyntaxKind::TaggedTemplateExpression
            | SyntaxKind::ArrayLiteralExpression
            | SyntaxKind::ParenthesizedExpression
            | SyntaxKind::ObjectLiteralExpression
            | SyntaxKind::ClassExpression
            | SyntaxKind::FunctionExpression
            | SyntaxKind::Identifier
            | SyntaxKind::PrivateIdentifier
            | SyntaxKind::RegularExpressionLiteral
            | SyntaxKind::NumericLiteral
            | SyntaxKind::BigIntLiteral
            | SyntaxKind::StringLiteral
            | SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::TemplateExpression
            | SyntaxKind::FalseKeyword
            | SyntaxKind::NullKeyword
            | SyntaxKind::ThisKeyword
            | SyntaxKind::TrueKeyword
            | SyntaxKind::SuperKeyword
            | SyntaxKind::NonNullExpression
            | SyntaxKind::ExpressionWithTypeArguments
            | SyntaxKind::MetaProperty
            | SyntaxKind::ImportKeyword
            | SyntaxKind::MissingDeclaration
    )
}

/// tsgo `IsStatementButNotDeclaration` (by kind).
pub(crate) fn is_statement_but_not_declaration_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::BreakStatement
            | SyntaxKind::ContinueStatement
            | SyntaxKind::DebuggerStatement
            | SyntaxKind::DoStatement
            | SyntaxKind::ExpressionStatement
            | SyntaxKind::EmptyStatement
            | SyntaxKind::ForInStatement
            | SyntaxKind::ForOfStatement
            | SyntaxKind::ForStatement
            | SyntaxKind::IfStatement
            | SyntaxKind::LabeledStatement
            | SyntaxKind::ReturnStatement
            | SyntaxKind::SwitchStatement
            | SyntaxKind::ThrowStatement
            | SyntaxKind::TryStatement
            | SyntaxKind::VariableStatement
            | SyntaxKind::WhileStatement
            | SyntaxKind::WithStatement
            | SyntaxKind::NotEmittedStatement
    )
}

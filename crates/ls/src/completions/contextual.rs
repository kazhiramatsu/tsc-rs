//! tsgo's contextual types for completions: `getContextualType`, the
//! argument a position is in (signature help's
//! `getImmediatelyContainingArgumentInfo`), `getConstraintOfTypeArgumentProperty`
//! and `getRecommendedCompletion`.

use tsc_checker::state::CheckResult;
use tsc_syntax::{NodeArrayId, NodeData, NodeId, SyntaxKind, TokenScanner};
use tsc_types::{ContextFlags, ElementFlags, SymbolFlags, SymbolId, TypeData, TypeFlags, TypeId};

use crate::utilities::Touching;

use super::context::{is_class_like, is_template_literal_kind, is_type_node_kind, Completions};

impl Completions<'_, '_, '_> {
    /// tsgo `getContextualType` (completions.go): the type the previous
    /// token's position expects.
    pub(crate) fn contextual_type_at(
        &mut self,
        previous: Touching,
        position: u32,
    ) -> CheckResult<Option<TypeId>> {
        let parent = self.parent(previous).expect("a previous token's parent");
        match self.kind(previous) {
            SyntaxKind::Identifier => {
                let Touching::Node(node) = previous else {
                    return Ok(None);
                };
                return self.contextual_type_from_parent(node, ContextFlags::NONE);
            }
            SyntaxKind::EqualsToken => {
                return match self.data(parent) {
                    NodeData::VariableDeclaration(data) => match data.initializer {
                        Some(initializer) => self
                            .request
                            .state
                            .language_service_contextual_type(initializer, ContextFlags::NONE),
                        None => Ok(None),
                    },
                    NodeData::BinaryExpression(data) => match data.left {
                        Some(left) => Ok(Some(self.request.state.get_type_at_location(left)?)),
                        None => Ok(None),
                    },
                    NodeData::JsxAttribute(_) => self
                        .request
                        .state
                        .get_contextual_type_for_jsx_attribute_like(parent),
                    _ => Ok(None),
                };
            }
            SyntaxKind::NewKeyword => {
                return self
                    .request
                    .state
                    .language_service_contextual_type(parent, ContextFlags::NONE);
            }
            SyntaxKind::CaseKeyword => {
                if self.node_kind(parent) == SyntaxKind::CaseClause {
                    return self.switched_type(parent);
                }
                return Ok(None);
            }
            SyntaxKind::OpenBraceToken => {
                if self.node_kind(parent) == SyntaxKind::JsxExpression {
                    let grandparent = self.parent_of(parent).expect("a JSX expression's parent");
                    if !matches!(
                        self.node_kind(grandparent),
                        SyntaxKind::JsxElement | SyntaxKind::JsxFragment
                    ) {
                        return self
                            .request
                            .state
                            .get_contextual_type_for_jsx_attribute_like(grandparent);
                    }
                }
                return Ok(None);
            }
            SyntaxKind::OpenBracketToken => {
                // After `[` of an array literal, the first element's type.
                if self.node_kind(parent) == SyntaxKind::ArrayLiteralExpression {
                    if let Some(array_type) = self
                        .request
                        .state
                        .language_service_contextual_type(parent, ContextFlags::NONE)?
                    {
                        return self
                            .request
                            .state
                            .get_contextual_type_for_array_literal_at_position(
                                array_type, parent, position,
                            );
                    }
                }
                return Ok(None);
            }
            // After `]`, nothing (an array literal's contextual type would
            // look for the bracket among its elements).
            SyntaxKind::CloseBracketToken => return Ok(None),
            SyntaxKind::QuestionToken => {
                // `foo(a ? /*here*/)`: the conditional's context.
                if self.node_kind(parent) == SyntaxKind::ConditionalExpression {
                    return self.contextual_type_for_conditional_expression(parent, position);
                }
                return Ok(None);
            }
            SyntaxKind::ColonToken => {
                // `foo(a ? b : /*here*/)`; other colons fall through.
                if self.node_kind(parent) == SyntaxKind::ConditionalExpression {
                    return self.contextual_type_for_conditional_expression(parent, position);
                }
            }
            SyntaxKind::CommaToken => {
                // After `,` of an array literal, the next element's type.
                if self.node_kind(parent) == SyntaxKind::ArrayLiteralExpression {
                    if let Some(array_type) = self
                        .request
                        .state
                        .language_service_contextual_type(parent, ContextFlags::NONE)?
                    {
                        return self
                            .request
                            .state
                            .get_contextual_type_for_array_literal_at_position(
                                array_type, parent, position,
                            );
                    }
                    return Ok(None);
                }
            }
            _ => {}
        }
        // An argument position.
        if let Some((invocation, argument_index)) =
            self.argument_info_for_completions(previous, position)?
        {
            return self
                .request
                .state
                .get_contextual_type_for_argument_at_index(invocation, argument_index);
        }
        if is_equality_operator_kind(self.kind(previous)) {
            if let NodeData::BinaryExpression(data) = self.data(parent) {
                if data
                    .operator_token
                    .is_some_and(|operator| is_equality_operator_kind(self.node_kind(operator)))
                {
                    // At `x ===/**/`.
                    return match data.left {
                        Some(left) => Ok(Some(self.request.state.get_type_at_location(left)?)),
                        None => Ok(None),
                    };
                }
            }
        }
        if let Some(ty) =
            self.checker_contextual_type(previous, ContextFlags::IGNORE_NODE_INFERENCES)?
        {
            return Ok(Some(ty));
        }
        self.checker_contextual_type(previous, ContextFlags::NONE)
    }

    /// The checker's `GetContextualType` of a node or a token.
    fn checker_contextual_type(
        &mut self,
        node: Touching,
        flags: ContextFlags,
    ) -> CheckResult<Option<TypeId>> {
        match node {
            Touching::Node(node) => self
                .request
                .state
                .language_service_contextual_type(node, flags),
            Touching::Token { parent, .. } => self
                .request
                .state
                .language_service_contextual_type_of_token(parent, flags),
        }
    }

    /// tsgo `getContextualTypeFromParent`.
    fn contextual_type_from_parent(
        &mut self,
        node: NodeId,
        flags: ContextFlags,
    ) -> CheckResult<Option<TypeId>> {
        let mut parent = self.parent_of(node).expect("an expression's parent");
        while self.node_kind(parent) == SyntaxKind::ParenthesizedExpression {
            match self.parent_of(parent) {
                Some(next) => parent = next,
                None => break,
            }
        }
        match self.data(parent) {
            NodeData::NewExpression(_) => self
                .request
                .state
                .language_service_contextual_type(parent, flags),
            NodeData::BinaryExpression(data) => {
                if data
                    .operator_token
                    .is_some_and(|operator| is_equality_operator_kind(self.node_kind(operator)))
                {
                    let other = if Some(node) == data.right {
                        data.left
                    } else {
                        data.right
                    };
                    return match other {
                        Some(other) => Ok(Some(self.request.state.get_type_at_location(other)?)),
                        None => Ok(None),
                    };
                }
                self.request
                    .state
                    .language_service_contextual_type(node, flags)
            }
            NodeData::CaseClause(_) => self.switched_type(parent),
            _ => self
                .request
                .state
                .language_service_contextual_type(node, flags),
        }
    }

    /// tsgo `getSwitchedType`.
    fn switched_type(&mut self, case_clause: NodeId) -> CheckResult<Option<TypeId>> {
        let switch = self
            .parent_of(case_clause)
            .and_then(|block| self.parent_of(block))
            .expect("a case clause's switch");
        match self.expression_of(switch) {
            Some(expression) => Ok(Some(self.request.state.get_type_at_location(expression)?)),
            None => Ok(None),
        }
    }

    /// tsgo `getContextualTypeForConditionalExpression`.
    fn contextual_type_for_conditional_expression(
        &mut self,
        conditional: NodeId,
        position: u32,
    ) -> CheckResult<Option<TypeId>> {
        if let Some((invocation, argument_index)) =
            self.argument_info_for_completions(Touching::Node(conditional), position)?
        {
            return self
                .request
                .state
                .get_contextual_type_for_argument_at_index(invocation, argument_index);
        }
        // Not in an argument: the conditional's own context.
        if let Some(ty) = self
            .request
            .state
            .language_service_contextual_type(conditional, ContextFlags::IGNORE_NODE_INFERENCES)?
        {
            return Ok(Some(ty));
        }
        self.request
            .state
            .language_service_contextual_type(conditional, ContextFlags::NONE)
    }

    /// tsgo `getArgumentInfoForCompletions`: the call and the index of the
    /// argument at the position.
    fn argument_info_for_completions(
        &mut self,
        node: Touching,
        position: u32,
    ) -> CheckResult<Option<(NodeId, usize)>> {
        self.immediately_containing_argument_info(node, position)
    }

    /// tsgo `getImmediatelyContainingArgumentInfo` (signaturehelp.go), as
    /// completions read it: the invocation and the argument index, none for
    /// a type argument list.
    fn immediately_containing_argument_info(
        &mut self,
        node: Touching,
        position: u32,
    ) -> CheckResult<Option<(NodeId, usize)>> {
        let Some(parent) = self.parent(node) else {
            return Ok(None);
        };
        let parent_kind = self.node_kind(parent);
        let node_kind = self.kind(node);
        if matches!(
            parent_kind,
            SyntaxKind::CallExpression | SyntaxKind::NewExpression
        ) {
            // The argument or type argument list the token is in, and its
            // index there.
            let Some((list, argument_index)) = self.argument_or_parameter_list_and_index(node)?
            else {
                return Ok(None);
            };
            let type_arguments = match self.data(parent) {
                NodeData::CallExpression(data) => data.type_arguments,
                NodeData::NewExpression(data) => data.type_arguments,
                _ => None,
            };
            let is_type_parameter_list = type_arguments.is_some_and(|type_arguments| {
                list.is_some_and(|list| {
                    self.list_range(type_arguments).0 == self.list_range(list).0
                })
            });
            if is_type_parameter_list {
                return Ok(None);
            }
            return Ok(Some((parent, argument_index)));
        }
        if node_kind == SyntaxKind::NoSubstitutionTemplateLiteral
            && parent_kind == SyntaxKind::TaggedTemplateExpression
        {
            // Only inside the template.
            if self.is_inside_template_literal(node, position) {
                return Ok(Some((parent, 0)));
            }
            return Ok(None);
        }
        if node_kind == SyntaxKind::TemplateHead {
            if let Some(tag) = self.parent_of(parent).filter(|&grandparent| {
                self.node_kind(grandparent) == SyntaxKind::TaggedTemplateExpression
            }) {
                let argument_index = if self.is_inside_template_literal(node, position) {
                    0
                } else {
                    1
                };
                return Ok(Some((tag, argument_index)));
            }
        }
        if parent_kind == SyntaxKind::TemplateSpan {
            let template = self.parent_of(parent);
            if let Some(tag) = template
                .and_then(|template| self.parent_of(template))
                .filter(|&tag| self.node_kind(tag) == SyntaxKind::TaggedTemplateExpression)
            {
                // Just after a template tail, no argument.
                if node_kind == SyntaxKind::TemplateTail
                    && !self.is_inside_template_literal(node, position)
                {
                    return Ok(None);
                }
                let NodeData::TemplateExpression(data) = self.data(template.expect("a template"))
                else {
                    return Ok(None);
                };
                let span_index = self
                    .node_list(data.template_spans)
                    .iter()
                    .position(|&span| span == parent)
                    .expect("a span of its template");
                let argument_index = if is_template_literal_kind(node_kind) {
                    if self.is_inside_template_literal(node, position) {
                        0
                    } else {
                        span_index + 2
                    }
                } else {
                    span_index + 1
                };
                return Ok(Some((tag, argument_index)));
            }
        }
        if matches!(
            parent_kind,
            SyntaxKind::JsxOpeningElement | SyntaxKind::JsxSelfClosingElement
        ) {
            return Ok(Some((parent, 0)));
        }
        // Type arguments of `f <`: a type parameter list.
        Ok(None)
    }

    /// tsgo `isInsideTemplateLiteral`.
    fn is_inside_template_literal(&self, node: Touching, position: u32) -> bool {
        is_template_literal_kind(self.kind(node))
            && (self.start(node) < position && position < self.end(node)
                || self.is_unterminated_literal(node) && position == self.end(node))
    }

    /// tsgo `getArgumentOrParameterListAndIndex`: the list after `<` or
    /// `(`, or the list holding the token, and the argument's index.
    fn argument_or_parameter_list_and_index(
        &mut self,
        node: Touching,
    ) -> CheckResult<Option<(Option<NodeArrayId>, usize)>> {
        let parent = self.parent(node).expect("a token's parent");
        let (type_arguments, arguments) = match self.data(parent) {
            NodeData::CallExpression(data) => (data.type_arguments, data.arguments),
            NodeData::NewExpression(data) => (data.type_arguments, data.arguments),
            _ => (None, None),
        };
        match self.kind(node) {
            // The list that starts right after: item 0 of a list just
            // opened.
            SyntaxKind::LessThanToken => return Ok(Some((type_arguments, 0))),
            SyntaxKind::OpenParenToken => return Ok(Some((arguments, 0))),
            _ => {}
        }
        // findContainingList: the last of the parent's lists that contains
        // the token.
        let (pos, end) = (self.pos(node), self.end(node));
        let mut list = None;
        for candidate in [type_arguments, arguments].into_iter().flatten() {
            let (list_pos, list_end) = self.list_range(candidate);
            if list_pos <= pos && end <= list_end {
                list = Some(candidate);
            }
        }
        let Some(list) = list else {
            return Ok(None);
        };
        let argument_index = self.argument_index(node, list, parent)?;
        Ok(Some((Some(list), argument_index)))
    }

    /// tsgo `getArgumentIndex`: the index of the argument holding `node`,
    /// counting a spread tuple's required elements.
    fn argument_index(
        &mut self,
        node: Touching,
        list: NodeArrayId,
        parent: NodeId,
    ) -> CheckResult<usize> {
        let arguments = self.tokens_of_node_list(list, parent);
        let mut argument_index = 0;
        let mut skip_comma = false;
        for argument in arguments {
            if argument == node {
                if !skip_comma && self.kind(argument) == SyntaxKind::CommaToken {
                    argument_index += 1;
                }
                return Ok(argument_index);
            }
            if self.kind(argument) == SyntaxKind::SpreadElement {
                let Touching::Node(spread) = argument else {
                    unreachable!("a spread element is a node");
                };
                argument_index += self.spread_element_count(spread)?;
                skip_comma = true;
                continue;
            }
            if self.kind(argument) != SyntaxKind::CommaToken {
                argument_index += 1;
                skip_comma = true;
                continue;
            }
            if skip_comma {
                skip_comma = false;
                continue;
            }
            argument_index += 1;
        }
        Ok(argument_index)
    }

    /// tsgo `getTokenFromNodeList`: a list's nodes and the tokens between
    /// them.
    fn tokens_of_node_list(&self, list: NodeArrayId, parent: NodeId) -> Vec<Touching> {
        let array = self.request.state.binder.node_array(list);
        let mut left = array.pos;
        let mut index = 0;
        let mut tokens = Vec::new();
        while left < array.end {
            if let Some(&node) = array.nodes.get(index) {
                if left == self.record(node).pos {
                    tokens.push(Touching::Node(node));
                    left = self.record(node).end;
                    index += 1;
                    continue;
                }
            }
            let scanner = TokenScanner::new(self.source, left as usize);
            let end = scanner.token_end() as u32;
            tokens.push(Touching::Token {
                kind: scanner.token(),
                pos: scanner.token_full_start() as u32,
                end,
                parent,
            });
            left = end;
        }
        tokens
    }

    /// tsgo `getSpreadElementCount`: the required elements a spread tuple
    /// gives.
    fn spread_element_count(&mut self, spread: NodeId) -> CheckResult<usize> {
        let Some(expression) = self.expression_of(spread) else {
            return Ok(0);
        };
        let spread_type = self.request.state.get_type_at_location(expression)?;
        let tables = &self.request.state.tables;
        if !tables.is_tuple_type(spread_type) {
            return Ok(0);
        }
        let target = tables.reference_target(spread_type);
        let TypeData::TupleTarget(tuple) = &tables.type_of(target).data else {
            return Ok(0);
        };
        if tuple.fixed_length == 0 {
            return Ok(0);
        }
        Ok(tuple
            .element_flags
            .iter()
            .position(|&flags| !flags.intersects(ElementFlags::REQUIRED))
            .unwrap_or(tuple.fixed_length))
    }

    /// tsgo `getConstraintOfTypeArgumentProperty`: what a type argument
    /// (or a property of an object type in one) must meet.
    pub(crate) fn constraint_of_type_argument_property(
        &mut self,
        node: Touching,
    ) -> CheckResult<Option<TypeId>> {
        if let Touching::Node(node_id) = node {
            if is_type_node_kind(self.node_kind(node_id)) {
                if let Some(constraint) =
                    self.request.state.get_type_argument_constraint(node_id)?
                {
                    return Ok(Some(constraint));
                }
            }
        }
        let Some(parent) = self.parent(node) else {
            return Ok(None);
        };
        let Some(ty) = self.constraint_of_type_argument_property(Touching::Node(parent))? else {
            return Ok(None);
        };
        match self.kind(node) {
            SyntaxKind::PropertySignature => {
                let Touching::Node(node_id) = node else {
                    return Ok(None);
                };
                if let Some(symbol) = self.request.state.node_symbol(node_id) {
                    let name = self.request.state.binder.symbol(symbol).escaped_name;
                    return self
                        .request
                        .state
                        .get_type_of_property_of_contextual_type_by_name(ty, name);
                }
                // A JSDoc type never attached has no symbol: its declared
                // name, as the best effort.
                let Some(name) = self.name_of(node_id) else {
                    return Ok(None);
                };
                let source = self.source;
                if let Some(text) =
                    tsc_binder::node_util::get_text_of_identifier_or_literal(source, name)
                {
                    let escaped = tsc_binder::escape_leading_underscores(&text);
                    return self
                        .request
                        .state
                        .get_type_of_property_of_contextual_type_by_name(ty, escaped);
                }
                Ok(None)
            }
            SyntaxKind::ColonToken => {
                // At a property's value, `Foo<{ x: | }`: the property's type.
                if self.node_kind(parent) == SyntaxKind::PropertySignature {
                    return Ok(Some(ty));
                }
                Ok(None)
            }
            SyntaxKind::IntersectionType | SyntaxKind::TypeLiteral | SyntaxKind::UnionType => {
                Ok(Some(ty))
            }
            SyntaxKind::OpenBracketToken => self.request.state.element_type_of_array_type(ty),
            _ => Ok(None),
        }
    }

    /// tsgo `getRecommendedCompletion`: the enum, enum member or class a
    /// contextual type (the first of a union's) recommends.
    pub(crate) fn recommended_completion(
        &mut self,
        previous: Touching,
        contextual_type: TypeId,
    ) -> CheckResult<Option<SymbolId>> {
        let types = if self
            .request
            .state
            .tables
            .flags_of(contextual_type)
            .intersects(TypeFlags::UNION)
        {
            self.union_types(contextual_type)
        } else {
            vec![contextual_type]
        };
        for ty in types {
            let Some(symbol) = self.request.state.tables.type_of(ty).symbol else {
                continue;
            };
            // No recommendation of an abstract class.
            if self
                .request
                .state
                .symbol_flags(symbol)
                .intersects(SymbolFlags::ENUM_MEMBER | SymbolFlags::ENUM | SymbolFlags::CLASS)
                && !self.is_abstract_constructor_symbol(symbol)
            {
                if let Some(first) = self.first_symbol_in_chain(symbol, Some(previous))? {
                    return Ok(Some(first));
                }
            }
        }
        Ok(None)
    }

    /// tsgo `isAbstractConstructorSymbol`: a class whose first class-like
    /// declaration is abstract.
    fn is_abstract_constructor_symbol(&self, symbol: SymbolId) -> bool {
        let data = self.request.state.binder.symbol(symbol);
        if !data.flags.intersects(SymbolFlags::CLASS) {
            return false;
        }
        let declaration = data.declarations.iter().copied().find(|&declaration| {
            let source = self.request.state.binder.source_of_node(declaration);
            is_class_like(source.arena.node(declaration).kind)
        });
        declaration.is_some_and(|declaration| {
            let source = self.request.state.binder.source_of_node(declaration);
            tsc_binder::node_util::has_syntactic_modifier(
                source,
                declaration,
                tsc_types::ModifierFlags::ABSTRACT,
            )
        })
    }
}

/// tsgo `isEqualityOperatorKind`.
pub(crate) fn is_equality_operator_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::EqualsEqualsEqualsToken
            | SyntaxKind::EqualsEqualsToken
            | SyntaxKind::ExclamationEqualsEqualsToken
            | SyntaxKind::ExclamationEqualsToken
    )
}

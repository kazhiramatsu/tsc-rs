//! tsgo's contextual types for completions: `getContextualType`, the
//! argument a position is in (signature help's
//! `getImmediatelyContainingArgumentInfo`), `getConstraintOfTypeArgumentProperty`
//! and `getRecommendedCompletion`.

use tsc_checker::state::CheckResult;
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{ContextFlags, SymbolFlags, SymbolId, TypeFlags, TypeId};

use crate::signature_help::Invocation;
use crate::utilities::Touching;

use super::context::{is_class_like, is_type_node_kind, Completions};

/// tsgo `argumentInfoForCompletions`: the call a position is an argument
/// of, the argument's index and how many arguments the call has.
pub(crate) struct ArgumentInfo {
    pub(crate) invocation: NodeId,
    pub(crate) argument_index: usize,
    pub(crate) argument_count: usize,
}

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
                return self
                    .request
                    .contextual_type_from_parent(node, ContextFlags::NONE);
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
                    return self.request.switched_type(parent);
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
        if let Some(info) = self.argument_info_for_completions(previous, position)? {
            return self
                .request
                .state
                .get_contextual_type_for_argument_at_index(info.invocation, info.argument_index);
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

    /// tsgo `getContextualTypeForConditionalExpression`.
    fn contextual_type_for_conditional_expression(
        &mut self,
        conditional: NodeId,
        position: u32,
    ) -> CheckResult<Option<TypeId>> {
        if let Some(info) =
            self.argument_info_for_completions(Touching::Node(conditional), position)?
        {
            return self
                .request
                .state
                .get_contextual_type_for_argument_at_index(info.invocation, info.argument_index);
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

    /// tsgo `getArgumentInfoForCompletions`: the call, the index of the
    /// argument at the position and the call's argument count (tsgo
    /// `getImmediatelyContainingArgumentInfo`'s, signature help's); none for
    /// a type argument list.
    pub(crate) fn argument_info_for_completions(
        &mut self,
        node: Touching,
        position: u32,
    ) -> CheckResult<Option<ArgumentInfo>> {
        let Some(info) = self.immediately_containing_argument_info(node, position)? else {
            return Ok(None);
        };
        let Invocation::Call(invocation) = info.invocation else {
            return Ok(None);
        };
        if info.is_type_parameter_list {
            return Ok(None);
        }
        Ok(Some(ArgumentInfo {
            invocation,
            argument_index: info.argument_index,
            argument_count: info.argument_count,
        }))
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

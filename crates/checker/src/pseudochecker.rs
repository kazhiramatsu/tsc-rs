//! tsgo's pseudochecker (TypeScript 7.1 `internal/pseudochecker`): the
//! "pseudo types" of declarations, expressions and signatures, read from the
//! syntax alone.
//!
//! A pseudo type is the skeleton of a type: the type node written for a
//! declaration, a literal, a tuple or object literal of pseudo types, or a
//! marker that the syntax is too complex to read (`Inferred`, `NoResult`).
//! The node builder checks a pseudo type against the checker's type and,
//! when they agree, builds the declaration's type node from it
//! (`node_builder/pseudo.rs`, tsgo checker/pseudotypenodebuilder.go).
//!
//! The pseudochecker reads only the parse tree and the binder. A JavaScript
//! declaration's types are the ones tsgo's reparser hosts on it from its
//! JSDoc (`tsc_binder::jsdoc_hosted`), and a hosted `@type` or `@satisfies`
//! cast stands for the `as` or `satisfies` expression tsgo's reparser wraps
//! around the expression it is keyed by.

use tsc_binder::node_util;
use tsc_syntax::{HostedCast, NodeData, NodeId, SyntaxKind};
use tsc_types::{ModifierFlags, NodeFlags};

use crate::state::CheckerState;

/// tsgo-port: PseudoType @7.1 (pseudochecker/type.go:20-376). tsgo's `Any`
/// pseudo type is never made, so it is not ported.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PseudoType {
    /// The type written as this type node.
    Direct(NodeId),
    /// An expression too complex for the pseudochecker.
    Inferred(Box<PseudoInferred>),
    /// A declaration or signature whose type is too complex for the
    /// pseudochecker.
    NoResult(NodeId),
    /// A literal whose type depends on whether its location is a const
    /// context.
    MaybeConstLocation(Box<PseudoMaybeConstLocation>),
    Union(Vec<PseudoType>),
    Undefined,
    Null,
    String,
    Number,
    BigInt,
    Boolean,
    False,
    True,
    /// The type of an arrow function or function expression.
    SingleCallSignature(Box<PseudoSingleCallSignature>),
    /// The tuple of an `as const` array literal.
    Tuple(Vec<PseudoType>),
    ObjectLiteral(Vec<PseudoObjectElement>),
    StringLiteral(NodeId),
    NumericLiteral(NodeId),
    BigIntLiteral(NodeId),
}

/// tsgo-port: PseudoTypeInferred @7.1 (pseudochecker/type.go:101-121).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PseudoInferred {
    pub(crate) expression: NodeId,
    /// The children (shorthand properties, spreads, non-literal computed
    /// names, …) that keep the expression from being typed.
    pub(crate) error_nodes: Vec<NodeId>,
    pub(crate) is_signature_return: bool,
}

/// tsgo-port: PseudoTypeMaybeConstLocation @7.1 (pseudochecker/type.go:136-161).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PseudoMaybeConstLocation {
    pub(crate) node: NodeId,
    pub(crate) const_type: PseudoType,
    pub(crate) regular_type: PseudoType,
}

/// tsgo-port: PseudoTypeSingleCallSignature @7.1 (pseudochecker/type.go:186-206).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PseudoSingleCallSignature {
    pub(crate) signature: NodeId,
    pub(crate) parameters: Vec<PseudoParameter>,
    pub(crate) type_parameters: Vec<NodeId>,
    pub(crate) return_type: PseudoType,
}

/// tsgo-port: PseudoParameter @7.1 (pseudochecker/type.go:175-184). tsgo
/// keeps the parameter's name and reads the declaration as its parent; this
/// keeps the declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PseudoParameter {
    pub(crate) rest: bool,
    pub(crate) parameter: NodeId,
    pub(crate) optional: bool,
    pub(crate) r#type: PseudoType,
}

/// tsgo-port: PseudoObjectElement @7.1 (pseudochecker/type.go:222-357). tsgo
/// keeps the member's name and reads the member as its parent; this keeps
/// both.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PseudoObjectElement {
    pub(crate) member: NodeId,
    pub(crate) name: NodeId,
    pub(crate) optional: bool,
    pub(crate) kind: PseudoObjectElementKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PseudoObjectElementKind {
    Method {
        signature: NodeId,
        type_parameters: Vec<NodeId>,
        parameters: Vec<PseudoParameter>,
        return_type: PseudoType,
    },
    PropertyAssignment {
        readonly: bool,
        r#type: PseudoType,
    },
    SetAccessor {
        signature: NodeId,
        parameter: Box<PseudoParameter>,
    },
    GetAccessor {
        signature: NodeId,
        r#type: PseudoType,
    },
}

impl PseudoObjectElement {
    /// tsgo-port: PseudoObjectElement.Signature @7.1 (pseudochecker/type.go:231-242).
    pub(crate) fn signature(&self) -> Option<NodeId> {
        match &self.kind {
            PseudoObjectElementKind::Method { signature, .. }
            | PseudoObjectElementKind::SetAccessor { signature, .. }
            | PseudoObjectElementKind::GetAccessor { signature, .. } => Some(*signature),
            PseudoObjectElementKind::PropertyAssignment { .. } => None,
        }
    }
}

impl PseudoType {
    pub(crate) fn inferred(expression: NodeId, is_signature_return: bool) -> Self {
        Self::inferred_with_errors(expression, is_signature_return, Vec::new())
    }

    pub(crate) fn inferred_with_errors(
        expression: NodeId,
        is_signature_return: bool,
        error_nodes: Vec<NodeId>,
    ) -> Self {
        Self::Inferred(Box::new(PseudoInferred {
            expression,
            error_nodes,
            is_signature_return,
        }))
    }

    fn maybe_const_location(node: NodeId, const_type: Self, regular_type: Self) -> Self {
        Self::MaybeConstLocation(Box::new(PseudoMaybeConstLocation {
            node,
            const_type,
            regular_type,
        }))
    }

    /// The pseudochecker's callers keep an expression's pseudo type unless
    /// it is `Inferred` without error nodes (lookup.go:78, 111, 137).
    fn is_inferred_without_errors(&self) -> bool {
        matches!(self, Self::Inferred(inferred) if inferred.error_nodes.is_empty())
    }
}

/// tsgo-port: isUndefinedPseudoType @7.1 (pseudochecker/lookup.go:546-548).
fn is_undefined_pseudo_type(r#type: &PseudoType) -> bool {
    match r#type {
        PseudoType::Undefined => true,
        PseudoType::MaybeConstLocation(location) => is_undefined_pseudo_type(&location.const_type),
        _ => false,
    }
}

/// tsgo-port: CouldAlreadyReferToUndefinedType @7.1 (pseudochecker/lookup.go:577-595):
/// the inverse of tsc 6.0's `canAddUndefined`.
pub(crate) fn could_already_refer_to_undefined_type(
    checker: &CheckerState<'_>,
    r#type: &PseudoType,
) -> bool {
    match r#type {
        PseudoType::NoResult(_) | PseudoType::Inferred(_) => true,
        _ if is_undefined_pseudo_type(r#type) => true,
        // If we're even asking this question, it's not a `const` location.
        PseudoType::MaybeConstLocation(location) => {
            could_already_refer_to_undefined_type(checker, &location.regular_type)
        }
        PseudoType::Direct(type_node) => type_node_could_refer_to_undefined(checker, *type_node),
        PseudoType::Union(types) => types
            .iter()
            .any(|member| could_already_refer_to_undefined_type(checker, member)),
        _ => false,
    }
}

/// tsgo-port: typeNodeCouldReferToUndefined @7.1 (pseudochecker/lookup.go:550-575).
fn type_node_could_refer_to_undefined(checker: &CheckerState<'_>, mut node: NodeId) -> bool {
    while let NodeData::ParenthesizedType(data) = checker.data_of(node) {
        match data.r#type {
            Some(inner) => node = inner,
            None => return false,
        }
    }
    match checker.data_of(node) {
        // These types need symbol or type resolution to know whether they
        // refer to `undefined`, so they might.
        NodeData::TypeReference(_)
        | NodeData::IndexedAccessType(_)
        | NodeData::TypeQuery(_)
        | NodeData::OptionalType(_)
        | NodeData::RestType(_)
        | NodeData::ImportType(_) => true,
        // tsgo treats intersections like unions (a strada behavior it keeps).
        NodeData::IntersectionType(data) => checker
            .nodes_of(data.types)
            .into_iter()
            .any(|member| type_node_could_refer_to_undefined(checker, member)),
        NodeData::UnionType(data) => checker
            .nodes_of(data.types)
            .into_iter()
            .any(|member| type_node_could_refer_to_undefined(checker, member)),
        NodeData::ConditionalType(_) | NodeData::TypeOperator(_) | NodeData::TypePredicate(_) => {
            true
        }
        _ => checker.kind_of(node) == SyntaxKind::UndefinedKeyword,
    }
}

/// tsgo-port: addUndefinedIfDefinitelyRequired @7.1 (pseudochecker/lookup.go:619-629).
fn add_undefined_if_definitely_required(
    checker: &CheckerState<'_>,
    r#type: PseudoType,
) -> PseudoType {
    if could_already_refer_to_undefined_type(checker, &r#type) {
        return r#type;
    }
    PseudoType::Union(vec![r#type, PseudoType::Undefined])
}

/// tsgo-port: isConstContextPropagatingKind @7.1 (pseudochecker/lookup.go:470-478).
const fn is_const_context_propagating_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ArrayLiteralExpression
            | SyntaxKind::ObjectLiteralExpression
            | SyntaxKind::ParenthesizedExpression
            | SyntaxKind::SpreadElement
            | SyntaxKind::PropertyAssignment
            | SyntaxKind::ShorthandPropertyAssignment
            | SyntaxKind::TemplateSpan
            | SyntaxKind::PrefixUnaryExpression
    )
}

/// tsgo-port: isValueSignatureDeclaration @7.1 (pseudochecker/lookup.go:196-198).
const fn is_value_signature_declaration(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::Constructor
    )
}

/// One ancestor of a node as tsgo's tree has it: a parse node, or the
/// `as`/`satisfies` expression tsgo's reparser wraps around a JavaScript
/// expression (keyed by that expression in the hosted table).
#[derive(Clone, Copy)]
enum Ancestor {
    Node(NodeId),
    Cast(HostedCast),
}

/// The accessor declarations of an accessor's symbol.
/// tsgo-port: AllAccessorDeclarations @7.1 (ast/utilities.go:4405-4450).
struct AccessorDeclarations {
    first_accessor: NodeId,
    second_accessor: Option<NodeId>,
    get_accessor: Option<NodeId>,
    set_accessor: Option<NodeId>,
}

/// tsgo-port: PseudoChecker @7.1 (pseudochecker/checker.go:14-21).
pub(crate) struct PseudoChecker<'c, 'a> {
    checker: &'c CheckerState<'a>,
    strict_null_checks: bool,
}

impl<'c, 'a> PseudoChecker<'c, 'a> {
    pub(crate) fn new(checker: &'c CheckerState<'a>) -> Self {
        Self {
            checker,
            strict_null_checks: checker
                .options
                .strict_option_value(checker.options.strict_null_checks),
        }
    }

    /// tsgo-port: PseudoChecker.GetReturnTypeOfSignature @7.1 (pseudochecker/lookup.go:11-24).
    pub(crate) fn get_return_type_of_signature(&self, signature: NodeId) -> PseudoType {
        match self.checker.kind_of(signature) {
            SyntaxKind::GetAccessor => self.get_type_of_accessor(signature),
            // tsc-rs keeps a JSDoc function type where tsgo's reparser makes a
            // function type.
            SyntaxKind::MethodDeclaration
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::Constructor
            | SyntaxKind::MethodSignature
            | SyntaxKind::CallSignature
            | SyntaxKind::ConstructSignature
            | SyntaxKind::SetAccessor
            | SyntaxKind::IndexSignature
            | SyntaxKind::FunctionType
            | SyntaxKind::ConstructorType
            | SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::JSDocSignature
            | SyntaxKind::JSDocFunctionType => self.create_return_from_signature(signature),
            // tsgo fails on any other kind.
            _ => PseudoType::NoResult(signature),
        }
    }

    /// tsgo-port: PseudoChecker.GetTypeOfAccessor @7.1 (pseudochecker/lookup.go:26-28).
    pub(crate) fn get_type_of_accessor(&self, accessor: NodeId) -> PseudoType {
        self.type_from_accessor(accessor)
    }

    /// tsgo-port: PseudoChecker.GetTypeOfDeclaration @7.1 (pseudochecker/lookup.go:34-67).
    pub(crate) fn get_type_of_declaration(&self, node: NodeId) -> PseudoType {
        match self.checker.data_of(node) {
            NodeData::Parameter(_) => self.type_from_parameter(node),
            NodeData::VariableDeclaration(_) => self.type_from_variable(node),
            NodeData::PropertySignature(_)
            | NodeData::PropertyDeclaration(_)
            | NodeData::JSDocPropertyTag(_) => self.type_from_property(node),
            NodeData::BindingElement(_) => PseudoType::NoResult(node),
            NodeData::ExportAssignment(data) => match data.expression {
                Some(expression) => self.type_from_expression(expression),
                None => PseudoType::NoResult(node),
            },
            NodeData::PropertyAccessExpression(_)
            | NodeData::ElementAccessExpression(_)
            | NodeData::BinaryExpression(_) => self.type_from_expando_property(node),
            NodeData::PropertyAssignment(_) | NodeData::ShorthandPropertyAssignment(_) => {
                self.type_from_property_assignment(node)
            }
            // tsgo has no pseudo type for `Object.defineProperty` yet.
            NodeData::CallExpression(_) => PseudoType::NoResult(node),
            // tsc-rs keeps the parameter tag of a `@callback` or `@overload`
            // signature where tsgo's reparser makes a parameter.
            NodeData::JSDocParameterTag(_) => match self.declared_type(node) {
                Some(r#type) => PseudoType::Direct(r#type),
                None => PseudoType::NoResult(node),
            },
            // tsgo fails on any other kind.
            _ => PseudoType::NoResult(node),
        }
    }

    /// tsgo-port: typeFromPropertyAssignment @7.1 (pseudochecker/lookup.go:69-85).
    fn type_from_property_assignment(&self, node: NodeId) -> PseudoType {
        if let Some(annotation) = self.declared_type(node) {
            return PseudoType::Direct(annotation);
        }
        if let NodeData::PropertyAssignment(data) = self.checker.data_of(node) {
            if let Some(initializer) = data.initializer {
                let expression = self.type_from_expression(initializer);
                if !expression.is_inferred_without_errors() {
                    return expression;
                }
            }
        }
        PseudoType::NoResult(node)
    }

    /// tsgo-port: typeFromExpandoProperty @7.1 (pseudochecker/lookup.go:87-96).
    fn type_from_expando_property(&self, node: NodeId) -> PseudoType {
        match self.declared_type(node) {
            Some(declared) => PseudoType::Direct(declared),
            None => PseudoType::NoResult(node),
        }
    }

    /// tsgo-port: typeFromProperty @7.1 (pseudochecker/lookup.go:98-122).
    fn type_from_property(&self, node: NodeId) -> PseudoType {
        if let Some(declared) = self.declared_type(node) {
            return PseudoType::Direct(declared);
        }
        if let NodeData::PropertyDeclaration(data) = self.checker.data_of(node) {
            if let Some(initializer) = data.initializer {
                if !self.is_contextually_typed(node) {
                    // Readonly template literals fail explicitly, leaving room
                    // for literal freshness in the future.
                    if self.has_modifier(node, ModifierFlags::READONLY)
                        && self.checker.kind_of(initializer) == SyntaxKind::TemplateExpression
                    {
                        return PseudoType::NoResult(node);
                    }
                    let expression = self.type_from_expression(initializer);
                    if !expression.is_inferred_without_errors() {
                        if !matches!(expression, PseudoType::Direct(_))
                            && data.question_token.is_some()
                        {
                            // The type of a `?` property comes from its
                            // initializer: add `| undefined`.
                            return add_undefined_if_definitely_required(self.checker, expression);
                        }
                        return expression;
                    }
                }
            }
        }
        PseudoType::NoResult(node)
    }

    /// tsgo-port: typeFromVariable @7.1 (pseudochecker/lookup.go:124-144).
    fn type_from_variable(&self, declaration: NodeId) -> PseudoType {
        if let Some(declared) = self.declared_type(declaration) {
            return PseudoType::Direct(declared);
        }
        let NodeData::VariableDeclaration(data) = self.checker.data_of(declaration) else {
            return PseudoType::NoResult(declaration);
        };
        if let (Some(initializer), Some(symbol)) =
            (data.initializer, self.checker.node_symbol(declaration))
        {
            let declarations = &self.checker.binder.symbol(symbol).declarations;
            let single_variable = declarations.len() == 1
                || declarations
                    .iter()
                    .filter(|&&other| {
                        self.checker.kind_of(other) == SyntaxKind::VariableDeclaration
                    })
                    .count()
                    == 1;
            if single_variable && !self.is_contextually_typed(declaration) {
                // Strada makes a `const` with a template expression fall
                // back, leaving room for template literal freshness.
                if self.is_var_const(declaration)
                    && self.checker.kind_of(initializer) == SyntaxKind::TemplateExpression
                {
                    return PseudoType::NoResult(declaration);
                }
                let expression = self.type_from_expression(initializer);
                if !expression.is_inferred_without_errors() {
                    return expression;
                }
            }
        }
        PseudoType::NoResult(declaration)
    }

    /// tsgo-port: typeFromAccessor @7.1 (pseudochecker/lookup.go:146-164).
    fn type_from_accessor(&self, accessor: NodeId) -> PseudoType {
        let accessors = self.all_accessor_declarations(accessor);
        let accessor_type =
            self.get_type_annotation_from_all_accessor_declarations(accessor, &accessors);
        if let Some(accessor_type) = accessor_type {
            if self.checker.kind_of(accessor_type) != SyntaxKind::TypePredicate {
                return PseudoType::Direct(accessor_type);
            }
        }
        if let Some(get_accessor) = accessors.get_accessor {
            let result = self.create_return_from_signature(get_accessor);
            if let PseudoType::Inferred(inferred) = &result {
                if inferred.error_nodes.is_empty() {
                    // Move the error up to the accessors.
                    let mut error_nodes = vec![get_accessor];
                    error_nodes.extend(accessors.set_accessor);
                    return PseudoType::inferred_with_errors(
                        inferred.expression,
                        inferred.is_signature_return,
                        error_nodes,
                    );
                }
            }
            return result;
        }
        PseudoType::NoResult(accessor)
    }

    /// tsgo-port: getTypeAnnotationFromAllAccessorDeclarations @7.1 (pseudochecker/lookup.go:166-175).
    fn get_type_annotation_from_all_accessor_declarations(
        &self,
        node: NodeId,
        accessors: &AccessorDeclarations,
    ) -> Option<NodeId> {
        let mut accessor_type = self.get_type_annotation_from_accessor(node);
        if accessor_type.is_none() && node != accessors.first_accessor {
            accessor_type = self.get_type_annotation_from_accessor(accessors.first_accessor);
        }
        if accessor_type.is_none() {
            if let Some(second) = accessors.second_accessor.filter(|&second| second != node) {
                accessor_type = self.get_type_annotation_from_accessor(second);
            }
        }
        accessor_type
    }

    /// tsgo-port: getTypeAnnotationFromAccessor @7.1 (pseudochecker/lookup.go:177-194).
    fn get_type_annotation_from_accessor(&self, node: NodeId) -> Option<NodeId> {
        if self.checker.kind_of(node) == SyntaxKind::GetAccessor {
            return self.checker.effective_return_type_node(node);
        }
        let parameter = *self.checker.parameters_of_function(node).first()?;
        if self.checker.kind_of(parameter) != SyntaxKind::Parameter {
            return None;
        }
        self.declared_type(parameter)
    }

    /// tsgo-port: createReturnFromSignature @7.1 (pseudochecker/lookup.go:200-214):
    /// never answers nothing; `NoResult` stands for no pseudo type.
    fn create_return_from_signature(&self, function: NodeId) -> PseudoType {
        let kind = self.checker.kind_of(function);
        if node_util::is_function_like_kind(kind) || kind == SyntaxKind::JSDocSignature {
            if let Some(return_type) = self.checker.effective_return_type_node(function) {
                return PseudoType::Direct(return_type);
            }
        }
        if is_value_signature_declaration(kind) {
            return self.type_from_single_return_expression(function);
        }
        PseudoType::NoResult(function)
    }

    /// tsgo-port: typeFromSingleReturnExpression @7.1 (pseudochecker/lookup.go:216-259).
    fn type_from_single_return_expression(&self, function: NodeId) -> PseudoType {
        let source = self.checker.binder.source_of_node(function);
        let body = node_util::body_of(source, function);
        let mut candidate = None;
        if let Some(body) = body.filter(|&body| !node_util::node_is_missing(source, Some(body))) {
            if self.checker.get_function_flags(function)
                & (crate::functions::FUNCTION_FLAGS_ASYNC
                    | crate::functions::FUNCTION_FLAGS_GENERATOR)
                != 0
            {
                return PseudoType::inferred(function, true);
            }
            if self.checker.kind_of(body) == SyntaxKind::Block {
                for_each_return_statement_in_order(self.checker, body, &mut |statement| {
                    // tsgo bails on nested return statements.
                    if self.checker.parent_of(statement) != Some(body) {
                        candidate = None;
                        return true;
                    }
                    if candidate.is_none() {
                        candidate = match self.checker.data_of(statement) {
                            NodeData::ReturnStatement(data) => data.expression,
                            _ => None,
                        };
                        false
                    } else {
                        candidate = None;
                        true
                    }
                });
            } else {
                candidate = Some(body);
            }
        }
        if let Some(candidate) = candidate {
            // A hosted cast is the candidate in tsgo's tree.
            let cast = self.cast_of(candidate);
            let contextually_typed = match cast {
                Some(_) => self.is_contextually_typed_from(self.checker.parent_of(candidate)),
                None => self.is_contextually_typed(candidate),
            };
            if contextually_typed {
                let assertion_type = match cast {
                    Some(cast) => cast.is_assertion.then_some(cast.type_node),
                    None => match self.checker.data_of(candidate) {
                        NodeData::TypeAssertionExpression(data) => data.r#type,
                        NodeData::AsExpression(data) => data.r#type,
                        _ => None,
                    },
                };
                if let Some(assertion_type) = assertion_type {
                    if !self.checker.is_const_type_reference_node(assertion_type) {
                        return PseudoType::Direct(assertion_type);
                    }
                }
            } else {
                return self.type_from_expression(candidate);
            }
        }
        PseudoType::inferred(function, true)
    }

    /// tsgo-port: typeFromExpression @7.1 (pseudochecker/lookup.go:261-313):
    /// `checkExpression` for pseudo types.
    fn type_from_expression(&self, node: NodeId) -> PseudoType {
        match self.cast_of(node) {
            Some(cast) if cast.is_assertion => {
                self.type_from_type_assertion(node, cast.type_node, true)
            }
            // A `satisfies` expression is not one the pseudochecker reads.
            Some(_) => PseudoType::inferred(node, false),
            None => self.type_from_uncast_expression(node),
        }
    }

    /// typeFromExpression's switch for a parse node (a hosted cast around it
    /// already handled).
    fn type_from_uncast_expression(&self, node: NodeId) -> PseudoType {
        match self.checker.kind_of(node) {
            SyntaxKind::NullKeyword => return PseudoType::Null,
            SyntaxKind::TrueKeyword => {
                return PseudoType::maybe_const_location(
                    node,
                    PseudoType::True,
                    PseudoType::Boolean,
                )
            }
            SyntaxKind::FalseKeyword => {
                return PseudoType::maybe_const_location(
                    node,
                    PseudoType::False,
                    PseudoType::Boolean,
                )
            }
            _ => {}
        }
        match self.checker.data_of(node) {
            NodeData::OmittedExpression(_) => PseudoType::Undefined,
            // Reparsed assertions are casts of the operand: just unwrap.
            NodeData::ParenthesizedExpression(data) => match data.expression {
                Some(expression) => self.type_from_expression(expression),
                None => PseudoType::inferred(node, false),
            },
            // tsgo does not check that the identifier refers to the global
            // `undefined` yet.
            NodeData::Identifier(_)
                if self.checker.identifier_text_of(node) == Some("undefined") =>
            {
                PseudoType::Undefined
            }
            NodeData::ArrowFunction(_) | NodeData::FunctionExpression(_) => {
                self.type_from_function_like_expression(node)
            }
            NodeData::TypeAssertionExpression(data) => match (data.expression, data.r#type) {
                (Some(expression), Some(r#type)) => {
                    self.type_from_type_assertion(expression, r#type, false)
                }
                _ => PseudoType::inferred(node, false),
            },
            NodeData::AsExpression(data) => match (data.expression, data.r#type) {
                (Some(expression), Some(r#type)) => {
                    self.type_from_type_assertion(expression, r#type, false)
                }
                _ => PseudoType::inferred(node, false),
            },
            NodeData::PrefixUnaryExpression(_) if self.is_primitive_literal_value(node, true) => {
                self.type_from_primitive_literal_prefix(node)
            }
            NodeData::ArrayLiteralExpression(_) => self.type_from_array_literal(node),
            NodeData::ObjectLiteralExpression(_) => self.type_from_object_literal(node),
            // There is no annotation or syntax to map a class to.
            NodeData::ClassExpression(_) => {
                PseudoType::inferred_with_errors(node, false, vec![node])
            }
            NodeData::TemplateExpression(_) => {
                // A template with holes `as const` is not supported.
                if self.is_in_const_context(node) {
                    PseudoType::inferred(node, false)
                } else {
                    PseudoType::maybe_const_location(
                        node,
                        PseudoType::inferred(node, false),
                        PseudoType::String,
                    )
                }
            }
            NodeData::NumericLiteral(_) => PseudoType::maybe_const_location(
                node,
                PseudoType::NumericLiteral(node),
                PseudoType::Number,
            ),
            NodeData::NoSubstitutionTemplateLiteral(_) | NodeData::StringLiteral(_) => {
                PseudoType::maybe_const_location(
                    node,
                    PseudoType::StringLiteral(node),
                    PseudoType::String,
                )
            }
            NodeData::BigIntLiteral(_) => PseudoType::maybe_const_location(
                node,
                PseudoType::BigIntLiteral(node),
                PseudoType::BigInt,
            ),
            _ => PseudoType::inferred(node, false),
        }
    }

    /// tsgo-port: typeFromObjectLiteral @7.1 (pseudochecker/lookup.go:315-360).
    fn type_from_object_literal(&self, node: NodeId) -> PseudoType {
        if let Some(error_nodes) = self.can_get_type_from_object_literal(node) {
            return PseudoType::inferred_with_errors(node, false, error_nodes);
        }
        let NodeData::ObjectLiteralExpression(data) = self.checker.data_of(node) else {
            return PseudoType::inferred(node, false);
        };
        // In a const context producing an object literal type there are no
        // shorthand or spread assignments.
        let properties = self.checker.nodes_of(data.properties);
        let mut results = Vec::with_capacity(properties.len());
        for member in properties {
            let Some(name) = self.member_name(member) else {
                continue;
            };
            match self.checker.data_of(member) {
                NodeData::MethodDeclaration(method) => {
                    let optional = method.question_token.is_some();
                    let kind = match self.checker.full_signature_node(member) {
                        Some(full_signature) => PseudoObjectElementKind::PropertyAssignment {
                            readonly: false,
                            r#type: PseudoType::Direct(full_signature),
                        },
                        None => PseudoObjectElementKind::Method {
                            signature: member,
                            type_parameters: self.clone_type_parameters(member),
                            parameters: self.clone_parameters(member),
                            return_type: self.create_return_from_signature(member),
                        },
                    };
                    results.push(PseudoObjectElement {
                        member,
                        name,
                        optional,
                        kind,
                    });
                }
                NodeData::PropertyAssignment(assignment) => {
                    let r#type = match assignment.initializer {
                        Some(initializer) => self.type_from_expression(initializer),
                        None => PseudoType::inferred(member, false),
                    };
                    results.push(PseudoObjectElement {
                        member,
                        name,
                        optional: assignment.question_token.is_some(),
                        kind: PseudoObjectElementKind::PropertyAssignment {
                            readonly: false,
                            r#type,
                        },
                    });
                }
                NodeData::SetAccessor(_) | NodeData::GetAccessor(_) => {
                    if let Some(element) = self.get_accessor_member(member, name) {
                        results.push(element);
                    }
                }
                _ => {}
            }
        }
        PseudoType::ObjectLiteral(results)
    }

    /// tsgo-port: getAccessorMember @7.1 (pseudochecker/lookup.go:362-401):
    /// strada's typeFromObjectLiteralAccessor.
    fn get_accessor_member(&self, accessor: NodeId, name: NodeId) -> Option<PseudoObjectElement> {
        let accessors = self.all_accessor_declarations(accessor);
        if let (Some(get_accessor), Some(set_accessor)) =
            (accessors.get_accessor, accessors.set_accessor)
        {
            let set_parameter_typed = self
                .checker
                .parameters_of_function(set_accessor)
                .first()
                .is_some_and(|&parameter| self.declared_type(parameter).is_some());
            if self
                .checker
                .effective_return_type_node(get_accessor)
                .is_some()
                && set_parameter_typed
            {
                // Both accessors have types that may differ: keep both.
                let kind = if self.checker.kind_of(accessor) == SyntaxKind::GetAccessor {
                    PseudoObjectElementKind::GetAccessor {
                        signature: accessor,
                        r#type: self.type_from_accessor(accessor),
                    }
                } else {
                    let parameter = self.clone_parameters(accessor).into_iter().next()?;
                    PseudoObjectElementKind::SetAccessor {
                        signature: accessor,
                        parameter: Box::new(parameter),
                    }
                };
                return Some(PseudoObjectElement {
                    member: accessor,
                    name,
                    optional: false,
                    kind,
                });
            }
        }
        if accessor == accessors.first_accessor {
            // One annotated accessor: a property, `readonly` for a single
            // get accessor.
            let readonly = self.checker.kind_of(accessor) == SyntaxKind::GetAccessor
                && accessors.second_accessor.is_none();
            return Some(PseudoObjectElement {
                member: accessor,
                name,
                optional: false,
                kind: PseudoObjectElementKind::PropertyAssignment {
                    readonly,
                    r#type: self.type_from_accessor(accessor),
                },
            });
        }
        None
    }

    /// tsgo-port: canGetTypeFromObjectLiteral @7.1 (pseudochecker/lookup.go:403-436):
    /// the shorthand and spread members, non-literal computed names and
    /// members with errors that keep the object literal from being typed.
    fn can_get_type_from_object_literal(&self, node: NodeId) -> Option<Vec<NodeId>> {
        let NodeData::ObjectLiteralExpression(data) = self.checker.data_of(node) else {
            return None;
        };
        let mut error_nodes = Vec::new();
        for member in self.checker.nodes_of(data.properties) {
            if self.has_error(member)
                || matches!(
                    self.checker.kind_of(member),
                    SyntaxKind::ShorthandPropertyAssignment | SyntaxKind::SpreadAssignment
                )
            {
                error_nodes.push(member);
                continue;
            }
            let Some(name) = self.member_name(member) else {
                error_nodes.push(member);
                continue;
            };
            if self.has_error(name) {
                error_nodes.push(name);
                continue;
            }
            match self.checker.data_of(name) {
                NodeData::PrivateIdentifier(_) => error_nodes.push(member),
                NodeData::ComputedPropertyName(computed) => {
                    if !computed.expression.is_some_and(|expression| {
                        self.is_primitive_literal_value(expression, false)
                    }) {
                        error_nodes.push(name);
                    }
                }
                _ => {}
            }
        }
        (!error_nodes.is_empty()).then_some(error_nodes)
    }

    /// tsgo-port: typeFromArrayLiteral @7.1 (pseudochecker/lookup.go:438-451).
    fn type_from_array_literal(&self, node: NodeId) -> PseudoType {
        if let Some(error_nodes) = self.can_get_type_from_array_literal(node) {
            return PseudoType::inferred_with_errors(node, false, error_nodes);
        }
        if self.is_in_const_context(node) && self.is_contextually_typed(node) {
            // An `as const` array with a contextual type has a variable
            // readonly state: bail.
            return PseudoType::inferred(node, false);
        }
        // In a const context producing a tuple there are no spread elements.
        let NodeData::ArrayLiteralExpression(data) = self.checker.data_of(node) else {
            return PseudoType::inferred(node, false);
        };
        PseudoType::Tuple(
            self.checker
                .nodes_of(data.elements)
                .into_iter()
                .map(|element| self.type_from_expression(element))
                .collect(),
        )
    }

    /// tsgo-port: canGetTypeFromArrayLiteral @7.1 (pseudochecker/lookup.go:453-467):
    /// the array itself outside a const context, else its first spread.
    fn can_get_type_from_array_literal(&self, node: NodeId) -> Option<Vec<NodeId>> {
        if !self.is_in_const_context(node) {
            return Some(vec![node]);
        }
        let NodeData::ArrayLiteralExpression(data) = self.checker.data_of(node) else {
            return None;
        };
        self.checker
            .nodes_of(data.elements)
            .into_iter()
            .find(|&element| self.checker.kind_of(element) == SyntaxKind::SpreadElement)
            .map(|spread| vec![spread])
    }

    /// tsgo-port: IsInConstContext @7.1 (pseudochecker/lookup.go:480-492):
    /// whether an ancestor reached through array and object literals is an
    /// `as const` assertion, without type information.
    pub(crate) fn is_in_const_context(&self, node: NodeId) -> bool {
        let found = self.find_ancestor(node, |ancestor| match ancestor {
            // An `as` or `satisfies` expression: neither propagates.
            Ancestor::Cast(_) => true,
            Ancestor::Node(ancestor) => {
                self.is_assertion_expression(ancestor)
                    || !is_const_context_propagating_kind(self.checker.kind_of(ancestor))
            }
        });
        match found {
            Some(Ancestor::Cast(cast)) => {
                cast.is_assertion && self.checker.is_const_type_reference_node(cast.type_node)
            }
            Some(Ancestor::Node(ancestor)) => self.is_const_assertion(ancestor),
            None => false,
        }
    }

    /// tsgo-port: typeFromPrimitiveLiteralPrefix @7.1 (pseudochecker/lookup.go:494-508).
    fn type_from_primitive_literal_prefix(&self, node: NodeId) -> PseudoType {
        let NodeData::PrefixUnaryExpression(data) = self.checker.data_of(node) else {
            return PseudoType::inferred(node, false);
        };
        let Some(operand) = data.operand else {
            return PseudoType::inferred(node, false);
        };
        let expression = if data.operator == SyntaxKind::PlusToken {
            operand
        } else {
            node
        };
        match self.checker.kind_of(operand) {
            SyntaxKind::BigIntLiteral => PseudoType::maybe_const_location(
                node,
                PseudoType::BigIntLiteral(expression),
                PseudoType::BigInt,
            ),
            SyntaxKind::NumericLiteral => PseudoType::maybe_const_location(
                node,
                PseudoType::NumericLiteral(expression),
                PseudoType::Number,
            ),
            // tsgo fails on any other operand.
            _ => PseudoType::inferred(node, false),
        }
    }

    /// tsgo-port: typeFromTypeAssertion @7.1 (pseudochecker/lookup.go:510-515).
    /// `hosted` is a JavaScript cast keyed by `expression` itself.
    fn type_from_type_assertion(
        &self,
        expression: NodeId,
        type_node: NodeId,
        hosted: bool,
    ) -> PseudoType {
        if self.checker.is_const_type_reference_node(type_node) {
            return if hosted {
                self.type_from_uncast_expression(expression)
            } else {
                self.type_from_expression(expression)
            };
        }
        PseudoType::Direct(type_node)
    }

    /// tsgo-port: typeFromFunctionLikeExpression @7.1 (pseudochecker/lookup.go:517-530).
    fn type_from_function_like_expression(&self, node: NodeId) -> PseudoType {
        if let Some(full_signature) = self.checker.full_signature_node(node) {
            return PseudoType::Direct(full_signature);
        }
        let return_type = self.create_return_from_signature(node);
        let type_parameters = self.clone_type_parameters(node);
        let parameters = self.clone_parameters(node);
        PseudoType::SingleCallSignature(Box::new(PseudoSingleCallSignature {
            signature: node,
            parameters,
            type_parameters,
            return_type,
        }))
    }

    /// tsgo-port: cloneTypeParameters @7.1 (pseudochecker/lookup.go:532-544):
    /// the function's type parameters, a JavaScript function's being the
    /// ones tsgo's reparser makes from its `@template` tags.
    fn clone_type_parameters(&self, function: NodeId) -> Vec<NodeId> {
        let syntactic = match self.checker.data_of(function) {
            NodeData::FunctionExpression(data) => data.type_parameters,
            NodeData::ArrowFunction(data) => data.type_parameters,
            NodeData::MethodDeclaration(data) => data.type_parameters,
            _ => None,
        };
        if syntactic.is_some() || !self.checker.is_in_js_file(function) {
            return self.checker.nodes_of(syntactic);
        }
        self.checker.reparsed_type_parameters(function)
    }

    /// tsgo-port: typeFromParameter @7.1 (pseudochecker/lookup.go:631-647).
    fn type_from_parameter(&self, node: NodeId) -> PseudoType {
        let Some(parent) = self.checker.parent_of(node) else {
            return PseudoType::NoResult(node);
        };
        if self.checker.kind_of(parent) == SyntaxKind::SetAccessor {
            return self.get_type_of_accessor(parent);
        }
        // Without an initializer the parameter's position does not matter.
        if self.checker.initializer_of(node).is_none() {
            return match self.declared_type(node) {
                Some(declared) => PseudoType::Direct(declared),
                None => PseudoType::NoResult(node),
            };
        }
        let parameters = self.checker.parameters_of_function(parent);
        let self_index = parameters
            .iter()
            .position(|&parameter| parameter == node)
            .map_or(-1, |index| index as i64);
        let last_required = self.last_required_param_index(&parameters);
        self.type_from_parameter_worker(node, self_index, last_required)
    }

    /// tsgo-port: typeFromParameterWorker @7.1 (pseudochecker/lookup.go:649-685).
    fn type_from_parameter_worker(
        &self,
        node: NodeId,
        self_index: i64,
        last_required: i64,
    ) -> PseudoType {
        if let Some(parent) = self.checker.parent_of(node) {
            if self.checker.kind_of(parent) == SyntaxKind::SetAccessor {
                return self.get_type_of_accessor(parent);
            }
        }
        let has_required_after = self_index < last_required - 1;
        let initializer = self.checker.initializer_of(node);
        if let Some(declared) = self.declared_type(node) {
            let result = PseudoType::Direct(declared);
            // An initialized parameter before a required one takes
            // `| undefined` (the checker's getTypeOfParameter optionality).
            if self.strict_null_checks && initializer.is_some() && has_required_after {
                return add_undefined_if_definitely_required(self.checker, result);
            }
            return result;
        }
        let name_is_identifier = match self.checker.data_of(node) {
            NodeData::Parameter(data) => data
                .name
                .is_some_and(|name| self.checker.kind_of(name) == SyntaxKind::Identifier),
            _ => false,
        };
        if let Some(initializer) = initializer {
            if name_is_identifier && !self.is_contextually_typed(node) {
                let mut expression = self.type_from_expression(initializer);
                if let PseudoType::Inferred(inferred) = &expression {
                    if inferred.error_nodes.is_empty() {
                        // Move the error up to the parameter.
                        expression = PseudoType::inferred_with_errors(
                            inferred.expression,
                            false,
                            vec![node],
                        );
                    }
                }
                if !self.strict_null_checks || !has_required_after {
                    return expression;
                }
                // A required parameter after this one makes `| undefined`
                // explicit, if it is not there already.
                return add_undefined_if_definitely_required(self.checker, expression);
            }
        }
        // The checker infers a binding pattern parameter's type from its
        // names, but strada's isolated declarations do not; that limit stays.
        PseudoType::NoResult(node)
    }

    /// tsgo-port: cloneParameters @7.1 (pseudochecker/lookup.go:687-713).
    fn clone_parameters(&self, function: NodeId) -> Vec<PseudoParameter> {
        let parameters = self.checker.parameters_of_function(function);
        if parameters.is_empty() {
            return Vec::new();
        }
        let last_required = self.last_required_param_index(&parameters);
        parameters
            .iter()
            .enumerate()
            .map(|(index, &parameter)| {
                let index = index as i64;
                let (rest, initializer) = match self.checker.data_of(parameter) {
                    NodeData::Parameter(data) => {
                        (data.dot_dot_dot_token.is_some(), data.initializer)
                    }
                    _ => (false, None),
                };
                let mut optional = self.has_question_token(parameter);
                if !optional && initializer.is_some() {
                    // An initialized parameter is optional only if every
                    // later parameter is optional, initialized or rest (the
                    // checker's isOptionalParameter).
                    optional = index >= last_required - 1;
                }
                PseudoParameter {
                    rest,
                    parameter,
                    optional,
                    r#type: self.type_from_parameter_worker(parameter, index, last_required),
                }
            })
            .collect()
    }

    /// tsgo-port: lastRequiredParamIndex @7.1 (pseudochecker/lookup.go:605-617):
    /// the index just past the last required parameter.
    fn last_required_param_index(&self, parameters: &[NodeId]) -> i64 {
        parameters
            .iter()
            .rposition(|&parameter| !self.is_optional_initialized_or_rest_parameter(parameter))
            .map_or(0, |index| index as i64 + 1)
    }

    /// tsgo-port: isOptionalInitializedOrRestParameter @7.1 (pseudochecker/lookup.go:597-603).
    fn is_optional_initialized_or_rest_parameter(&self, parameter: NodeId) -> bool {
        match self.checker.data_of(parameter) {
            NodeData::Parameter(data) => {
                data.dot_dot_dot_token.is_some()
                    || data.initializer.is_some()
                    || self.has_question_token(parameter)
            }
            _ => false,
        }
    }

    /// tsgo-port: isContextuallyTyped @7.1 (pseudochecker/lookup.go:715-729):
    /// whether a call, a `satisfies`, an annotated declaration or assertion,
    /// or JSX above the node may type it, so that local inference is
    /// unreliable.
    fn is_contextually_typed(&self, node: NodeId) -> bool {
        self.find_ancestor(node, |ancestor| {
            self.is_contextual_typing_ancestor(ancestor)
        })
        .is_some()
    }

    /// isContextuallyTyped from a node that tsgo's tree has as the parent:
    /// the walk includes `start`.
    fn is_contextually_typed_from(&self, start: Option<NodeId>) -> bool {
        let Some(start) = start else {
            return false;
        };
        self.is_contextual_typing_ancestor(Ancestor::Node(start))
            || self.is_contextually_typed(start)
    }

    fn is_contextual_typing_ancestor(&self, ancestor: Ancestor) -> bool {
        match ancestor {
            // `satisfies` types the expression; an `as` cast does unless it
            // is `as const`.
            Ancestor::Cast(cast) => {
                !cast.is_assertion || !self.checker.is_const_type_reference_node(cast.type_node)
            }
            Ancestor::Node(node) => match self.checker.kind_of(node) {
                SyntaxKind::CallExpression | SyntaxKind::SatisfiesExpression => true,
                SyntaxKind::VariableDeclaration
                | SyntaxKind::Parameter
                | SyntaxKind::PropertySignature
                | SyntaxKind::PropertyDeclaration => self.declared_type(node).is_some(),
                SyntaxKind::TypeAssertionExpression | SyntaxKind::AsExpression => {
                    !self.is_const_assertion(node)
                }
                SyntaxKind::JsxElement | SyntaxKind::JsxExpression => true,
                _ => false,
            },
        }
    }

    /// tsgo `ast.FindAncestor(node.Parent, predicate)` over tsgo's tree: a
    /// hosted cast around a node is that node's parent there.
    fn find_ancestor(
        &self,
        node: NodeId,
        mut predicate: impl FnMut(Ancestor) -> bool,
    ) -> Option<Ancestor> {
        let mut current = node;
        loop {
            if let Some(cast) = self.cast_of(current) {
                if predicate(Ancestor::Cast(cast)) {
                    return Some(Ancestor::Cast(cast));
                }
            }
            current = self.checker.parent_of(current)?;
            if predicate(Ancestor::Node(current)) {
                return Some(Ancestor::Node(current));
            }
        }
    }

    // ---- syntax helpers ----

    /// tsgo `node.Type()` for the declarations the pseudochecker reads: the
    /// written annotation or, in JavaScript, the type tsgo's reparser hosts
    /// from the JSDoc.
    fn declared_type(&self, node: NodeId) -> Option<NodeId> {
        match self.checker.data_of(node) {
            NodeData::VariableDeclaration(_)
            | NodeData::Parameter(_)
            | NodeData::PropertyDeclaration(_)
            | NodeData::PropertySignature(_)
            | NodeData::JSDocPropertyTag(_)
            | NodeData::JSDocParameterTag(_) => self.checker.effective_type_annotation_node(node),
            NodeData::PropertyAssignment(_)
            | NodeData::ShorthandPropertyAssignment(_)
            | NodeData::ExportAssignment(_)
            | NodeData::BinaryExpression(_)
                if self.checker.is_in_js_file(node) =>
            {
                self.checker.reparsed_type_node(node)
            }
            _ => None,
        }
    }

    /// The `as`/`satisfies` cast tsgo's reparser wraps around a JavaScript
    /// expression.
    fn cast_of(&self, node: NodeId) -> Option<HostedCast> {
        if !self.checker.is_in_js_file(node) {
            return None;
        }
        self.checker.reparsed_cast(node)
    }

    /// tsgo `HasQuestionToken` of a parameter: a JavaScript parameter's may
    /// be the one tsgo's reparser hosts from its `@param` tag.
    fn has_question_token(&self, parameter: NodeId) -> bool {
        self.checker.is_optional_declaration(parameter)
    }

    /// tsgo `ast.HasModifier` (the reparsed JSDoc modifiers included).
    fn has_modifier(&self, node: NodeId, flags: ModifierFlags) -> bool {
        node_util::get_effective_modifier_flags(self.checker.binder.source_of_node(node), node)
            .intersects(flags)
    }

    /// tsgo `ast.IsVarConst`.
    fn is_var_const(&self, node: NodeId) -> bool {
        let source = self.checker.binder.source_of_node(node);
        node_util::get_combined_node_flags(source, node).bits() & NodeFlags::BLOCK_SCOPED.bits()
            == NodeFlags::CONST.bits()
    }

    /// The node has a parse error (tsgo `NodeFlagsThisNodeHasError`).
    fn has_error(&self, node: NodeId) -> bool {
        self.checker.node_flags(node) & NodeFlags::THIS_NODE_HAS_ERROR.bits() != 0
    }

    /// tsgo `e.Name()` of an object literal member.
    fn member_name(&self, member: NodeId) -> Option<NodeId> {
        match self.checker.data_of(member) {
            NodeData::PropertyAssignment(data) => data.name,
            NodeData::ShorthandPropertyAssignment(data) => data.name,
            NodeData::MethodDeclaration(data) => data.name,
            NodeData::GetAccessor(data) => data.name,
            NodeData::SetAccessor(data) => data.name,
            _ => None,
        }
    }

    /// tsgo `ast.IsAssertionExpression`.
    fn is_assertion_expression(&self, node: NodeId) -> bool {
        matches!(
            self.checker.kind_of(node),
            SyntaxKind::TypeAssertionExpression | SyntaxKind::AsExpression
        )
    }

    /// tsgo `ast.IsConstAssertion`.
    fn is_const_assertion(&self, node: NodeId) -> bool {
        let r#type = match self.checker.data_of(node) {
            NodeData::AsExpression(data) => data.r#type,
            NodeData::TypeAssertionExpression(data) => data.r#type,
            _ => None,
        };
        r#type.is_some_and(|r#type| self.checker.is_const_type_reference_node(r#type))
    }

    /// tsgo-port: IsPrimitiveLiteralValue @7.1 (ast/utilities.go:4119-4140).
    fn is_primitive_literal_value(&self, node: NodeId, include_big_int: bool) -> bool {
        match self.checker.data_of(node) {
            NodeData::NumericLiteral(_)
            | NodeData::StringLiteral(_)
            | NodeData::NoSubstitutionTemplateLiteral(_) => true,
            NodeData::BigIntLiteral(_) => include_big_int,
            NodeData::PrefixUnaryExpression(data) => {
                let operand = data.operand.map(|operand| self.checker.kind_of(operand));
                match data.operator {
                    SyntaxKind::MinusToken => {
                        operand == Some(SyntaxKind::NumericLiteral)
                            || (include_big_int && operand == Some(SyntaxKind::BigIntLiteral))
                    }
                    SyntaxKind::PlusToken => operand == Some(SyntaxKind::NumericLiteral),
                    _ => false,
                }
            }
            _ => matches!(
                self.checker.kind_of(node),
                SyntaxKind::TrueKeyword | SyntaxKind::FalseKeyword
            ),
        }
    }

    /// tsgo-port: GetAllAccessorDeclarationsForDeclaration @7.1
    /// (ast/utilities.go:4405-4450) over the accessor's binder symbol.
    fn all_accessor_declarations(&self, accessor: NodeId) -> AccessorDeclarations {
        let kind = self.checker.kind_of(accessor);
        let other_kind = if kind == SyntaxKind::SetAccessor {
            SyntaxKind::GetAccessor
        } else {
            SyntaxKind::SetAccessor
        };
        let other = self.checker.node_symbol(accessor).and_then(|symbol| {
            self.checker
                .binder
                .symbol(symbol)
                .declarations
                .iter()
                .copied()
                .find(|&declaration| self.checker.kind_of(declaration) == other_kind)
        });
        let position = |node: NodeId| {
            self.checker
                .binder
                .source_of_node(node)
                .arena
                .node(node)
                .pos
        };
        let (first_accessor, second_accessor) = match other {
            Some(other) if position(other) < position(accessor) => (other, Some(accessor)),
            _ => (accessor, other),
        };
        let (get_accessor, set_accessor) = if kind == SyntaxKind::SetAccessor {
            (other, Some(accessor))
        } else {
            (Some(accessor), other)
        };
        AccessorDeclarations {
            first_accessor,
            second_accessor,
            get_accessor,
            set_accessor,
        }
    }
}

/// tsgo-port: ForEachReturnStatement @7.1 (ast/utilities.go:1157-1171): the
/// return statements of a body in source order, stopping when the visitor
/// answers true.
fn for_each_return_statement_in_order(
    checker: &CheckerState<'_>,
    body: NodeId,
    visitor: &mut dyn FnMut(NodeId) -> bool,
) -> bool {
    match checker.kind_of(body) {
        SyntaxKind::ReturnStatement => visitor(body),
        SyntaxKind::CaseBlock
        | SyntaxKind::Block
        | SyntaxKind::IfStatement
        | SyntaxKind::DoStatement
        | SyntaxKind::WhileStatement
        | SyntaxKind::ForStatement
        | SyntaxKind::ForInStatement
        | SyntaxKind::ForOfStatement
        | SyntaxKind::WithStatement
        | SyntaxKind::SwitchStatement
        | SyntaxKind::CaseClause
        | SyntaxKind::DefaultClause
        | SyntaxKind::LabeledStatement
        | SyntaxKind::TryStatement
        | SyntaxKind::CatchClause => {
            let source = checker.binder.source_of_node(body);
            let mut children = Vec::new();
            tsc_syntax::for_each_child(&source.arena, source.arena.node(body), |child| {
                children.push(child);
                false
            });
            children
                .into_iter()
                .any(|child| for_each_return_statement_in_order(checker, child, visitor))
        }
        _ => false,
    }
}

#[cfg(test)]
#[path = "../tests/unit/pseudochecker/tests.rs"]
mod tests;

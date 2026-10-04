//! tsgo's pseudo-type node builder (TypeScript 7.1
//! checker/pseudotypenodebuilder.go, and the node reuse of nodecopy.go):
//! check a declaration's pseudo type (`crate::pseudochecker`) against the
//! checker's type, and build the declaration's type node from the pseudo
//! type when they agree.

use tsc_emitter::{
    EmitFlags, EmitNodeBuilderFlags, TransformArena, TransformNode, TransformSourceId,
};
use tsc_syntax::nodes::{
    FunctionTypeData, GetAccessorData, LiteralTypeData, MethodSignatureData, ParameterData,
    PropertySignatureData, SetAccessorData, TypeLiteralData,
};
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{
    ContextFlags, ElementFlags, SymbolFlags, Ternary, TypeData, TypeFacts, TypeFlags, TypeId,
    UnionReduction,
};

use crate::narrow::{TypePredicate, TypePredicateKind};
use crate::pseudochecker::{
    PseudoChecker, PseudoObjectElement, PseudoObjectElementKind, PseudoParameter, PseudoType,
};
use crate::state::{CheckerState, SignatureId};

use super::serialize::{
    report_inference_fallback, serialize_return_type_for_signature_in_context,
    serialize_type_for_declaration_in_context, syntactic_try_reuse_existing_node,
};
use super::signatures::{
    enter_signature_scope, exit_new_scope, parameter_to_parameter_declaration_name,
};
use super::type_nodes::{
    checker_abort_error, create_node, create_node_array, create_token, factory_error,
    set_comment_range_2, type_to_type_node_helper, update_factory_node, BuildResult,
};
use super::{
    existing_type_node_is_not_reference_or_is_reference_with_compatible_type_argument_count,
    get_type_from_type_node2, restore_flags, save_restore_flags,
    syntactic_try_reuse_existing_type_node, NodeBuilderContext,
};

const IN_OBJECT_TYPE_LITERAL: u32 = 4_194_304;

/// typeToTypeNode: `any` when it builds nothing.
fn type_to_type_node(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    r#type: TypeId,
) -> BuildResult<TransformNode> {
    match type_to_type_node_helper(checker, arena, target, r#type, context)? {
        Some(node) => Ok(node),
        None => keyword(arena, target, SyntaxKind::AnyKeyword),
    }
}

/// typeToTypeNode with inference fallback reports suppressed.
fn type_to_type_node_without_fallback_reports(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    r#type: TypeId,
) -> BuildResult<TransformNode> {
    let old_suppress = context.suppress_report_inference_fallback;
    context.suppress_report_inference_fallback = true;
    let result = type_to_type_node(checker, arena, target, context, r#type);
    context.suppress_report_inference_fallback = old_suppress;
    result
}

fn keyword(
    arena: &mut TransformArena,
    target: TransformSourceId,
    kind: SyntaxKind,
) -> BuildResult<TransformNode> {
    arena
        .factory()
        .create_keyword_type_node(target, kind)
        .map_err(factory_error)
}

fn literal_type(
    arena: &mut TransformArena,
    target: TransformSourceId,
    literal: TransformNode,
) -> BuildResult<TransformNode> {
    create_node(
        arena,
        target,
        NodeData::LiteralType(LiteralTypeData {
            literal: Some(literal.node()),
        }),
    )
}

/// tsgo `ast.IsDeclaration`: the kinds whose nodes carry declaration data
/// in tsgo's tree (ast_generated.go), a type parameter only with a parent.
fn is_declaration_node(checker: &CheckerState<'_>, node: NodeId) -> bool {
    match checker.kind_of(node) {
        SyntaxKind::TypeParameter => checker.parent_of(node).is_some(),
        SyntaxKind::ArrowFunction
        | SyntaxKind::BinaryExpression
        | SyntaxKind::BindingElement
        | SyntaxKind::CallExpression
        | SyntaxKind::CallSignature
        | SyntaxKind::ClassDeclaration
        | SyntaxKind::ClassExpression
        | SyntaxKind::ClassStaticBlockDeclaration
        | SyntaxKind::Constructor
        | SyntaxKind::ConstructSignature
        | SyntaxKind::ConstructorType
        | SyntaxKind::EnumDeclaration
        | SyntaxKind::EnumMember
        | SyntaxKind::ExportAssignment
        | SyntaxKind::ExportDeclaration
        | SyntaxKind::ExportSpecifier
        | SyntaxKind::FunctionDeclaration
        | SyntaxKind::FunctionExpression
        | SyntaxKind::FunctionType
        | SyntaxKind::GetAccessor
        | SyntaxKind::ImportClause
        | SyntaxKind::ImportDeclaration
        | SyntaxKind::ImportEqualsDeclaration
        | SyntaxKind::ImportSpecifier
        | SyntaxKind::IndexSignature
        | SyntaxKind::InterfaceDeclaration
        | SyntaxKind::JSDocSignature
        | SyntaxKind::JSDocTypeLiteral
        | SyntaxKind::JsxAttribute
        | SyntaxKind::JsxAttributes
        | SyntaxKind::JsxSpreadAttribute
        | SyntaxKind::MappedType
        | SyntaxKind::MethodDeclaration
        | SyntaxKind::MethodSignature
        | SyntaxKind::MissingDeclaration
        | SyntaxKind::ModuleDeclaration
        | SyntaxKind::NamedTupleMember
        | SyntaxKind::NamespaceExport
        | SyntaxKind::NamespaceExportDeclaration
        | SyntaxKind::NamespaceImport
        | SyntaxKind::NoSubstitutionTemplateLiteral
        | SyntaxKind::NotEmittedTypeElement
        | SyntaxKind::ObjectLiteralExpression
        | SyntaxKind::Parameter
        | SyntaxKind::PropertyAssignment
        | SyntaxKind::PropertyDeclaration
        | SyntaxKind::PropertySignature
        | SyntaxKind::SemicolonClassElement
        | SyntaxKind::SetAccessor
        | SyntaxKind::ShorthandPropertyAssignment
        | SyntaxKind::SourceFile
        | SyntaxKind::SpreadAssignment
        | SyntaxKind::TypeAliasDeclaration
        | SyntaxKind::TypeLiteral
        | SyntaxKind::VariableDeclaration => true,
        _ => false,
    }
}

fn is_accessor(checker: &CheckerState<'_>, node: NodeId) -> bool {
    matches!(
        checker.kind_of(node),
        SyntaxKind::GetAccessor | SyntaxKind::SetAccessor
    )
}

/// serializeReturnTypeForSignature without reuse (`tryReuse` false): the
/// checker's return type of the declaration's signature.
fn serialize_return_type_of_declaration(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    declaration: NodeId,
) -> BuildResult<TransformNode> {
    let signature = checker
        .get_signature_from_declaration(declaration)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    match serialize_return_type_for_signature_in_context(
        checker, arena, target, context, signature, false,
    )? {
        Some(node) => Ok(node),
        None => keyword(arena, target, SyntaxKind::AnyKeyword),
    }
}

/// tsgo-port: NodeBuilderImpl.pseudoTypeToNodeWithCheckerFallback @7.1
/// (pseudotypenodebuilder.go:11-45): an `Inferred` pseudo type at the top
/// reports its error nodes and serializes the checker's type, so the type is
/// not derived from the declaration's expression in an instantiated context.
pub(super) fn pseudo_type_to_node_with_checker_fallback(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    pseudo: &PseudoType,
    checker_type: TypeId,
) -> BuildResult<TransformNode> {
    match pseudo {
        PseudoType::Inferred(inferred) => {
            if !context.suppress_report_inference_fallback {
                if inferred.error_nodes.is_empty() {
                    report_inference_fallback(checker, context, inferred.expression)?;
                } else {
                    for &node in &inferred.error_nodes {
                        report_inference_fallback(checker, context, node)?;
                    }
                }
            }
            return type_to_type_node_without_fallback_reports(
                checker,
                arena,
                target,
                context,
                checker_type,
            );
        }
        PseudoType::Direct(existing) => {
            if !can_reuse_existing_js_type_node(checker, context, *existing, checker_type)? {
                if !context.suppress_report_inference_fallback {
                    report_inference_fallback(checker, context, *existing)?;
                }
                return type_to_type_node_without_fallback_reports(
                    checker,
                    arena,
                    target,
                    context,
                    checker_type,
                );
            }
        }
        _ => {}
    }
    pseudo_type_to_node(checker, arena, target, context, pseudo)
}

/// tsgo-port: NodeBuilderImpl.canReuseExistingJSTypeNode @7.1 (nodebuilderimpl.go:507-509).
fn can_reuse_existing_js_type_node(
    checker: &mut CheckerState<'_>,
    context: &NodeBuilderContext<'_>,
    existing: NodeId,
    r#type: TypeId,
) -> BuildResult<bool> {
    if checker
        .get_intended_type_from_jsdoc_type_reference(existing)
        .map_err(|abort| checker_abort_error(checker, context, abort))?
        .is_some()
    {
        return Ok(false);
    }
    existing_type_node_is_not_reference_or_is_reference_with_compatible_type_argument_count(
        checker, existing, r#type, context,
    )
}

/// tsgo-port: NodeBuilderImpl.pseudoTypeToNode @7.1 (pseudotypenodebuilder.go:47-325):
/// map a pseudo type into a type node, reporting the inference fallbacks
/// its structure implies.
pub(super) fn pseudo_type_to_node(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    pseudo: &PseudoType,
) -> BuildResult<TransformNode> {
    let strict_null_checks = checker
        .options
        .strict_option_value(checker.options.strict_null_checks);
    match pseudo {
        PseudoType::Direct(type_node) => {
            reuse_type_node(checker, arena, target, context, *type_node)
        }
        PseudoType::Inferred(inferred) => {
            let node = inferred.expression;
            let parent = checker.parent_of(node);
            if !inferred.error_nodes.is_empty() {
                for &error_node in &inferred.error_nodes {
                    report_inference_fallback(checker, context, error_node)?;
                }
            } else if let Some(parent) = parent.filter(|&parent| {
                checker.is_entity_name_expression(node) && is_declaration_node(checker, parent)
            }) {
                report_inference_fallback(checker, context, parent)?;
            } else {
                report_inference_fallback(checker, context, node)?;
            }
            if inferred.is_signature_return {
                return serialize_return_type_of_declaration(checker, arena, target, context, node);
            }
            // The parent declaration's symbol type handles the expression's
            // widening without duplicating it.
            if let Some(parent) = parent {
                if checker.kind_of(parent) == SyntaxKind::ReturnStatement {
                    if let Some(enclosing) = checker.get_containing_function(node) {
                        if is_accessor(checker, enclosing) {
                            return serialize_type_for_declaration_in_context(
                                checker,
                                arena,
                                target,
                                context,
                                Some(enclosing),
                                None,
                                None,
                                false,
                            );
                        }
                        return serialize_return_type_of_declaration(
                            checker, arena, target, context, enclosing,
                        );
                    }
                }
                if matches!(
                    checker.data_of(parent),
                    NodeData::ArrowFunction(data) if data.body == Some(node)
                ) {
                    return serialize_return_type_of_declaration(
                        checker, arena, target, context, parent,
                    );
                }
                if is_declaration_node(checker, parent) {
                    return serialize_type_for_declaration_in_context(
                        checker,
                        arena,
                        target,
                        context,
                        Some(parent),
                        None,
                        None,
                        false,
                    );
                }
            }
            let r#type = checker
                .get_type_of_expression(node)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            type_to_type_node(checker, arena, target, context, r#type)
        }
        PseudoType::NoResult(node) => {
            let node = *node;
            report_inference_fallback(checker, context, node)?;
            if tsc_binder::node_util::is_function_like_kind(checker.kind_of(node))
                && !is_accessor(checker, node)
            {
                return serialize_return_type_of_declaration(checker, arena, target, context, node);
            }
            serialize_type_for_declaration_in_context(
                checker,
                arena,
                target,
                context,
                Some(node),
                None,
                None,
                false,
            )
        }
        PseudoType::MaybeConstLocation(location) => {
            // checkExpressionWithContextualType's literal widening rules.
            let mut in_const_context = checker
                .is_const_context(location.node)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            if !in_const_context && PseudoChecker::new(checker).is_in_const_context(location.node) {
                // Only consult the contextual type when the syntax is a const
                // context too: getContextualType answers post-inference types
                // here that initial checking may not have had.
                let contextual_type = checker
                    .get_contextual_type(location.node, ContextFlags::NONE)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?;
                if let Some(const_type) =
                    pseudo_type_to_type(checker, context, &location.const_type)?
                {
                    let instantiated = checker
                        .instantiate_contextual_type(
                            contextual_type,
                            location.node,
                            ContextFlags::NONE,
                        )
                        .map_err(|abort| checker_abort_error(checker, context, abort))?;
                    if checker
                        .is_literal_of_contextual_type(const_type, instantiated)
                        .map_err(|abort| checker_abort_error(checker, context, abort))?
                    {
                        in_const_context = true;
                    }
                }
            }
            let chosen = if in_const_context {
                &location.const_type
            } else {
                &location.regular_type
            };
            pseudo_type_to_node(checker, arena, target, context, chosen)
        }
        PseudoType::Union(members) => {
            let mut types = Vec::with_capacity(members.len());
            let mut has_elided_type = false;
            let mut has_undefined = false;
            for member in members {
                if !strict_null_checks && matches!(member, PseudoType::Undefined | PseudoType::Null)
                {
                    has_elided_type = true;
                    continue;
                }
                let node = pseudo_type_to_node(checker, arena, target, context, member)?;
                append_union_member(arena, node, &mut types, &mut has_undefined)?;
            }
            match types.len() {
                0 => keyword(
                    arena,
                    target,
                    if has_elided_type {
                        SyntaxKind::AnyKeyword
                    } else {
                        SyntaxKind::NeverKeyword
                    },
                ),
                1 => Ok(types[0]),
                _ => {
                    let types = arena
                        .factory()
                        .create_node_array(target, types)
                        .map_err(factory_error)?;
                    arena
                        .factory()
                        .create_union_type_node(target, types)
                        .map_err(factory_error)
                }
            }
        }
        PseudoType::Undefined => keyword(
            arena,
            target,
            if strict_null_checks {
                SyntaxKind::UndefinedKeyword
            } else {
                SyntaxKind::AnyKeyword
            },
        ),
        PseudoType::Null => {
            if !strict_null_checks {
                return keyword(arena, target, SyntaxKind::AnyKeyword);
            }
            let null = create_token(arena, target, SyntaxKind::NullKeyword)?;
            literal_type(arena, target, null)
        }
        PseudoType::String => keyword(arena, target, SyntaxKind::StringKeyword),
        PseudoType::Number => keyword(arena, target, SyntaxKind::NumberKeyword),
        PseudoType::BigInt => keyword(arena, target, SyntaxKind::BigIntKeyword),
        PseudoType::Boolean => keyword(arena, target, SyntaxKind::BooleanKeyword),
        PseudoType::False => {
            let literal = create_token(arena, target, SyntaxKind::FalseKeyword)?;
            literal_type(arena, target, literal)
        }
        PseudoType::True => {
            let literal = create_token(arena, target, SyntaxKind::TrueKeyword)?;
            literal_type(arena, target, literal)
        }
        PseudoType::SingleCallSignature(signature) => {
            let signature_id = checker
                .get_signature_from_declaration(signature.signature)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            let (_, scope) = enter_signature_scope(checker, arena, target, context, signature_id)?;
            let result = (|| {
                let type_parameters = reuse_type_parameters(
                    checker,
                    arena,
                    target,
                    context,
                    &signature.type_parameters,
                )?;
                let parameters = pseudo_parameters_to_node_list(
                    checker,
                    arena,
                    target,
                    context,
                    &signature.parameters,
                )?;
                let return_type =
                    pseudo_type_to_node(checker, arena, target, context, &signature.return_type)?;
                create_function_type(arena, target, type_parameters, parameters, return_type)
            })();
            exit_new_scope(context, scope);
            result
        }
        PseudoType::Tuple(elements) => {
            let mut nodes = Vec::with_capacity(elements.len());
            for element in elements {
                nodes.push(pseudo_type_to_node(
                    checker, arena, target, context, element,
                )?);
            }
            // Pseudo tuples come from `as const`, so they are `readonly`.
            let elements = arena
                .factory()
                .create_node_array(target, nodes)
                .map_err(factory_error)?;
            let tuple = arena
                .factory()
                .create_tuple_type_node(target, elements)
                .map_err(factory_error)?;
            arena.metadata_mut(tuple).add_flags(EmitFlags::SINGLE_LINE);
            arena
                .factory()
                .create_type_operator_node(target, SyntaxKind::ReadonlyKeyword, tuple)
                .map_err(factory_error)
        }
        PseudoType::ObjectLiteral(elements) => {
            pseudo_object_literal_to_node(checker, arena, target, context, elements)
        }
        PseudoType::StringLiteral(source)
        | PseudoType::NumericLiteral(source)
        | PseudoType::BigIntLiteral(source) => {
            let literal = reuse_node_or_clone(checker, arena, target, context, *source)?;
            literal_type(arena, target, literal)
        }
    }
}

/// pseudoTypeToNode's union member append: nested unions flatten and only
/// the first `undefined` keyword stays.
fn append_union_member(
    arena: &TransformArena,
    node: TransformNode,
    types: &mut Vec<TransformNode>,
    has_undefined: &mut bool,
) -> BuildResult<()> {
    let record = arena.node(node).map_err(factory_error)?;
    if let NodeData::UnionType(data) = &record.data {
        let members = match data
            .types
            .and_then(|array| arena.node_array_ref(node.source(), array))
        {
            Some(array) => arena
                .node_array(array)
                .map_err(factory_error)?
                .nodes
                .iter()
                .map(|&member| TransformNode::new(node.source(), member))
                .collect(),
            None => Vec::new(),
        };
        for member in members {
            append_union_member(arena, member, types, has_undefined)?;
        }
        return Ok(());
    }
    if record.kind == SyntaxKind::UndefinedKeyword {
        if *has_undefined {
            return Ok(());
        }
        *has_undefined = true;
    }
    types.push(node);
    Ok(())
}

fn create_function_type(
    arena: &mut TransformArena,
    target: TransformSourceId,
    type_parameters: Option<Vec<TransformNode>>,
    parameters: Vec<TransformNode>,
    return_type: TransformNode,
) -> BuildResult<TransformNode> {
    let type_parameters = match type_parameters {
        Some(type_parameters) => Some(create_node_array(arena, target, type_parameters)?),
        None => None,
    };
    let parameters = create_node_array(arena, target, parameters)?;
    create_node(
        arena,
        target,
        NodeData::FunctionType(FunctionTypeData {
            type_parameters,
            parameters: Some(parameters),
            r#type: Some(return_type.node()),
            modifiers: None,
        }),
    )
}

/// The reused type parameters of a pseudo signature or method, `None` for
/// none.
fn reuse_type_parameters(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    type_parameters: &[NodeId],
) -> BuildResult<Option<Vec<TransformNode>>> {
    if type_parameters.is_empty() {
        return Ok(None);
    }
    let mut nodes = Vec::with_capacity(type_parameters.len());
    for &type_parameter in type_parameters {
        let mut reused = reuse_node_or_clone(checker, arena, target, context, type_parameter)?;
        if let Some(constraint) = template_tag_constraint(checker, type_parameter) {
            let constraint = reuse_type_node(checker, arena, target, context, constraint)?;
            if let NodeData::TypeParameter(mut data) =
                arena.node(reused).map_err(factory_error)?.data.clone()
            {
                data.constraint = Some(constraint.node());
                reused = update_factory_node(arena, reused, NodeData::TypeParameter(data))?;
            }
        }
        nodes.push(reused);
    }
    Ok(Some(nodes))
}

/// The constraint tsgo's reparser gives a JavaScript `@template` type
/// parameter: its tag's, for the tag's first type parameter
/// (reparser.go gatherTypeParameters).
fn template_tag_constraint(checker: &CheckerState<'_>, type_parameter: NodeId) -> Option<NodeId> {
    let tag = checker.parent_of(type_parameter)?;
    let NodeData::JSDocTemplateTag(data) = checker.data_of(tag) else {
        return None;
    };
    if checker.nodes_of(data.type_parameters).first() != Some(&type_parameter) {
        return None;
    }
    match checker.data_of(data.constraint?) {
        NodeData::JSDocTypeExpression(expression) => expression.r#type,
        _ => None,
    }
}

/// pseudoTypeToNode's object literal arm (pseudotypenodebuilder.go:205-317).
fn pseudo_object_literal_to_node(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    elements: &[PseudoObjectElement],
) -> BuildResult<TransformNode> {
    let Some(first) = elements.first() else {
        let members = create_node_array(arena, target, Vec::new())?;
        let literal = create_node(
            arena,
            target,
            NodeData::TypeLiteral(TypeLiteralData {
                members: Some(members),
            }),
        )?;
        arena
            .metadata_mut(literal)
            .add_flags(EmitFlags::SINGLE_LINE);
        return Ok(literal);
    };
    // The checker's isConstContext (not the pseudochecker's) also makes an
    // object in a const type parameter's argument position readonly.
    let object_literal = checker.parent_of(first.member).unwrap_or(first.member);
    let is_const = checker
        .is_const_context(object_literal)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    // The members are serialized within an object type literal, as
    // createTypeNodeFromObjectType marks them (an inaccessible `this` in a
    // member reports TS2527).
    let restore_object_literal_flags = save_restore_flags(context);
    context.flags.0 |= IN_OBJECT_TYPE_LITERAL;
    let result = (|| {
        let mut members = Vec::with_capacity(elements.len());
        for element in elements {
            members.push(pseudo_object_element_to_node(
                checker, arena, target, context, element, is_const,
            )?);
        }
        Ok::<_, tsc_emitter::EmitResolverError>(members)
    })();
    restore_flags(context, restore_object_literal_flags);
    let members = result?;
    let members = create_node_array(arena, target, members)?;
    let literal = create_node(
        arena,
        target,
        NodeData::TypeLiteral(TypeLiteralData {
            members: Some(members),
        }),
    )?;
    if !context
        .flags
        .contains(EmitNodeBuilderFlags::MULTILINE_OBJECT_LITERALS)
    {
        arena
            .metadata_mut(literal)
            .add_flags(EmitFlags::SINGLE_LINE);
    }
    Ok(literal)
}

fn pseudo_object_element_to_node(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    element: &PseudoObjectElement,
    is_const: bool,
) -> BuildResult<TransformNode> {
    let readonly = is_const
        || matches!(
            element.kind,
            PseudoObjectElementKind::PropertyAssignment { readonly: true, .. }
        );
    let scope = match element.signature() {
        Some(signature) => {
            let signature_id = checker
                .get_signature_from_declaration(signature)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            Some(enter_signature_scope(checker, arena, target, context, signature_id)?.1)
        }
        None => None,
    };
    let result = (|| {
        let modifiers = if readonly {
            let readonly = create_token(arena, target, SyntaxKind::ReadonlyKeyword)?;
            Some(create_node_array(arena, target, vec![readonly])?)
        } else {
            None
        };
        let member = match &element.kind {
            PseudoObjectElementKind::Method {
                type_parameters,
                parameters,
                return_type,
                ..
            } => {
                let type_parameters =
                    reuse_type_parameters(checker, arena, target, context, type_parameters)?;
                if is_const {
                    let name = reuse_name(checker, arena, target, context, element.name, false)?;
                    let parameters = pseudo_parameters_to_node_list(
                        checker, arena, target, context, parameters,
                    )?;
                    let return_type =
                        pseudo_type_to_node(checker, arena, target, context, return_type)?;
                    let function_type = create_function_type(
                        arena,
                        target,
                        type_parameters,
                        parameters,
                        return_type,
                    )?;
                    create_node(
                        arena,
                        target,
                        NodeData::PropertySignature(PropertySignatureData {
                            name: Some(name.node()),
                            modifiers,
                            question_token: None,
                            r#type: Some(function_type.node()),
                            initializer: None,
                        }),
                    )?
                } else {
                    let name = reuse_name(checker, arena, target, context, element.name, true)?;
                    let type_parameters = match type_parameters {
                        Some(type_parameters) => {
                            Some(create_node_array(arena, target, type_parameters)?)
                        }
                        None => None,
                    };
                    let parameters = pseudo_parameters_to_node_list(
                        checker, arena, target, context, parameters,
                    )?;
                    let parameters = create_node_array(arena, target, parameters)?;
                    let return_type =
                        pseudo_type_to_node(checker, arena, target, context, return_type)?;
                    create_node(
                        arena,
                        target,
                        NodeData::MethodSignature(MethodSignatureData {
                            name: Some(name.node()),
                            modifiers,
                            question_token: None,
                            type_parameters,
                            parameters: Some(parameters),
                            r#type: Some(return_type.node()),
                        }),
                    )?
                }
            }
            PseudoObjectElementKind::PropertyAssignment { r#type, .. } => {
                let name = reuse_name(checker, arena, target, context, element.name, false)?;
                let r#type = pseudo_type_to_node(checker, arena, target, context, r#type)?;
                create_node(
                    arena,
                    target,
                    NodeData::PropertySignature(PropertySignatureData {
                        name: Some(name.node()),
                        modifiers,
                        question_token: None,
                        r#type: Some(r#type.node()),
                        initializer: None,
                    }),
                )?
            }
            PseudoObjectElementKind::SetAccessor { parameter, .. } => {
                let name = reuse_name(checker, arena, target, context, element.name, false)?;
                let parameter =
                    pseudo_parameter_to_node(checker, arena, target, context, parameter)?;
                let parameters = create_node_array(arena, target, vec![parameter])?;
                create_node(
                    arena,
                    target,
                    NodeData::SetAccessor(SetAccessorData {
                        name: Some(name.node()),
                        type_parameters: None,
                        parameters: Some(parameters),
                        r#type: None,
                        body: None,
                        modifiers: None,
                    }),
                )?
            }
            PseudoObjectElementKind::GetAccessor { r#type, .. } => {
                let name = reuse_name(checker, arena, target, context, element.name, false)?;
                let parameters = create_node_array(arena, target, Vec::new())?;
                let r#type = pseudo_type_to_node(checker, arena, target, context, r#type)?;
                create_node(
                    arena,
                    target,
                    NodeData::GetAccessor(GetAccessorData {
                        name: Some(name.node()),
                        type_parameters: None,
                        parameters: Some(parameters),
                        r#type: Some(r#type.node()),
                        body: None,
                        modifiers: None,
                    }),
                )?
            }
        };
        // The member's comments, when it is in the file being emitted.
        set_comment_range_2(checker, arena, member, element.member, context)
    })();
    if let Some(scope) = scope {
        exit_new_scope(context, scope);
    }
    result
}

/// tsgo-port: NodeBuilderImpl.pseudoParametersToNodeList @7.1 (pseudotypenodebuilder.go:327-333).
fn pseudo_parameters_to_node_list(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    parameters: &[PseudoParameter],
) -> BuildResult<Vec<TransformNode>> {
    let mut nodes = Vec::with_capacity(parameters.len());
    for parameter in parameters {
        nodes.push(pseudo_parameter_to_node(
            checker, arena, target, context, parameter,
        )?);
    }
    Ok(nodes)
}

/// tsgo-port: NodeBuilderImpl.pseudoParameterToNode @7.1 (pseudotypenodebuilder.go:335-357).
fn pseudo_parameter_to_node(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    parameter: &PseudoParameter,
) -> BuildResult<TransformNode> {
    let dot_dot_dot_token = parameter
        .rest
        .then(|| create_token(arena, target, SyntaxKind::DotDotDotToken))
        .transpose()?;
    let question_token = parameter
        .optional
        .then(|| create_token(arena, target, SyntaxKind::QuestionToken))
        .transpose()?;
    // Strada always reserializes parameter names from scratch.
    let symbol = checker
        .get_symbol_of_declaration(parameter.parameter)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    let name = parameter_to_parameter_declaration_name(
        checker,
        arena,
        target,
        symbol,
        Some(parameter.parameter),
        context,
    )?;
    let r#type = pseudo_type_to_node(checker, arena, target, context, &parameter.r#type)?;
    let node = create_node(
        arena,
        target,
        NodeData::Parameter(ParameterData {
            name: Some(name.node()),
            modifiers: None,
            dot_dot_dot_token: dot_dot_dot_token.map(TransformNode::node),
            question_token: question_token.map(TransformNode::node),
            r#type: Some(r#type.node()),
            initializer: None,
        }),
    )?;
    if checker.kind_of(parameter.parameter) == SyntaxKind::Parameter {
        return set_comment_range_2(checker, arena, node, parameter.parameter, context);
    }
    Ok(node)
}

/// tsgo-port: NodeBuilderImpl.pseudoTypeEquivalentToType @7.1 (pseudotypenodebuilder.go:359-583):
/// whether the pseudo type describes the checker's type, reporting the
/// inference fallbacks that keep it from doing so.
pub(super) fn pseudo_type_equivalent_to_type(
    checker: &mut CheckerState<'_>,
    context: &mut NodeBuilderContext<'_>,
    pseudo: &PseudoType,
    r#type: TypeId,
    is_optional_annotated: bool,
    report_errors: bool,
) -> BuildResult<bool> {
    // An error type is charitably equal: this may be single-file checking.
    if checker.tables.is_error_type(r#type) {
        return Ok(true);
    }
    // Compare types when the pseudo type maps to one (objects and other
    // structures are compared below).
    let type_from_pseudo = pseudo_type_to_type(checker, context, pseudo)?;
    if type_from_pseudo == Some(r#type) {
        return Ok(true);
    }
    let undefined_stripped = if is_optional_annotated {
        checker
            .get_type_with_facts(r#type, TypeFacts::NE_UNDEFINED)
            .map_err(|abort| checker_abort_error(checker, context, abort))?
    } else {
        r#type
    };
    if let Some(type_from_pseudo) = type_from_pseudo {
        if is_optional_annotated {
            if undefined_stripped == type_from_pseudo {
                return Ok(true);
            }
            // Unions may not be identical by id, through aliasing and the
            // like.
            if both_unions(checker, type_from_pseudo, undefined_stripped)
                && types_identical(checker, context, type_from_pseudo, undefined_stripped)?
            {
                return Ok(true);
            }
        }
        // Freshness may differ (a fresh `true` against a regular one in
        // `as const`).
        if checker.regular_type_of_literal_type(type_from_pseudo)
            == checker.regular_type_of_literal_type(r#type)
        {
            return Ok(true);
        }
        if both_unions(checker, type_from_pseudo, r#type)
            && types_identical(checker, context, type_from_pseudo, r#type)?
        {
            return Ok(true);
        }
    }
    match pseudo {
        PseudoType::Inferred(inferred) => {
            // Error nodes name the problematic children: report them and
            // let the parent fall back to the checker's type.
            if report_errors {
                if inferred.error_nodes.is_empty() {
                    report_inference_fallback(checker, context, inferred.expression)?;
                } else {
                    for &node in &inferred.error_nodes {
                        report_inference_fallback(checker, context, node)?;
                    }
                }
            }
            Ok(false)
        }
        PseudoType::ObjectLiteral(elements) => pseudo_object_literal_equivalent_to_type(
            checker,
            context,
            elements,
            undefined_stripped,
            report_errors,
        ),
        PseudoType::Tuple(elements) => {
            if !checker.tables.is_tuple_type(undefined_stripped) {
                return Ok(false);
            }
            // Pseudo tuples come from `as const` arrays and have only
            // required elements.
            let tuple_target = checker.tables.reference_target(undefined_stripped);
            let non_required = match &checker.tables.type_of(tuple_target).data {
                TypeData::TupleTarget(tuple) => {
                    tuple.combined_flags.intersects(ElementFlags::NON_REQUIRED)
                }
                _ => true,
            };
            if non_required {
                return Ok(false);
            }
            let element_types = checker
                .get_type_arguments(undefined_stripped)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            if elements.len() != element_types.len() {
                return Ok(false);
            }
            for (element, element_type) in elements.iter().zip(element_types) {
                if !pseudo_type_equivalent_to_type(
                    checker,
                    context,
                    element,
                    element_type,
                    false,
                    report_errors,
                )? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        PseudoType::SingleCallSignature(signature) => {
            let Some(target_signature) = checker
                .get_single_call_signature(undefined_stripped)
                .map_err(|abort| checker_abort_error(checker, context, abort))?
            else {
                return Ok(false);
            };
            let target_type_parameter_count = checker
                .signature_of(target_signature)
                .type_parameters
                .as_ref()
                .map_or(0, Vec::len);
            if target_type_parameter_count != signature.type_parameters.len() {
                if report_errors {
                    report_inference_fallback(checker, context, signature.signature)?;
                }
                return Ok(false);
            }
            if !pseudo_parameters_equivalent_to_parameters(
                checker,
                context,
                &signature.parameters,
                target_signature,
                report_errors,
                signature.signature,
            )? {
                return Ok(false);
            }
            let predicate = checker
                .get_type_predicate_of_signature(target_signature)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            if let Some(predicate) = predicate {
                if !pseudo_return_type_matches_predicate(
                    checker,
                    context,
                    &signature.return_type,
                    &predicate,
                )? {
                    if report_errors {
                        report_inference_fallback(checker, context, signature.signature)?;
                    }
                    return Ok(false);
                }
            } else {
                let return_type = checker
                    .get_return_type_of_signature(target_signature)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?;
                // The return type reports its own error.
                if !pseudo_type_equivalent_to_type(
                    checker,
                    context,
                    &signature.return_type,
                    return_type,
                    false,
                    report_errors,
                )? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        PseudoType::NoResult(declaration) => {
            if report_errors {
                report_inference_fallback(checker, context, *declaration)?;
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}

fn both_unions(checker: &CheckerState<'_>, left: TypeId, right: TypeId) -> bool {
    checker.tables.flags_of(left).intersects(TypeFlags::UNION)
        && checker.tables.flags_of(right).intersects(TypeFlags::UNION)
}

fn types_identical(
    checker: &mut CheckerState<'_>,
    context: &NodeBuilderContext<'_>,
    left: TypeId,
    right: TypeId,
) -> BuildResult<bool> {
    Ok(checker
        .compare_types_identical(left, right)
        .map_err(|abort| checker_abort_error(checker, context, abort))?
        == Ternary::TRUE)
}

/// pseudoTypeEquivalentToType's object literal arm
/// (pseudotypenodebuilder.go:418-524).
fn pseudo_object_literal_equivalent_to_type(
    checker: &mut CheckerState<'_>,
    context: &mut NodeBuilderContext<'_>,
    elements: &[PseudoObjectElement],
    undefined_stripped: TypeId,
    report_errors: bool,
) -> BuildResult<bool> {
    let target_properties = checker
        .get_properties_of_type(undefined_stripped)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    // A get/set pair is two elements but one property: count declarations.
    let target_declaration_count: usize = target_properties
        .iter()
        .map(|&property| checker.binder.symbol(property).declarations.len())
        .sum();
    if elements.len() != target_declaration_count {
        return Ok(false);
    }
    for element in elements {
        let mut target_property = None;
        if let Some(element_symbol) = checker.node_symbol(element.member) {
            let name = checker.binder.symbol(element_symbol).escaped_name;
            target_property = checker
                .get_property_of_type_ex(undefined_stripped, name, false)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
        }
        let target_property = match target_property {
            Some(property) => property,
            None => {
                // The name lookup failed: find the property whose
                // declaration has this name node.
                let by_name = target_properties.iter().copied().find(|&property| {
                    checker
                        .binder
                        .symbol(property)
                        .value_declaration
                        .is_some_and(|declaration| {
                            tsc_binder::node_util::get_name_of_declaration(
                                checker.binder.source_of_node(declaration),
                                declaration,
                            ) == Some(element.name)
                        })
                });
                match by_name {
                    Some(property) => property,
                    None => {
                        if report_errors {
                            report_inference_fallback(checker, context, element.member)?;
                        }
                        return Ok(false);
                    }
                }
            }
        };
        let target_is_optional = checker
            .symbol_flags(target_property)
            .intersects(SymbolFlags::OPTIONAL);
        if element.optional != target_is_optional {
            if report_errors {
                report_inference_fallback(checker, context, element.member)?;
            }
            return Ok(false);
        }
        let property_type = checker
            .get_type_of_symbol(target_property)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        let property_type = checker.remove_missing_type(property_type, target_is_optional);
        match &element.kind {
            PseudoObjectElementKind::PropertyAssignment { r#type, .. } => {
                if !pseudo_type_equivalent_to_type(
                    checker,
                    context,
                    r#type,
                    property_type,
                    element.optional,
                    false,
                )? {
                    if report_errors {
                        match r#type {
                            // Re-report the fine-grained error nodes the
                            // non-reporting comparison skipped.
                            PseudoType::Inferred(inferred) if !inferred.error_nodes.is_empty() => {
                                for &node in &inferred.error_nodes {
                                    report_inference_fallback(checker, context, node)?;
                                }
                            }
                            _ if !is_structural_pseudo_type(r#type) => {
                                report_inference_fallback(checker, context, element.member)?;
                            }
                            _ => {}
                        }
                    }
                    return Ok(false);
                }
            }
            PseudoObjectElementKind::Method {
                parameters,
                return_type,
                ..
            } => {
                let Some(target_signature) = checker
                    .get_single_call_signature(property_type)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?
                else {
                    // Without a single call signature there is nothing to
                    // validate.
                    continue;
                };
                if !pseudo_parameters_equivalent_to_parameters(
                    checker,
                    context,
                    parameters,
                    target_signature,
                    report_errors,
                    element.member,
                )? {
                    return Ok(false);
                }
                let predicate = checker
                    .get_type_predicate_of_signature(target_signature)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?;
                let matches = match predicate {
                    Some(predicate) => pseudo_return_type_matches_predicate(
                        checker,
                        context,
                        return_type,
                        &predicate,
                    )?,
                    None => {
                        let target_return = checker
                            .get_return_type_of_signature(target_signature)
                            .map_err(|abort| checker_abort_error(checker, context, abort))?;
                        pseudo_type_equivalent_to_type(
                            checker,
                            context,
                            return_type,
                            target_return,
                            false,
                            false,
                        )?
                    }
                };
                if !matches {
                    if report_errors {
                        report_inference_fallback(checker, context, element.member)?;
                    }
                    return Ok(false);
                }
            }
            PseudoObjectElementKind::GetAccessor { r#type, .. } => {
                if !pseudo_type_equivalent_to_type(
                    checker,
                    context,
                    r#type,
                    property_type,
                    false,
                    false,
                )? {
                    if report_errors {
                        report_inference_fallback(checker, context, element.member)?;
                    }
                    return Ok(false);
                }
            }
            PseudoObjectElementKind::SetAccessor { parameter, .. } => {
                let write_type = checker
                    .get_write_type_of_symbol(target_property)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?;
                if !pseudo_type_equivalent_to_type(
                    checker,
                    context,
                    &parameter.r#type,
                    write_type,
                    false,
                    false,
                )? {
                    if report_errors {
                        report_inference_fallback(checker, context, element.member)?;
                    }
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

/// tsgo-port: NodeBuilderImpl.pseudoParametersEquivalentToParameters @7.1
/// (pseudotypenodebuilder.go:585-630).
fn pseudo_parameters_equivalent_to_parameters(
    checker: &mut CheckerState<'_>,
    context: &mut NodeBuilderContext<'_>,
    parameters: &[PseudoParameter],
    target_signature: SignatureId,
    report_errors: bool,
    non_parameter_error_location: NodeId,
) -> BuildResult<bool> {
    let signature = checker.signature_of(target_signature).clone();
    let mut parameters = parameters;
    if let Some(this_parameter) = signature.this_parameter {
        let Some(first) = parameters.first() else {
            // A missing `this` parameter.
            if report_errors {
                report_inference_fallback(checker, context, non_parameter_error_location)?;
            }
            return Ok(false);
        };
        if !checker.parameter_is_this_keyword(first.parameter) {
            if report_errors {
                report_inference_fallback(checker, context, non_parameter_error_location)?;
            }
            return Ok(false);
        }
        let parameter_type = checker
            .get_type_of_parameter(this_parameter)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        if !pseudo_type_equivalent_to_type(
            checker,
            context,
            &first.r#type,
            parameter_type,
            first.optional,
            false,
        )? {
            if report_errors {
                report_inference_fallback(checker, context, first.parameter)?;
            }
            return Ok(false);
        }
        parameters = &parameters[1..];
    }
    if signature.parameters.len() != parameters.len() {
        if report_errors {
            report_inference_fallback(checker, context, non_parameter_error_location)?;
        }
        return Ok(false);
    }
    for (parameter, &target_parameter) in parameters.iter().zip(&signature.parameters) {
        let target_optional = match checker.binder.symbol(target_parameter).value_declaration {
            Some(declaration) => checker
                .emit_is_optional_parameter(declaration)
                .map_err(|abort| checker_abort_error(checker, context, abort))?,
            None => false,
        };
        if parameter.optional != target_optional {
            if report_errors {
                report_inference_fallback(checker, context, parameter.parameter)?;
            }
            return Ok(false);
        }
        let parameter_type = checker
            .get_type_of_parameter(target_parameter)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        if !pseudo_type_equivalent_to_type(
            checker,
            context,
            &parameter.r#type,
            parameter_type,
            parameter.optional,
            false,
        )? {
            if report_errors {
                report_inference_fallback(checker, context, parameter.parameter)?;
            }
            return Ok(false);
        }
    }
    Ok(true)
}

/// tsgo-port: isStructuralPseudoType @7.1 (pseudotypenodebuilder.go:632-641).
fn is_structural_pseudo_type(pseudo: &PseudoType) -> bool {
    match pseudo {
        PseudoType::ObjectLiteral(_)
        | PseudoType::Tuple(_)
        | PseudoType::SingleCallSignature(_) => true,
        PseudoType::MaybeConstLocation(location) => {
            is_structural_pseudo_type(&location.const_type)
                || is_structural_pseudo_type(&location.regular_type)
        }
        _ => false,
    }
}

/// tsgo-port: NodeBuilderImpl.pseudoReturnTypeMatchesPredicate @7.1
/// (pseudotypenodebuilder.go:643-687): whether a written type predicate
/// return type is the checker's predicate.
pub(super) fn pseudo_return_type_matches_predicate(
    checker: &mut CheckerState<'_>,
    context: &NodeBuilderContext<'_>,
    return_type: &PseudoType,
    predicate: &TypePredicate,
) -> BuildResult<bool> {
    let PseudoType::Direct(node) = return_type else {
        return Ok(false);
    };
    let NodeData::TypePredicate(data) = checker.data_of(*node) else {
        return Ok(false);
    };
    let is_asserts = data.asserts_modifier.is_some();
    let predicate_is_asserts = matches!(
        predicate.kind,
        TypePredicateKind::AssertsThis | TypePredicateKind::AssertsIdentifier
    );
    if is_asserts != predicate_is_asserts {
        return Ok(false);
    }
    let Some(parameter_name) = data.parameter_name else {
        return Ok(false);
    };
    let is_this = checker.kind_of(parameter_name) == SyntaxKind::ThisType;
    let predicate_is_this = matches!(
        predicate.kind,
        TypePredicateKind::This | TypePredicateKind::AssertsThis
    );
    if is_this != predicate_is_this {
        return Ok(false);
    }
    if !is_this && checker.identifier_text_of(parameter_name) != predicate.parameter_name.as_deref()
    {
        return Ok(false);
    }
    let predicate_type_node = data.r#type;
    match predicate.ty {
        Some(predicate_type) => {
            let Some(type_node) = predicate_type_node else {
                return Ok(false);
            };
            let from_node = checker
                .get_type_from_type_node(type_node)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            if from_node != predicate_type
                && !types_identical(checker, context, from_node, predicate_type)?
            {
                return Ok(false);
            }
        }
        None => {
            if predicate_type_node.is_some() {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// tsgo-port: NodeBuilderImpl.pseudoTypeToType @7.1 (pseudotypenodebuilder.go:689-765):
/// the checker type of a pseudo type that maps to one directly. Only
/// literals and the types around them map; the structures do not.
pub(super) fn pseudo_type_to_type(
    checker: &mut CheckerState<'_>,
    context: &NodeBuilderContext<'_>,
    pseudo: &PseudoType,
) -> BuildResult<Option<TypeId>> {
    let strict_null_checks = checker
        .options
        .strict_option_value(checker.options.strict_null_checks);
    let intrinsics = &checker.tables.intrinsics;
    let r#type = match pseudo {
        PseudoType::Direct(type_node) => checker
            .get_type_from_type_node(*type_node)
            .map_err(|abort| checker_abort_error(checker, context, abort))?,
        PseudoType::Inferred(inferred) => {
            if inferred.is_signature_return {
                let signature = checker
                    .get_signature_from_declaration(inferred.expression)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?;
                checker
                    .get_return_type_of_signature(signature)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?
            } else {
                let regular = regular_type_of_expression(checker, context, inferred.expression)?;
                checker
                    .get_widened_type(regular)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?
            }
        }
        PseudoType::NoResult(_) => return Ok(None),
        PseudoType::MaybeConstLocation(location) => {
            let in_const_context = checker
                .is_const_context(location.node)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            return pseudo_type_to_type(
                checker,
                context,
                if in_const_context {
                    &location.const_type
                } else {
                    &location.regular_type
                },
            );
        }
        PseudoType::Union(members) => {
            let mut types = Vec::with_capacity(members.len());
            let mut has_elided_type = false;
            for member in members {
                if !strict_null_checks && matches!(member, PseudoType::Undefined | PseudoType::Null)
                {
                    has_elided_type = true;
                    continue;
                }
                match pseudo_type_to_type(checker, context, member)? {
                    Some(r#type) => types.push(r#type),
                    None => return Ok(None),
                }
            }
            match types.len() {
                0 => {
                    if has_elided_type {
                        checker.tables.intrinsics.any
                    } else {
                        checker.tables.intrinsics.never
                    }
                }
                1 => types[0],
                _ => checker
                    .get_union_type_ex(&types, UnionReduction::Literal)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?,
            }
        }
        PseudoType::Undefined => intrinsics.undefined_widening,
        PseudoType::Null => intrinsics.null_widening,
        PseudoType::String => intrinsics.string,
        PseudoType::Number => intrinsics.number,
        PseudoType::BigInt => intrinsics.bigint,
        PseudoType::Boolean => intrinsics.boolean,
        PseudoType::False => intrinsics.false_fresh,
        PseudoType::True => intrinsics.true_fresh,
        // The cached expression types are a shortcut to the literal types.
        PseudoType::StringLiteral(source)
        | PseudoType::NumericLiteral(source)
        | PseudoType::BigIntLiteral(source) => {
            regular_type_of_expression(checker, context, *source)?
        }
        PseudoType::ObjectLiteral(_)
        | PseudoType::SingleCallSignature(_)
        | PseudoType::Tuple(_) => return Ok(None),
    };
    Ok(Some(r#type))
}

/// tsgo getRegularTypeOfExpression: the regular type of the expression's
/// type.
fn regular_type_of_expression(
    checker: &mut CheckerState<'_>,
    context: &NodeBuilderContext<'_>,
    expression: NodeId,
) -> BuildResult<TypeId> {
    let r#type = checker
        .get_type_of_expression(expression)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    Ok(checker.regular_type_of_literal_type(r#type))
}

/// tsgo-port: NodeBuilderImpl.reuseTypeNode @7.1 (nodecopy.go:56-76): reuse a
/// written type node, or report it and serialize its type.
fn reuse_type_node(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    node: NodeId,
) -> BuildResult<TransformNode> {
    if let Some(reused) =
        syntactic_try_reuse_existing_type_node(checker, arena, target, context, node)?
    {
        return Ok(reused);
    }
    report_inference_fallback(checker, context, node)?;
    let r#type = get_type_from_type_node2(checker, context, node, false)?
        .unwrap_or(checker.tables.intrinsics.error);
    type_to_type_node(checker, arena, target, context, r#type)
}

/// tsgo-port: NodeBuilderImpl.reuseNode @7.1 (nodecopy.go:12-18): the reused
/// node, or a plain clone of it when the reuse fails (tsgo would write
/// nothing there).
fn reuse_node_or_clone(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    node: NodeId,
) -> BuildResult<TransformNode> {
    if let Some(reused) = syntactic_try_reuse_existing_node(checker, arena, target, context, node)?
    {
        return Ok(reused);
    }
    let Some(clone) = super::type_nodes::clone_parse_node(checker, arena, node)? else {
        return keyword(arena, target, SyntaxKind::AnyKeyword);
    };
    if clone.source() == target {
        return Ok(clone);
    }
    arena
        .factory()
        .clone_node_to_source(clone, target)
        .map_err(factory_error)
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum PropertyNameKind {
    Identifier,
    StringLiteral,
    NumericLiteral,
}

/// tsgo-port: classifyPropertyName @7.1 (nodebuilderimpl.go:2456-2464).
fn classify_property_name(
    name: tsc_types::JsStr<'_>,
    string_named: bool,
    is_method: bool,
) -> PropertyNameKind {
    if is_method && name == "new" {
        return PropertyNameKind::StringLiteral;
    }
    if name.as_str().is_some_and(tsc_syntax::is_identifier_text) {
        return PropertyNameKind::Identifier;
    }
    if !string_named
        && crate::evaluate::is_numeric_literal_name(name)
        && crate::evaluate::js_string_to_number(name) >= 0.0
    {
        PropertyNameKind::NumericLiteral
    } else {
        PropertyNameKind::StringLiteral
    }
}

/// tsgo-port: NodeBuilderImpl.reuseName @7.1 (nodecopy.go:24-54): reuse a
/// property name, respelled as an identifier or a string literal where its
/// text calls for one.
fn reuse_name(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    name: NodeId,
    is_method: bool,
) -> BuildResult<TransformNode> {
    let reused = reuse_node_or_clone(checker, arena, target, context, name)?;
    let record = arena.node(reused).map_err(factory_error)?;
    // ast.TryGetTextOfPropertyName on the reused name.
    let (text, is_identifier, is_string_literal) = match &record.data {
        NodeData::Identifier(data) => (
            tsc_types::JsString::from(data.escaped_text.identifier_text()),
            true,
            false,
        ),
        NodeData::StringLiteral(data) => (data.text.clone(), false, true),
        NodeData::NumericLiteral(data) => {
            (tsc_types::JsString::from(data.text.as_str()), false, false)
        }
        NodeData::ComputedPropertyName(data) => {
            let expression = data
                .expression
                .map(|expression| arena.node(TransformNode::new(reused.source(), expression)))
                .transpose()
                .map_err(factory_error)?;
            match expression.map(|expression| &expression.data) {
                Some(NodeData::StringLiteral(data)) => (data.text.clone(), false, false),
                Some(NodeData::NoSubstitutionTemplateLiteral(data)) => {
                    (data.text.clone(), false, false)
                }
                Some(NodeData::NumericLiteral(data)) => {
                    (tsc_types::JsString::from(data.text.as_str()), false, false)
                }
                _ => return Ok(reused),
            }
        }
        _ => return Ok(reused),
    };
    let kind = classify_property_name(text.as_js(), is_string_literal, is_method);
    if (is_identifier && kind == PropertyNameKind::Identifier)
        || (is_string_literal && kind == PropertyNameKind::StringLiteral)
    {
        return Ok(reused);
    }
    let renamed = match kind {
        PropertyNameKind::Identifier => super::type_nodes::create_identifier(
            arena,
            target,
            text.as_str().expect("identifier text is scalar"),
        )?,
        PropertyNameKind::StringLiteral => arena
            .factory()
            .create_string_literal(target, text, false)
            .map_err(factory_error)?,
        PropertyNameKind::NumericLiteral => return Ok(reused),
    };
    arena
        .set_original_node(renamed, Some(reused))
        .map_err(factory_error)?;
    arena
        .factory()
        .set_text_range(renamed, reused)
        .map_err(factory_error)
}

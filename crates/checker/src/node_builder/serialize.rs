use tsc_binder::{node_util, SymbolId};
use tsc_emitter::{
    EmitFunctionProperty, EmitInternalNodeBuilderFlags, EmitNodeBuilderFlags, EmitResolverError,
    EmitResolverMethod, EmitResolverNode, EmitSymbolAccessibility, EmitSymbolAccessibilityResult,
    EmitSymbolExpansionOut, EmitSymbolMeaning, EmitSymbolTracker, EmitTrackerAccess,
    EmitTrackerNode, EmitTrackerNodeDescription, EmitTrackerSymbol, EmitTrackerSymbolDescription,
    SourceFileId, TransformArena, TransformNode, TransformNodeArray, TransformSourceId,
};
use tsc_syntax::nodes::{ImportTypeData, UnionTypeData};
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{CheckMode, ObjectFlags, SymbolFlags, TypeData, TypeFacts, TypeFlags, TypeId};

use crate::narrow::TypePredicate;
use crate::state::{CheckAbort, CheckerState, IndexInfo, SignatureId};

use super::signatures::{elide_initializer_and_set_emit_flags, track_computed_name};
use super::type_nodes::{
    checker_abort_error, clone_parse_node, create_identifier, create_node, create_node_array,
    create_token, factory_error, project_parse_node, set_no_ascii_escaping,
    type_to_type_node_helper, BuildResult,
};
use super::{
    add_symbol_type_to_context, can_possibly_expand_type, chains_symbol_to_expression,
    chains_symbol_to_type_node,
    existing_type_node_is_not_reference_or_is_reference_with_compatible_type_argument_count,
    get_declaration_with_type_annotation, get_enclosing_declaration_ignoring_fake_scope,
    get_module_specifier_override, get_type_from_type_node2,
    index_info_to_index_signature_declaration_helper, restore_flags,
    restore_symbol_type_to_context, save_restore_flags, serialize_inferred_type_for_declaration,
    set_text_range2, type_predicate_to_type_predicate_node_helper, with_context,
    NodeBuilderContext, SyntacticAccessorDeclarations, SyntacticBuilderResolver,
    SyntacticRecoveryBoundary, SyntacticScopeCleanup, SyntacticSymbol, SyntacticTrackedEntityName,
    SyntacticTypeNodeBuilder,
};

const METHOD: EmitResolverMethod = EmitResolverMethod::CreateTypeOfDeclaration;
const ALLOW_UNRESOLVED_NAMES: u32 = 8;

fn program_source_id(checker: &CheckerState<'_>, file_index: usize) -> SourceFileId {
    let raw = checker
        .authoritative_source_tokens
        .get(file_index)
        .map_or_else(
            || u32::try_from(file_index).unwrap_or_default(),
            |token| token.0,
        );
    SourceFileId::from_raw(raw)
}

fn resolver_node(checker: &CheckerState<'_>, node: NodeId) -> EmitResolverNode {
    EmitResolverNode::new(
        program_source_id(checker, checker.binder.file_index_of_node(node)),
        node,
    )
}

fn callback_abort_error(
    checker: &CheckerState<'_>,
    method: EmitResolverMethod,
    node: Option<NodeId>,
    abort: CheckAbort,
) -> EmitResolverError {
    let node = node.unwrap_or_else(|| checker.binder.source(0).root);
    EmitResolverError::CheckerAborted {
        method,
        node: resolver_node(checker, node),
        reason: abort.description(),
    }
}

fn syntactic_symbol(checker: &CheckerState<'_>, symbol: SymbolId) -> SyntacticSymbol {
    let declarations = &checker.binder.symbol(symbol).declarations;
    SyntacticSymbol {
        id: symbol,
        declaration_count: declarations.len(),
        variable_declaration_count: declarations
            .iter()
            .filter(|&&declaration| checker.kind_of(declaration) == SyntaxKind::VariableDeclaration)
            .count(),
    }
}

fn has_inferred_type(checker: &CheckerState<'_>, node: NodeId) -> bool {
    matches!(
        checker.kind_of(node),
        SyntaxKind::Parameter
            | SyntaxKind::PropertySignature
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::BindingElement
            | SyntaxKind::PropertyAccessExpression
            | SyntaxKind::ElementAccessExpression
            | SyntaxKind::BinaryExpression
            | SyntaxKind::VariableDeclaration
            | SyntaxKind::ExportAssignment
            | SyntaxKind::PropertyAssignment
            | SyntaxKind::ShorthandPropertyAssignment
            | SyntaxKind::JSDocParameterTag
            | SyntaxKind::JSDocPropertyTag
    )
}

fn node_is_synthesized(checker: &CheckerState<'_>, node: NodeId) -> bool {
    tsc_types::NodeFlags::from_bits(checker.node_flags(node))
        .intersects(tsc_types::NodeFlags::SYNTHESIZED)
}

const fn should_use_syntactic_inferred_declaration(
    has_inferred_type: bool,
    node_is_synthesized: bool,
    requires_widening: bool,
) -> bool {
    has_inferred_type && !node_is_synthesized && !requires_widening
}

fn is_accessor(checker: &CheckerState<'_>, node: NodeId) -> bool {
    matches!(
        checker.kind_of(node),
        SyntaxKind::GetAccessor | SyntaxKind::SetAccessor
    )
}

fn declaration_symbol(
    checker: &mut CheckerState<'_>,
    declaration: Option<NodeId>,
    supplied: Option<SymbolId>,
    context: &NodeBuilderContext<'_>,
) -> BuildResult<Option<SymbolId>> {
    if supplied.is_some() {
        return Ok(supplied);
    }
    declaration
        .map(|declaration| {
            checker
                .get_symbol_of_declaration(declaration)
                .map(Some)
                .map_err(|abort| checker_abort_error(checker, context, abort))
        })
        .unwrap_or(Ok(None))
}

/// tsc-port: parameterToParameterDeclarationName @6.0.3
/// tsc-hash: f8c988288813b2b174b4e49718f6864d442cbfe82334ec499d89013ec3df06a4
/// tsc-span: _tsc.js:52876-52909
fn serialize_parameter_name_from_parse(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    parameter: NodeId,
) -> BuildResult<TransformNode> {
    let symbol = checker
        .get_symbol_of_declaration(parameter)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    let name = match checker.data_of(parameter) {
        NodeData::Parameter(data) => data.name,
        NodeData::JSDocParameterTag(data) => data.name,
        _ => None,
    };
    let Some(name) = name else {
        return create_identifier(arena, target, &checker.symbol_display_name(symbol));
    };
    match checker.kind_of(name) {
        SyntaxKind::Identifier => {
            let name = clone_parse_node(checker, arena, name)?.unwrap_or(create_identifier(
                arena,
                target,
                &checker.symbol_display_name(symbol),
            )?);
            Ok(set_no_ascii_escaping(arena, name))
        }
        SyntaxKind::QualifiedName => {
            let right = match checker.data_of(name) {
                NodeData::QualifiedName(data) => data.right,
                _ => None,
            };
            let name = right
                .map(|right| clone_parse_node(checker, arena, right))
                .transpose()?
                .flatten()
                .unwrap_or(create_identifier(
                    arena,
                    target,
                    &checker.symbol_display_name(symbol),
                )?);
            Ok(set_no_ascii_escaping(arena, name))
        }
        SyntaxKind::ArrayBindingPattern | SyntaxKind::ObjectBindingPattern => {
            elide_initializer_and_set_emit_flags(checker, arena, target, name, context)
        }
        _ => create_identifier(arena, target, &checker.symbol_display_name(symbol)),
    }
}

fn syntactic_type_of_declaration(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    declaration: NodeId,
    symbol: SymbolId,
) -> BuildResult<Option<TransformNode>> {
    let Some(declaration) = project_parse_node(checker, arena, declaration)? else {
        return Ok(None);
    };
    let symbol = syntactic_symbol(checker, symbol);
    let builder = SyntacticTypeNodeBuilder::new(checker.options);
    let snapshot = arena.clone();
    let mut resolver = ProductionSyntacticBuilderResolver::new(checker, snapshot, METHOD);
    builder.serialize_type_of_declaration(
        &mut resolver,
        arena,
        target,
        context,
        declaration,
        Some(symbol),
    )
}

fn syntactic_type_of_accessor(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    declaration: NodeId,
    symbol: SymbolId,
) -> BuildResult<Option<TransformNode>> {
    let Some(declaration) = project_parse_node(checker, arena, declaration)? else {
        return Ok(None);
    };
    let symbol = syntactic_symbol(checker, symbol);
    let builder = SyntacticTypeNodeBuilder::new(checker.options);
    let snapshot = arena.clone();
    let mut resolver = ProductionSyntacticBuilderResolver::new(checker, snapshot, METHOD);
    builder.serialize_type_of_accessor(
        &mut resolver,
        arena,
        target,
        context,
        declaration,
        Some(symbol),
    )
}

/// tsc-port: serializeTypeForDeclaration @6.0.3
/// tsc-hash: 61ebc9bf5f2f88bf1e2a94886d4878fb12a562ca515d046a7d782c34c54ce979
/// tsc-span: _tsc.js:53487-53508
fn serialize_type_for_declaration_in_context(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    declaration: Option<NodeId>,
    mut r#type: TypeId,
    supplied_symbol: Option<SymbolId>,
) -> BuildResult<Option<TransformNode>> {
    let symbol = declaration_symbol(checker, declaration, supplied_symbol, context)?;
    let add_undefined_for_parameter = declaration.is_some_and(|declaration| {
        matches!(
            checker.kind_of(declaration),
            SyntaxKind::Parameter | SyntaxKind::JSDocParameterTag
        )
    }) && checker
        .emit_requires_adding_implicit_undefined(
            declaration.expect("parameter declaration selected above"),
            context.enclosing_declaration,
        )
        .map_err(|abort| checker_abort_error(checker, context, abort))?;

    let decl = match (declaration, symbol) {
        (Some(declaration), _) => Some(declaration),
        (None, Some(symbol)) => checker
            .binder
            .symbol(symbol)
            .value_declaration
            .or(get_declaration_with_type_annotation(
                checker,
                symbol,
                context.enclosing_declaration,
                context,
            )?)
            .or_else(|| checker.binder.symbol(symbol).declarations.first().copied()),
        (None, None) => None,
    };

    let mut result = None;
    if let (Some(symbol), Some(decl)) = (symbol, decl) {
        if !can_possibly_expand_type(r#type, context) {
            let restore = add_symbol_type_to_context(context, symbol, r#type);
            let syntactic = if is_accessor(checker, decl) {
                syntactic_type_of_accessor(checker, arena, target, context, decl, symbol)
            } else if should_use_syntactic_inferred_declaration(
                has_inferred_type(checker, decl),
                node_is_synthesized(checker, decl),
                checker
                    .tables
                    .object_flags_of(r#type)
                    .intersects(ObjectFlags::REQUIRES_WIDENING),
            ) {
                syntactic_type_of_declaration(checker, arena, target, context, decl, symbol)
            } else {
                Ok(None)
            };
            restore_symbol_type_to_context(context, restore);
            result = syntactic?;
        }
    }

    if result.is_none() {
        if add_undefined_for_parameter {
            r#type = checker
                .get_optional_type(r#type, false)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
        }
        result = match symbol {
            Some(symbol) => serialize_inferred_type_for_declaration(
                checker, arena, target, symbol, context, r#type,
            )?,
            None => type_to_type_node_helper(checker, arena, target, r#type, context)?,
        };
    }
    match result {
        Some(result) => Ok(Some(result)),
        None => create_token(arena, target, SyntaxKind::AnyKeyword).map(Some),
    }
}

/// tsc-port: typeNodeIsEquivalentToType @6.0.3
/// tsc-hash: 4d74998c72ff7900940c68b5e83d4ed47975b2fe6a352c61e7956862c7043927
/// tsc-span: _tsc.js:53509-53523
fn type_node_is_equivalent_to_type(
    checker: &mut CheckerState<'_>,
    annotated_declaration: Option<NodeId>,
    r#type: TypeId,
    type_from_type_node: TypeId,
) -> Result<bool, CheckAbort> {
    if type_from_type_node == r#type {
        return Ok(true);
    }
    let Some(annotated_declaration) = annotated_declaration else {
        return Ok(false);
    };
    let question_equivalent = match checker.kind_of(annotated_declaration) {
        SyntaxKind::PropertySignature | SyntaxKind::PropertyDeclaration => {
            checker.has_question_token(annotated_declaration)
        }
        SyntaxKind::Parameter => checker.is_optional_declaration(annotated_declaration),
        _ => false,
    };
    if !question_equivalent {
        return Ok(false);
    }
    Ok(checker.get_type_with_facts(r#type, TypeFacts::NE_UNDEFINED)? == type_from_type_node)
}

/// tsc-port: serializeInferredReturnTypeForSignature @6.0.3
/// tsc-hash: 390f2fcd36f4dba76b558db8246c41346b6c2ee61a9aef3ae2193329be77c292
/// tsc-span: _tsc.js:53547-53554
fn serialize_inferred_return_type_for_signature(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    signature: SignatureId,
    return_type: TypeId,
) -> BuildResult<Option<TransformNode>> {
    let old_suppress = context.suppress_report_inference_fallback;
    context.suppress_report_inference_fallback = true;
    let result = (|| {
        let predicate = checker
            .get_type_predicate_of_signature(signature)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        if let Some(mut predicate) = predicate {
            if let (Some(mapper), Some(predicate_type)) = (context.mapper, predicate.ty) {
                predicate.ty = Some(
                    checker
                        .instantiate_type(predicate_type, Some(mapper))
                        .map_err(|abort| checker_abort_error(checker, context, abort))?,
                );
            }
            type_predicate_to_type_predicate_node_helper(
                checker, arena, target, &predicate, context,
            )
            .map(Some)
        } else {
            type_to_type_node_helper(checker, arena, target, return_type, context)
        }
    })();
    context.suppress_report_inference_fallback = old_suppress;
    result
}

fn syntactic_return_type_for_signature(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    declaration: NodeId,
    symbol: SymbolId,
) -> BuildResult<Option<TransformNode>> {
    let Some(declaration) = project_parse_node(checker, arena, declaration)? else {
        return Ok(None);
    };
    let symbol = syntactic_symbol(checker, symbol);
    let builder = SyntacticTypeNodeBuilder::new(checker.options);
    let snapshot = arena.clone();
    let mut resolver = ProductionSyntacticBuilderResolver::new(
        checker,
        snapshot,
        EmitResolverMethod::CreateReturnTypeOfSignatureDeclaration,
    );
    builder.serialize_return_type_for_signature(
        &mut resolver,
        arena,
        target,
        context,
        declaration,
        Some(symbol),
    )
}

/// tsc-port: serializeReturnTypeForSignature @6.0.3
/// tsc-hash: 31fc902e4dc5253fc144eb471e4f27423714c36d86bf4af777b4186cabb4b123
/// tsc-span: _tsc.js:53524-53546
fn serialize_return_type_for_signature_in_context(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    signature: SignatureId,
) -> BuildResult<Option<TransformNode>> {
    let suppress_any = context
        .flags
        .contains(EmitNodeBuilderFlags::SUPPRESS_ANY_RETURN_TYPE);
    let restore_flags_value = save_restore_flags(context);
    if suppress_any {
        context.flags.0 &= !EmitNodeBuilderFlags::SUPPRESS_ANY_RETURN_TYPE.0;
    }
    let result = (|| {
        let return_type = checker
            .get_return_type_of_signature(signature)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        let mut return_type_node = None;
        if !(suppress_any
            && checker
                .tables
                .flags_of(return_type)
                .intersects(TypeFlags::ANY))
        {
            if let Some(declaration) = checker.signature_of(signature).declaration {
                if !node_is_synthesized(checker, declaration)
                    && !can_possibly_expand_type(return_type, context)
                {
                    let declaration_symbol = checker
                        .get_symbol_of_declaration(declaration)
                        .map_err(|abort| checker_abort_error(checker, context, abort))?;
                    let restore =
                        add_symbol_type_to_context(context, declaration_symbol, return_type);
                    let syntactic = syntactic_return_type_for_signature(
                        checker,
                        arena,
                        target,
                        context,
                        declaration,
                        declaration_symbol,
                    );
                    restore_symbol_type_to_context(context, restore);
                    return_type_node = syntactic?;
                }
            }
            if return_type_node.is_none() {
                return_type_node = serialize_inferred_return_type_for_signature(
                    checker,
                    arena,
                    target,
                    context,
                    signature,
                    return_type,
                )?;
            }
        }
        if return_type_node.is_none() && !suppress_any {
            return_type_node = Some(create_token(arena, target, SyntaxKind::AnyKeyword)?);
        }
        Ok(return_type_node)
    })();
    restore_flags(context, restore_flags_value);
    result
}

pub(crate) fn serialize_type_for_declaration_seam(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    declaration: Option<NodeId>,
    r#type: TypeId,
    symbol: Option<SymbolId>,
) -> BuildResult<Option<TransformNode>> {
    serialize_type_for_declaration_in_context(
        checker,
        arena,
        target,
        context,
        declaration,
        r#type,
        symbol,
    )
}

pub(crate) fn serialize_return_type_for_signature_seam(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    signature: SignatureId,
) -> BuildResult<Option<TransformNode>> {
    serialize_return_type_for_signature_in_context(checker, arena, target, context, signature)
}

pub(crate) fn syntactic_try_reuse_existing_type_node(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    type_node: NodeId,
) -> BuildResult<Option<TransformNode>> {
    let Some(type_node) = project_parse_node(checker, arena, type_node)? else {
        return Ok(None);
    };
    let builder = SyntacticTypeNodeBuilder::new(checker.options);
    let snapshot = arena.clone();
    let mut resolver = ProductionSyntacticBuilderResolver::new(checker, snapshot, METHOD);
    builder.try_reuse_existing_type_node(&mut resolver, arena, target, context, type_node)
}

pub(crate) fn syntactic_serialize_name_of_parameter_seam(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    parameter: NodeId,
) -> BuildResult<Option<TransformNode>> {
    serialize_parameter_name_from_parse(checker, arena, target, context, parameter).map(Some)
}

/// tsc-port: typeToTypeNode (createNodeBuilder API) @6.0.3
/// tsc-hash: b69637a60229522776d46a72086e27f8689094ecbb8a3686f6eb28e61f5a51fa
/// tsc-span: _tsc.js:50959
#[allow(clippy::too_many_arguments)]
pub(crate) fn type_to_type_node(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    r#type: TypeId,
    enclosing_declaration: Option<NodeId>,
    flags: Option<EmitNodeBuilderFlags>,
    internal_flags: Option<EmitInternalNodeBuilderFlags>,
    tracker: Option<&mut dyn EmitSymbolTracker>,
    maximum_length: Option<u32>,
    verbosity_level: Option<i32>,
    out: Option<&mut EmitSymbolExpansionOut>,
) -> BuildResult<Option<TransformNode>> {
    with_context(
        checker,
        arena,
        target,
        enclosing_declaration,
        flags,
        internal_flags,
        tracker,
        maximum_length,
        verbosity_level,
        |checker, arena, target, context| {
            type_to_type_node_helper(checker, arena, target, r#type, context)
        },
        out,
    )
    .map(Option::flatten)
}

/// tsc-port: typePredicateToTypePredicateNode (createNodeBuilder API) @6.0.3
/// tsc-hash: d9642e57675b431e6f70a1efc27777227e772d3877deb5d0635c21e68034f6fc
/// tsc-span: _tsc.js:50960-50970
#[allow(clippy::too_many_arguments)]
pub(crate) fn type_predicate_to_type_predicate_node(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    predicate: &TypePredicate,
    enclosing_declaration: Option<NodeId>,
    flags: Option<EmitNodeBuilderFlags>,
    internal_flags: Option<EmitInternalNodeBuilderFlags>,
    tracker: Option<&mut dyn EmitSymbolTracker>,
) -> BuildResult<Option<TransformNode>> {
    with_context(
        checker,
        arena,
        target,
        enclosing_declaration,
        flags,
        internal_flags,
        tracker,
        None,
        None,
        |checker, arena, target, context| {
            type_predicate_to_type_predicate_node_helper(checker, arena, target, predicate, context)
        },
        None,
    )
}

/// tsc-port: serializeTypeForDeclaration (createNodeBuilder API) @6.0.3
/// tsc-hash: 0b9c6849911106ffd21f1f820c2e628f53811a4ccc92c47764571ba0bdad25fb
/// tsc-span: _tsc.js:50971-50981
#[allow(clippy::too_many_arguments)]
pub(crate) fn serialize_type_for_declaration(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    declaration: NodeId,
    symbol: SymbolId,
    enclosing_declaration: Option<NodeId>,
    flags: Option<EmitNodeBuilderFlags>,
    internal_flags: Option<EmitInternalNodeBuilderFlags>,
    tracker: Option<&mut dyn EmitSymbolTracker>,
) -> BuildResult<Option<TransformNode>> {
    with_context(
        checker,
        arena,
        target,
        enclosing_declaration,
        flags,
        internal_flags,
        tracker,
        None,
        None,
        |checker, arena, target, context| {
            let Some(declaration) = project_parse_node(checker, arena, declaration)? else {
                return Ok(None);
            };
            let symbol = syntactic_symbol(checker, symbol);
            let builder = SyntacticTypeNodeBuilder::new(checker.options);
            let snapshot = arena.clone();
            let mut resolver = ProductionSyntacticBuilderResolver::new(checker, snapshot, METHOD);
            builder.serialize_type_of_declaration(
                &mut resolver,
                arena,
                target,
                context,
                declaration,
                Some(symbol),
            )
        },
        None,
    )
    .map(Option::flatten)
}

/// tsc-port: serializeReturnTypeForSignature (createNodeBuilder API) @6.0.3
/// tsc-hash: 354ab894aedc697bd7aac7bcc8c242ad52dc95f63f8adc67493b609e0b2d3909
/// tsc-span: _tsc.js:50982-50992
#[allow(clippy::too_many_arguments)]
pub(crate) fn serialize_return_type_for_signature(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    signature_declaration: NodeId,
    enclosing_declaration: Option<NodeId>,
    flags: Option<EmitNodeBuilderFlags>,
    internal_flags: Option<EmitInternalNodeBuilderFlags>,
    tracker: Option<&mut dyn EmitSymbolTracker>,
) -> BuildResult<Option<TransformNode>> {
    with_context(
        checker,
        arena,
        target,
        enclosing_declaration,
        flags,
        internal_flags,
        tracker,
        None,
        None,
        |checker, arena, target, context| {
            let symbol = checker
                .get_symbol_of_declaration(signature_declaration)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            let Some(signature_declaration) =
                project_parse_node(checker, arena, signature_declaration)?
            else {
                return Ok(None);
            };
            let symbol = syntactic_symbol(checker, symbol);
            let builder = SyntacticTypeNodeBuilder::new(checker.options);
            let snapshot = arena.clone();
            let mut resolver = ProductionSyntacticBuilderResolver::new(
                checker,
                snapshot,
                EmitResolverMethod::CreateReturnTypeOfSignatureDeclaration,
            );
            builder.serialize_return_type_for_signature(
                &mut resolver,
                arena,
                target,
                context,
                signature_declaration,
                Some(symbol),
            )
        },
        None,
    )
    .map(Option::flatten)
}

/// tsc-port: serializeTypeForExpression (createNodeBuilder API) @6.0.3
/// tsc-hash: a16196e77a3c9ff3cfad115c05536b0fec9c8bebc5fd8969124d9d31221ae6dd
/// tsc-span: _tsc.js:50993-51003
#[allow(clippy::too_many_arguments)]
pub(crate) fn serialize_type_for_expression(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    expression: NodeId,
    enclosing_declaration: Option<NodeId>,
    flags: Option<EmitNodeBuilderFlags>,
    internal_flags: Option<EmitInternalNodeBuilderFlags>,
    tracker: Option<&mut dyn EmitSymbolTracker>,
) -> BuildResult<Option<TransformNode>> {
    with_context(
        checker,
        arena,
        target,
        enclosing_declaration,
        flags,
        internal_flags,
        tracker,
        None,
        None,
        |checker, arena, target, context| {
            let Some(expression) = project_parse_node(checker, arena, expression)? else {
                return Ok(None);
            };
            let builder = SyntacticTypeNodeBuilder::new(checker.options);
            let snapshot = arena.clone();
            let mut resolver = ProductionSyntacticBuilderResolver::new(
                checker,
                snapshot,
                EmitResolverMethod::CreateTypeOfExpression,
            );
            builder.serialize_type_of_expression(&mut resolver, arena, target, context, expression)
        },
        None,
    )
    .map(Option::flatten)
}

/// tsc-port: indexInfoToIndexSignatureDeclaration (createNodeBuilder API) @6.0.3
/// tsc-hash: f680044bff5ea7267a26b9dddc4d284a4a95efcbecbf2d1448d2bc2dbd8a8805
/// tsc-span: _tsc.js:51004-51019
#[allow(clippy::too_many_arguments)]
pub(crate) fn index_info_to_index_signature_declaration(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    index_info: &IndexInfo,
    enclosing_declaration: Option<NodeId>,
    flags: Option<EmitNodeBuilderFlags>,
    internal_flags: Option<EmitInternalNodeBuilderFlags>,
    tracker: Option<&mut dyn EmitSymbolTracker>,
) -> BuildResult<Option<TransformNode>> {
    with_context(
        checker,
        arena,
        target,
        enclosing_declaration,
        flags,
        internal_flags,
        tracker,
        None,
        None,
        |checker, arena, target, context| {
            index_info_to_index_signature_declaration_helper(
                checker, arena, target, index_info, context, None,
            )
        },
        None,
    )
}

struct ProductionSyntacticBuilderResolver<'state, 'program> {
    checker: &'state mut CheckerState<'program>,
    arena_snapshot: TransformArena,
    method: EmitResolverMethod,
}

impl<'state, 'program> ProductionSyntacticBuilderResolver<'state, 'program> {
    fn new(
        checker: &'state mut CheckerState<'program>,
        arena_snapshot: TransformArena,
        method: EmitResolverMethod,
    ) -> Self {
        Self {
            checker,
            arena_snapshot,
            method,
        }
    }

    fn parse_node(&self, node: TransformNode) -> BuildResult<NodeId> {
        self.arena_snapshot
            .require_parse_tree_resolver_node(node)
            .map(|node| node.node())
            .map_err(factory_error)
    }

    fn symbol(&self, symbol: EmitTrackerSymbol) -> Option<SymbolId> {
        u32::try_from(symbol.0)
            .ok()
            .map(SymbolId)
            .filter(|&symbol| self.checker.binder.try_symbol(symbol).is_some())
    }

    fn tracker_node(&self, node: EmitTrackerNode) -> Option<NodeId> {
        u32::try_from(node.0)
            .ok()
            .map(NodeId)
            .filter(|&node| self.checker.binder.try_file_index_of_node(node).is_some())
    }

    fn invalid_token_error(&self, node: Option<NodeId>) -> EmitResolverError {
        let node = node.unwrap_or_else(|| self.checker.binder.source(0).root);
        EmitResolverError::CheckerAborted {
            method: self.method,
            node: resolver_node(self.checker, node),
            reason: "syntacticBuilderResolver received an invalid checker token",
        }
    }

    fn parse_symbol(
        &mut self,
        node: NodeId,
        supplied: Option<SyntacticSymbol>,
        context: &NodeBuilderContext<'_>,
    ) -> BuildResult<SymbolId> {
        match supplied {
            Some(symbol) if self.checker.binder.try_symbol(symbol.id).is_some() => Ok(symbol.id),
            Some(_) => Err(self.invalid_token_error(Some(node))),
            None => self
                .checker
                .get_symbol_of_declaration(node)
                .map_err(|abort| checker_abort_error(self.checker, context, abort)),
        }
    }

    fn project_parse_node(&self, node: NodeId) -> BuildResult<Option<TransformNode>> {
        self.arena_snapshot
            .parse_tree_transform_node(resolver_node(self.checker, node))
            .map_err(factory_error)
    }
}

impl EmitTrackerAccess for ProductionSyntacticBuilderResolver<'_, '_> {
    fn is_symbol_accessible(
        &mut self,
        symbol: EmitTrackerSymbol,
        enclosing_declaration: Option<EmitTrackerNode>,
        meaning: EmitSymbolMeaning,
        should_compute_aliases: bool,
    ) -> Result<EmitSymbolAccessibilityResult, EmitResolverError> {
        let enclosing = enclosing_declaration.and_then(|node| self.tracker_node(node));
        let symbol = self
            .symbol(symbol)
            .ok_or_else(|| self.invalid_token_error(enclosing))?;
        let enclosing = enclosing.ok_or_else(|| self.invalid_token_error(None))?;
        self.checker
            .emit_is_symbol_accessible(symbol, enclosing, meaning, should_compute_aliases)
            .map_err(|abort| {
                callback_abort_error(self.checker, self.method, Some(enclosing), abort)
            })
    }

    fn is_expando_function_declaration(
        &mut self,
        node: EmitTrackerNode,
    ) -> Result<bool, EmitResolverError> {
        let node = self
            .tracker_node(node)
            .ok_or_else(|| self.invalid_token_error(None))?;
        self.checker
            .emit_is_expando_function_declaration(node)
            .map_err(|abort| callback_abort_error(self.checker, self.method, Some(node), abort))
    }

    fn get_properties_of_container_function(
        &mut self,
        node: EmitTrackerNode,
    ) -> Result<Vec<EmitFunctionProperty>, EmitResolverError> {
        let node = self
            .tracker_node(node)
            .ok_or_else(|| self.invalid_token_error(None))?;
        self.checker
            .emit_get_properties_of_container_function(node, 0)
            .map_err(|abort| callback_abort_error(self.checker, self.method, Some(node), abort))
    }

    fn requires_adding_implicit_undefined(
        &mut self,
        parameter: EmitTrackerNode,
        enclosing_declaration: Option<EmitTrackerNode>,
    ) -> Result<bool, EmitResolverError> {
        let parameter = self
            .tracker_node(parameter)
            .ok_or_else(|| self.invalid_token_error(None))?;
        let enclosing = enclosing_declaration.and_then(|node| self.tracker_node(node));
        self.checker
            .emit_requires_adding_implicit_undefined(parameter, enclosing)
            .map_err(|abort| {
                callback_abort_error(self.checker, self.method, Some(parameter), abort)
            })
    }

    fn describe_symbol(&mut self, symbol: EmitTrackerSymbol) -> EmitTrackerSymbolDescription {
        let Some(symbol) = self.symbol(symbol) else {
            return EmitTrackerSymbolDescription::default();
        };
        let data = self.checker.binder.symbol(symbol);
        EmitTrackerSymbolDescription {
            escaped_name: data.escaped_name.clone(),
            declaration_count: u32::try_from(data.declarations.len()).unwrap_or(u32::MAX),
            declarations: data
                .declarations
                .iter()
                .take(8)
                .map(|&node| EmitTrackerNodeDescription {
                    parse: Some(resolver_node(self.checker, node)),
                    original: None,
                })
                .collect(),
        }
    }

    fn describe_node(&mut self, node: EmitTrackerNode) -> EmitTrackerNodeDescription {
        self.tracker_node(node)
            .map(|node| EmitTrackerNodeDescription {
                parse: Some(resolver_node(self.checker, node)),
                original: None,
            })
            .unwrap_or_default()
    }
}

fn literal_import_type(data: &ImportTypeData, checker: &CheckerState<'_>) -> bool {
    data.argument.is_some_and(|argument| {
        matches!(
            checker.data_of(argument),
            NodeData::LiteralType(literal)
                if literal
                    .literal
                    .is_some_and(|literal| checker.kind_of(literal) == SyntaxKind::StringLiteral)
        )
    })
}

fn some_type_is_undefined(checker: &CheckerState<'_>, r#type: TypeId) -> bool {
    if let TypeData::Union { types, .. } = &checker.tables.type_of(r#type).data {
        return types.iter().any(|&member| {
            checker
                .tables
                .flags_of(member)
                .intersects(TypeFlags::UNDEFINED)
        });
    }
    checker
        .tables
        .flags_of(r#type)
        .intersects(TypeFlags::UNDEFINED)
}

fn contains_non_missing_undefined(checker: &CheckerState<'_>, r#type: TypeId) -> bool {
    let candidate = match &checker.tables.type_of(r#type).data {
        TypeData::Union { types, .. } => types.first().copied().unwrap_or(r#type),
        _ => r#type,
    };
    candidate != checker.tables.intrinsics.missing
        && checker
            .tables
            .flags_of(candidate)
            .intersects(TypeFlags::UNDEFINED)
}

fn is_value_signature_declaration(checker: &CheckerState<'_>, node: NodeId) -> bool {
    matches!(
        checker.kind_of(node),
        SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::Constructor
    )
}

fn is_declaration_name(checker: &CheckerState<'_>, node: NodeId) -> bool {
    let Some(parent) = checker.parent_of(node) else {
        return false;
    };
    match checker.data_of(parent) {
        NodeData::TypeParameter(data) => data.name == Some(node),
        NodeData::Parameter(data) => data.name == Some(node),
        NodeData::PropertySignature(data) => data.name == Some(node),
        NodeData::PropertyDeclaration(data) => data.name == Some(node),
        NodeData::MethodSignature(data) => data.name == Some(node),
        NodeData::MethodDeclaration(data) => data.name == Some(node),
        NodeData::VariableDeclaration(data) => data.name == Some(node),
        NodeData::BindingElement(data) => data.name == Some(node),
        NodeData::InferType(data) => data.type_parameter == Some(node),
        _ => false,
    }
}

fn is_js_exports_entity_name(checker: &CheckerState<'_>, leftmost: NodeId) -> bool {
    let source = checker.binder.source_of_node(leftmost);
    if tsc_binder::assignment::is_exports_identifier(source, leftmost) {
        return true;
    }
    let Some(parent) = checker.parent_of(leftmost) else {
        return false;
    };
    if tsc_binder::assignment::is_module_exports_access_expression(source, parent) {
        return true;
    }
    matches!(
        checker.data_of(parent),
        NodeData::QualifiedName(data)
            if data.left.is_some_and(|left| checker.identifier_text_of(left) == Some("module"))
                && data.right.is_some_and(|right| {
                    tsc_binder::assignment::is_exports_identifier(source, right)
                })
    )
}

fn entity_meaning_flags(meaning: EmitSymbolMeaning) -> SymbolFlags {
    SymbolFlags::from_bits(meaning.0 as i32)
}

impl ProductionSyntacticBuilderResolver<'_, '_> {
    fn report_inference_fallback(
        &mut self,
        context: &mut NodeBuilderContext<'_>,
        node: NodeId,
    ) -> BuildResult<()> {
        let NodeBuilderContext {
            tracker,
            reported_diagnostic,
            suppress_report_inference_fallback,
            ..
        } = context;
        tracker.report_inference_fallback(
            reported_diagnostic,
            *suppress_report_inference_fallback,
            self,
            node,
        )
    }

    fn track_symbol(
        &mut self,
        context: &mut NodeBuilderContext<'_>,
        symbol: SymbolId,
        meaning: EmitSymbolMeaning,
    ) -> BuildResult<()> {
        let symbol_flags = self.checker.symbol_flags(symbol);
        let NodeBuilderContext {
            tracker,
            reported_diagnostic,
            tracked_symbols,
            enclosing_declaration,
            ..
        } = context;
        tracker.track_symbol(
            reported_diagnostic,
            tracked_symbols,
            self,
            symbol,
            symbol_flags,
            *enclosing_declaration,
            meaning,
        )?;
        Ok(())
    }

    /// tsc-port: trackExistingEntityName.attachSymbolToLeftmostIdentifier @6.0.3
    /// tsc-hash: a6a0748d03d57db3841aec3cc92e8e26e7fb3de1e2f1a14370d4d9cf69f9caed
    /// tsc-span: _tsc.js:53640-53654
    fn attach_symbol_to_entity_name(
        &mut self,
        arena: &mut TransformArena,
        context: &mut NodeBuilderContext<'_>,
        node: TransformNode,
        leftmost: NodeId,
        symbol: Option<SymbolId>,
    ) -> BuildResult<TransformNode> {
        if node.node() == leftmost {
            if let Some(symbol) = symbol.filter(|&symbol| {
                self.checker
                    .symbol_flags(symbol)
                    .intersects(SymbolFlags::TYPE_PARAMETER)
            }) {
                let declared = self.checker.get_declared_type_of_type_parameter(symbol);
                let name = super::type_parameter_to_name(
                    self.checker,
                    arena,
                    node.source(),
                    declared,
                    context,
                )?;
                arena
                    .metadata_mut(name)
                    .add_flags(tsc_emitter::EmitFlags::NO_ASCII_ESCAPING);
                return set_text_range2(self.checker, arena, context, name, Some(node));
            }
        }
        let cloned = arena.factory().clone_node(node).map_err(factory_error)?;
        if arena.node(cloned).map_err(factory_error)?.kind == SyntaxKind::Identifier {
            arena
                .metadata_mut(cloned)
                .add_flags(tsc_emitter::EmitFlags::NO_ASCII_ESCAPING);
        } else if let Some(leftmost) = self.project_parse_node(leftmost)? {
            arena
                .metadata_mut(leftmost)
                .add_flags(tsc_emitter::EmitFlags::NO_ASCII_ESCAPING);
        }
        set_text_range2(self.checker, arena, context, cloned, Some(node))
    }

    /// tsc-port: canReuseTypeNode @6.0.3
    /// tsc-hash: af141a7d202b5ffec61fda03caf2df8dbc2cd77d7eea3dc4e40383975bf30673
    /// tsc-span: _tsc.js:53675-53711
    fn can_reuse_type_node_parse(
        &mut self,
        context: &mut NodeBuilderContext<'_>,
        existing: NodeId,
    ) -> BuildResult<bool> {
        let Some(r#type) = get_type_from_type_node2(self.checker, context, existing, true)? else {
            return Ok(false);
        };
        if let NodeData::ImportType(data) = self.checker.data_of(existing).clone() {
            if self.checker.is_in_js_file(existing) && literal_import_type(&data, self.checker) {
                self.checker
                    .get_type_from_type_node(existing)
                    .map_err(|abort| checker_abort_error(self.checker, context, abort))?;
                let Some(symbol) = self.checker.links.node(existing).resolved_symbol.resolved()
                else {
                    return Ok(true);
                };
                if !data.is_type_of
                    && !self
                        .checker
                        .symbol_flags(symbol)
                        .intersects(SymbolFlags::TYPE)
                {
                    return Ok(false);
                }
                let parameters = self
                    .checker
                    .get_local_type_parameters_of_class_or_interface_or_type_alias(symbol);
                return Ok(self.checker.nodes_of(data.type_arguments).len()
                    >= self.checker.get_min_type_argument_count(Some(&parameters)));
            }
        }
        if self.checker.kind_of(existing) == SyntaxKind::TypeReference {
            if self.checker.is_const_type_reference_node(existing) {
                return Ok(false);
            }
            self.checker
                .get_type_from_type_node(existing)
                .map_err(|abort| checker_abort_error(self.checker, context, abort))?;
            let Some(symbol) = self.checker.links.node(existing).resolved_symbol.resolved() else {
                return Ok(false);
            };
            if self
                .checker
                .symbol_flags(symbol)
                .intersects(SymbolFlags::TYPE_PARAMETER)
            {
                let declared = self.checker.get_declared_type_of_type_parameter(symbol);
                if let Some(mapper) = context.mapper {
                    return self
                        .checker
                        .get_mapped_type(declared, mapper)
                        .map(|mapped| mapped == declared)
                        .map_err(|abort| checker_abort_error(self.checker, context, abort));
                }
                return Ok(true);
            }
            if self.checker.is_jsdoc_type_reference(existing) {
                return Ok(existing_type_node_is_not_reference_or_is_reference_with_compatible_type_argument_count(
                    self.checker,
                    existing,
                    r#type,
                    context,
                )? && self
                    .checker
                    .get_intended_type_from_jsdoc_type_reference(existing)
                    .map_err(|abort| checker_abort_error(self.checker, context, abort))?
                    .is_none()
                    && self
                        .checker
                        .symbol_flags(symbol)
                        .intersects(SymbolFlags::TYPE));
            }
        }
        if let NodeData::TypeOperator(data) = self.checker.data_of(existing) {
            if data.operator == SyntaxKind::UniqueKeyword
                && data
                    .r#type
                    .is_some_and(|inner| self.checker.kind_of(inner) == SyntaxKind::SymbolKeyword)
            {
                let Some(enclosing) = context.enclosing_declaration else {
                    return Ok(false);
                };
                let enclosing = get_enclosing_declaration_ignoring_fake_scope(enclosing);
                return Ok(self.checker.is_node_descendant_of(existing, enclosing));
            }
        }
        Ok(true)
    }
}

/// tsc-port: syntacticBuilderResolver production object @6.0.3
/// tsc-hash: 4435e40ac4ba06bf9e97dd48b84835ddcec09e878d5b6163f041aa5ea0398894
/// tsc-span: _tsc.js:50778-50956
impl SyntacticBuilderResolver for ProductionSyntacticBuilderResolver<'_, '_> {
    fn evaluate_entity_name_expression(
        &mut self,
        expression: TransformNode,
    ) -> Result<crate::evaluate::EvaluatorResult, EmitResolverError> {
        let expression = self.parse_node(expression)?;
        self.checker.evaluate(expression, None).map_err(|abort| {
            callback_abort_error(self.checker, self.method, Some(expression), abort)
        })
    }

    fn is_expando_function_declaration(
        &mut self,
        node: TransformNode,
    ) -> Result<bool, EmitResolverError> {
        let node = self.parse_node(node)?;
        self.checker
            .emit_is_expando_function_declaration(node)
            .map_err(|abort| callback_abort_error(self.checker, self.method, Some(node), abort))
    }

    fn has_late_bindable_name(&mut self, node: TransformNode) -> Result<bool, EmitResolverError> {
        let node = self.parse_node(node)?;
        self.checker
            .has_late_bindable_name(node)
            .map_err(|abort| callback_abort_error(self.checker, self.method, Some(node), abort))
    }

    fn should_remove_declaration(
        &mut self,
        context: &mut NodeBuilderContext<'_>,
        node: TransformNode,
    ) -> Result<bool, EmitResolverError> {
        if context.internal_flags.0 & ALLOW_UNRESOLVED_NAMES == 0 {
            return Ok(true);
        }
        let node = self.parse_node(node)?;
        let Some(name) =
            node_util::get_name_of_declaration(self.checker.binder.source_of_node(node), node)
        else {
            return Ok(true);
        };
        let NodeData::ComputedPropertyName(data) = self.checker.data_of(name) else {
            return Ok(true);
        };
        let Some(expression) = data.expression else {
            return Ok(true);
        };
        if !self.checker.is_entity_name_expression(expression) {
            return Ok(true);
        }
        self.checker
            .check_computed_property_name(name)
            .map(|r#type| {
                !self
                    .checker
                    .tables
                    .flags_of(r#type)
                    .intersects(TypeFlags::ANY)
            })
            .map_err(|abort| callback_abort_error(self.checker, self.method, Some(node), abort))
    }

    fn create_recovery_boundary(
        &mut self,
        context: &mut NodeBuilderContext<'_>,
    ) -> Result<SyntacticRecoveryBoundary, EmitResolverError> {
        Ok(SyntacticRecoveryBoundary::new(context))
    }

    fn is_definitely_reference_to_global_symbol_object(
        &mut self,
        node: TransformNode,
    ) -> Result<bool, EmitResolverError> {
        let node = self.parse_node(node)?;
        self.checker
            .emit_is_definitely_reference_to_global_symbol_object(node)
            .map_err(|abort| callback_abort_error(self.checker, self.method, Some(node), abort))
    }

    /// tsc-port: getAllAccessorDeclarationsForDeclaration @6.0.3
    /// tsc-hash: 22c3c3fbba9f8d171de8ad28d2e576f91993378168a1be1ebc9c4dc06c85926d
    /// tsc-span: _tsc.js:88367-88381
    fn get_all_accessor_declarations(
        &mut self,
        accessor: TransformNode,
    ) -> Result<SyntacticAccessorDeclarations, EmitResolverError> {
        let accessor_node = self.parse_node(accessor)?;
        let symbol = self
            .checker
            .get_symbol_of_declaration(accessor_node)
            .map_err(|abort| {
                callback_abort_error(self.checker, self.method, Some(accessor_node), abort)
            })?;
        let other_kind = if self.checker.kind_of(accessor_node) == SyntaxKind::SetAccessor {
            SyntaxKind::GetAccessor
        } else {
            SyntaxKind::SetAccessor
        };
        let other_node = self
            .checker
            .binder
            .symbol(symbol)
            .declarations
            .iter()
            .copied()
            .find(|&declaration| self.checker.kind_of(declaration) == other_kind);
        let other = other_node
            .map(|other| self.project_parse_node(other))
            .transpose()?
            .flatten();
        let other_precedes = other_node.is_some_and(|other| {
            self.checker
                .binder
                .source_of_node(other)
                .arena
                .node(other)
                .pos
                < self
                    .checker
                    .binder
                    .source_of_node(accessor_node)
                    .arena
                    .node(accessor_node)
                    .pos
        });
        let (first_accessor, second_accessor) = if other_precedes {
            (other.unwrap_or(accessor), Some(accessor))
        } else {
            (accessor, other)
        };
        let (get_accessor, set_accessor) =
            if self.checker.kind_of(accessor_node) == SyntaxKind::GetAccessor {
                (Some(accessor), other)
            } else {
                (other, Some(accessor))
            };
        Ok(SyntacticAccessorDeclarations {
            first_accessor,
            second_accessor,
            get_accessor,
            set_accessor,
        })
    }

    fn requires_adding_implicit_undefined(
        &mut self,
        declaration: TransformNode,
        symbol: Option<SyntacticSymbol>,
        enclosing_declaration: Option<NodeId>,
    ) -> Result<bool, EmitResolverError> {
        let declaration = self.parse_node(declaration)?;
        match self.checker.kind_of(declaration) {
            SyntaxKind::PropertyDeclaration
            | SyntaxKind::PropertySignature
            | SyntaxKind::JSDocPropertyTag => {
                let context_node = enclosing_declaration.unwrap_or(declaration);
                let symbol = match symbol {
                    Some(symbol) => symbol.id,
                    None => self
                        .checker
                        .get_symbol_of_declaration(declaration)
                        .map_err(|abort| {
                            callback_abort_error(
                                self.checker,
                                self.method,
                                Some(context_node),
                                abort,
                            )
                        })?,
                };
                let r#type = self.checker.get_type_of_symbol(symbol).map_err(|abort| {
                    callback_abort_error(self.checker, self.method, Some(context_node), abort)
                })?;
                let flags = self.checker.symbol_flags(symbol);
                Ok(flags.intersects(SymbolFlags::PROPERTY)
                    && flags.intersects(SymbolFlags::OPTIONAL)
                    && self.checker.is_optional_declaration(declaration)
                    && self.checker.links.symbol(symbol).mapped_type.is_some()
                    && contains_non_missing_undefined(self.checker, r#type))
            }
            SyntaxKind::Parameter | SyntaxKind::JSDocParameterTag => self
                .checker
                .emit_requires_adding_implicit_undefined(declaration, enclosing_declaration)
                .map_err(|abort| {
                    callback_abort_error(self.checker, self.method, Some(declaration), abort)
                }),
            _ => Ok(false),
        }
    }

    fn is_optional_parameter(
        &mut self,
        parameter: TransformNode,
    ) -> Result<bool, EmitResolverError> {
        let parameter = self.parse_node(parameter)?;
        self.checker
            .emit_is_optional_parameter(parameter)
            .map_err(|abort| {
                callback_abort_error(self.checker, self.method, Some(parameter), abort)
            })
    }

    fn is_undefined_identifier_expression(
        &mut self,
        node: TransformNode,
    ) -> Result<bool, EmitResolverError> {
        let node = self.parse_node(node)?;
        self.checker
            .get_resolved_symbol(node)
            .map(|symbol| symbol == Some(self.checker.undefined_symbol))
            .map_err(|abort| callback_abort_error(self.checker, self.method, Some(node), abort))
    }

    fn is_entity_name_visible(
        &mut self,
        context: &mut NodeBuilderContext<'_>,
        entity_name: TransformNode,
        should_compute_aliases_to_make_visible: bool,
    ) -> Result<EmitSymbolAccessibilityResult, EmitResolverError> {
        let entity_name = self.parse_node(entity_name)?;
        let Some(enclosing) = context.enclosing_declaration else {
            return Ok(EmitSymbolAccessibilityResult {
                accessibility: EmitSymbolAccessibility::Accessible,
                aliases_to_make_visible: None,
                error_symbol_name: None,
                error_module_name: None,
                error_node: None,
            });
        };
        self.checker
            .emit_is_entity_name_visible(
                entity_name,
                enclosing,
                should_compute_aliases_to_make_visible,
            )
            .map_err(|abort| {
                callback_abort_error(self.checker, self.method, Some(entity_name), abort)
            })
    }

    /// tsc-port: serializeExistingTypeNode @6.0.3
    /// tsc-hash: 433daa463f78335a63960c6658ccab7a037a667922af31e6eb4320cadafe30ff
    /// tsc-span: _tsc.js:53712-53721
    fn serialize_existing_type_node(
        &mut self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        type_node: TransformNode,
        add_undefined: bool,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let type_node_parse = arena
            .require_parse_tree_resolver_node(type_node)
            .map_err(factory_error)?
            .node();
        let Some(r#type) = get_type_from_type_node2(self.checker, context, type_node_parse, false)?
        else {
            return Ok(None);
        };
        if add_undefined
            && !some_type_is_undefined(self.checker, r#type)
            && self.can_reuse_type_node_parse(context, type_node_parse)?
        {
            let builder = SyntacticTypeNodeBuilder::new(self.checker.options);
            if let Some(clone) =
                builder.try_reuse_existing_type_node(self, arena, target, context, type_node)?
            {
                let undefined = create_token(arena, target, SyntaxKind::UndefinedKeyword)?;
                let types = create_node_array(arena, target, vec![clone, undefined])?;
                return create_node(
                    arena,
                    target,
                    NodeData::UnionType(UnionTypeData { types: Some(types) }),
                )
                .map(Some);
            }
        }
        type_to_type_node_helper(self.checker, arena, target, r#type, context)
    }

    fn serialize_return_type_for_signature(
        &mut self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        signature_declaration: TransformNode,
        symbol: Option<SyntacticSymbol>,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let declaration = arena
            .require_parse_tree_resolver_node(signature_declaration)
            .map_err(factory_error)?
            .node();
        let signature = self
            .checker
            .get_signature_from_declaration(declaration)
            .map_err(|abort| checker_abort_error(self.checker, context, abort))?;
        let symbol = self.parse_symbol(declaration, symbol, context)?;
        let return_type = match context.enclosing_symbol_types.get(&symbol).copied() {
            Some(r#type) => r#type,
            None => {
                let return_type = self
                    .checker
                    .get_return_type_of_signature(signature)
                    .map_err(|abort| checker_abort_error(self.checker, context, abort))?;
                self.checker
                    .instantiate_type(return_type, context.mapper)
                    .map_err(|abort| checker_abort_error(self.checker, context, abort))?
            }
        };
        serialize_inferred_return_type_for_signature(
            self.checker,
            arena,
            target,
            context,
            signature,
            return_type,
        )
    }

    fn serialize_type_of_expression(
        &mut self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        expression: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let mut expression = arena
            .require_parse_tree_resolver_node(expression)
            .map_err(factory_error)?
            .node();
        if let Some(parent) = self.checker.parent_of(expression) {
            let is_right_side = matches!(
                self.checker.data_of(parent),
                NodeData::QualifiedName(data) if data.right == Some(expression)
            ) || matches!(
                self.checker.data_of(parent),
                NodeData::PropertyAccessExpression(data) if data.name == Some(expression)
            );
            if is_right_side {
                expression = parent;
            }
        }
        let expression_type = self
            .checker
            .get_type_of_expression(expression)
            .map_err(|abort| checker_abort_error(self.checker, context, abort))?;
        let regular = self
            .checker
            .tables
            .get_regular_type_of_literal_type(expression_type);
        let widened = self
            .checker
            .get_widened_type(regular)
            .map_err(|abort| checker_abort_error(self.checker, context, abort))?;
        let instantiated = self
            .checker
            .instantiate_type(widened, context.mapper)
            .map_err(|abort| checker_abort_error(self.checker, context, abort))?;
        type_to_type_node_helper(self.checker, arena, target, instantiated, context)
    }

    fn serialize_type_of_declaration(
        &mut self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        declaration: TransformNode,
        symbol: Option<SyntacticSymbol>,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let declaration = arena
            .require_parse_tree_resolver_node(declaration)
            .map_err(factory_error)?
            .node();
        let symbol = self.parse_symbol(declaration, symbol, context)?;
        let mut r#type = match context.enclosing_symbol_types.get(&symbol).copied() {
            Some(r#type) => r#type,
            None => {
                let flags = self.checker.symbol_flags(symbol);
                if flags.intersects(SymbolFlags::GET_ACCESSOR | SymbolFlags::SET_ACCESSOR)
                    && self.checker.kind_of(declaration) == SyntaxKind::SetAccessor
                {
                    let write = self
                        .checker
                        .get_write_type_of_symbol(symbol)
                        .map_err(|abort| checker_abort_error(self.checker, context, abort))?;
                    self.checker
                        .instantiate_type(write, context.mapper)
                        .map_err(|abort| checker_abort_error(self.checker, context, abort))?
                } else if !flags.intersects(SymbolFlags::TYPE_LITERAL | SymbolFlags::SIGNATURE) {
                    let symbol_type = self
                        .checker
                        .get_type_of_symbol(symbol)
                        .map_err(|abort| checker_abort_error(self.checker, context, abort))?;
                    let widened = self
                        .checker
                        .get_widened_literal_type(symbol_type)
                        .map_err(|abort| checker_abort_error(self.checker, context, abort))?;
                    self.checker
                        .instantiate_type(widened, context.mapper)
                        .map_err(|abort| checker_abort_error(self.checker, context, abort))?
                } else {
                    self.checker.tables.intrinsics.error
                }
            }
        };
        if matches!(
            self.checker.kind_of(declaration),
            SyntaxKind::Parameter | SyntaxKind::JSDocParameterTag
        ) && self
            .checker
            .emit_requires_adding_implicit_undefined(declaration, context.enclosing_declaration)
            .map_err(|abort| checker_abort_error(self.checker, context, abort))?
        {
            r#type = self
                .checker
                .get_optional_type(r#type, false)
                .map_err(|abort| checker_abort_error(self.checker, context, abort))?;
        }
        serialize_inferred_type_for_declaration(
            self.checker,
            arena,
            target,
            symbol,
            context,
            r#type,
        )
    }

    fn serialize_name_of_parameter(
        &mut self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        parameter: TransformNode,
    ) -> Result<TransformNode, EmitResolverError> {
        let parameter = arena
            .require_parse_tree_resolver_node(parameter)
            .map_err(factory_error)?
            .node();
        serialize_parameter_name_from_parse(self.checker, arena, target, context, parameter)
    }

    fn serialize_entity_name(
        &mut self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let node = arena
            .require_parse_tree_resolver_node(node)
            .map_err(factory_error)?
            .node();
        let symbol = self
            .checker
            .get_resolved_symbol(node)
            .map_err(|abort| checker_abort_error(self.checker, context, abort))?;
        let Some(symbol) = symbol else {
            return Ok(None);
        };
        if let Some(enclosing) = context.enclosing_declaration {
            let accessibility = self
                .checker
                .emit_is_symbol_accessible(
                    symbol,
                    enclosing,
                    EmitSymbolMeaning::VALUE_EXPORT_VALUE,
                    false,
                )
                .map_err(|abort| checker_abort_error(self.checker, context, abort))?;
            if accessibility.accessibility != EmitSymbolAccessibility::Accessible {
                return Ok(None);
            }
        }
        chains_symbol_to_expression(
            self.checker,
            arena,
            target,
            context,
            symbol,
            EmitSymbolMeaning::VALUE_EXPORT_VALUE,
        )
        .map(Some)
    }

    /// tsc-port: serializeTypeName @6.0.3
    /// tsc-hash: df4a76962d3a7605e7ad28b17db185ce5908de4271994b98a0e436257ce89990
    /// tsc-span: _tsc.js:53656-53674
    fn serialize_type_name(
        &mut self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        node: TransformNode,
        is_type_of: bool,
        type_arguments: Option<TransformNodeArray>,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let node_parse = arena
            .require_parse_tree_resolver_node(node)
            .map_err(factory_error)?
            .node();
        let meaning = if is_type_of {
            EmitSymbolMeaning::VALUE_EXPORT_VALUE
        } else {
            EmitSymbolMeaning::TYPE
        };
        let symbol = self
            .checker
            .resolve_entity_name_ex(node_parse, entity_meaning_flags(meaning), true, None, false)
            .map_err(|abort| checker_abort_error(self.checker, context, abort))?;
        let Some(symbol) = symbol else {
            return Ok(None);
        };
        if let Some(enclosing) = context.enclosing_declaration {
            let accessible = self
                .checker
                .emit_is_symbol_accessible(symbol, enclosing, meaning, false)
                .map_err(|abort| checker_abort_error(self.checker, context, abort))?;
            if accessible.accessibility != EmitSymbolAccessibility::Accessible {
                return Ok(None);
            }
        }
        let resolved = if self
            .checker
            .symbol_flags(symbol)
            .intersects(SymbolFlags::ALIAS)
        {
            self.checker
                .resolve_alias(symbol)
                .map_err(|abort| checker_abort_error(self.checker, context, abort))?
        } else {
            symbol
        };
        let type_arguments = type_arguments
            .map(|arguments| {
                let source = arguments.source();
                arena
                    .node_array(arguments)
                    .map(|arguments| {
                        arguments
                            .nodes
                            .iter()
                            .filter_map(|&node| arena.node_ref(source, node))
                            .collect()
                    })
                    .map_err(factory_error)
            })
            .transpose()?;
        chains_symbol_to_type_node(
            self.checker,
            arena,
            target,
            context,
            resolved,
            meaning,
            type_arguments,
        )
        .map(Some)
    }

    fn get_js_doc_property_override(
        &mut self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        js_doc_type_literal: TransformNode,
        js_doc_property: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let literal = arena
            .require_parse_tree_resolver_node(js_doc_type_literal)
            .map_err(factory_error)?
            .node();
        let property = arena
            .require_parse_tree_resolver_node(js_doc_property)
            .map_err(factory_error)?
            .node();
        let NodeData::JSDocPropertyTag(property_data) = self.checker.data_of(property) else {
            return Ok(None);
        };
        let Some(property_name) = property_data.name else {
            return Ok(None);
        };
        let name = match self.checker.data_of(property_name) {
            NodeData::Identifier(data) => data.escaped_text.clone(),
            NodeData::QualifiedName(data) => data
                .right
                .and_then(|right| self.checker.identifier_text_of(right).map(str::to_owned))
                .unwrap_or_default(),
            _ => return Ok(None),
        };
        let Some(parent_type) = get_type_from_type_node2(self.checker, context, literal, false)?
        else {
            return Ok(None);
        };
        let type_via_parent = self
            .checker
            .get_type_of_property_of_type(parent_type, &name)
            .map_err(|abort| checker_abort_error(self.checker, context, abort))?;
        let Some(type_via_parent) = type_via_parent else {
            return Ok(None);
        };
        let existing_type = property_data.type_expression.and_then(|expression| {
            match self.checker.data_of(expression) {
                NodeData::JSDocTypeExpression(data) => data.r#type,
                _ => None,
            }
        });
        let Some(existing_type) = existing_type else {
            return Ok(None);
        };
        if get_type_from_type_node2(self.checker, context, existing_type, false)?
            == Some(type_via_parent)
        {
            return Ok(None);
        }
        type_to_type_node_helper(self.checker, arena, target, type_via_parent, context)
    }

    fn enter_new_scope(
        &mut self,
        context: &mut NodeBuilderContext<'_>,
        node: TransformNode,
    ) -> Result<SyntacticScopeCleanup, EmitResolverError> {
        let node = self.parse_node(node)?;
        let cleanup = SyntacticScopeCleanup::capture(context);
        if node_util::is_function_like_declaration_kind(self.checker.kind_of(node))
            || self.checker.kind_of(node) == SyntaxKind::JSDocSignature
        {
            let signature = self
                .checker
                .get_signature_from_declaration(node)
                .map_err(|abort| checker_abort_error(self.checker, context, abort))?;
            let signature = self.checker.signature_of(signature).clone();
            let _restore = super::enter_new_scope(
                context,
                Some(node),
                Some(&signature.parameters),
                signature.type_parameters.as_deref(),
                None,
                None,
            );
        } else {
            let type_parameters = if self.checker.kind_of(node) == SyntaxKind::ConditionalType {
                self.checker.get_infer_type_parameters(node)
            } else {
                match self.checker.data_of(node) {
                    NodeData::InferType(data) => data
                        .type_parameter
                        .and_then(|type_parameter| self.checker.node_symbol(type_parameter))
                        .map(|symbol| {
                            vec![self.checker.get_declared_type_of_type_parameter(symbol)]
                        })
                        .unwrap_or_default(),
                    _ => Vec::new(),
                }
            };
            let _restore = super::enter_new_scope(
                context,
                Some(node),
                None,
                Some(&type_parameters),
                None,
                None,
            );
        }
        Ok(cleanup)
    }

    fn mark_node_reuse(
        &mut self,
        arena: &mut TransformArena,
        context: &mut NodeBuilderContext<'_>,
        range: TransformNode,
        location: TransformNode,
    ) -> Result<TransformNode, EmitResolverError> {
        set_text_range2(self.checker, arena, context, range, Some(location))
    }

    /// tsc-port: trackExistingEntityName @6.0.3
    /// tsc-hash: 209b12123fd836edaefcaef413f04659f4e3b998dac70ab139b159a0125e85ed
    /// tsc-span: _tsc.js:53555-53655
    fn track_existing_entity_name(
        &mut self,
        arena: &mut TransformArena,
        _target: TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        node: TransformNode,
    ) -> Result<SyntacticTrackedEntityName, EmitResolverError> {
        let parse = arena
            .require_parse_tree_resolver_node(node)
            .map_err(factory_error)?
            .node();
        let leftmost = self.checker.first_identifier(parse);
        if self.checker.is_in_js_file(parse) && is_js_exports_entity_name(self.checker, leftmost) {
            return Ok(SyntacticTrackedEntityName {
                node,
                introduces_error: true,
            });
        }
        let meaning = self.checker.get_meaning_of_entity_name_reference(parse);
        if self.checker.is_this_identifier(leftmost) {
            let container = node_util::get_this_container(
                self.checker.binder.source_of_node(leftmost),
                leftmost,
                false,
            );
            let symbol = match container {
                Some(container) => Some(
                    self.checker
                        .get_symbol_of_declaration(container)
                        .map_err(|abort| checker_abort_error(self.checker, context, abort))?,
                ),
                None => None,
            };
            let inaccessible = match symbol {
                Some(symbol) => {
                    self.checker
                        .emit_is_symbol_accessible(symbol, leftmost, meaning, false)
                        .map_err(|abort| checker_abort_error(self.checker, context, abort))?
                        .accessibility
                        != EmitSymbolAccessibility::Accessible
                }
                None => false,
            };
            if inaccessible {
                context
                    .tracker
                    .report_inaccessible_this_error(&mut context.reported_diagnostic);
            }
            let node = self.attach_symbol_to_entity_name(arena, context, node, leftmost, symbol)?;
            return Ok(SyntacticTrackedEntityName {
                node,
                introduces_error: inaccessible,
            });
        }

        let flags = entity_meaning_flags(meaning);
        let mut symbol = self
            .checker
            .resolve_entity_name_ex(leftmost, flags, true, None, true)
            .map_err(|abort| checker_abort_error(self.checker, context, abort))?;
        if context.enclosing_declaration.is_some()
            && !symbol.is_some_and(|symbol| {
                self.checker
                    .symbol_flags(symbol)
                    .intersects(SymbolFlags::TYPE_PARAMETER)
            })
        {
            symbol = symbol.map(|symbol| {
                self.checker
                    .get_export_symbol_of_value_symbol_if_exported(symbol)
            });
            let at_location = self
                .checker
                .resolve_entity_name_ex(leftmost, flags, true, context.enclosing_declaration, true)
                .map_err(|abort| checker_abort_error(self.checker, context, abort))?;
            let mismatched = at_location == Some(self.checker.unknown_symbol)
                || at_location.is_none() && symbol.is_some()
                || match (at_location, symbol) {
                    (Some(at_location), Some(original)) => {
                        let at_location = self
                            .checker
                            .get_export_symbol_of_value_symbol_if_exported(at_location);
                        self.checker
                            .get_symbol_if_same_reference(at_location, original)
                            .map_err(|abort| checker_abort_error(self.checker, context, abort))?
                            .is_none()
                    }
                    _ => false,
                };
            if mismatched {
                if at_location != Some(self.checker.unknown_symbol) {
                    self.report_inference_fallback(context, parse)?;
                }
                return Ok(SyntacticTrackedEntityName {
                    node,
                    introduces_error: true,
                });
            }
            symbol = at_location;
        }

        let mut introduces_error = false;
        if let Some(symbol) = symbol {
            let data = self.checker.binder.symbol(symbol);
            let parameter_symbol = data.flags.intersects(SymbolFlags::FUNCTION_SCOPED_VARIABLE)
                && data.value_declaration.is_some_and(|declaration| {
                    node_util::is_part_of_parameter_declaration(
                        self.checker.binder.source_of_node(declaration),
                        declaration,
                    ) || self.checker.kind_of(declaration) == SyntaxKind::JSDocParameterTag
                });
            if !parameter_symbol {
                let inaccessible = !self
                    .checker
                    .symbol_flags(symbol)
                    .intersects(SymbolFlags::TYPE_PARAMETER)
                    && !is_declaration_name(self.checker, parse)
                    && match context.enclosing_declaration {
                        Some(enclosing) => {
                            self.checker
                                .emit_is_symbol_accessible(symbol, enclosing, meaning, false)
                                .map_err(|abort| checker_abort_error(self.checker, context, abort))?
                                .accessibility
                                != EmitSymbolAccessibility::Accessible
                        }
                        None => false,
                    };
                if inaccessible {
                    self.report_inference_fallback(context, parse)?;
                    introduces_error = true;
                } else {
                    self.track_symbol(context, symbol, meaning)?;
                }
            }
            let node =
                self.attach_symbol_to_entity_name(arena, context, node, leftmost, Some(symbol))?;
            return Ok(SyntacticTrackedEntityName {
                node,
                introduces_error,
            });
        }
        Ok(SyntacticTrackedEntityName {
            node,
            introduces_error,
        })
    }

    fn track_computed_name(
        &mut self,
        context: &mut NodeBuilderContext<'_>,
        access_expression: TransformNode,
    ) -> Result<(), EmitResolverError> {
        let access_expression = self.parse_node(access_expression)?;
        track_computed_name(self.checker, access_expression, context)
    }

    fn get_module_specifier_override(
        &mut self,
        context: &mut NodeBuilderContext<'_>,
        parent: TransformNode,
        literal: TransformNode,
    ) -> Result<Option<String>, EmitResolverError> {
        get_module_specifier_override(self.checker, &self.arena_snapshot, context, parent, literal)
    }

    fn can_reuse_type_node(
        &mut self,
        context: &mut NodeBuilderContext<'_>,
        type_node: TransformNode,
    ) -> Result<bool, EmitResolverError> {
        let type_node = self.parse_node(type_node)?;
        self.can_reuse_type_node_parse(context, type_node)
    }

    /// tsc-port: syntacticBuilderResolver.canReuseTypeNodeAnnotation @6.0.3
    /// tsc-hash: edfd54626c63d3d1645a16cfcad8561dab1388e09a7278579ada789709becc6d
    /// tsc-span: _tsc.js:50932-50955
    fn can_reuse_type_node_annotation(
        &mut self,
        context: &mut NodeBuilderContext<'_>,
        node: TransformNode,
        existing: TransformNode,
        symbol: Option<SyntacticSymbol>,
        requires_adding_undefined: Option<bool>,
    ) -> Result<bool, EmitResolverError> {
        // Reuse the annotation-reuse decision core directly. This callback must
        // never route through the text-slice renderer used by declaration emit.
        if context.enclosing_declaration.is_none() {
            return Ok(false);
        }
        let node = self.parse_node(node)?;
        let existing = self.parse_node(existing)?;
        let symbol = self.parse_symbol(node, symbol, context)?;
        let r#type = match context.enclosing_symbol_types.get(&symbol).copied() {
            Some(r#type) => r#type,
            None => {
                let flags = self.checker.symbol_flags(symbol);
                if flags.intersects(SymbolFlags::GET_ACCESSOR | SymbolFlags::SET_ACCESSOR) {
                    if self.checker.kind_of(node) == SyntaxKind::SetAccessor {
                        self.checker
                            .get_write_type_of_symbol(symbol)
                            .map_err(|abort| checker_abort_error(self.checker, context, abort))?
                    } else {
                        self.checker
                            .get_type_of_accessors(symbol)
                            .map_err(|abort| checker_abort_error(self.checker, context, abort))?
                    }
                } else if is_value_signature_declaration(self.checker, node) {
                    let signature = self
                        .checker
                        .get_signature_from_declaration(node)
                        .map_err(|abort| checker_abort_error(self.checker, context, abort))?;
                    self.checker
                        .get_return_type_of_signature(signature)
                        .map_err(|abort| checker_abort_error(self.checker, context, abort))?
                } else {
                    self.checker
                        .get_type_of_symbol(symbol)
                        .map_err(|abort| checker_abort_error(self.checker, context, abort))?
                }
            }
        };
        let mut annotation_type = self
            .checker
            .get_type_from_type_node(existing)
            .map_err(|abort| checker_abort_error(self.checker, context, abort))?;
        if self.checker.tables.is_error_type(annotation_type) {
            return Ok(true);
        }
        if requires_adding_undefined == Some(true) {
            annotation_type = self
                .checker
                .get_optional_type(
                    annotation_type,
                    self.checker.kind_of(node) != SyntaxKind::Parameter,
                )
                .map_err(|abort| checker_abort_error(self.checker, context, abort))?;
        }
        Ok(type_node_is_equivalent_to_type(
            self.checker,
            Some(node),
            r#type,
            annotation_type,
        )
        .map_err(|abort| checker_abort_error(self.checker, context, abort))?
            && existing_type_node_is_not_reference_or_is_reference_with_compatible_type_argument_count(
                self.checker,
                existing,
                r#type,
                context,
            )?)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use tsc_emitter::{EmitFlags, EmitTrackerNode};
    use tsc_types::CompilerOptions;

    use crate::narrow::TypePredicateKind;
    use crate::state::test_support::with_program_state;

    use super::*;

    #[derive(Clone, Default)]
    struct RecordingTracker {
        events: Rc<RefCell<Vec<&'static str>>>,
    }

    impl EmitSymbolTracker for RecordingTracker {
        fn can_track_symbol(&self) -> bool {
            true
        }

        fn track_symbol(
            &mut self,
            _access: &mut dyn EmitTrackerAccess,
            _symbol: EmitTrackerSymbol,
            _enclosing_declaration: Option<EmitTrackerNode>,
            _meaning: EmitSymbolMeaning,
        ) -> Result<bool, EmitResolverError> {
            self.events.borrow_mut().push("track");
            Ok(false)
        }

        fn report_inference_fallback(
            &mut self,
            _access: &mut dyn EmitTrackerAccess,
            _node: EmitTrackerNode,
        ) -> Result<(), EmitResolverError> {
            self.events.borrow_mut().push("fallback");
            Ok(())
        }
    }

    fn root_statements(checker: &CheckerState<'_>) -> Vec<NodeId> {
        let root = checker.binder.source(0).root;
        match checker.data_of(root) {
            NodeData::SourceFile(data) => checker.nodes_of(data.statements),
            _ => Vec::new(),
        }
    }

    fn variable_declaration(checker: &CheckerState<'_>, name: &str) -> NodeId {
        for statement in root_statements(checker) {
            let NodeData::VariableStatement(statement) = checker.data_of(statement) else {
                continue;
            };
            let Some(list) = statement.declaration_list else {
                continue;
            };
            let NodeData::VariableDeclarationList(list) = checker.data_of(list) else {
                continue;
            };
            for declaration in checker.nodes_of(list.declarations) {
                let NodeData::VariableDeclaration(data) = checker.data_of(declaration) else {
                    continue;
                };
                if data.name.and_then(|name| checker.identifier_text_of(name)) == Some(name) {
                    return declaration;
                }
            }
        }
        panic!("variable declaration {name}")
    }

    fn function_declaration(checker: &CheckerState<'_>, name: &str) -> NodeId {
        root_statements(checker)
            .into_iter()
            .find(|&statement| {
                matches!(
                    checker.data_of(statement),
                    NodeData::FunctionDeclaration(data)
                        if data
                            .name
                            .and_then(|name_node| checker.identifier_text_of(name_node))
                            == Some(name)
                )
            })
            .unwrap_or_else(|| panic!("function declaration {name}"))
    }

    fn accessor_declaration(checker: &CheckerState<'_>, kind: SyntaxKind) -> NodeId {
        for statement in root_statements(checker) {
            let NodeData::ClassDeclaration(data) = checker.data_of(statement) else {
                continue;
            };
            if let Some(member) = checker
                .nodes_of(data.members)
                .into_iter()
                .find(|&member| checker.kind_of(member) == kind)
            {
                return member;
            }
        }
        panic!("accessor {kind:?}")
    }

    fn mounted_arena(checker: &CheckerState<'_>) -> (TransformArena, TransformSourceId) {
        let mut arena = TransformArena::new();
        let target = arena.add_source(
            checker.binder.source(0),
            Some(program_source_id(checker, 0)),
        );
        (arena, target)
    }

    fn kind(arena: &TransformArena, node: TransformNode) -> SyntaxKind {
        arena.node(node).expect("transform node").kind
    }

    #[test]
    fn declaration_arms_reuse_annotations_consult_accessors_and_fallback_to_semantics() {
        let source = r#"
            declare function make(): { value: string };
            const annotated: { value: string } = make();
            const inferred = make();
            class C {
                get value(): string { return ""; }
                set value(value: string) {}
            }
        "#;
        with_program_state(
            &[("/main.ts", source)],
            &CompilerOptions::default(),
            |checker| {
                let root = checker.binder.source(0).root;
                let annotated = variable_declaration(checker, "annotated");
                let inferred = variable_declaration(checker, "inferred");
                let getter = accessor_declaration(checker, SyntaxKind::GetAccessor);
                let annotated_symbol = checker
                    .get_symbol_of_declaration(annotated)
                    .expect("annotated symbol");
                let inferred_symbol = checker
                    .get_symbol_of_declaration(inferred)
                    .expect("inferred symbol");
                let getter_symbol = checker
                    .get_symbol_of_declaration(getter)
                    .expect("getter symbol");
                let mut tracker = RecordingTracker::default();
                let events = Rc::clone(&tracker.events);
                let (mut arena, target) = mounted_arena(checker);
                let built = with_context(
                    checker,
                    &mut arena,
                    target,
                    Some(root),
                    Some(EmitNodeBuilderFlags::NONE),
                    None,
                    Some(&mut tracker),
                    None,
                    None,
                    |checker, arena, target, context| {
                        let annotated_type = checker
                            .get_type_of_symbol(annotated_symbol)
                            .map_err(|abort| checker_abort_error(checker, context, abort))?;
                        let inferred_type = checker
                            .get_type_of_symbol(inferred_symbol)
                            .map_err(|abort| checker_abort_error(checker, context, abort))?;
                        let getter_type = checker
                            .get_type_of_symbol(getter_symbol)
                            .map_err(|abort| checker_abort_error(checker, context, abort))?;
                        let annotated_node = serialize_type_for_declaration_in_context(
                            checker,
                            arena,
                            target,
                            context,
                            Some(annotated),
                            annotated_type,
                            Some(annotated_symbol),
                        )?
                        .expect("annotated type node");
                        let accessor_node = serialize_type_for_declaration_in_context(
                            checker,
                            arena,
                            target,
                            context,
                            Some(getter),
                            getter_type,
                            Some(getter_symbol),
                        )?
                        .expect("accessor type node");
                        events.borrow_mut().clear();
                        let inferred_node = serialize_type_for_declaration_in_context(
                            checker,
                            arena,
                            target,
                            context,
                            Some(inferred),
                            inferred_type,
                            Some(inferred_symbol),
                        )?
                        .expect("inferred type node");
                        Ok((annotated_node, inferred_node, accessor_node))
                    },
                    None,
                )
                .expect("serialization succeeds")
                .expect("context succeeds");

                assert_eq!(kind(&arena, built.0), SyntaxKind::TypeLiteral);
                assert_eq!(kind(&arena, built.1), SyntaxKind::TypeLiteral);
                assert_eq!(kind(&arena, built.2), SyntaxKind::StringKeyword);
                assert!(arena
                    .metadata(built.0)
                    .and_then(tsc_emitter::EmitMetadata::original)
                    .is_some());
                assert_eq!(events.borrow().first().copied(), Some("fallback"));
            },
        );
    }

    #[test]
    fn inferred_declaration_gate_declines_synthesized_and_widening_nodes() {
        assert!(should_use_syntactic_inferred_declaration(
            true, false, false
        ));
        assert!(!should_use_syntactic_inferred_declaration(
            true, true, false
        ));
        assert!(!should_use_syntactic_inferred_declaration(
            true, false, true
        ));
        assert!(!should_use_syntactic_inferred_declaration(
            false, false, false
        ));
    }

    #[test]
    fn initialized_parameter_before_required_parameter_adds_undefined_union() {
        let options = CompilerOptions {
            strict_null_checks: Some(true),
            ..CompilerOptions::default()
        };
        with_program_state(
            &[("/main.ts", "function f(value = 1, required: string) {}")],
            &options,
            |checker| {
                let root = checker.binder.source(0).root;
                let function = function_declaration(checker, "f");
                let parameter = checker.parameters_of_function(function)[0];
                assert!(checker
                    .emit_requires_adding_implicit_undefined(parameter, Some(root))
                    .expect("implicit undefined query"));
                let symbol = checker
                    .get_symbol_of_declaration(parameter)
                    .expect("parameter symbol");
                let parameter_type = checker.get_type_of_symbol(symbol).expect("parameter type");
                let (mut arena, target) = mounted_arena(checker);
                let built = with_context(
                    checker,
                    &mut arena,
                    target,
                    Some(root),
                    None,
                    None,
                    None,
                    None,
                    None,
                    |checker, arena, target, context| {
                        serialize_type_for_declaration_in_context(
                            checker,
                            arena,
                            target,
                            context,
                            Some(parameter),
                            parameter_type,
                            Some(symbol),
                        )
                    },
                    None,
                )
                .expect("serialization succeeds")
                .flatten()
                .expect("type node");
                let NodeData::UnionType(data) = &arena.node(built).expect("union").data else {
                    panic!("undefined composition must be a union")
                };
                let types = arena
                    .source(built.source())
                    .expect("source")
                    .syntax()
                    .arena
                    .node_array(data.types.expect("union types"));
                assert!(types.nodes.iter().any(|&node| {
                    arena
                        .node_ref(built.source(), node)
                        .is_some_and(|node| kind(&arena, node) == SyntaxKind::UndefinedKeyword)
                }));
            },
        );
    }

    #[test]
    fn suppress_any_return_type_skips_node_and_restores_the_flag() {
        with_program_state(
            &[("/main.ts", "declare function f(): any;")],
            &CompilerOptions::default(),
            |checker| {
                let root = checker.binder.source(0).root;
                let declaration = function_declaration(checker, "f");
                let signature = checker
                    .get_signature_from_declaration(declaration)
                    .expect("signature");
                let (mut arena, target) = mounted_arena(checker);
                let (node, restored) = with_context(
                    checker,
                    &mut arena,
                    target,
                    Some(root),
                    Some(EmitNodeBuilderFlags::SUPPRESS_ANY_RETURN_TYPE),
                    None,
                    None,
                    None,
                    None,
                    |checker, arena, target, context| {
                        let node = serialize_return_type_for_signature_in_context(
                            checker, arena, target, context, signature,
                        )?;
                        Ok((
                            node,
                            context
                                .flags
                                .contains(EmitNodeBuilderFlags::SUPPRESS_ANY_RETURN_TYPE),
                        ))
                    },
                    None,
                )
                .expect("serialization succeeds")
                .expect("context succeeds");
                assert!(node.is_none());
                assert!(restored);
            },
        );
    }

    #[test]
    fn front_doors_preserve_flags_and_build_real_node_shapes() {
        let source = "declare function f(): string; const annotated: { value: string } = { value: '' }; const n = 1;";
        with_program_state(
            &[("/main.ts", source)],
            &CompilerOptions::default(),
            |checker| {
                let root = checker.binder.source(0).root;
                let annotated = variable_declaration(checker, "annotated");
                let symbol = checker
                    .get_symbol_of_declaration(annotated)
                    .expect("symbol");
                let numeric = variable_declaration(checker, "n");
                let numeric_symbol = checker
                    .get_symbol_of_declaration(numeric)
                    .expect("numeric symbol");
                let function = function_declaration(checker, "f");
                let initializer = match checker.data_of(numeric) {
                    NodeData::VariableDeclaration(data) => data.initializer.expect("initializer"),
                    _ => unreachable!(),
                };
                let (mut arena, target) = mounted_arena(checker);
                let annotation = serialize_type_for_declaration(
                    checker,
                    &mut arena,
                    target,
                    annotated,
                    symbol,
                    Some(root),
                    Some(EmitNodeBuilderFlags::NONE),
                    None,
                    None,
                )
                .expect("declaration front door")
                .expect("annotation node");
                assert_eq!(kind(&arena, annotation), SyntaxKind::TypeLiteral);
                assert!(arena
                    .metadata(annotation)
                    .is_some_and(|metadata| metadata.flags().contains(EmitFlags::SINGLE_LINE)));

                let inferred = serialize_type_for_declaration(
                    checker,
                    &mut arena,
                    target,
                    numeric,
                    numeric_symbol,
                    Some(root),
                    Some(EmitNodeBuilderFlags::NONE),
                    None,
                    None,
                )
                .expect("inferred declaration front door")
                .expect("inferred declaration node");
                assert!(matches!(
                    kind(&arena, inferred),
                    SyntaxKind::LiteralType | SyntaxKind::NumberKeyword
                ));

                let return_type = serialize_return_type_for_signature(
                    checker,
                    &mut arena,
                    target,
                    function,
                    Some(root),
                    Some(EmitNodeBuilderFlags::NONE),
                    None,
                    None,
                )
                .expect("return-type front door")
                .expect("return-type node");
                assert_eq!(kind(&arena, return_type), SyntaxKind::StringKeyword);

                let number = type_to_type_node(
                    checker,
                    &mut arena,
                    target,
                    checker.tables.intrinsics.number,
                    Some(root),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                )
                .expect("type front door")
                .expect("number node");
                assert_eq!(kind(&arena, number), SyntaxKind::NumberKeyword);

                let expression = serialize_type_for_expression(
                    checker,
                    &mut arena,
                    target,
                    initializer,
                    Some(root),
                    None,
                    None,
                    None,
                )
                .expect("expression front door")
                .expect("expression type");
                assert!(matches!(
                    kind(&arena, expression),
                    SyntaxKind::LiteralType | SyntaxKind::NumberKeyword
                ));

                let predicate = TypePredicate {
                    kind: TypePredicateKind::Identifier,
                    parameter_name: Some("value".to_owned()),
                    parameter_index: 0,
                    ty: Some(checker.tables.intrinsics.string),
                };
                let predicate = type_predicate_to_type_predicate_node(
                    checker,
                    &mut arena,
                    target,
                    &predicate,
                    Some(root),
                    None,
                    None,
                    None,
                )
                .expect("predicate front door")
                .expect("predicate node");
                assert_eq!(kind(&arena, predicate), SyntaxKind::TypePredicate);

                let index = IndexInfo {
                    key_type: checker.tables.intrinsics.string,
                    value_type: checker.tables.intrinsics.number,
                    is_readonly: false,
                    declaration: None,
                    components: None,
                    is_enum_number_index_info: false,
                };
                let index = index_info_to_index_signature_declaration(
                    checker,
                    &mut arena,
                    target,
                    &index,
                    Some(root),
                    None,
                    None,
                    None,
                )
                .expect("index front door")
                .expect("index node");
                assert_eq!(kind(&arena, index), SyntaxKind::IndexSignature);
            },
        );
    }
}

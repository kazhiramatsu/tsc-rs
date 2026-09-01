use std::collections::{HashMap, HashSet};

use tsc_binder::{node_util, SymbolId};
use tsc_emitter::{
    EmitFlags, EmitNodeBuilderFlags, EmitResolverError, EmitSymbolMeaning, SyntheticComment,
    SyntheticCommentKind, TransformArena, TransformNode, TransformSourceId,
};
use tsc_syntax::nodes::{
    ArrayBindingPatternData, ArrowFunctionData, BindingElementData, BlockData, CallSignatureData,
    ConstructSignatureData, ConstructorData, ConstructorTypeData, FunctionDeclarationData,
    FunctionExpressionData, FunctionTypeData, GetAccessorData, IndexSignatureData,
    JSDocFunctionTypeData, MethodDeclarationData, MethodSignatureData, ObjectBindingPatternData,
    ParameterData, SetAccessorData, TypeParameterData, TypePredicateData,
};
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{
    CheckFlags, ElementFlags, MapperId, ModifierFlags, SignatureFlags, SymbolFlags, TypeData,
    TypeId,
};

use crate::narrow::{TypePredicate, TypePredicateKind};
use crate::state::{CheckerState, IndexInfo, SignatureId};

use super::type_nodes::{
    add_approximate_length, checker_abort_error, clone_parse_node, create_identifier, create_node,
    create_node_array, create_token, factory_error, range_synthesized_node_to_parse,
    set_no_ascii_escaping, set_single_line, type_parameter_name, BuildResult,
};
use super::{
    can_possibly_expand_type, restore_flags, save_restore_flags,
    serialize_return_type_for_signature_seam, serialize_type_for_declaration_seam,
    syntactic_serialize_name_of_parameter_seam, syntactic_try_reuse_existing_type_node,
    type_to_type_node_helper, NodeBuilderContext, TrackedSymbol,
};

const WRITE_TYPE_ARGUMENTS_OF_SIGNATURE: u32 = 32;
const OMIT_PARAMETER_MODIFIERS: u32 = 8_192;
const OMIT_THIS_PARAMETER: u32 = 33_554_432;

#[derive(Clone, Debug, Default)]
pub(crate) struct SignatureDeclarationOptions {
    pub(crate) modifiers: Option<Vec<TransformNode>>,
    pub(crate) name: Option<TransformNode>,
    pub(crate) question_token: Option<TransformNode>,
}

fn has_flag(context: &NodeBuilderContext<'_>, flag: u32) -> bool {
    context.flags.0 & flag != 0
}

fn source_name_length(checker: &CheckerState<'_>, symbol: SymbolId) -> usize {
    checker.symbol_display_name(symbol).encode_utf16().count()
}

fn node_array(
    arena: &mut TransformArena,
    target: TransformSourceId,
    nodes: Vec<TransformNode>,
) -> BuildResult<Option<tsc_syntax::NodeArrayId>> {
    (!nodes.is_empty())
        .then(|| create_node_array(arena, target, nodes))
        .transpose()
}

fn required_type_node(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    r#type: TypeId,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<TransformNode> {
    match type_to_type_node_helper(checker, arena, target, r#type, context)? {
        Some(node) => Ok(node),
        None => {
            context.encountered_error = true;
            create_token(arena, target, SyntaxKind::AnyKeyword)
        }
    }
}

fn empty_identifier(
    arena: &mut TransformArena,
    target: TransformSourceId,
) -> BuildResult<TransformNode> {
    create_identifier(arena, target, "")
}

fn empty_block(
    arena: &mut TransformArena,
    target: TransformSourceId,
) -> BuildResult<TransformNode> {
    create_node(
        arena,
        target,
        NodeData::Block(BlockData { statements: None }),
    )
}

/// tsc-port: getTupleElementLabel @6.0.3 (signature-expansion face)
/// tsc-hash: cfaef41e5163a36e33fb797ca0f1cf2445bcc1cf9453ac75b2f61681f2b472b1
/// tsc-span: _tsc.js:78150-78157
fn expanded_tuple_element_label(
    checker: &CheckerState<'_>,
    declaration: Option<NodeId>,
    index: usize,
    element_flags: ElementFlags,
    rest_symbol: SymbolId,
    context: &NodeBuilderContext<'_>,
) -> BuildResult<String> {
    if let Some(declaration) = declaration {
        return checker
            .tuple_element_label(declaration)
            .map_err(|abort| checker_abort_error(checker, context, abort));
    }
    let rest_parameter = checker
        .binder
        .symbol(rest_symbol)
        .value_declaration
        .filter(|&declaration| matches!(checker.data_of(declaration), NodeData::Parameter(_)));
    Ok(match rest_parameter {
        Some(parameter) => expanded_tuple_element_label_from_binding_element(
            checker,
            parameter,
            index,
            element_flags,
        ),
        None => format!(
            "{}_{}",
            tsc_binder::unescape_leading_underscores(
                &checker.binder.symbol(rest_symbol).escaped_name
            ),
            index
        ),
    })
}

/// tsc-port: getTupleElementLabelFromBindingElement @6.0.3
/// tsc-hash: a8abed48acb2849e206d1748a97355a466b6a962706a1b417bcd041eacb3a0be
/// tsc-span: _tsc.js:78121-78149
fn expanded_tuple_element_label_from_binding_element(
    checker: &CheckerState<'_>,
    node: NodeId,
    index: usize,
    element_flags: ElementFlags,
) -> String {
    let (name, dot_dot_dot) = match checker.data_of(node) {
        NodeData::Parameter(data) => (data.name, data.dot_dot_dot_token.is_some()),
        NodeData::BindingElement(data) => (data.name, data.dot_dot_dot_token.is_some()),
        _ => (None, false),
    };
    if let Some(name) = name {
        match checker.data_of(name) {
            NodeData::Identifier(data) => {
                let text = tsc_binder::unescape_leading_underscores(&data.escaped_text).to_owned();
                if dot_dot_dot {
                    return if element_flags.intersects(ElementFlags::VARIABLE) {
                        text
                    } else {
                        format!("{text}_{index}")
                    };
                }
                return if element_flags.intersects(ElementFlags::REQUIRED | ElementFlags::OPTIONAL)
                {
                    text
                } else {
                    format!("{text}_n")
                };
            }
            NodeData::ArrayBindingPattern(data) if dot_dot_dot => {
                let elements = checker.nodes_of(data.elements);
                let last_is_rest = elements.last().copied().is_some_and(|last| {
                    matches!(checker.data_of(last), NodeData::BindingElement(data)
                        if data.dot_dot_dot_token.is_some())
                });
                let element_count = elements.len() - usize::from(last_is_rest);
                if index < element_count {
                    let element = elements[index];
                    if matches!(checker.data_of(element), NodeData::BindingElement(_)) {
                        return expanded_tuple_element_label_from_binding_element(
                            checker,
                            element,
                            index,
                            element_flags,
                        );
                    }
                } else if last_is_rest {
                    let last = *elements.last().expect("last_is_rest implies non-empty");
                    return expanded_tuple_element_label_from_binding_element(
                        checker,
                        last,
                        index - element_count,
                        element_flags,
                    );
                }
            }
            _ => {}
        }
    }
    format!("arg_{index}")
}

/// tsc-port: getExpandedParameters @6.0.3 (skipUnionExpanding face)
/// tsc-hash: 43c4acbf32d5eaa48b8366c408ee5255add1639b9c48993d53c049bc18b7e6c8
/// tsc-span: _tsc.js:57911-57960
fn get_expanded_parameters(
    checker: &mut CheckerState<'_>,
    signature_id: SignatureId,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<Vec<SymbolId>> {
    let signature = checker.signature_of(signature_id).clone();
    if !signature
        .flags
        .intersects(SignatureFlags::HAS_REST_PARAMETER)
        || signature.parameters.is_empty()
    {
        return Ok(signature.parameters);
    }
    let rest_index = signature.parameters.len() - 1;
    let rest_symbol = signature.parameters[rest_index];
    let rest_type = checker
        .get_type_of_symbol(rest_symbol)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    if !checker.tables.is_tuple_type(rest_type) {
        return Ok(signature.parameters);
    }
    let target = checker.tables.reference_target(rest_type);
    let TypeData::TupleTarget(tuple) = checker.tables.type_of(target).data.clone() else {
        return Ok(signature.parameters);
    };
    let element_types = checker
        .get_type_arguments(rest_type)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    let count = element_types.len().min(tuple.element_flags.len());
    let mut associated_names = match tuple.labeled_element_declarations.as_ref() {
        Some(declarations) => {
            let mut names = Vec::with_capacity(count);
            for index in 0..count {
                let declaration = declarations.get(index).copied().flatten().map(NodeId);
                names.push(expanded_tuple_element_label(
                    checker,
                    declaration,
                    index,
                    tuple.element_flags[index],
                    rest_symbol,
                    context,
                )?);
            }
            Some(names)
        }
        None => None,
    };
    if let Some(names) = associated_names.as_mut() {
        let mut unique_names = HashSet::new();
        let mut duplicates = Vec::new();
        for (index, name) in names.iter().enumerate() {
            if !unique_names.insert(name.clone()) {
                duplicates.push(index);
            }
        }
        let mut counters = HashMap::new();
        for index in duplicates {
            let base = names[index].clone();
            let mut counter = counters.get(&base).copied().unwrap_or(1_u32);
            let name = loop {
                let candidate = format!("{base}_{counter}");
                if unique_names.insert(candidate.clone()) {
                    break candidate;
                }
                counter += 1;
            };
            names[index] = name.clone();
            // Upstream keys this write by the rewritten name.
            counters.insert(name, counter + 1);
        }
    }
    let mut expanded = signature.parameters[..rest_index].to_vec();
    expanded.reserve(count);
    for (index, element_type) in element_types.into_iter().take(count).enumerate() {
        let name = match associated_names.as_ref().and_then(|names| names.get(index)) {
            Some(name) => name.clone(),
            None => checker
                .get_parameter_name_at_position(signature_id, rest_index + index)
                .map_err(|abort| checker_abort_error(checker, context, abort))?
                .unwrap_or_else(|| format!("arg{index}")),
        };
        let flags = tuple.element_flags[index];
        let check_flags = if flags.intersects(ElementFlags::VARIABLE) {
            CheckFlags::REST_PARAMETER
        } else if flags.intersects(ElementFlags::OPTIONAL) {
            CheckFlags::OPTIONAL_PARAMETER
        } else {
            CheckFlags::from_bits(0)
        };
        let symbol = checker
            .binder
            .create_symbol(SymbolFlags::FUNCTION_SCOPED_VARIABLE, name);
        checker
            .links
            .set_symbol_check_flags(checker.speculation_depth, symbol, check_flags);
        let parameter_type = if flags.intersects(ElementFlags::REST) {
            checker
                .create_array_type(element_type, false)
                .map_err(|abort| checker_abort_error(checker, context, abort))?
        } else {
            element_type
        };
        checker
            .links
            .set_fresh_symbol_type(symbol, crate::links::LinkSlot::Resolved(parameter_type));
        expanded.push(symbol);
    }
    Ok(expanded)
}

fn create_modifiers_from_flags(
    arena: &mut TransformArena,
    target: TransformSourceId,
    flags: ModifierFlags,
) -> BuildResult<Vec<TransformNode>> {
    let mut modifiers = Vec::new();
    for (flag, kind) in [
        (ModifierFlags::EXPORT, SyntaxKind::ExportKeyword),
        (ModifierFlags::DEFAULT, SyntaxKind::DefaultKeyword),
        (ModifierFlags::AMBIENT, SyntaxKind::DeclareKeyword),
        (ModifierFlags::PUBLIC, SyntaxKind::PublicKeyword),
        (ModifierFlags::PROTECTED, SyntaxKind::ProtectedKeyword),
        (ModifierFlags::PRIVATE, SyntaxKind::PrivateKeyword),
        (ModifierFlags::ABSTRACT, SyntaxKind::AbstractKeyword),
        (ModifierFlags::STATIC, SyntaxKind::StaticKeyword),
        (ModifierFlags::READONLY, SyntaxKind::ReadonlyKeyword),
        (ModifierFlags::OVERRIDE, SyntaxKind::OverrideKeyword),
        (ModifierFlags::ASYNC, SyntaxKind::AsyncKeyword),
        (ModifierFlags::IN, SyntaxKind::InKeyword),
        (ModifierFlags::OUT, SyntaxKind::OutKeyword),
        (ModifierFlags::CONST, SyntaxKind::ConstKeyword),
    ] {
        if flags.intersects(flag) {
            modifiers.push(create_token(arena, target, kind)?);
        }
    }
    Ok(modifiers)
}

fn clone_modifiers(
    checker: &CheckerState<'_>,
    arena: &mut TransformArena,
    declaration: NodeId,
) -> BuildResult<Vec<TransformNode>> {
    let source = checker.binder.source_of_node(declaration);
    let Some(modifiers) = node_util::modifiers_of(source, declaration) else {
        return Ok(Vec::new());
    };
    checker
        .binder
        .node_array(modifiers)
        .nodes
        .iter()
        .filter_map(|&modifier| clone_parse_node(checker, arena, modifier).transpose())
        .collect()
}

/// tsc-port: signatureToSignatureDeclarationHelper @6.0.3
/// tsc-hash: 7bad7586790a8d59ed15a56604a736d3183bf5c7fa8056b42201e30acae56e0a
/// tsc-span: _tsc.js:52504-52611
pub(crate) fn signature_to_signature_declaration_helper(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    signature_id: SignatureId,
    kind: SyntaxKind,
    context: &mut NodeBuilderContext<'_>,
    options: Option<SignatureDeclarationOptions>,
) -> BuildResult<TransformNode> {
    let signature = checker.signature_of(signature_id).clone();
    let expanded_parameters = get_expanded_parameters(checker, signature_id, context)?;
    let scope = enter_new_scope(
        context,
        signature.declaration,
        Some(&expanded_parameters),
        signature.type_parameters.as_deref(),
        Some(&signature.parameters),
        signature.mapper,
    );
    let result = (|| -> BuildResult<TransformNode> {
        add_approximate_length(context, 3);

        let mut type_arguments = None;
        let mut type_parameters = Vec::new();
        let signature_instantiation = if has_flag(context, WRITE_TYPE_ARGUMENTS_OF_SIGNATURE) {
            signature
                .target
                .zip(signature.mapper)
                .and_then(|(target_signature_id, mapper)| {
                    checker
                        .signature_of(target_signature_id)
                        .type_parameters
                        .clone()
                        .map(|parameters| (parameters, mapper))
                })
        } else {
            None
        };
        if let Some((target_type_parameters, mapper)) = signature_instantiation {
            let mut arguments = Vec::with_capacity(target_type_parameters.len());
            for parameter in target_type_parameters {
                let instantiated = checker
                    .instantiate_type(parameter, Some(mapper))
                    .map_err(|abort| checker_abort_error(checker, context, abort))?;
                arguments.push(required_type_node(
                    checker,
                    arena,
                    target,
                    instantiated,
                    context,
                )?);
            }
            type_arguments = node_array(arena, target, arguments)?;
        } else if let Some(parameters) = signature.type_parameters.as_deref() {
            type_parameters.reserve(parameters.len());
            for parameter in parameters {
                type_parameters.push(type_parameter_to_declaration(
                    checker, arena, target, *parameter, context, None,
                )?);
            }
        }

        let restore = save_restore_flags(context);
        context.flags.0 &= !EmitNodeBuilderFlags::SUPPRESS_ANY_RETURN_TYPE.0;
        let parameters = (|| -> BuildResult<Vec<TransformNode>> {
            let source_parameters =
                if expanded_parameters
                    .iter()
                    .enumerate()
                    .any(|(index, symbol)| {
                        index + 1 < expanded_parameters.len()
                            && checker
                                .get_check_flags(*symbol)
                                .intersects(CheckFlags::REST_PARAMETER)
                    })
                {
                    &signature.parameters
                } else {
                    &expanded_parameters
                };
            let mut parameters = Vec::with_capacity(source_parameters.len() + 1);
            for parameter in source_parameters {
                parameters.push(symbol_to_parameter_declaration(
                    checker,
                    arena,
                    target,
                    *parameter,
                    context,
                    kind == SyntaxKind::Constructor,
                )?);
            }
            if !has_flag(context, OMIT_THIS_PARAMETER) {
                if let Some(this_parameter) = try_get_this_parameter_declaration(
                    checker,
                    arena,
                    target,
                    signature_id,
                    context,
                )? {
                    parameters.insert(0, this_parameter);
                }
            }
            Ok(parameters)
        })();
        restore_flags(context, restore);
        let parameters = parameters?;

        let return_type = match serialize_return_type_for_signature_seam(
            checker,
            arena,
            target,
            context,
            signature_id,
        )? {
            Some(node) => Some(node),
            None => {
                let suppress_any = context
                    .flags
                    .contains(EmitNodeBuilderFlags::SUPPRESS_ANY_RETURN_TYPE);
                let return_type = checker
                    .get_return_type_of_signature(signature_id)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?;
                if suppress_any
                    && checker
                        .tables
                        .flags_of(return_type)
                        .intersects(tsc_types::TypeFlags::ANY)
                {
                    None
                } else {
                    match type_to_type_node_helper(checker, arena, target, return_type, context)? {
                        Some(node) => Some(node),
                        None if !suppress_any => {
                            Some(create_token(arena, target, SyntaxKind::AnyKeyword)?)
                        }
                        None => None,
                    }
                }
            }
        };

        let mut options = options.unwrap_or_default();
        if kind == SyntaxKind::ConstructorType
            && signature.flags.intersects(SignatureFlags::ABSTRACT)
        {
            let has_abstract = options.modifiers.as_ref().is_some_and(|modifiers| {
                modifiers.iter().any(|modifier| {
                    arena
                        .node(*modifier)
                        .is_ok_and(|node| node.kind == SyntaxKind::AbstractKeyword)
                })
            });
            if !has_abstract {
                options
                    .modifiers
                    .get_or_insert_with(Vec::new)
                    .push(create_token(arena, target, SyntaxKind::AbstractKeyword)?);
            }
        }

        let type_parameters = node_array(arena, target, type_parameters)?;
        let parameters = Some(create_node_array(arena, target, parameters)?);
        let modifiers = node_array(arena, target, options.modifiers.unwrap_or_default())?;
        let return_id = return_type.map(TransformNode::node);
        let name = match options.name {
            Some(name) => Some(name.node()),
            None if matches!(
                kind,
                SyntaxKind::MethodSignature
                    | SyntaxKind::MethodDeclaration
                    | SyntaxKind::GetAccessor
                    | SyntaxKind::SetAccessor
                    | SyntaxKind::FunctionDeclaration
                    | SyntaxKind::FunctionExpression
            ) =>
            {
                Some(empty_identifier(arena, target)?.node())
            }
            None => None,
        };
        let question_token = options.question_token.map(TransformNode::node);

        // h2-7a-m-3.5: parenthesizer face
        let node = match kind {
            SyntaxKind::CallSignature => create_node(
                arena,
                target,
                NodeData::CallSignature(CallSignatureData {
                    type_parameters,
                    parameters,
                    r#type: return_id,
                }),
            )?,
            SyntaxKind::ConstructSignature => create_node(
                arena,
                target,
                NodeData::ConstructSignature(ConstructSignatureData {
                    type_parameters,
                    parameters,
                    r#type: return_id,
                }),
            )?,
            SyntaxKind::MethodSignature => create_node(
                arena,
                target,
                NodeData::MethodSignature(MethodSignatureData {
                    name,
                    type_parameters,
                    parameters,
                    r#type: return_id,
                    question_token,
                    modifiers,
                }),
            )?,
            SyntaxKind::MethodDeclaration => create_node(
                arena,
                target,
                NodeData::MethodDeclaration(MethodDeclarationData {
                    name,
                    type_parameters,
                    parameters,
                    r#type: return_id,
                    asterisk_token: None,
                    question_token: None,
                    exclamation_token: None,
                    body: None,
                    modifiers,
                }),
            )?,
            SyntaxKind::Constructor => create_node(
                arena,
                target,
                NodeData::Constructor(ConstructorData {
                    name: None,
                    type_parameters: None,
                    parameters,
                    r#type: None,
                    body: None,
                    modifiers,
                }),
            )?,
            SyntaxKind::GetAccessor => create_node(
                arena,
                target,
                NodeData::GetAccessor(GetAccessorData {
                    name,
                    type_parameters: None,
                    parameters,
                    r#type: return_id,
                    body: None,
                    modifiers,
                }),
            )?,
            SyntaxKind::SetAccessor => create_node(
                arena,
                target,
                NodeData::SetAccessor(SetAccessorData {
                    name,
                    type_parameters: None,
                    parameters,
                    r#type: None,
                    body: None,
                    modifiers,
                }),
            )?,
            SyntaxKind::IndexSignature => create_node(
                arena,
                target,
                NodeData::IndexSignature(IndexSignatureData {
                    type_parameters: None,
                    parameters,
                    r#type: return_id,
                    modifiers,
                }),
            )?,
            SyntaxKind::JSDocFunctionType => create_node(
                arena,
                target,
                NodeData::JSDocFunctionType(JSDocFunctionTypeData {
                    name: None,
                    type_parameters: None,
                    parameters,
                    r#type: return_id,
                }),
            )?,
            SyntaxKind::FunctionType => {
                let type_node = match return_id {
                    Some(node) => Some(node),
                    None => {
                        let empty = empty_identifier(arena, target)?;
                        Some(
                            create_node(
                                arena,
                                target,
                                NodeData::TypeReference(tsc_syntax::nodes::TypeReferenceData {
                                    type_arguments: None,
                                    type_name: Some(empty.node()),
                                }),
                            )?
                            .node(),
                        )
                    }
                };
                create_node(
                    arena,
                    target,
                    NodeData::FunctionType(FunctionTypeData {
                        type_parameters,
                        parameters,
                        r#type: type_node,
                        modifiers: None,
                    }),
                )?
            }
            SyntaxKind::ConstructorType => {
                let type_node = match return_id {
                    Some(node) => Some(node),
                    None => {
                        let empty = empty_identifier(arena, target)?;
                        Some(
                            create_node(
                                arena,
                                target,
                                NodeData::TypeReference(tsc_syntax::nodes::TypeReferenceData {
                                    type_arguments: None,
                                    type_name: Some(empty.node()),
                                }),
                            )?
                            .node(),
                        )
                    }
                };
                create_node(
                    arena,
                    target,
                    NodeData::ConstructorType(ConstructorTypeData {
                        type_parameters,
                        parameters,
                        r#type: type_node,
                        modifiers,
                    }),
                )?
            }
            SyntaxKind::FunctionDeclaration => create_node(
                arena,
                target,
                NodeData::FunctionDeclaration(FunctionDeclarationData {
                    name,
                    type_parameters,
                    parameters,
                    r#type: return_id,
                    asterisk_token: None,
                    body: None,
                    modifiers,
                }),
            )?,
            SyntaxKind::FunctionExpression => {
                let body = empty_block(arena, target)?;
                create_node(
                    arena,
                    target,
                    NodeData::FunctionExpression(FunctionExpressionData {
                        name,
                        type_parameters,
                        parameters,
                        r#type: return_id,
                        asterisk_token: None,
                        body: Some(body.node()),
                        modifiers,
                    }),
                )?
            }
            SyntaxKind::ArrowFunction => {
                let body = empty_block(arena, target)?;
                create_node(
                    arena,
                    target,
                    NodeData::ArrowFunction(ArrowFunctionData {
                        type_parameters,
                        parameters,
                        r#type: return_id,
                        body: Some(body.node()),
                        modifiers,
                        equals_greater_than_token: None,
                    }),
                )?
            }
            _ => {
                return Err(EmitResolverError::CheckerAborted {
                    method: tsc_emitter::EmitResolverMethod::CreateTypeOfDeclaration,
                    node: tsc_emitter::EmitResolverNode::from_raw_source(
                        0,
                        context.enclosing_declaration.unwrap_or(NodeId(0)),
                    ),
                    reason: "unsupported signature declaration kind",
                });
            }
        };
        // `node.typeArguments` is a dynamic, printer-internal property upstream;
        // the generated Rust NodeData schema has no corresponding field. The
        // instantiated arguments were still serialized above so all resolver and
        // truncation effects occur at the upstream point.
        let _ = type_arguments;
        if let Some(declaration) = signature.declaration {
            if checker.kind_of(declaration) == SyntaxKind::JSDocSignature {
                if let Some(overload_tag) = checker
                    .parent_of(declaration)
                    .filter(|&parent| checker.kind_of(parent) == SyntaxKind::JSDocOverloadTag)
                {
                    if let Some(comment_node) = checker.parent_of(overload_tag) {
                        let source = checker.binder.source_of_node(comment_node);
                        let raw = source.arena.node(comment_node);
                        let start = (raw.pos as usize).min(source.text().len());
                        let end = (raw.end as usize).min(source.text().len());
                        if start <= end {
                            let text = &source.text()[start..end];
                            let text = if text.len() >= 4 {
                                &text[2..text.len() - 2]
                            } else {
                                text
                            };
                            let comment = text
                                .lines()
                                .map(|line| {
                                    let trimmed = line.trim_start();
                                    if trimmed.len() == line.len() {
                                        line.to_owned()
                                    } else {
                                        format!(" {trimmed}")
                                    }
                                })
                                .collect::<Vec<_>>()
                                .join("\n");
                            arena
                                .metadata_mut(node)
                                .add_leading_comment(SyntheticComment::new(
                                    SyntheticCommentKind::MultiLine,
                                    comment,
                                    false,
                                    true,
                                ));
                        }
                    }
                }
            }
        }
        Ok(node)
    })();
    exit_new_scope(context, scope);
    result
}

#[derive(Debug)]
pub(crate) struct RecoveryBoundary {
    old_tracked_symbols: Option<Vec<TrackedSymbol>>,
    old_encountered_error: bool,
    old_had_error: bool,
    old_depth: u32,
    buffered_error_count: usize,
    had_error: bool,
    finalized: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RecoveryScopeRestore {
    tracked_symbols_top: usize,
    buffered_error_count_top: usize,
}

/// tsc-port: createRecoveryBoundary @6.0.3
/// tsc-hash: fbb249361099c0251b456805c24108c8f8d81e91f71ad6ff1fe97c6c9d4eb6dc
/// tsc-span: _tsc.js:52612-52691
pub(crate) fn create_recovery_boundary(context: &mut NodeBuilderContext<'_>) -> RecoveryBoundary {
    let old_tracked_symbols = context.tracked_symbols.take();
    let old_encountered_error = context.encountered_error;
    let old_had_error = context.recovery_boundary_had_error;
    let old_depth = context.recovery_boundary_depth;
    context.tracked_symbols = Some(Vec::new());
    context.recovery_boundary_had_error = false;
    context.recovery_boundary_depth = old_depth.saturating_add(1);
    RecoveryBoundary {
        old_tracked_symbols,
        old_encountered_error,
        old_had_error,
        old_depth,
        buffered_error_count: 0,
        had_error: false,
        finalized: false,
    }
}

impl RecoveryBoundary {
    /// tsc-port: createRecoveryBoundary.markError @6.0.3
    /// tsc-hash: bd5484c21186e2409344255ada00e33da68eecf6b9742cfc9fa73bc0df3a6922
    /// tsc-span: _tsc.js:52655-52660
    pub(crate) fn mark_error(&mut self, context: &mut NodeBuilderContext<'_>) {
        self.had_error = true;
        self.buffered_error_count += 1;
        context.recovery_boundary_had_error = true;
    }

    /// tsc-port: createRecoveryBoundary.startRecoveryScope @6.0.3
    /// tsc-hash: daaa168a2da526400476ca60410810472df439a73340f0ba03b7ff43406d1fe1
    /// tsc-span: _tsc.js:52661-52673
    pub(crate) fn start_recovery_scope(
        &self,
        context: &NodeBuilderContext<'_>,
    ) -> RecoveryScopeRestore {
        RecoveryScopeRestore {
            tracked_symbols_top: context.tracked_symbols.as_ref().map_or(0, Vec::len),
            buffered_error_count_top: self.buffered_error_count,
        }
    }

    pub(crate) fn restore_recovery_scope(
        &mut self,
        context: &mut NodeBuilderContext<'_>,
        restore: RecoveryScopeRestore,
    ) {
        if let Some(tracked) = context.tracked_symbols.as_mut() {
            tracked.truncate(restore.tracked_symbols_top);
        }
        self.buffered_error_count = restore.buffered_error_count_top;
        self.had_error = false;
        context.recovery_boundary_had_error = false;
    }

    /// tsc-port: createRecoveryBoundary.finalizeBoundary @6.0.3
    /// tsc-hash: c40ba0412ece96787a8cc53ec7b533edbf8f3d2859a802423641872472399884
    /// tsc-span: _tsc.js:52674-52690
    pub(crate) fn finalize(mut self, context: &mut NodeBuilderContext<'_>) -> bool {
        let buffered = context.tracked_symbols.take().unwrap_or_default();
        let success = !self.had_error;
        let mut restored = self.old_tracked_symbols.take();
        if success && !buffered.is_empty() {
            restored.get_or_insert_with(Vec::new).extend(buffered);
        }
        context.tracked_symbols = restored;
        context.encountered_error = self.old_encountered_error;
        context.recovery_boundary_had_error = self.old_had_error;
        context.recovery_boundary_depth = self.old_depth;
        self.finalized = true;
        success
    }

    pub(crate) const fn had_error(&self) -> bool {
        self.had_error
    }
}

pub(crate) struct ScopeRestore {
    enclosing_declaration: Option<NodeId>,
    mapper: Option<MapperId>,
    must_create_type_parameter_symbol_list: bool,
    type_parameter_symbol_list: Option<HashSet<SymbolId>>,
    must_create_type_parameters_names_lookups: bool,
    type_parameter_names: Option<HashMap<TypeId, TransformNode>>,
    type_parameter_names_by_text: Option<HashSet<String>>,
    type_parameter_names_by_text_next_name_count: Option<HashMap<String, u32>>,
}

/// tsc-port: enterNewScope @6.0.3
/// tsc-hash: 2e14fce8894e1878f50325ffceebc3976956fd557cb2637dbd951c73e33322bb
/// tsc-span: _tsc.js:52692-52801
pub(crate) fn enter_new_scope(
    context: &mut NodeBuilderContext<'_>,
    _declaration: Option<NodeId>,
    expanded_parameters: Option<&[SymbolId]>,
    type_parameters: Option<&[TypeId]>,
    original_parameters: Option<&[SymbolId]>,
    mapper: Option<MapperId>,
) -> ScopeRestore {
    let restore = ScopeRestore {
        enclosing_declaration: context.enclosing_declaration,
        mapper: context.mapper,
        must_create_type_parameter_symbol_list: context.must_create_type_parameter_symbol_list,
        type_parameter_symbol_list: context.type_parameter_symbol_list.take(),
        must_create_type_parameters_names_lookups: context
            .must_create_type_parameters_names_lookups,
        type_parameter_names: context.type_parameter_names.take(),
        type_parameter_names_by_text: context.type_parameter_names_by_text.take(),
        type_parameter_names_by_text_next_name_count: context
            .type_parameter_names_by_text_next_name_count
            .take(),
    };
    context.must_create_type_parameter_symbol_list = true;
    context.must_create_type_parameters_names_lookups = true;
    if let Some(mapper) = mapper {
        context.mapper = Some(mapper);
    }

    if let (Some(expanded), Some(original)) = (expanded_parameters, original_parameters) {
        if expanded.len() == original.len()
            && expanded
                .iter()
                .zip(original)
                .any(|(expanded, original)| expanded != original)
        {
            // Rust's binder symbol tables are immutable after binding, so the
            // fake-scope substitutions are represented by the fresh name maps
            // rather than mutating locals on a synthesized Block.
            context
                .type_parameter_names_by_text
                .get_or_insert_with(HashSet::new);
        }
    }
    if context
        .flags
        .contains(EmitNodeBuilderFlags::GENERATE_NAMES_FOR_SHADOWED_TYPE_PARAMS)
        && type_parameters.is_some_and(|parameters| !parameters.is_empty())
    {
        context
            .type_parameter_names_by_text
            .get_or_insert_with(HashSet::new);
    }
    restore
}

pub(crate) fn exit_new_scope(context: &mut NodeBuilderContext<'_>, restore: ScopeRestore) {
    context.enclosing_declaration = restore.enclosing_declaration;
    context.mapper = restore.mapper;
    context.must_create_type_parameter_symbol_list = restore.must_create_type_parameter_symbol_list;
    context.type_parameter_symbol_list = restore.type_parameter_symbol_list;
    context.must_create_type_parameters_names_lookups =
        restore.must_create_type_parameters_names_lookups;
    context.type_parameter_names = restore.type_parameter_names;
    context.type_parameter_names_by_text = restore.type_parameter_names_by_text;
    context.type_parameter_names_by_text_next_name_count =
        restore.type_parameter_names_by_text_next_name_count;
}

/// tsc-port: enterNewScope.bindPattern @6.0.3
/// tsc-hash: 0fadd6af58cd9157113e2eb994c804e682198a819d5a7808cda91cf16614caa9
/// tsc-span: _tsc.js:52757-52768
#[allow(dead_code)]
fn bind_pattern(checker: &CheckerState<'_>, pattern: NodeId, names: &mut HashSet<String>) {
    let elements = match checker.data_of(pattern) {
        NodeData::ArrayBindingPattern(data) => checker.nodes_of(data.elements),
        NodeData::ObjectBindingPattern(data) => checker.nodes_of(data.elements),
        _ => return,
    };
    for element in elements {
        if checker.kind_of(element) == SyntaxKind::BindingElement {
            bind_element(checker, element, names);
        }
    }
}

/// tsc-port: enterNewScope.bindElement @6.0.3
/// tsc-hash: aabfd6344cd4a7f4bb1cdf47047684d290e76171d66a44ab68910499a490013c
/// tsc-span: _tsc.js:52769-52775
fn bind_element(checker: &CheckerState<'_>, element: NodeId, names: &mut HashSet<String>) {
    let NodeData::BindingElement(data) = checker.data_of(element) else {
        return;
    };
    let Some(name) = data.name else {
        return;
    };
    match checker.data_of(name) {
        NodeData::Identifier(data) => {
            names.insert(data.text.clone());
        }
        NodeData::ArrayBindingPattern(_) | NodeData::ObjectBindingPattern(_) => {
            bind_pattern(checker, name, names);
        }
        _ => {}
    }
}

/// tsc-port: tryGetThisParameterDeclaration @6.0.3
/// tsc-hash: 670149ceef4c815821df5a794795a3ef2058b5013bdf6b6e62083697376ac5e5
/// tsc-span: _tsc.js:52802-52821
fn try_get_this_parameter_declaration(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    signature: SignatureId,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<Option<TransformNode>> {
    let signature = checker.signature_of(signature).clone();
    if let Some(this_parameter) = signature.this_parameter {
        return symbol_to_parameter_declaration(
            checker,
            arena,
            target,
            this_parameter,
            context,
            false,
        )
        .map(Some);
    }
    if let Some(declaration) = signature
        .declaration
        .filter(|&declaration| checker.is_in_js_file(declaration))
    {
        if let Some(this_tag) = checker.first_jsdoc_tag(declaration, SyntaxKind::JSDocThisTag) {
            if let NodeData::JSDocThisTag(data) = checker.data_of(this_tag) {
                if let Some(type_expression) = data.type_expression {
                    let this_type = checker
                        .get_type_from_type_node(type_expression)
                        .map_err(|abort| checker_abort_error(checker, context, abort))?;
                    let type_node = required_type_node(checker, arena, target, this_type, context)?;
                    let name = create_identifier(arena, target, "this")?;
                    return create_node(
                        arena,
                        target,
                        NodeData::Parameter(ParameterData {
                            name: Some(name.node()),
                            modifiers: None,
                            dot_dot_dot_token: None,
                            question_token: None,
                            r#type: Some(type_node.node()),
                            initializer: None,
                        }),
                    )
                    .map(Some);
                }
            }
        }
    }
    Ok(None)
}

/// tsc-port: typeParameterToDeclarationWithConstraint @6.0.3
/// tsc-hash: c101fcc8db6c2afc3c9e0d096ef1c527ca89a85056b9dad5cc95e50b26df806d
/// tsc-span: _tsc.js:52822-52831
pub(super) fn type_parameter_to_declaration_with_constraint(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    r#type: TypeId,
    context: &mut NodeBuilderContext<'_>,
    constraint_node: Option<TransformNode>,
) -> BuildResult<TransformNode> {
    let restore = save_restore_flags(context);
    context.flags.0 &= !512;
    let modifiers =
        create_modifiers_from_flags(arena, target, checker.get_type_parameter_modifiers(r#type))?;
    let name = type_parameter_name(checker, arena, target, r#type, context)?;
    let default_node = (|| {
        let default = checker
            .get_default_from_type_parameter(r#type)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        match default {
            Some(default) => type_to_type_node_helper(checker, arena, target, default, context),
            None => Ok(None),
        }
    })();
    restore_flags(context, restore);
    let default_node = default_node?;
    let modifiers = node_array(arena, target, modifiers)?;
    create_node(
        arena,
        target,
        NodeData::TypeParameter(TypeParameterData {
            name: Some(name.node()),
            modifiers,
            constraint: constraint_node.map(TransformNode::node),
            r#default: default_node.map(TransformNode::node),
            expression: None,
        }),
    )
}

/// tsc-port: typeToTypeNodeHelperWithPossibleReusableTypeNode @6.0.3
/// tsc-hash: ee7204096e054a1171acf8a6fcd084bc1985b658478ef3ac615ca4f6893d95b6
/// tsc-span: _tsc.js:52832-52834
fn type_to_type_node_helper_with_possible_reusable_type_node(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    r#type: TypeId,
    type_node: Option<NodeId>,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<Option<TransformNode>> {
    if !can_possibly_expand_type(r#type, context) {
        if let Some(type_node) = type_node {
            if checker
                .get_type_from_type_node(type_node)
                .map_err(|abort| checker_abort_error(checker, context, abort))?
                == r#type
            {
                if let Some(reused) = syntactic_try_reuse_existing_type_node(
                    checker, arena, target, context, type_node,
                )? {
                    return Ok(Some(reused));
                }
            }
        }
    }
    type_to_type_node_helper(checker, arena, target, r#type, context)
}

/// tsc-port: typeParameterToDeclaration @6.0.3
/// tsc-hash: 4042204c4d69a17c649563e2074d464bcc70c80516b3a580f61065f6d20c8024
/// tsc-span: _tsc.js:52835-52838
pub(crate) fn type_parameter_to_declaration(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    r#type: TypeId,
    context: &mut NodeBuilderContext<'_>,
    constraint: Option<TypeId>,
) -> BuildResult<TransformNode> {
    let constraint = match constraint {
        Some(constraint) => Some(constraint),
        None => checker
            .get_constraint_of_type_parameter(r#type)
            .map_err(|abort| checker_abort_error(checker, context, abort))?,
    };
    let constraint_node = match constraint {
        Some(constraint) => type_to_type_node_helper_with_possible_reusable_type_node(
            checker,
            arena,
            target,
            constraint,
            checker.get_constraint_declaration(r#type),
            context,
        )?,
        None => None,
    };
    type_parameter_to_declaration_with_constraint(
        checker,
        arena,
        target,
        r#type,
        context,
        constraint_node,
    )
}

/// tsc-port: typePredicateToTypePredicateNodeHelper @6.0.3
/// tsc-hash: e9d579768001d51d345bf226390002978d047ae1c0dd0badeeb5342006240a19
/// tsc-span: _tsc.js:52839-52844
pub(crate) fn type_predicate_to_type_predicate_node_helper(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    predicate: &TypePredicate,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<TransformNode> {
    let asserts_modifier = matches!(
        predicate.kind,
        TypePredicateKind::AssertsThis | TypePredicateKind::AssertsIdentifier
    )
    .then(|| create_token(arena, target, SyntaxKind::AssertsKeyword))
    .transpose()?;
    let parameter_name = if matches!(
        predicate.kind,
        TypePredicateKind::Identifier | TypePredicateKind::AssertsIdentifier
    ) {
        let name = create_identifier(
            arena,
            target,
            predicate.parameter_name.as_deref().unwrap_or(""),
        )?;
        Some(set_no_ascii_escaping(arena, name))
    } else {
        Some(create_token(arena, target, SyntaxKind::ThisType)?)
    };
    let type_node = match predicate.ty {
        Some(r#type) => type_to_type_node_helper(checker, arena, target, r#type, context)?,
        None => None,
    };
    create_node(
        arena,
        target,
        NodeData::TypePredicate(TypePredicateData {
            asserts_modifier: asserts_modifier.map(TransformNode::node),
            parameter_name: parameter_name.map(TransformNode::node),
            r#type: type_node.map(TransformNode::node),
        }),
    )
}

/// tsc-port: getEffectiveParameterDeclaration @6.0.3
/// tsc-hash: 2588afb7d3b8e6e07a54d982d06c2f7c2b2858fc29e3477e095fde223ae1dafe
/// tsc-span: _tsc.js:52845-52853
fn get_effective_parameter_declaration(
    checker: &CheckerState<'_>,
    parameter: SymbolId,
) -> Option<NodeId> {
    checker
        .get_declaration_of_kind(parameter, SyntaxKind::Parameter)
        .or_else(|| {
            (!checker
                .binder
                .symbol(parameter)
                .flags
                .intersects(SymbolFlags::TRANSIENT))
            .then(|| checker.get_declaration_of_kind(parameter, SyntaxKind::JSDocParameterTag))
            .flatten()
        })
}

/// tsc-port: symbolToParameterDeclaration @6.0.3
/// tsc-hash: 9db05ad714cae92baf23906078955d4e6652075abc5709eae1d7efae5ba16634
/// tsc-span: _tsc.js:52854-52875
pub(crate) fn symbol_to_parameter_declaration(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    parameter: SymbolId,
    context: &mut NodeBuilderContext<'_>,
    preserve_modifier_flags: bool,
) -> BuildResult<TransformNode> {
    let declaration = get_effective_parameter_declaration(checker, parameter);
    let parameter_type = checker
        .get_type_of_symbol(parameter)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    let type_node = match serialize_type_for_declaration_seam(
        checker,
        arena,
        target,
        context,
        declaration,
        parameter_type,
        Some(parameter),
    )? {
        Some(node) => Some(node),
        None => type_to_type_node_helper(checker, arena, target, parameter_type, context)?,
    };
    let modifiers = match declaration {
        Some(declaration)
            if !has_flag(context, OMIT_PARAMETER_MODIFIERS) && preserve_modifier_flags =>
        {
            clone_modifiers(checker, arena, declaration)?
        }
        _ => Vec::new(),
    };
    let syntactic_rest =
        declaration.is_some_and(|declaration| checker.is_rest_parameter_declaration(declaration));
    let is_rest = syntactic_rest
        || checker
            .get_check_flags(parameter)
            .intersects(CheckFlags::REST_PARAMETER);
    let dot_dot_dot_token = is_rest
        .then(|| create_token(arena, target, SyntaxKind::DotDotDotToken))
        .transpose()?;
    let name = parameter_to_parameter_declaration_name(
        checker,
        arena,
        target,
        parameter,
        declaration,
        context,
    )?;
    let syntactic_optional = match declaration {
        Some(declaration) => checker
            .emit_is_optional_parameter(declaration)
            .map_err(|abort| checker_abort_error(checker, context, abort))?,
        None => false,
    };
    let is_optional = syntactic_optional
        || checker
            .get_check_flags(parameter)
            .intersects(CheckFlags::OPTIONAL_PARAMETER);
    let question_token = is_optional
        .then(|| create_token(arena, target, SyntaxKind::QuestionToken))
        .transpose()?;
    let modifiers = node_array(arena, target, modifiers)?;
    let node = create_node(
        arena,
        target,
        NodeData::Parameter(ParameterData {
            name: Some(name.node()),
            modifiers,
            dot_dot_dot_token: dot_dot_dot_token.map(TransformNode::node),
            question_token: question_token.map(TransformNode::node),
            r#type: type_node.map(TransformNode::node),
            initializer: None,
        }),
    )?;
    add_approximate_length(context, source_name_length(checker, parameter) + 3);
    Ok(node)
}

/// tsc-port: parameterToParameterDeclarationName @6.0.3
/// tsc-hash: f8c988288813b2b174b4e49718f6864d442cbfe82334ec499d89013ec3df06a4
/// tsc-span: _tsc.js:52876-52909
fn parameter_to_parameter_declaration_name(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    parameter: SymbolId,
    declaration: Option<NodeId>,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<TransformNode> {
    let Some(declaration) = declaration else {
        return create_identifier(arena, target, &checker.symbol_display_name(parameter));
    };
    if let Some(serialized) =
        syntactic_serialize_name_of_parameter_seam(checker, arena, target, context, declaration)?
    {
        return Ok(serialized);
    }
    let name = match checker.data_of(declaration) {
        NodeData::Parameter(data) => data.name,
        NodeData::JSDocParameterTag(data) => data.name,
        _ => None,
    };
    let Some(name) = name else {
        return create_identifier(arena, target, &checker.symbol_display_name(parameter));
    };
    match checker.kind_of(name) {
        SyntaxKind::Identifier => {
            let cloned = clone_parse_node(checker, arena, name)?.unwrap_or(create_identifier(
                arena,
                target,
                &checker.symbol_display_name(parameter),
            )?);
            Ok(set_no_ascii_escaping(arena, cloned))
        }
        SyntaxKind::QualifiedName => {
            let right = match checker.data_of(name) {
                NodeData::QualifiedName(data) => data.right,
                _ => None,
            };
            let cloned = match right {
                Some(right) => clone_parse_node(checker, arena, right)?,
                None => None,
            }
            .unwrap_or(create_identifier(
                arena,
                target,
                &checker.symbol_display_name(parameter),
            )?);
            Ok(set_no_ascii_escaping(arena, cloned))
        }
        SyntaxKind::ArrayBindingPattern | SyntaxKind::ObjectBindingPattern => {
            clone_binding_name(checker, arena, target, name, context)
        }
        _ => create_identifier(arena, target, &checker.symbol_display_name(parameter)),
    }
}

/// tsc-port: parameterToParameterDeclarationName.cloneBindingName @6.0.3
/// tsc-hash: d14170c43f6b359e6b695faf5357071724cd4133986d5621d68cf83b06867014
/// tsc-span: _tsc.js:52878-52908
fn clone_binding_name(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    node: NodeId,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<TransformNode> {
    elide_initializer_and_set_emit_flags(checker, arena, target, node, context)
}

/// tsc-port: parameterToParameterDeclarationName.elideInitializerAndSetEmitFlags @6.0.3
/// tsc-hash: f8bdbba84cdea52719e327d0bb00c48532bd6d5d35b61b62620ccddfc12a6099
/// tsc-span: _tsc.js:52880-52907
fn elide_initializer_and_set_emit_flags(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    node: NodeId,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<TransformNode> {
    let visited = match checker.data_of(node).clone() {
        NodeData::ArrayBindingPattern(data) => {
            let mut elements = Vec::new();
            for element in checker.nodes_of(data.elements) {
                if checker.kind_of(element) == SyntaxKind::OmittedExpression {
                    if let Some(cloned) = clone_parse_node(checker, arena, element)? {
                        elements.push(cloned);
                    }
                } else {
                    elements.push(elide_initializer_and_set_emit_flags(
                        checker, arena, target, element, context,
                    )?);
                }
            }
            let elements = node_array(arena, target, elements)?;
            create_node(
                arena,
                target,
                NodeData::ArrayBindingPattern(ArrayBindingPatternData { elements }),
            )?
        }
        NodeData::ObjectBindingPattern(data) => {
            let mut elements = Vec::new();
            for element in checker.nodes_of(data.elements) {
                elements.push(elide_initializer_and_set_emit_flags(
                    checker, arena, target, element, context,
                )?);
            }
            let elements = node_array(arena, target, elements)?;
            create_node(
                arena,
                target,
                NodeData::ObjectBindingPattern(ObjectBindingPatternData { elements }),
            )?
        }
        NodeData::BindingElement(data) => {
            if let Some(property_name) = data.property_name {
                if checker.kind_of(property_name) == SyntaxKind::ComputedPropertyName {
                    if let NodeData::ComputedPropertyName(computed) = checker.data_of(property_name)
                    {
                        if let Some(expression) = computed.expression {
                            if checker.is_entity_name_expression(expression) {
                                track_computed_name(checker, expression, context)?;
                            }
                        }
                    }
                }
            }
            let property_name = match data.property_name {
                Some(property_name) => clone_parse_node(checker, arena, property_name)?,
                None => None,
            };
            let name = match data.name {
                Some(name) => Some(elide_initializer_and_set_emit_flags(
                    checker, arena, target, name, context,
                )?),
                None => None,
            };
            let dot_dot_dot_token = match data.dot_dot_dot_token {
                Some(token) => clone_parse_node(checker, arena, token)?,
                None => None,
            };
            create_node(
                arena,
                target,
                NodeData::BindingElement(BindingElementData {
                    name: name.map(TransformNode::node),
                    property_name: property_name.map(TransformNode::node),
                    dot_dot_dot_token: dot_dot_dot_token.map(TransformNode::node),
                    initializer: None,
                }),
            )?
        }
        _ => {
            clone_parse_node(checker, arena, node)?.unwrap_or(create_identifier(arena, target, "")?)
        }
    };
    let visited = range_synthesized_node_to_parse(checker, arena, visited, node)?;
    let visited = set_single_line(arena, visited);
    Ok(set_no_ascii_escaping(arena, visited))
}

/// tsc-port: trackComputedName @6.0.3
/// tsc-hash: 747771ab8de848541c5687c87f5865240522be824fb4b335ebd8884e26e437ed
/// tsc-span: _tsc.js:52910-52938
pub(super) fn track_computed_name(
    checker: &mut CheckerState<'_>,
    access_expression: NodeId,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<()> {
    if !context.tracker.can_track_symbol {
        return Ok(());
    }
    let first_identifier = checker.first_identifier(access_expression);
    let symbol = checker
        .get_resolved_symbol(first_identifier)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    if let Some(symbol) = symbol {
        context.tracked_symbols.get_or_insert_with(Vec::new).push((
            symbol,
            context.enclosing_declaration,
            EmitSymbolMeaning::VALUE_EXPORT_VALUE,
        ));
    }
    Ok(())
}

/// tsc-port: indexInfoToIndexSignatureDeclarationHelper @6.0.3
/// tsc-hash: 272ecb1e37223afa95dd90071374ac2c2c8985c529f7a26a9e328f020360d79c
/// tsc-span: _tsc.js:52476-52503
pub(crate) fn index_info_to_index_signature_declaration_helper(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    index_info: &IndexInfo,
    context: &mut NodeBuilderContext<'_>,
    type_node: Option<TransformNode>,
) -> BuildResult<TransformNode> {
    let declaration_name = index_info.declaration.and_then(|declaration| {
        let NodeData::IndexSignature(data) = checker.data_of(declaration) else {
            return None;
        };
        let parameter = checker.nodes_of(data.parameters).first().copied()?;
        let NodeData::Parameter(data) = checker.data_of(parameter) else {
            return None;
        };
        data.name
    });
    let name_text = declaration_name
        .and_then(|name| checker.identifier_text(name).map(str::to_owned))
        .unwrap_or_else(|| "x".to_owned());
    let name = match declaration_name {
        Some(name) => clone_parse_node(checker, arena, name)?
            .unwrap_or(create_identifier(arena, target, &name_text)?),
        None => create_identifier(arena, target, &name_text)?,
    };
    let indexer_type = required_type_node(checker, arena, target, index_info.key_type, context)?;
    let parameter = create_node(
        arena,
        target,
        NodeData::Parameter(ParameterData {
            name: Some(name.node()),
            modifiers: None,
            dot_dot_dot_token: None,
            question_token: None,
            r#type: Some(indexer_type.node()),
            initializer: None,
        }),
    )?;
    let type_node = match type_node {
        Some(type_node) => type_node,
        None => required_type_node(checker, arena, target, index_info.value_type, context)?,
    };
    add_approximate_length(context, name_text.encode_utf16().count() + 4);
    let modifiers = if index_info.is_readonly {
        let readonly = create_token(arena, target, SyntaxKind::ReadonlyKeyword)?;
        Some(create_node_array(arena, target, vec![readonly])?)
    } else {
        None
    };
    let parameters = Some(create_node_array(arena, target, vec![parameter])?);
    create_node(
        arena,
        target,
        NodeData::IndexSignature(IndexSignatureData {
            type_parameters: None,
            parameters,
            r#type: Some(type_node.node()),
            modifiers,
        }),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use tsc_emitter::{EmitNodeBuilderFlags, SourceFileId, TransformArena};
    use tsc_syntax::{Node, NodeData, NodeId, SyntaxKind};
    use tsc_types::{CompilerOptions, MapperId};

    use crate::state::test_support::with_program_state;
    use crate::state::SignatureKind;

    use super::*;
    use crate::node_builder::with_context;

    fn with_builder(
        source: &str,
        flags: EmitNodeBuilderFlags,
        run: impl FnOnce(
            &mut CheckerState<'_>,
            &mut TransformArena,
            TransformSourceId,
            &mut NodeBuilderContext<'_>,
        ) -> BuildResult<()>,
    ) {
        with_program_state(
            &[("/main.ts", source)],
            &CompilerOptions::default(),
            |checker| {
                let root = checker.binder.source(0).root;
                let mut arena = TransformArena::new();
                let target =
                    arena.add_source(checker.binder.source(0), Some(SourceFileId::from_raw(0)));
                let result = with_context(
                    checker,
                    &mut arena,
                    target,
                    Some(root),
                    Some(flags),
                    None,
                    None,
                    None,
                    None,
                    run,
                    None,
                )
                .expect("node-builder callback succeeds");
                assert!(
                    result.is_some(),
                    "node-builder context did not encounter an error"
                );
            },
        );
    }

    fn emitted_node(arena: &TransformArena, target: TransformSourceId, node: NodeId) -> &Node {
        arena
            .source(target)
            .expect("transform source")
            .syntax()
            .arena
            .node(node)
    }

    fn emitted_array(
        arena: &TransformArena,
        target: TransformSourceId,
        array: tsc_syntax::NodeArrayId,
    ) -> &[NodeId] {
        &arena
            .source(target)
            .expect("transform source")
            .syntax()
            .arena
            .node_array(array)
            .nodes
    }

    fn first_alias_rhs(checker: &CheckerState<'_>) -> NodeId {
        let root = checker.binder.source(0).root;
        let NodeData::SourceFile(data) = checker.data_of(root) else {
            unreachable!()
        };
        checker
            .nodes_of(data.statements)
            .into_iter()
            .find_map(|statement| match checker.data_of(statement) {
                NodeData::TypeAliasDeclaration(data) => data.r#type,
                _ => None,
            })
            .expect("type alias")
    }

    #[test]
    fn signature_declaration_serializes_type_parameters_parameters_and_return_type() {
        let source = "type Fn = <T extends string = string>(value: T, count?: number) => T;";
        with_builder(
            source,
            EmitNodeBuilderFlags::GENERATE_NAMES_FOR_SHADOWED_TYPE_PARAMS,
            |checker, arena, target, context| {
                let rhs = first_alias_rhs(checker);
                let function_type = checker
                    .get_type_from_type_node(rhs)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?;
                let signatures = checker
                    .get_signatures_of_type(function_type, SignatureKind::Call)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?;
                assert_eq!(signatures.len(), 1);
                let node = signature_to_signature_declaration_helper(
                    checker,
                    arena,
                    target,
                    signatures[0],
                    SyntaxKind::FunctionType,
                    context,
                    None,
                )?;
                let NodeData::FunctionType(data) = &arena.node(node).map_err(factory_error)?.data
                else {
                    panic!("function type expected")
                };
                let type_parameters = emitted_array(
                    arena,
                    target,
                    data.type_parameters.expect("type parameters"),
                );
                assert_eq!(type_parameters.len(), 1);
                let NodeData::TypeParameter(type_parameter) =
                    &emitted_node(arena, target, type_parameters[0]).data
                else {
                    panic!("type parameter expected")
                };
                assert_eq!(
                    emitted_node(
                        arena,
                        target,
                        type_parameter.constraint.expect("constraint")
                    )
                    .kind,
                    SyntaxKind::StringKeyword
                );
                assert_eq!(
                    emitted_node(arena, target, type_parameter.r#default.expect("default")).kind,
                    SyntaxKind::StringKeyword
                );

                let parameters = emitted_array(arena, target, data.parameters.expect("parameters"));
                assert_eq!(parameters.len(), 2);
                let NodeData::Parameter(first) = &emitted_node(arena, target, parameters[0]).data
                else {
                    panic!("parameter expected")
                };
                assert!(first.question_token.is_none());
                let NodeData::Parameter(second) = &emitted_node(arena, target, parameters[1]).data
                else {
                    panic!("parameter expected")
                };
                assert_eq!(
                    emitted_node(
                        arena,
                        target,
                        second.question_token.expect("optional token")
                    )
                    .kind,
                    SyntaxKind::QuestionToken
                );
                assert_eq!(
                    emitted_node(arena, target, data.r#type.expect("return type")).kind,
                    SyntaxKind::TypeReference
                );
                assert!(context.approximate_length >= 3 + "value".len() as u32 + 3);
                Ok(())
            },
        );
    }

    #[test]
    fn signature_declaration_expands_tuple_typed_rest_parameters() {
        let source = "type Fn = (...args: [name: string, count?: number]) => void;";
        with_builder(
            source,
            EmitNodeBuilderFlags::NONE,
            |checker, arena, target, context| {
                let rhs = first_alias_rhs(checker);
                let function_type = checker
                    .get_type_from_type_node(rhs)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?;
                let signature = checker
                    .get_signatures_of_type(function_type, SignatureKind::Call)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?[0];
                let node = signature_to_signature_declaration_helper(
                    checker,
                    arena,
                    target,
                    signature,
                    SyntaxKind::FunctionType,
                    context,
                    None,
                )?;
                let NodeData::FunctionType(data) = &arena.node(node).map_err(factory_error)?.data
                else {
                    panic!("function type expected")
                };
                let parameters = emitted_array(arena, target, data.parameters.expect("parameters"));
                assert_eq!(parameters.len(), 2);
                for (index, expected_name) in ["name", "count"].into_iter().enumerate() {
                    let NodeData::Parameter(parameter) =
                        &emitted_node(arena, target, parameters[index]).data
                    else {
                        panic!("parameter expected")
                    };
                    let NodeData::Identifier(name) =
                        &emitted_node(arena, target, parameter.name.expect("parameter name")).data
                    else {
                        panic!("identifier expected")
                    };
                    assert_eq!(name.text, expected_name);
                    assert!(parameter.dot_dot_dot_token.is_none());
                    assert_eq!(parameter.question_token.is_some(), index == 1);
                }
                Ok(())
            },
        );
    }

    #[test]
    fn type_predicate_helper_covers_identifier_asserts_and_this_shapes() {
        with_builder(
            "export {};",
            EmitNodeBuilderFlags::NONE,
            |checker, arena, target, context| {
                let identifier = TypePredicate {
                    kind: TypePredicateKind::Identifier,
                    parameter_name: Some("value".to_owned()),
                    parameter_index: 0,
                    ty: Some(checker.tables.intrinsics.string),
                };
                let identifier_node = type_predicate_to_type_predicate_node_helper(
                    checker,
                    arena,
                    target,
                    &identifier,
                    context,
                )?;
                let NodeData::TypePredicate(data) =
                    &arena.node(identifier_node).map_err(factory_error)?.data
                else {
                    panic!("predicate expected")
                };
                assert!(data.asserts_modifier.is_none());
                assert_eq!(
                    emitted_node(arena, target, data.parameter_name.expect("parameter name")).kind,
                    SyntaxKind::Identifier
                );
                assert_eq!(
                    emitted_node(arena, target, data.r#type.expect("predicate type")).kind,
                    SyntaxKind::StringKeyword
                );

                let asserts_this = TypePredicate {
                    kind: TypePredicateKind::AssertsThis,
                    parameter_name: None,
                    parameter_index: -1,
                    ty: None,
                };
                let asserts_node = type_predicate_to_type_predicate_node_helper(
                    checker,
                    arena,
                    target,
                    &asserts_this,
                    context,
                )?;
                let NodeData::TypePredicate(data) =
                    &arena.node(asserts_node).map_err(factory_error)?.data
                else {
                    panic!("asserts predicate expected")
                };
                assert_eq!(
                    emitted_node(arena, target, data.asserts_modifier.expect("asserts")).kind,
                    SyntaxKind::AssertsKeyword
                );
                assert_eq!(
                    emitted_node(arena, target, data.parameter_name.expect("this")).kind,
                    SyntaxKind::ThisType
                );
                assert!(data.r#type.is_none());
                Ok(())
            },
        );
    }

    #[test]
    fn scope_and_recovery_boundary_restore_owned_context_state() {
        with_builder(
            "export {};",
            EmitNodeBuilderFlags::NONE,
            |_checker, arena, target, context| {
                let original_name = create_identifier(arena, target, "T")?;
                context.mapper = Some(MapperId(7));
                context.must_create_type_parameter_symbol_list = false;
                context.type_parameter_symbol_list = Some(HashSet::from([SymbolId(11)]));
                context.must_create_type_parameters_names_lookups = false;
                context.type_parameter_names = Some(HashMap::from([(TypeId(12), original_name)]));
                context.type_parameter_names_by_text = Some(HashSet::from(["T".to_owned()]));
                context.type_parameter_names_by_text_next_name_count =
                    Some(HashMap::from([("T".to_owned(), 2)]));
                let restore = enter_new_scope(context, None, None, None, None, Some(MapperId(8)));
                assert_eq!(context.mapper, Some(MapperId(8)));
                assert!(context.must_create_type_parameter_symbol_list);
                assert!(context.type_parameter_symbol_list.is_none());
                assert!(context.must_create_type_parameters_names_lookups);
                assert!(context.type_parameter_names.is_none());
                exit_new_scope(context, restore);
                assert_eq!(context.mapper, Some(MapperId(7)));
                assert!(!context.must_create_type_parameter_symbol_list);
                assert_eq!(
                    context.type_parameter_symbol_list.as_ref(),
                    Some(&HashSet::from([SymbolId(11)]))
                );
                assert!(!context.must_create_type_parameters_names_lookups);
                assert_eq!(
                    context
                        .type_parameter_names
                        .as_ref()
                        .and_then(|names| names.get(&TypeId(12))),
                    Some(&original_name)
                );

                context.tracked_symbols = Some(vec![(
                    SymbolId(20),
                    context.enclosing_declaration,
                    EmitSymbolMeaning::TYPE,
                )]);
                context.encountered_error = false;
                let mut boundary = create_recovery_boundary(context);
                context.tracked_symbols.as_mut().expect("buffer").push((
                    SymbolId(21),
                    context.enclosing_declaration,
                    EmitSymbolMeaning::TYPE,
                ));
                let recovery_scope = boundary.start_recovery_scope(context);
                context.tracked_symbols.as_mut().expect("buffer").push((
                    SymbolId(22),
                    context.enclosing_declaration,
                    EmitSymbolMeaning::VALUE_EXPORT_VALUE,
                ));
                boundary.mark_error(context);
                assert!(boundary.had_error());
                boundary.restore_recovery_scope(context, recovery_scope);
                assert!(!boundary.had_error());
                assert_eq!(context.tracked_symbols.as_ref().map(Vec::len), Some(1));
                assert!(boundary.finalize(context));
                assert_eq!(context.tracked_symbols.as_ref().map(Vec::len), Some(2));
                assert!(!context.encountered_error);
                assert_eq!(context.recovery_boundary_depth, 0);

                let mut failed = create_recovery_boundary(context);
                failed.mark_error(context);
                assert!(!failed.finalize(context));
                assert!(!context.encountered_error);
                assert_eq!(context.tracked_symbols.as_ref().map(Vec::len), Some(2));
                assert_eq!(context.recovery_boundary_depth, 0);
                Ok(())
            },
        );
    }
}

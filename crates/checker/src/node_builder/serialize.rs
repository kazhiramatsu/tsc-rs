use std::cell::RefCell;
use std::rc::Rc;
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
use tsc_types::{ObjectFlags, SymbolFlags, TypeData, TypeFacts, TypeFlags, TypeId};

use crate::narrow::TypePredicate;
use crate::state::{CheckAbort, CheckerState, IndexInfo, SignatureId};

use super::signatures::{
    elide_initializer_and_set_emit_flags, parameter_scope_symbols, track_computed_name,
};
use super::type_nodes::{
    checker_abort_error, clone_parse_node_to_source, create_identifier, create_node,
    create_node_array, create_token, factory_error, project_parse_node, set_no_ascii_escaping,
    type_to_type_node_helper, BuildResult,
};
use super::{
    add_symbol_type_to_context, can_possibly_expand_type, chains_symbol_to_entity_name_node,
    chains_symbol_to_expression, chains_symbol_to_type_node,
    existing_type_node_is_not_reference_or_is_reference_with_compatible_type_argument_count,
    get_declaration_with_type_annotation, get_enclosing_declaration_ignoring_fake_scope,
    get_module_specifier_override, get_type_from_type_node2,
    index_info_to_index_signature_declaration_helper, restore_flags,
    restore_symbol_type_to_context, save_restore_flags, serialize_inferred_type_for_declaration,
    set_text_range2, symbol_to_node, type_predicate_to_type_predicate_node_helper, with_context,
    with_context_in_synthetic_module_scope, NodeBuilderContext, SyntacticAccessorDeclarations,
    SyntacticBuilderResolver, SyntacticRecoveryBoundary, SyntacticScopeCleanup, SyntacticSymbol,
    SyntacticTrackedEntityName, SyntacticTypeNodeBuilder, SyntheticModuleScope,
};

const METHOD: EmitResolverMethod = EmitResolverMethod::CreateTypeOfDeclaration;
const ALLOW_UNRESOLVED_NAMES: u32 = 8;
const IGNORE_ERRORS: EmitNodeBuilderFlags = EmitNodeBuilderFlags(70_221_824);

/// Build the syntax consumed by checker `symbolToString` through the same
/// m-3 NodeBuilder context as declaration serialization.
///
/// tsc-port: symbolToString @6.0.3
/// tsc-hash: db59b39300442558c3a8f0e1f1d1681dbfaf0fdb3951350b225677ed4851157e
/// tsc-span: _tsc.js:50649-50681
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_symbol_display_node(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    symbol: SymbolId,
    enclosing: Option<NodeId>,
    meaning: EmitSymbolMeaning,
    flags: EmitNodeBuilderFlags,
    internal_flags: EmitInternalNodeBuilderFlags,
    allow_any_node_kind: bool,
) -> BuildResult<TransformNode> {
    if let Some(enclosing) = enclosing {
        let file_index = checker.binder.file_index_of_node(enclosing);
        let resolver = EmitResolverNode::new(
            SourceFileId::from_raw(
                u32::try_from(file_index).expect("checker source index exceeds u32"),
            ),
            enclosing,
        );
        if arena
            .parse_tree_transform_node(resolver)
            .map_err(factory_error)?
            .is_none()
        {
            arena.add_source(
                checker.binder.source(file_index),
                Some(SourceFileId::from_raw(
                    u32::try_from(file_index).expect("checker source index exceeds u32"),
                )),
            );
        }
    }
    with_context(
        checker,
        arena,
        target,
        enclosing,
        Some(flags),
        Some(internal_flags),
        None,
        None,
        None,
        |checker, arena, target, context| {
            if allow_any_node_kind {
                symbol_to_node(checker, arena, target, context, symbol, meaning)
            } else {
                chains_symbol_to_entity_name_node(checker, arena, target, context, symbol)
            }
        },
        None,
    )
    .map(|node| node.expect("IgnoreErrors symbolToString must produce a node"))
}

impl CheckerState<'_> {
    /// Build one symbol display node directly into the session-owned emit
    /// display result. The enclosing and declaration files are mounted only
    /// when this symbol first needs them.
    /// tsrs-native: session-owned display-result adapter around build_symbol_display_node.
    pub(crate) fn emit_build_symbol_display_node(
        &mut self,
        symbol: SymbolId,
        enclosing: Option<NodeId>,
        meaning: EmitSymbolMeaning,
        flags: EmitNodeBuilderFlags,
        internal_flags: EmitInternalNodeBuilderFlags,
        allow_any_node_kind: bool,
    ) -> BuildResult<TransformNode> {
        let target_file = enclosing
            .map(|node| self.binder.file_index_of_node(node))
            .or_else(|| {
                self.binder
                    .symbol(symbol)
                    .declarations
                    .first()
                    .map(|&node| self.binder.file_index_of_node(node))
            })
            .unwrap_or(0);
        let target = self.emit_display_target(target_file);
        let mut files = self
            .binder
            .symbol(symbol)
            .declarations
            .iter()
            .map(|&node| self.binder.file_index_of_node(node))
            .collect::<std::collections::BTreeSet<_>>();
        if let Some(enclosing) = enclosing {
            files.insert(self.binder.file_index_of_node(enclosing));
        }
        for file_index in files {
            self.emit_display_target(file_index);
        }

        let display = self.emit_display_result();
        let mut display = display.borrow_mut();
        build_symbol_display_node(
            self,
            display
                .arena_mut()
                .expect("checker display result remains live"),
            target,
            symbol,
            enclosing,
            meaning,
            flags,
            internal_flags,
            allow_any_node_kind,
        )
    }
}

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

fn node_modules_resolution_candidates(importer: &str, specifier: &str) -> Vec<String> {
    let importer = importer.replace('\\', "/");
    let mut directory =
        importer.rsplit_once('/').map_or(
            ".",
            |(directory, _)| {
                if directory.is_empty() {
                    "/"
                } else {
                    directory
                }
            },
        );
    let mut candidates = Vec::new();
    loop {
        let base = if directory == "/" {
            format!("/node_modules/{specifier}")
        } else if directory == "." {
            format!("node_modules/{specifier}")
        } else {
            format!("{directory}/node_modules/{specifier}")
        };
        for suffix in [
            ".ts",
            ".tsx",
            ".d.ts",
            ".js",
            ".jsx",
            "/index.ts",
            "/index.tsx",
            "/index.d.ts",
            "/index.js",
            "/index.jsx",
        ] {
            candidates.push(format!("{base}{suffix}"));
        }
        if matches!(directory, "/" | ".") {
            break;
        }
        directory =
            directory.rsplit_once('/').map_or(
                ".",
                |(parent, _)| {
                    if parent.is_empty() {
                        "/"
                    } else {
                        parent
                    }
                },
            );
    }
    candidates
}

fn recover_suppressed_import_target(
    checker: &mut CheckerState<'_>,
    alias: SymbolId,
) -> Option<SymbolId> {
    if !checker.symbol_flags(alias).intersects(SymbolFlags::ALIAS) {
        return None;
    }
    let import_specifier = checker
        .binder
        .symbol(alias)
        .declarations
        .iter()
        .copied()
        .find(|&node| checker.kind_of(node) == SyntaxKind::ImportSpecifier)?;
    let imported_name_node = match checker.data_of(import_specifier) {
        NodeData::ImportSpecifier(data) => data.property_name.or(data.name)?,
        _ => return None,
    };
    let imported_name = node_util::get_text_of_identifier_or_literal(
        checker.binder.source_of_node(imported_name_node),
        imported_name_node,
    )?;

    let mut ancestor = import_specifier;
    let module_specifier = loop {
        ancestor = checker.parent_of(ancestor)?;
        if let NodeData::ImportDeclaration(data) = checker.data_of(ancestor) {
            break data.module_specifier?;
        }
    };
    let module_name = match checker.data_of(module_specifier) {
        NodeData::StringLiteral(data) => data.text.clone(),
        _ => return None,
    };
    if module_name.starts_with('.') || module_name.starts_with('/') {
        return None;
    }

    let importer = checker
        .binder
        .source_of_node(import_specifier)
        .file_name
        .clone();
    for candidate in node_modules_resolution_candidates(&importer, &module_name) {
        let Some(file_index) = (0..checker.binder.file_count())
            .find(|&file_index| checker.binder.source(file_index).file_name == candidate)
        else {
            continue;
        };
        let root = checker.binder.source(file_index).root;
        let module_symbol = checker.node_symbol(root)?;
        let exports = checker.get_exports_of_module(module_symbol).ok()?;
        return exports.get(&imported_name).copied();
    }
    None
}

fn recover_suppressed_type_reference(
    checker: &mut CheckerState<'_>,
    type_node: NodeId,
) -> Option<TypeId> {
    let type_name = match checker.data_of(type_node) {
        NodeData::TypeReference(data) => data.type_name?,
        _ => return None,
    };
    let alias = checker.get_resolved_symbol(type_name).ok()??;
    let target = recover_suppressed_import_target(checker, alias)?;
    checker.get_declared_type_of_symbol_slice(target).ok()
}

/// Recover the declaration type that upstream obtains through its ordinary
/// node_modules resolver when this port deliberately suppresses that resolver
/// band. The recovery is limited to a directly imported, zero-argument call
/// initializer whose symbol type is already the error intrinsic.
fn recover_suppressed_import_call_return_type(
    checker: &mut CheckerState<'_>,
    declaration: NodeId,
) -> Option<TypeId> {
    let initializer = match checker.data_of(declaration) {
        NodeData::VariableDeclaration(data) => data.initializer?,
        _ => return None,
    };
    let expression = match checker.data_of(initializer) {
        NodeData::CallExpression(data) if checker.nodes_of(data.arguments).is_empty() => {
            data.expression?
        }
        _ => return None,
    };
    let alias = checker.get_resolved_symbol(expression).ok()??;
    let exported = recover_suppressed_import_target(checker, alias)?;
    let exported_type = checker.get_type_of_symbol(exported).ok()?;
    let signature = checker
        .get_signatures_of_type(exported_type, crate::state::SignatureKind::Call)
        .ok()?
        .into_iter()
        .next()?;
    let mut return_type = checker.get_return_type_of_signature(signature).ok()?;

    if checker.tables.is_tuple_type(return_type) {
        let target = checker.tables.reference_target(return_type);
        let TypeData::TupleTarget(tuple) = checker.tables.type_of(target).data.clone() else {
            return Some(return_type);
        };
        let signature_declaration = checker.signature_of(signature).declaration?;
        let annotation = match checker.data_of(signature_declaration) {
            NodeData::FunctionDeclaration(data) => data.r#type?,
            _ => return Some(return_type),
        };
        let elements = match checker.data_of(annotation) {
            NodeData::TupleType(data) => checker.nodes_of(data.elements).to_vec(),
            _ => return Some(return_type),
        };
        let mut arguments = checker.get_type_arguments(return_type).ok()?;
        let mut changed = false;
        for (argument, element) in arguments.iter_mut().zip(elements) {
            if checker.tables.is_error_type(*argument) {
                if let Some(recovered) = recover_suppressed_type_reference(checker, element) {
                    *argument = recovered;
                    changed = true;
                }
            }
        }
        if changed {
            return_type = checker
                .create_tuple_type_forced(
                    &arguments,
                    Some(&tuple.element_flags),
                    tuple.readonly,
                    tuple.labeled_element_declarations.as_deref(),
                )
                .ok()?;
        }
    }
    Some(return_type)
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
            let name = clone_parse_node_to_source(checker, arena, target, name)?.unwrap_or(
                create_identifier(arena, target, &checker.symbol_display_name(symbol))?,
            );
            Ok(set_no_ascii_escaping(arena, name))
        }
        SyntaxKind::QualifiedName => {
            let right = match checker.data_of(name) {
                NodeData::QualifiedName(data) => data.right,
                _ => None,
            };
            let name = right
                .map(|right| clone_parse_node_to_source(checker, arena, target, right))
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
    let mut resolver = ProductionSyntacticBuilderResolver::new(checker, METHOD);
    {
        let result = builder.serialize_type_of_declaration(
            &mut resolver,
            arena,
            target,
            context,
            declaration,
            Some(symbol),
        )?;
        into_target(arena, target, result)
    }
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
    let mut resolver = ProductionSyntacticBuilderResolver::new(checker, METHOD);
    {
        let result = builder.serialize_type_of_accessor(
            &mut resolver,
            arena,
            target,
            context,
            declaration,
            Some(symbol),
        )?;
        into_target(arena, target, result)
    }
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
    let mut resolver = ProductionSyntacticBuilderResolver::new(
        checker,
        EmitResolverMethod::CreateReturnTypeOfSignatureDeclaration,
    );
    {
        let result = builder.serialize_return_type_for_signature(
            &mut resolver,
            arena,
            target,
            context,
            declaration,
            Some(symbol),
        )?;
        into_target(arena, target, result)
    }
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

/// tsrs-native: the syntactic routing seams hand the finished subtree to the
/// emitted target source (h2-7b-m-2 #4e — a reused annotation from another
/// file is rebuilt in its own source, then cloned here; same source = identity).
fn into_target(
    arena: &mut TransformArena,
    target: TransformSourceId,
    result: Option<TransformNode>,
) -> BuildResult<Option<TransformNode>> {
    result
        .map(|node| {
            if node.source() == target {
                Ok(node)
            } else {
                arena
                    .factory()
                    .clone_node_to_source(node, target)
                    .map_err(factory_error)
            }
        })
        .transpose()
}

/// tsrs-native: checker-side routing seam behind the syntactic resolver member.
pub(crate) fn serialize_type_for_declaration_seam(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    declaration: Option<NodeId>,
    r#type: TypeId,
    symbol: Option<SymbolId>,
) -> BuildResult<Option<TransformNode>> {
    let result = serialize_type_for_declaration_in_context(
        checker,
        arena,
        target,
        context,
        declaration,
        r#type,
        symbol,
    )?;
    result
        .map(|node| {
            if node.source() == target {
                Ok(node)
            } else {
                arena
                    .factory()
                    .clone_node_to_source(node, target)
                    .map_err(factory_error)
            }
        })
        .transpose()
}

/// tsrs-native: checker-side routing seam behind the syntactic resolver member.
pub(crate) fn serialize_return_type_for_signature_seam(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    signature: SignatureId,
) -> BuildResult<Option<TransformNode>> {
    let result =
        serialize_return_type_for_signature_in_context(checker, arena, target, context, signature)?;
    result
        .map(|node| {
            if node.source() == target {
                Ok(node)
            } else {
                arena
                    .factory()
                    .clone_node_to_source(node, target)
                    .map_err(factory_error)
            }
        })
        .transpose()
}

/// tsrs-native: checker-side routing seam behind the syntactic tryReuse member.
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
    let mut resolver = ProductionSyntacticBuilderResolver::new(checker, METHOD);
    {
        let result = builder.try_reuse_existing_type_node(
            &mut resolver,
            arena,
            target,
            context,
            type_node,
        )?;
        into_target(arena, target, result)
    }
}

/// tsrs-native: checker-side routing seam behind the syntactic resolver member.
pub(crate) fn syntactic_serialize_name_of_parameter_seam(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    parameter: NodeId,
) -> BuildResult<Option<TransformNode>> {
    serialize_parameter_name_from_parse(checker, arena, target, context, parameter).map(Some)
}

/// tsc-port: typeToTypeNode @6.0.3 (createNodeBuilder API)
/// tsc-hash: b69637a60229522776d46a72086e27f8689094ecbb8a3686f6eb28e61f5a51fa
/// tsc-span: _tsc.js:50959-50959
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

/// tsc-port: typePredicateToTypePredicateNode @6.0.3 (createNodeBuilder API)
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

/// tsc-port: serializeTypeForDeclaration @6.0.3 (createNodeBuilder API)
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
    synthetic_module_scope: Option<SyntheticModuleScope<'_>>,
) -> BuildResult<Option<TransformNode>> {
    let serialize = |checker: &mut CheckerState<'_>,
                     arena: &mut TransformArena,
                     target: TransformSourceId,
                     context: &mut NodeBuilderContext<'_>| {
        let Some(declaration) = project_parse_node(checker, arena, declaration)? else {
            return Ok(None);
        };
        let symbol = syntactic_symbol(checker, symbol);
        let builder = SyntacticTypeNodeBuilder::new(checker.options);
        let mut resolver = ProductionSyntacticBuilderResolver::new(checker, METHOD);
        {
            let result = builder.serialize_type_of_declaration(
                &mut resolver,
                arena,
                target,
                context,
                declaration,
                Some(symbol),
            )?;
            into_target(arena, target, result)
        }
    };
    match synthetic_module_scope {
        Some(scope) => with_context_in_synthetic_module_scope(
            checker,
            arena,
            target,
            enclosing_declaration,
            flags,
            internal_flags,
            tracker,
            None,
            None,
            scope,
            serialize,
            None,
        ),
        None => with_context(
            checker,
            arena,
            target,
            enclosing_declaration,
            flags,
            internal_flags,
            tracker,
            None,
            None,
            serialize,
            None,
        ),
    }
    .map(Option::flatten)
}

/// tsc-port: serializeReturnTypeForSignature @6.0.3 (createNodeBuilder API)
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
            let mut resolver = ProductionSyntacticBuilderResolver::new(
                checker,
                EmitResolverMethod::CreateReturnTypeOfSignatureDeclaration,
            );
            {
                let result = builder.serialize_return_type_for_signature(
                    &mut resolver,
                    arena,
                    target,
                    context,
                    signature_declaration,
                    Some(symbol),
                )?;
                into_target(arena, target, result)
            }
        },
        None,
    )
    .map(Option::flatten)
}

/// tsc-port: serializeTypeForExpression @6.0.3 (createNodeBuilder API)
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
            let mut resolver = ProductionSyntacticBuilderResolver::new(
                checker,
                EmitResolverMethod::CreateTypeOfExpression,
            );
            {
                let result = builder.serialize_type_of_expression(
                    &mut resolver,
                    arena,
                    target,
                    context,
                    expression,
                )?;
                into_target(arena, target, result)
            }
        },
        None,
    )
    .map(Option::flatten)
}

/// tsc-port: indexInfoToIndexSignatureDeclaration @6.0.3 (createNodeBuilder API)
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
    method: EmitResolverMethod,
    /// Display-node scratch for the tracker-access accessibility path (no
    /// builder arena is in scope there); mounted lazily, reused per resolver.
    scratch_arena: Option<TransformArena>,
}

impl<'state, 'program> ProductionSyntacticBuilderResolver<'state, 'program> {
    fn new(checker: &'state mut CheckerState<'program>, method: EmitResolverMethod) -> Self {
        Self {
            checker,
            method,
            scratch_arena: None,
        }
    }

    fn parse_node(&self, arena: &TransformArena, node: TransformNode) -> BuildResult<NodeId> {
        arena
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
        super::tracker::tracker_node_id(node)
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

    fn project_parse_node(
        &mut self,
        arena: &mut TransformArena,
        node: NodeId,
    ) -> BuildResult<Option<TransformNode>> {
        let file_index = self.checker.binder.file_index_of_node(node);
        let resolver = EmitResolverNode::new(
            SourceFileId::from_raw(
                u32::try_from(file_index).expect("checker source index exceeds u32"),
            ),
            node,
        );
        if arena
            .parse_tree_transform_node(resolver)
            .map_err(factory_error)?
            .is_none()
        {
            arena.add_source(
                self.checker.binder.source(file_index),
                Some(SourceFileId::from_raw(
                    u32::try_from(file_index).expect("checker source index exceeds u32"),
                )),
            );
        }
        arena
            .parse_tree_transform_node(resolver)
            .map_err(factory_error)
    }

    /// `isSymbolAccessibleWorker` formats inaccessible symbol/module names
    /// through the public NodeBuilder `symbolToNode` front door upstream.
    fn build_accessibility_error_name(
        &mut self,
        arena: &mut TransformArena,
        symbol: SymbolId,
        enclosing: NodeId,
        enclosing_is_synthetic: bool,
        meaning: EmitSymbolMeaning,
    ) -> BuildResult<TransformNode> {
        let target = self
            .project_parse_node(arena, enclosing)?
            .ok_or_else(|| self.invalid_token_error(Some(enclosing)))?
            .source();
        build_symbol_display_node(
            self.checker,
            arena,
            target,
            symbol,
            (!enclosing_is_synthetic).then_some(enclosing),
            meaning,
            IGNORE_ERRORS,
            EmitInternalNodeBuilderFlags::NONE,
            true,
        )
    }

    fn accessibility_error_module_symbol(
        &mut self,
        symbol: SymbolId,
        error_module_name: &str,
    ) -> BuildResult<Option<SymbolId>> {
        let mut parent = self.checker.binder.symbol(symbol).parent;
        while let Some(candidate) = parent {
            if self.checker.symbol_display_name(candidate) == error_module_name {
                return Ok(Some(candidate));
            }
            parent = self.checker.binder.symbol(candidate).parent;
        }
        for declaration in self.checker.binder.symbol(symbol).declarations.clone() {
            if let Some(candidate) = self
                .checker
                .get_external_module_container(declaration)
                .map_err(|abort| {
                    callback_abort_error(self.checker, self.method, Some(declaration), abort)
                })?
            {
                if self.checker.symbol_display_name(candidate) == error_module_name {
                    return Ok(Some(candidate));
                }
            }
        }
        Ok(None)
    }

    fn is_symbol_accessible_with_error_names(
        &mut self,
        arena: &mut TransformArena,
        symbol: SymbolId,
        enclosing: NodeId,
        enclosing_is_synthetic: bool,
        meaning: EmitSymbolMeaning,
        should_compute_aliases: bool,
    ) -> BuildResult<EmitSymbolAccessibilityResult> {
        // Rust represents the signature fake block with its real source-file
        // token. Preserve upstream's module-container accessibility decision,
        // while the replay observation retains the member symbol passed at
        // the resolver entry.
        let access_symbol = if enclosing_is_synthetic && !should_compute_aliases {
            self.checker
                .binder
                .symbol(symbol)
                .parent
                .filter(|&parent| {
                    self.checker
                        .symbol_flags(parent)
                        .intersects(SymbolFlags::VALUE_MODULE | SymbolFlags::NAMESPACE_MODULE)
                        && self.checker.binder.symbol(parent).declarations.iter().any(
                            |&declaration| {
                                self.checker.kind_of(declaration) == SyntaxKind::SourceFile
                            },
                        )
                })
                .unwrap_or(symbol)
        } else {
            symbol
        };
        let result = self
            .checker
            .emit_is_symbol_accessible_with_observation(
                access_symbol,
                symbol,
                enclosing,
                enclosing_is_synthetic,
                meaning,
                should_compute_aliases,
            )
            .map_err(|abort| {
                callback_abort_error(self.checker, self.method, Some(enclosing), abort)
            })?;
        if result.error_symbol_name.is_some() {
            self.build_accessibility_error_name(
                arena,
                symbol,
                enclosing,
                enclosing_is_synthetic,
                meaning,
            )?;
        }
        if let Some(error_module_name) = result.error_module_name.as_deref() {
            if let Some(module_symbol) =
                self.accessibility_error_module_symbol(symbol, error_module_name)?
            {
                let module_meaning =
                    if result.accessibility == EmitSymbolAccessibility::NotAccessible {
                        EmitSymbolMeaning::NAMESPACE
                    } else {
                        EmitSymbolMeaning(0)
                    };
                self.build_accessibility_error_name(
                    arena,
                    module_symbol,
                    enclosing,
                    enclosing_is_synthetic,
                    module_meaning,
                )?;
            }
        }
        Ok(result)
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
        let enclosing_is_synthetic =
            enclosing_declaration.is_some_and(super::tracker::tracker_node_is_synthetic);
        let enclosing = enclosing_declaration.and_then(|node| self.tracker_node(node));
        let symbol = self
            .symbol(symbol)
            .ok_or_else(|| self.invalid_token_error(enclosing))?;
        let enclosing = enclosing.ok_or_else(|| self.invalid_token_error(None))?;
        // The declaration-transform tracker records the callback before its
        // own TypeParameter fast return (:114360-114362), so the probe has a
        // tracker event but no nested accessibility query/name formatting.
        if self
            .checker
            .symbol_flags(symbol)
            .intersects(SymbolFlags::TYPE_PARAMETER)
            || self
                .checker
                .binder
                .symbol(symbol)
                .declarations
                .iter()
                .any(|&declaration| self.checker.kind_of(declaration) == SyntaxKind::TypeParameter)
        {
            return Ok(self.checker.emit_accessible_symbol_observation(
                symbol,
                enclosing,
                enclosing_is_synthetic,
                meaning,
                should_compute_aliases,
            ));
        }
        let mut scratch = self.scratch_arena.take().unwrap_or_default();
        let result = self.is_symbol_accessible_with_error_names(
            &mut scratch,
            symbol,
            enclosing,
            enclosing_is_synthetic,
            meaning,
            should_compute_aliases,
        );
        self.scratch_arena = Some(scratch);
        result
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
        if super::tracker::tracker_node_is_synthetic(node) {
            return EmitTrackerNodeDescription::default();
        }
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
        let symbol_is_remapped = super::is_statement_symbol_remapped(self.checker, context, symbol);
        let NodeBuilderContext {
            tracker,
            reported_diagnostic,
            tracked_symbols,
            recovery_tracked_symbols,
            enclosing_declaration,
            enclosing_declaration_is_synthetic,
            ..
        } = context;
        tracker.track_symbol(
            reported_diagnostic,
            tracked_symbols,
            recovery_tracked_symbols,
            self,
            symbol,
            symbol_flags,
            *enclosing_declaration,
            *enclosing_declaration_is_synthetic,
            meaning,
            symbol_is_remapped,
        )?;
        Ok(())
    }

    /// tsrs-native: `trackExistingEntityName`'s body; the trait member places
    /// its result in the requested source (#4e).
    fn track_existing_entity_name_in_source(
        &mut self,
        arena: &mut TransformArena,
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
                    self.is_symbol_accessible_with_error_names(
                        arena, symbol, leftmost, false, meaning, false,
                    )?
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
            let fake_scope_symbol = context
                .enclosing_declaration_is_synthetic
                .then(|| match self.checker.data_of(leftmost) {
                    NodeData::Identifier(data) => context
                        .synthetic_scope_locals
                        .as_ref()
                        .and_then(|locals| locals.get(&data.escaped_text).copied()),
                    _ => None,
                })
                .flatten();
            // Parameter locals in a synthesized signature scope are not in
            // scope for their own JSDoc type annotations. The parse-site
            // resolver can conservatively return that parameter in Rust;
            // recover the outer symbol that upstream resolved before it
            // installed the fake scope so the reference-mismatch arm fires.
            if let (Some(original), Some(fake), Some(enclosing)) =
                (symbol, fake_scope_symbol, context.enclosing_declaration)
            {
                if original == fake
                    && self
                        .checker
                        .symbol_flags(fake)
                        .intersects(SymbolFlags::FUNCTION_SCOPED_VARIABLE)
                    && self
                        .checker
                        .binder
                        .symbol(fake)
                        .value_declaration
                        .is_some_and(|declaration| {
                            node_util::is_part_of_parameter_declaration(
                                self.checker.binder.source_of_node(declaration),
                                declaration,
                            )
                        })
                {
                    let outer = self
                        .checker
                        .resolve_entity_name_ex(leftmost, flags, true, Some(enclosing), true)
                        .map_err(|abort| checker_abort_error(self.checker, context, abort))?;
                    if outer != Some(fake) {
                        symbol = outer.map(|symbol| {
                            self.checker
                                .get_export_symbol_of_value_symbol_if_exported(symbol)
                        });
                    }
                }
            }
            let at_location = match (
                context.enclosing_declaration_is_synthetic,
                fake_scope_symbol,
            ) {
                (_, Some(symbol)) => Some(symbol),
                (true, None) => match context.enclosing_declaration.and_then(|enclosing| {
                    matches!(
                        self.checker.kind_of(enclosing),
                        SyntaxKind::SourceFile | SyntaxKind::ModuleBlock
                    )
                    .then_some(enclosing)
                    .or_else(|| self.checker.parent_of(enclosing))
                }) {
                    Some(parent) => self
                        .checker
                        .resolve_entity_name_ex(leftmost, flags, true, Some(parent), true)
                        .map_err(|abort| checker_abort_error(self.checker, context, abort))?,
                    None => None,
                },
                (false, None) => self
                    .checker
                    .resolve_entity_name_ex(
                        leftmost,
                        flags,
                        true,
                        context.enclosing_declaration,
                        true,
                    )
                    .map_err(|abort| checker_abort_error(self.checker, context, abort))?,
            };
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
            // Rust keeps local and export symbols distinct where upstream's
            // symbol identity is projected through `exportSymbol`. Preserve
            // the remapped-symbol suppression used by the statement tracker
            // after resolving in the synthesized scope.
            symbol = at_location.map(|symbol| {
                self.checker
                    .get_export_symbol_of_value_symbol_if_exported(symbol)
            });
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
                            self.is_symbol_accessible_with_error_names(
                                arena,
                                symbol,
                                enclosing,
                                context.enclosing_declaration_is_synthetic,
                                meaning,
                                false,
                            )?
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
        } else if let Some(leftmost) = self.project_parse_node(arena, leftmost)? {
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
            // getTypeFromTypeReference writes resolvedSymbol and resolvedType as
            // one NodeLinks transaction upstream. Rust can reach this point
            // with only the latter cached by an earlier checker path; recover
            // the same decision from that resolved type rather than rejecting
            // an otherwise reusable annotation.
            let symbol = self.checker.links.node(existing).resolved_symbol.resolved();
            let type_is_type_parameter = self
                .checker
                .tables
                .type_of(r#type)
                .flags
                .intersects(TypeFlags::TYPE_PARAMETER);
            if symbol.is_some_and(|symbol| {
                self.checker
                    .symbol_flags(symbol)
                    .intersects(SymbolFlags::TYPE_PARAMETER)
            }) || type_is_type_parameter
            {
                let declared = symbol.map_or(r#type, |symbol| {
                    self.checker.get_declared_type_of_type_parameter(symbol)
                });
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
                let symbol = symbol
                    .or(self.checker.tables.type_of(r#type).alias_symbol)
                    .or(self.checker.tables.type_of(r#type).symbol);
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
                    && symbol.is_some_and(|symbol| {
                        self.checker
                            .symbol_flags(symbol)
                            .intersects(SymbolFlags::TYPE)
                    }));
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

/// tsc-port: syntacticBuilderResolver @6.0.3 (production object)
/// tsc-hash: 4435e40ac4ba06bf9e97dd48b84835ddcec09e878d5b6163f041aa5ea0398894
/// tsc-span: _tsc.js:50778-50956
impl SyntacticBuilderResolver for ProductionSyntacticBuilderResolver<'_, '_> {
    fn evaluate_entity_name_expression(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        expression: TransformNode,
    ) -> Result<crate::evaluate::EvaluatorResult, EmitResolverError> {
        let expression = self.parse_node(arena, expression)?;
        self.checker.evaluate(expression, None).map_err(|abort| {
            callback_abort_error(self.checker, self.method, Some(expression), abort)
        })
    }

    fn is_expando_function_declaration(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        node: TransformNode,
    ) -> Result<bool, EmitResolverError> {
        let node = self.parse_node(arena, node)?;
        self.checker
            .emit_is_expando_function_declaration(node)
            .map_err(|abort| callback_abort_error(self.checker, self.method, Some(node), abort))
    }

    fn has_late_bindable_name(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        node: TransformNode,
    ) -> Result<bool, EmitResolverError> {
        let node = self.parse_node(arena, node)?;
        self.checker
            .has_late_bindable_name(node)
            .map_err(|abort| callback_abort_error(self.checker, self.method, Some(node), abort))
    }

    fn should_remove_declaration(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        context: &mut NodeBuilderContext<'_>,
        node: TransformNode,
    ) -> Result<bool, EmitResolverError> {
        if context.internal_flags.0 & ALLOW_UNRESOLVED_NAMES == 0 {
            return Ok(true);
        }
        let node = self.parse_node(arena, node)?;
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
        _arena: &mut tsc_emitter::TransformArena,
        context: &mut NodeBuilderContext<'_>,
    ) -> Result<SyntacticRecoveryBoundary, EmitResolverError> {
        Ok(SyntacticRecoveryBoundary::new(context))
    }

    fn is_definitely_reference_to_global_symbol_object(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        node: TransformNode,
    ) -> Result<bool, EmitResolverError> {
        let node = self.parse_node(arena, node)?;
        self.checker
            .emit_is_definitely_reference_to_global_symbol_object(node)
            .map_err(|abort| callback_abort_error(self.checker, self.method, Some(node), abort))
    }

    /// tsc-port: getAllAccessorDeclarationsForDeclaration @6.0.3
    /// tsc-hash: 794fe073022a3aebb21778d82647171a1213ce230d4575e61de2a3f44b2741c7
    /// tsc-span: _tsc.js:88367-88381
    fn get_all_accessor_declarations(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        accessor: TransformNode,
    ) -> Result<SyntacticAccessorDeclarations, EmitResolverError> {
        let accessor_node = self.parse_node(arena, accessor)?;
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
            .map(|other| self.project_parse_node(arena, other))
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
        arena: &mut tsc_emitter::TransformArena,
        declaration: TransformNode,
        symbol: Option<SyntacticSymbol>,
        enclosing_declaration: Option<NodeId>,
    ) -> Result<bool, EmitResolverError> {
        let declaration = self.parse_node(arena, declaration)?;
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
        arena: &mut tsc_emitter::TransformArena,
        parameter: TransformNode,
    ) -> Result<bool, EmitResolverError> {
        let parameter = self.parse_node(arena, parameter)?;
        self.checker
            .emit_is_optional_parameter(parameter)
            .map_err(|abort| {
                callback_abort_error(self.checker, self.method, Some(parameter), abort)
            })
    }

    fn is_undefined_identifier_expression(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        node: TransformNode,
    ) -> Result<bool, EmitResolverError> {
        let node = self.parse_node(arena, node)?;
        self.checker
            .get_resolved_symbol(node)
            .map(|symbol| symbol == Some(self.checker.undefined_symbol))
            .map_err(|abort| callback_abort_error(self.checker, self.method, Some(node), abort))
    }

    fn is_entity_name_visible(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        context: &mut NodeBuilderContext<'_>,
        entity_name: TransformNode,
        should_compute_aliases_to_make_visible: bool,
    ) -> Result<EmitSymbolAccessibilityResult, EmitResolverError> {
        let entity_name = self.parse_node(arena, entity_name)?;
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
            if let Some(clone) = {
                let result = builder
                    .try_reuse_existing_type_node(self, arena, target, context, type_node)?;
                into_target(arena, target, result)
            }? {
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
        if r#type == self.checker.tables.intrinsics.error {
            if let Some(recovered) =
                recover_suppressed_import_call_return_type(self.checker, declaration)
            {
                r#type = self
                    .checker
                    .instantiate_type(recovered, context.mapper)
                    .map_err(|abort| checker_abort_error(self.checker, context, abort))?;
            }
        }
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
            let accessibility = self.is_symbol_accessible_with_error_names(
                arena,
                symbol,
                enclosing,
                context.enclosing_declaration_is_synthetic,
                // isValueSymbolAccessible checks the plain Value face;
                // Value|ExportValue is only used below to spell the emitted
                // expression chain.
                EmitSymbolMeaning(SymbolFlags::VALUE.bits() as u32),
                false,
            )?;
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
            // checker.ts:serializeTypeName selects SymbolFlags.Value, then
            // passes that same face to isSymbolAccessible/symbolToTypeNode.
            EmitSymbolMeaning(SymbolFlags::VALUE.bits() as u32)
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
            let accessible = self.is_symbol_accessible_with_error_names(
                arena,
                symbol,
                enclosing,
                context.enclosing_declaration_is_synthetic,
                meaning,
                false,
            )?;
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
        arena: &mut TransformArena,
        target: TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        node: TransformNode,
    ) -> Result<SyntacticScopeCleanup, EmitResolverError> {
        let node = self.parse_node(arena, node)?;
        let mut cleanup = SyntacticScopeCleanup::capture(context);
        if node_util::is_function_like_kind(self.checker.kind_of(node))
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
                None,
                None,
                None,
            );
            if context.enclosing_declaration_is_synthetic {
                for &parameter in &signature.parameters {
                    for (name, symbol) in parameter_scope_symbols(self.checker, parameter) {
                        let old_symbol = context
                            .synthetic_scope_locals
                            .as_ref()
                            .and_then(|locals| locals.get(&name).copied());
                        cleanup.record_parameter_local(&name, old_symbol);
                        context
                            .synthetic_scope_locals
                            .get_or_insert_with(std::collections::HashMap::new)
                            .insert(name, symbol);
                    }
                }
            }
            if context
                .flags
                .contains(EmitNodeBuilderFlags::GENERATE_NAMES_FOR_SHADOWED_TYPE_PARAMS)
            {
                for &type_parameter in signature.type_parameters.as_deref().unwrap_or_default() {
                    let name = super::type_parameter_to_name(
                        self.checker,
                        arena,
                        target,
                        type_parameter,
                        context,
                    )?;
                    if let (Some(symbol), NodeData::Identifier(data)) = (
                        self.checker.tables.type_of(type_parameter).symbol,
                        &arena.node(name).map_err(factory_error)?.data,
                    ) {
                        let old_symbol = context
                            .synthetic_scope_locals
                            .as_ref()
                            .and_then(|locals| locals.get(&data.escaped_text).copied());
                        cleanup.record_type_parameter_local(&data.escaped_text, old_symbol);
                        context
                            .synthetic_scope_locals
                            .get_or_insert_with(std::collections::HashMap::new)
                            .insert(data.escaped_text.clone(), symbol);
                    }
                }
                if context.enclosing_declaration.is_some()
                    && signature
                        .type_parameters
                        .as_ref()
                        .is_some_and(|parameters| !parameters.is_empty())
                {
                    context.enclosing_declaration_is_synthetic = true;
                }
            }
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
            let _restore = super::enter_new_scope(context, Some(node), None, None, None, None);
            if context
                .flags
                .contains(EmitNodeBuilderFlags::GENERATE_NAMES_FOR_SHADOWED_TYPE_PARAMS)
            {
                for &type_parameter in &type_parameters {
                    let name = super::type_parameter_to_name(
                        self.checker,
                        arena,
                        target,
                        type_parameter,
                        context,
                    )?;
                    if let (Some(symbol), NodeData::Identifier(data)) = (
                        self.checker.tables.type_of(type_parameter).symbol,
                        &arena.node(name).map_err(factory_error)?.data,
                    ) {
                        let old_symbol = context
                            .synthetic_scope_locals
                            .as_ref()
                            .and_then(|locals| locals.get(&data.escaped_text).copied());
                        cleanup.record_type_parameter_local(&data.escaped_text, old_symbol);
                        context
                            .synthetic_scope_locals
                            .get_or_insert_with(std::collections::HashMap::new)
                            .insert(data.escaped_text.clone(), symbol);
                    }
                }
                if context.enclosing_declaration.is_some() && !type_parameters.is_empty() {
                    context.enclosing_declaration_is_synthetic = true;
                }
            }
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
        target: TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        node: TransformNode,
    ) -> Result<SyntacticTrackedEntityName, EmitResolverError> {
        // h2-7b-m-2 #4e: the tracked name is handed back in the source the
        // builder is rebuilding (a reused annotation from another file keeps
        // its children in one arena table; same source = identity).
        let tracked = self.track_existing_entity_name_in_source(arena, context, node)?;
        let node = if tracked.node.source() == target {
            tracked.node
        } else {
            arena
                .factory()
                .clone_node_to_source(tracked.node, target)
                .map_err(factory_error)?
        };
        Ok(SyntacticTrackedEntityName {
            node,
            introduces_error: tracked.introduces_error,
        })
    }

    fn track_computed_name(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        context: &mut NodeBuilderContext<'_>,
        access_expression: TransformNode,
    ) -> Result<(), EmitResolverError> {
        let access_expression = self.parse_node(arena, access_expression)?;
        track_computed_name(self.checker, access_expression, context)
    }

    fn get_module_specifier_override(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        context: &mut NodeBuilderContext<'_>,
        parent: TransformNode,
        literal: TransformNode,
    ) -> Result<Option<String>, EmitResolverError> {
        get_module_specifier_override(self.checker, arena, context, parent, literal)
    }

    fn can_reuse_type_node(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        context: &mut NodeBuilderContext<'_>,
        type_node: TransformNode,
    ) -> Result<bool, EmitResolverError> {
        let type_node = self.parse_node(arena, type_node)?;
        self.can_reuse_type_node_parse(context, type_node)
    }

    /// tsc-port: syntacticBuilderResolver.canReuseTypeNodeAnnotation @6.0.3
    /// tsc-hash: edfd54626c63d3d1645a16cfcad8561dab1388e09a7278579ada789709becc6d
    /// tsc-span: _tsc.js:50932-50955
    fn can_reuse_type_node_annotation(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
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
        let node = self.parse_node(arena, node)?;
        let existing = self.parse_node(arena, existing)?;
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
        // addOptionality(annotationType, !isParameter(node)) @6.0.3
        // (_tsc.js:56029-56031): `strictNullChecks && isOptional ?
        // getOptionalType(type, isProperty) : type` — the strictNullChecks
        // gate lives INSIDE addOptionality, so a mapped-type optional
        // property that requires undefined under `strictNullChecks: false`
        // compares its annotation as-is (getOptionalType asserts
        // strictNullChecks; h2-7b-m-2 fence amendment #4).
        if requires_adding_undefined == Some(true) && self.checker.tables.strict_null_checks {
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
#[path = "../../tests/unit/node_builder_serialize/tests.rs"]
mod tests;

use tsc_binder::{node_util, SymbolId};
use tsc_emitter::{
    EmitFunctionProperty, EmitInternalNodeBuilderFlags, EmitNodeBuilderFlags, EmitResolverError,
    EmitResolverMethod, EmitResolverNode, EmitSymbolAccessibility, EmitSymbolAccessibilityResult,
    EmitSymbolExpansionOut, EmitSymbolMeaning, EmitSymbolTracker, EmitTrackerAccess,
    EmitTrackerNode, EmitTrackerNodeDescription, EmitTrackerSymbol, EmitTrackerSymbolDescription,
    SourceFileId, TransformArena, TransformNode, TransformNodeArray, TransformSourceId,
};
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{ObjectFlags, SymbolFlags, TypeData, TypeFlags, TypeId};

use crate::pseudochecker::{could_already_refer_to_undefined_type, PseudoChecker, PseudoType};
use crate::state::{CheckAbort, CheckerState, IndexInfo, SignatureId};

use super::pseudo::{
    pseudo_return_type_matches_predicate, pseudo_type_equivalent_to_type,
    pseudo_type_to_node_with_checker_fallback, pseudo_type_to_type,
};
use super::signatures::{enter_signature_scope, exit_new_scope};

use super::signatures::{elide_initializer_and_set_emit_flags, parameter_scope_symbols};
use super::type_nodes::{
    checker_abort_error, clone_parameter_name_to_source, create_identifier, create_token,
    factory_error, project_parse_node, set_no_ascii_escaping, type_to_type_node_helper,
    BuildResult,
};
use super::{
    add_symbol_type_to_context, chains_symbol_to_entity_name_node, chains_symbol_to_type_node,
    existing_type_node_is_not_reference_or_is_reference_with_compatible_type_argument_count,
    get_enclosing_declaration_ignoring_fake_scope, get_module_specifier_override,
    get_type_from_type_node2, index_info_to_index_signature_declaration_helper, restore_flags,
    restore_symbol_type_to_context, save_restore_flags, set_text_range2, symbol_to_node,
    type_predicate_to_type_predicate_node_helper, with_context,
    with_context_in_synthetic_module_scope, NodeBuilderContext, SyntacticBuilderResolver,
    SyntacticRecoveryBoundary, SyntacticScopeCleanup, SyntacticTrackedEntityName,
    SyntacticTypeNodeBuilder, SyntheticModuleScope,
};

const METHOD: EmitResolverMethod = EmitResolverMethod::CreateTypeOfDeclaration;
const ALLOW_UNRESOLVED_NAMES: u32 = 8;
const ALLOW_UNIQUE_ES_SYMBOL_TYPE: u32 = 1_048_576;
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

        let mut display = self.take_emit_display();
        let built = build_symbol_display_node(
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
        );
        self.restore_emit_display(display);
        built
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
            | SyntaxKind::CallExpression
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

fn node_modules_resolution_candidates(
    importer: &tsc_types::JsString,
    specifier: &tsc_types::JsString,
) -> Vec<tsc_types::JsString> {
    let importer = super::specifier::normalized_slashes(importer);
    let mut directory =
        importer
            .as_js()
            .rsplit_once("/")
            .map_or(tsc_types::JsStr::from("."), |(directory, _)| {
                if directory.is_empty() {
                    tsc_types::JsStr::from("/")
                } else {
                    directory
                }
            });
    let mut candidates = Vec::new();
    loop {
        let base = if directory == "/" {
            crate::concat_js(&[&"/node_modules/", &specifier])
        } else if directory == "." {
            crate::concat_js(&[&"node_modules/", &specifier])
        } else {
            crate::concat_js(&[&directory, &"/node_modules/", &specifier])
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
            candidates.push(crate::concat_js(&[&base, &suffix]));
        }
        if matches!(directory.as_str(), Some("/" | ".")) {
            break;
        }
        directory =
            directory
                .rsplit_once("/")
                .map_or(tsc_types::JsStr::from("."), |(parent, _)| {
                    if parent.is_empty() {
                        tsc_types::JsStr::from("/")
                    } else {
                        parent
                    }
                });
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
    if module_name.starts_with(".") || module_name.starts_with("/") {
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

/// The symbol an identifier names, for the recoveries below: the symbol the
/// check resolved for the node, otherwise a lookup that reports nothing and
/// caches nothing. The declaration may be described before its source is
/// checked, and a name that is not an identifier names no import.
fn symbol_named_without_report(checker: &mut CheckerState<'_>, name: NodeId) -> Option<SymbolId> {
    if checker.kind_of(name) != SyntaxKind::Identifier {
        return None;
    }
    checker.emit_get_referenced_value_symbol(name, false).ok()?
}

fn recover_suppressed_type_reference(
    checker: &mut CheckerState<'_>,
    type_node: NodeId,
) -> Option<TypeId> {
    let type_name = match checker.data_of(type_node) {
        NodeData::TypeReference(data) => data.type_name?,
        _ => return None,
    };
    let alias = symbol_named_without_report(checker, type_name)?;
    let target = recover_suppressed_import_target(checker, alias)?;
    checker.get_declared_type_of_symbol(target).ok()
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
    let alias = symbol_named_without_report(checker, expression)?;
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

fn is_accessor(checker: &CheckerState<'_>, node: NodeId) -> bool {
    matches!(
        checker.kind_of(node),
        SyntaxKind::GetAccessor | SyntaxKind::SetAccessor
    )
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
        return create_identifier(
            arena,
            target,
            checker
                .symbol_display_name(symbol)
                .as_str()
                .expect("parameter names have scalar identifier grammar"),
        );
    };
    match checker.kind_of(name) {
        SyntaxKind::Identifier => {
            let name = clone_parameter_name_to_source(checker, arena, target, name)?.unwrap_or(
                create_identifier(
                    arena,
                    target,
                    checker
                        .symbol_display_name(symbol)
                        .as_str()
                        .expect("parameter names have scalar identifier grammar"),
                )?,
            );
            Ok(set_no_ascii_escaping(arena, name))
        }
        SyntaxKind::QualifiedName => {
            let right = match checker.data_of(name) {
                NodeData::QualifiedName(data) => data.right,
                _ => None,
            };
            let name = right
                .map(|right| clone_parameter_name_to_source(checker, arena, target, right))
                .transpose()?
                .flatten()
                .unwrap_or(create_identifier(
                    arena,
                    target,
                    checker
                        .symbol_display_name(symbol)
                        .as_str()
                        .expect("parameter names have scalar identifier grammar"),
                )?);
            Ok(set_no_ascii_escaping(arena, name))
        }
        SyntaxKind::ArrayBindingPattern | SyntaxKind::ObjectBindingPattern => {
            elide_initializer_and_set_emit_flags(checker, arena, target, name, context)
        }
        _ => create_identifier(
            arena,
            target,
            checker
                .symbol_display_name(symbol)
                .as_str()
                .expect("parameter names have scalar identifier grammar"),
        ),
    }
}

/// tsgo `isActivelyExpanding` (nodebuilderimpl.go:229-233): type node reuse
/// is skipped while a hover expands named types.
fn is_actively_expanding(context: &NodeBuilderContext<'_>) -> bool {
    context.max_expansion_depth > 0 && context.depth < context.max_expansion_depth
}

/// tsgo `hasTypeAnnotation` (nodebuilderimpl.go:234-244): the declaration
/// has a written type (a type alias's type is not an annotation).
fn has_type_annotation(checker: &CheckerState<'_>, declaration: NodeId) -> bool {
    match checker.kind_of(declaration) {
        SyntaxKind::TypeAliasDeclaration | SyntaxKind::JSDocTypedefTag => false,
        kind if node_util::is_function_like_kind(kind) => {
            checker.effective_return_type_node(declaration).is_some()
        }
        SyntaxKind::PropertyAssignment
        | SyntaxKind::ShorthandPropertyAssignment
        | SyntaxKind::ExportAssignment
        | SyntaxKind::BinaryExpression => {
            checker.is_in_js_file(declaration) && checker.reparsed_type_node(declaration).is_some()
        }
        _ => checker
            .effective_type_annotation_node(declaration)
            .is_some(),
    }
}

/// tsgo `ast.IsVariableLike` (ast/utilities.go:3077-3084).
fn is_variable_like(checker: &CheckerState<'_>, node: NodeId) -> bool {
    matches!(
        checker.kind_of(node),
        SyntaxKind::BindingElement
            | SyntaxKind::EnumMember
            | SyntaxKind::Parameter
            | SyntaxKind::PropertyAssignment
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::PropertySignature
            | SyntaxKind::ShorthandPropertyAssignment
            | SyntaxKind::VariableDeclaration
    )
}

/// The declaration type serializeTypeForDeclaration writes when its caller
/// supplies none (nodebuilderimpl.go:2266-2285): the type in the enclosing
/// symbol types, else a set accessor's write type, else the symbol's
/// widened literal type.
fn declaration_type_for_serialization(
    checker: &mut CheckerState<'_>,
    context: &NodeBuilderContext<'_>,
    declaration: Option<NodeId>,
    symbol: Option<SymbolId>,
) -> BuildResult<TypeId> {
    let Some(symbol) = symbol else {
        return match declaration.filter(|&declaration| is_variable_like(checker, declaration)) {
            Some(declaration) => Ok(checker
                .get_type_for_variable_like_declaration(
                    declaration,
                    false,
                    tsc_types::CheckMode::NORMAL,
                )
                .map_err(|abort| checker_abort_error(checker, context, abort))?
                .unwrap_or(checker.tables.intrinsics.error)),
            None => Ok(checker.tables.intrinsics.error),
        };
    };
    if let Some(&r#type) = context.enclosing_symbol_types.get(&symbol) {
        return Ok(r#type);
    }
    let flags = checker.symbol_flags(symbol);
    let mut r#type = if flags.intersects(SymbolFlags::ACCESSOR)
        && declaration
            .is_some_and(|declaration| checker.kind_of(declaration) == SyntaxKind::SetAccessor)
    {
        let write = checker
            .get_write_type_of_symbol(symbol)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        checker
            .instantiate_type(write, context.mapper)
            .map_err(|abort| checker_abort_error(checker, context, abort))?
    } else if !flags.intersects(SymbolFlags::TYPE_LITERAL | SymbolFlags::SIGNATURE) {
        let symbol_type = checker
            .get_type_of_symbol(symbol)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        let widened = checker
            .get_widened_literal_type(symbol_type)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        checker
            .instantiate_type(widened, context.mapper)
            .map_err(|abort| checker_abort_error(checker, context, abort))?
    } else {
        checker.tables.intrinsics.error
    };
    if r#type == checker.tables.intrinsics.error {
        if let Some(recovered) = declaration.and_then(|declaration| {
            recover_suppressed_import_call_return_type(checker, declaration)
        }) {
            r#type = checker
                .instantiate_type(recovered, context.mapper)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
        }
    }
    Ok(r#type)
}

/// tsgo-port: NodeBuilderImpl.serializeTypeForDeclaration @7.1 (nodebuilderimpl.go:2246-2370).
///
/// Writes the type of a declaration: the type the caller supplies, or the
/// declaration's (`declaration_type_for_serialization`). With `try_reuse`,
/// the declaration's pseudo type is checked against that type and, when they
/// agree, builds the node (`pseudo.rs`); otherwise the checker's type is
/// serialized. A `unique symbol` type of the symbol itself is written as
/// `unique symbol`.
#[allow(clippy::too_many_arguments)]
pub(super) fn serialize_type_for_declaration_in_context(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    declaration: Option<NodeId>,
    r#type: Option<TypeId>,
    symbol: Option<SymbolId>,
    try_reuse: bool,
) -> BuildResult<TransformNode> {
    let declaration = declaration.or_else(|| {
        symbol.and_then(|symbol| {
            let data = checker.binder.symbol(symbol);
            // tsgo does not prefer an annotated declaration here yet.
            data.value_declaration
                .or_else(|| data.declarations.first().copied())
        })
    });
    // tsgo's getSymbolOfDeclaration is nil where tsc-rs answers the unknown
    // symbol, as for a variable named by a binding pattern.
    let symbol = match symbol {
        Some(symbol) => Some(symbol),
        None => declaration
            .map(|declaration| {
                checker
                    .get_symbol_of_declaration(declaration)
                    .map_err(|abort| checker_abort_error(checker, context, abort))
            })
            .transpose()?,
    }
    .filter(|&symbol| symbol != checker.unknown_symbol);
    let mut r#type = match r#type {
        Some(r#type) => r#type,
        None => declaration_type_for_serialization(checker, context, declaration, symbol)?,
    };

    let annotated_kind = declaration.is_some_and(|declaration| {
        matches!(
            checker.kind_of(declaration),
            SyntaxKind::Parameter | SyntaxKind::PropertySignature | SyntaxKind::PropertyDeclaration
        )
    });
    let requires_adding_undefined = match declaration.filter(|_| annotated_kind) {
        Some(declaration) => checker
            .emit_requires_adding_implicit_undefined_to_declaration(
                declaration,
                symbol,
                context.enclosing_declaration,
            )
            .map_err(|abort| checker_abort_error(checker, context, abort))?,
        None => false,
    };
    let add_undefined_for_parameter = requires_adding_undefined
        && declaration
            .is_some_and(|declaration| checker.kind_of(declaration) == SyntaxKind::Parameter);
    if add_undefined_for_parameter {
        r#type = checker
            .get_optional_type(r#type, false)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
    }

    let restore_flags_value = save_restore_flags(context);
    let type_data = checker.tables.type_of(r#type);
    if type_data.flags.intersects(TypeFlags::UNIQUE_ES_SYMBOL)
        && symbol.is_some()
        && type_data.symbol == symbol
        && (context.enclosing_declaration.is_none()
            || symbol.is_some_and(|symbol| {
                checker
                    .binder
                    .symbol(symbol)
                    .declarations
                    .iter()
                    .any(|&declaration| {
                        Some(checker.binder.source_of_node(declaration).root)
                            == context.enclosing_file
                    })
            }))
    {
        context.flags.0 |= ALLOW_UNIQUE_ES_SYMBOL_TYPE;
    }
    let result = (|| {
        let mut result = None;
        let mut reported_inference_fallback = false;
        // tsgo has no expandable hover reuse yet.
        if let Some(declaration) = declaration.filter(|&declaration| {
            !is_actively_expanding(context)
                && try_reuse
                && context.enclosing_declaration.is_some()
                && (is_accessor(checker, declaration)
                    || (has_inferred_type(checker, declaration)
                        && !node_is_synthesized(checker, declaration)
                        && !checker
                            .tables
                            .object_flags_of(r#type)
                            .intersects(ObjectFlags::REQUIRES_WIDENING)))
        }) {
            let restore = symbol.map(|symbol| add_symbol_type_to_context(context, symbol, r#type));
            let reused = (|| {
                let mut pseudo = {
                    let pseudochecker = PseudoChecker::new(checker);
                    if is_accessor(checker, declaration) {
                        pseudochecker.get_type_of_accessor(declaration)
                    } else {
                        pseudochecker.get_type_of_declaration(declaration)
                    }
                };
                // Binary expressions annotate first-in-wins: the first one with
                // an annotation types the rest.
                if matches!(pseudo, PseudoType::NoResult(_))
                    && checker.kind_of(declaration) == SyntaxKind::BinaryExpression
                {
                    if let Some(annotated) = symbol.and_then(|symbol| {
                        checker
                            .binder
                            .symbol(symbol)
                            .declarations
                            .iter()
                            .copied()
                            .find(|&other| has_type_annotation(checker, other))
                    }) {
                        pseudo = PseudoChecker::new(checker).get_type_of_declaration(annotated);
                    }
                }
                let report_errors = !context.suppress_report_inference_fallback;
                let is_optional_annotated = !requires_adding_undefined
                    && annotated_kind
                    && checker.is_optional_declaration(declaration);
                if pseudo_type_equivalent_to_type(
                    checker,
                    context,
                    &pseudo,
                    r#type,
                    is_optional_annotated,
                    report_errors,
                )? {
                    // A reference with too few type arguments should still be
                    // serialized (strada's canReuseTypeNodeAnnotation); tsgo
                    // does not do that yet.
                    if requires_adding_undefined && contains_non_missing_undefined(checker, r#type)
                    {
                        if let Some(pseudo_type) = pseudo_type_to_type(checker, context, &pseudo)? {
                            if !contains_non_missing_undefined(checker, pseudo_type) {
                                pseudo = PseudoType::Union(vec![pseudo, PseudoType::Undefined]);
                            }
                        }
                    }
                    return pseudo_type_to_node_with_checker_fallback(
                        checker, arena, target, context, &pseudo, r#type,
                    )
                    .map(Some);
                }
                // The equivalence failed. Errors reported for an `Inferred`
                // pseudo type with error nodes suppress the nested errors of
                // the fallback serialization, as
                // pseudoTypeToNodeWithCheckerFallback does.
                reported_inference_fallback = report_errors
                    && matches!(&pseudo, PseudoType::Inferred(inferred) if !inferred.error_nodes.is_empty());
                let should_add_undefined = requires_adding_undefined
                    && match pseudo_type_to_type(checker, context, &pseudo)? {
                        Some(pseudo_type) => !contains_non_missing_undefined(checker, pseudo_type),
                        None => !could_already_refer_to_undefined_type(checker, &pseudo),
                    };
                if should_add_undefined {
                    let pseudo = PseudoType::Union(vec![pseudo, PseudoType::Undefined]);
                    if pseudo_type_equivalent_to_type(
                        checker,
                        context,
                        &pseudo,
                        r#type,
                        false,
                        report_errors,
                    )? {
                        reported_inference_fallback = false;
                        return pseudo_type_to_node_with_checker_fallback(
                            checker, arena, target, context, &pseudo, r#type,
                        )
                        .map(Some);
                    }
                }
                Ok(None)
            })();
            if let Some(restore) = restore {
                restore_symbol_type_to_context(context, restore);
            }
            result = reused?;
        }
        if result.is_none() {
            let old_suppress = context.suppress_report_inference_fallback;
            if reported_inference_fallback {
                context.suppress_report_inference_fallback = true;
            }
            let serialized = type_to_type_node_helper(checker, arena, target, r#type, context);
            context.suppress_report_inference_fallback = old_suppress;
            result = serialized?;
        }
        Ok(result)
    })();
    restore_flags(context, restore_flags_value);
    match result? {
        Some(result) => Ok(result),
        None => create_token(arena, target, SyntaxKind::AnyKeyword),
    }
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

/// tsgo-port: NodeBuilderImpl.serializeReturnTypeForSignature @7.1 (nodebuilderimpl.go:2092-2148).
///
/// Writes a signature's return type: the declaration's type in the
/// enclosing symbol types, else the signature's return type. With
/// `try_reuse`, the declaration's pseudo return type is checked against it
/// and, when they agree (an inferred type predicate included), builds the
/// node. A top-level `any` is left out under `SuppressAnyReturnType`.
pub(super) fn serialize_return_type_for_signature_in_context(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    signature: SignatureId,
    try_reuse: bool,
) -> BuildResult<Option<TransformNode>> {
    let suppress_any = context
        .flags
        .contains(EmitNodeBuilderFlags::SUPPRESS_ANY_RETURN_TYPE);
    let restore_flags_value = save_restore_flags(context);
    if suppress_any {
        // Only a top-level `any` is suppressed.
        context.flags.0 &= !EmitNodeBuilderFlags::SUPPRESS_ANY_RETURN_TYPE.0;
    }
    let result = (|| {
        let declaration = checker
            .signature_of(signature)
            .declaration
            .filter(|&declaration| !node_is_synthesized(checker, declaration));
        let return_type = match declaration {
            Some(declaration) => {
                let symbol = checker
                    .get_symbol_of_declaration(declaration)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?;
                match context.enclosing_symbol_types.get(&symbol).copied() {
                    Some(return_type) => return_type,
                    None => {
                        let return_type = checker
                            .get_return_type_of_signature(signature)
                            .map_err(|abort| checker_abort_error(checker, context, abort))?;
                        checker
                            .instantiate_type(return_type, context.mapper)
                            .map_err(|abort| checker_abort_error(checker, context, abort))?
                    }
                }
            }
            None => checker
                .get_return_type_of_signature(signature)
                .map_err(|abort| checker_abort_error(checker, context, abort))?,
        };
        let mut return_type_node = None;
        if !(suppress_any
            && checker
                .tables
                .flags_of(return_type)
                .intersects(TypeFlags::ANY))
        {
            if let Some(declaration) = declaration.filter(|_| {
                !is_actively_expanding(context)
                    && try_reuse
                    && context.enclosing_declaration.is_some()
            }) {
                let declaration_symbol = checker
                    .get_symbol_of_declaration(declaration)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?;
                let restore = add_symbol_type_to_context(context, declaration_symbol, return_type);
                let reused = (|| {
                    let pseudo =
                        PseudoChecker::new(checker).get_return_type_of_signature(declaration);
                    let report_errors = !context.suppress_report_inference_fallback;
                    if !pseudo_type_equivalent_to_type(
                        checker,
                        context,
                        &pseudo,
                        return_type,
                        false,
                        report_errors,
                    )? {
                        return Ok(None);
                    }
                    // The pseudochecker does not know inferred type
                    // predicates: it reads `boolean` where the checker infers
                    // `x is string`.
                    let predicate = checker
                        .get_type_predicate_of_signature(signature)
                        .map_err(|abort| checker_abort_error(checker, context, abort))?;
                    if let Some(predicate) = predicate {
                        if !pseudo_return_type_matches_predicate(
                            checker, context, &pseudo, &predicate,
                        )? {
                            if report_errors {
                                report_inference_fallback(checker, context, declaration)?;
                            }
                            return Ok(None);
                        }
                    }
                    // A reference with too few type arguments should still be
                    // serialized (strada's canReuseTypeNodeAnnotation); tsgo
                    // does not do that yet.
                    pseudo_type_to_node_with_checker_fallback(
                        checker,
                        arena,
                        target,
                        context,
                        &pseudo,
                        return_type,
                    )
                    .map(Some)
                })();
                restore_symbol_type_to_context(context, restore);
                return_type_node = reused?;
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

/// tsgo serializeTypeForDeclaration for a declaration without a symbol, such
/// as a variable named by a binding pattern (nodebuilderimpl.go:2263-2272):
/// the caller supplies getTypeForVariableLikeDeclaration's type.
#[allow(clippy::too_many_arguments)]
pub(crate) fn serialize_type_for_symbolless_declaration(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    declaration: NodeId,
    r#type: TypeId,
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
        let result = serialize_type_for_declaration_in_context(
            checker,
            arena,
            target,
            context,
            Some(declaration),
            Some(r#type),
            None,
            true,
        )?;
        into_target(arena, target, Some(result))
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

/// tsgo `serializeTypeForDeclaration(declaration, type, symbol, true)` for
/// the node builder's parameters and properties (nodebuilderimpl.go:1730,
/// 2697).
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
        Some(r#type),
        symbol,
        true,
    )?;
    into_target(arena, target, Some(result))
}

/// tsgo `serializeReturnTypeForSignature(signature, true)` for the node
/// builder's signatures (nodebuilderimpl.go:1900), moved into the target
/// source.
pub(crate) fn serialize_return_type_for_signature_seam(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    signature: SignatureId,
) -> BuildResult<Option<TransformNode>> {
    let result = serialize_return_type_for_signature_in_context(
        checker, arena, target, context, signature, true,
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

/// tsgo-port: NodeBuilder.TryJSTypeNodeToTypeNode @7.1
/// (checker/nodebuilder.go:272-275, nodecopy.go:12-22): reuse a JSDoc type
/// node in a fresh node-builder context.
#[allow(clippy::too_many_arguments)]
pub(crate) fn try_js_type_node_to_type_node(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    type_node: NodeId,
    enclosing_declaration: NodeId,
    flags: EmitNodeBuilderFlags,
    internal_flags: EmitInternalNodeBuilderFlags,
    tracker: &mut dyn EmitSymbolTracker,
) -> BuildResult<Option<TransformNode>> {
    with_context(
        checker,
        arena,
        target,
        Some(enclosing_declaration),
        Some(flags),
        Some(internal_flags),
        Some(tracker),
        None,
        None,
        |checker, arena, target, context| {
            syntactic_try_reuse_existing_type_node(checker, arena, target, context, type_node)
        },
        None,
    )
    .map(Option::flatten)
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
    let builder = SyntacticTypeNodeBuilder;
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

/// tsgo-port: NodeBuilderImpl.reuseNode @7.1 (nodecopy.go:12-18, 222-232):
/// the existing-node visitor under a recovery boundary, without strada's
/// canReuseTypeNode gate.
pub(crate) fn syntactic_try_reuse_existing_node(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    node: NodeId,
) -> BuildResult<Option<TransformNode>> {
    let Some(node) = project_parse_node(checker, arena, node)? else {
        return Ok(None);
    };
    let builder = SyntacticTypeNodeBuilder;
    let mut resolver = ProductionSyntacticBuilderResolver::new(checker, METHOD);
    {
        let result =
            builder.try_reuse_existing_node(&mut resolver, arena, target, context, node)?;
        into_target(arena, target, result)
    }
}

/// tsgo-port: SymbolTrackerImpl.ReportInferenceFallback @7.1
/// (symboltracker.go:110-115): the node builder's report, which neither
/// checks suppressReportInferenceFallback (its callers do) nor marks a
/// reported diagnostic.
pub(super) fn report_inference_fallback(
    checker: &mut CheckerState<'_>,
    context: &mut NodeBuilderContext<'_>,
    node: NodeId,
) -> BuildResult<()> {
    let mut resolver = ProductionSyntacticBuilderResolver::new(checker, METHOD);
    let mut reported_diagnostic = false;
    context
        .tracker
        .report_inference_fallback(&mut reported_diagnostic, false, &mut resolver, node)
}

/// The written name of a parameter declaration for the node builder's
/// parameters (tsgo parameterToParameterDeclarationName,
/// nodebuilderimpl.go:1763).
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

/// tsgo-port: NodeBuilder.SerializeTypeForDeclaration @7.1 (nodebuilder.go:133-137).
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
        let result = serialize_type_for_declaration_in_context(
            checker,
            arena,
            target,
            context,
            Some(declaration),
            None,
            Some(symbol),
            true,
        )?;
        into_target(arena, target, Some(result))
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

/// tsgo-port: NodeBuilder.SerializeReturnTypeForSignature @7.1 (nodebuilder.go:116-124):
/// the return type of a signature declaration, in the signature's scope.
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
            let signature = checker
                .get_signature_from_declaration(signature_declaration)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            let (_, scope) = enter_signature_scope(checker, arena, target, context, signature)?;
            let result = serialize_return_type_for_signature_in_context(
                checker, arena, target, context, signature, true,
            );
            exit_new_scope(context, scope);
            into_target(arena, target, result?)
        },
        None,
    )
    .map(Option::flatten)
}

/// tsgo-port: NodeBuilder.SerializeTypeForExpression @7.1
/// (nodebuilder.go:139-143): the expression's type in a new context.
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
            let result = serialize_type_for_expression_in_context(
                checker, arena, target, context, expression,
            );
            into_target(arena, target, result?)
        },
        None,
    )
    .map(Option::flatten)
}

/// tsgo-port: NodeBuilderImpl.serializeTypeForExpression @7.1
/// (nodebuilderimpl.go:1811-1815): the widened regular type of the
/// expression, instantiated by the context's mapper (tsgo's shim, without
/// node reuse).
fn serialize_type_for_expression_in_context(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    expression: NodeId,
) -> BuildResult<Option<TransformNode>> {
    let regular = get_regular_type_of_expression(checker, context, expression)?;
    let widened = checker
        .get_widened_type(regular)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    let instantiated = checker
        .instantiate_type(widened, context.mapper)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    type_to_type_node_helper(checker, arena, target, instantiated, context)
}

/// tsgo-port: Checker.getRegularTypeOfExpression @7.1 (checker.go:32603-32608):
/// the regular type of the expression, or of its parent when it is the right
/// side of a qualified name or property access.
pub(super) fn get_regular_type_of_expression(
    checker: &mut CheckerState<'_>,
    context: &NodeBuilderContext<'_>,
    mut expression: NodeId,
) -> BuildResult<TypeId> {
    if let Some(parent) = checker.parent_of(expression) {
        let is_right_side = matches!(
            checker.data_of(parent),
            NodeData::QualifiedName(data) if data.right == Some(expression)
        ) || matches!(
            checker.data_of(parent),
            NodeData::PropertyAccessExpression(data) if data.name == Some(expression)
        );
        if is_right_side {
            expression = parent;
        }
    }
    let r#type = checker
        .get_type_of_expression(expression)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    Ok(checker.regular_type_of_literal_type(r#type))
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
            .map(SymbolId::new)
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
        error_module_name: tsc_types::JsStr<'_>,
    ) -> BuildResult<Option<SymbolId>> {
        let mut parent = self.checker.binder.symbol(symbol).parent;
        while let Some(candidate) = parent {
            if self.checker.symbol_display_name(candidate).as_js() == error_module_name {
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
                if self.checker.symbol_display_name(candidate).as_js() == error_module_name {
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
        let mut result = self
            .checker
            .emit_is_symbol_accessible(access_symbol, enclosing, meaning, should_compute_aliases)
            .map_err(|abort| {
                callback_abort_error(self.checker, self.method, Some(enclosing), abort)
            })?;
        // tsgo's signature fake scope is a synthesized block, which is not in
        // a JavaScript file, so a name that cannot be named from it carries no
        // error node and the diagnostic context places the error
        // (nodebuilderscopes.go:142-147, symbolaccessibility.go:861).
        if enclosing_is_synthetic {
            result.error_node = None;
        }
        if result.error_symbol_name.is_some() {
            self.build_accessibility_error_name(
                arena,
                symbol,
                enclosing,
                enclosing_is_synthetic,
                meaning,
            )?;
        }
        if let Some(error_module_name) = result
            .error_module_name
            .as_ref()
            .map(tsc_types::JsString::as_js)
        {
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
    fn is_entity_in_type_node(
        &mut self,
        node: tsc_emitter::EmitTrackerNode,
    ) -> Result<bool, tsc_emitter::EmitResolverError> {
        let node = self
            .tracker_node(node)
            .ok_or_else(|| self.invalid_token_error(None))?;
        Ok(super::tracker_is_entity_in_type_node(self.checker, node))
    }

    fn is_child_of_bound_expando(
        &mut self,
        node: tsc_emitter::EmitTrackerNode,
    ) -> Result<bool, tsc_emitter::EmitResolverError> {
        let node = self
            .tracker_node(node)
            .ok_or_else(|| self.invalid_token_error(None))?;
        super::tracker_is_child_of_bound_expando(self.checker, node)
            .map_err(|abort| callback_abort_error(self.checker, self.method, Some(node), abort))
    }

    fn accessor_declarations(
        &mut self,
        node: tsc_emitter::EmitTrackerNode,
    ) -> Result<tsc_emitter::EmitAccessorDeclarations, tsc_emitter::EmitResolverError> {
        let node = self
            .tracker_node(node)
            .ok_or_else(|| self.invalid_token_error(None))?;
        super::tracker_accessor_declarations(self.checker, node)
            .ok_or_else(|| self.invalid_token_error(None))
    }

    fn parent_node(
        &mut self,
        node: EmitTrackerNode,
    ) -> Result<Option<EmitTrackerNode>, EmitResolverError> {
        let node = self
            .tracker_node(node)
            .ok_or_else(|| self.invalid_token_error(None))?;
        Ok(self
            .checker
            .parent_of(node)
            .map(|parent| EmitTrackerNode(u64::from(parent.index()))))
    }

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
            return Ok(self.checker.accessible_result());
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
            escaped_name: data.escaped_name.as_js().to_owned(),
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
            // The fake scope's locals answer a lookup only for a matching
            // meaning (`getSymbol`: `symbol.flags & meaning`, or an alias
            // whose target has it): a parameter `Model` or a type parameter
            // `Rpc` does not hide the namespace in `Model.Any` or `Rpc.Any`.
            let fake_scope_symbol = match fake_scope_symbol {
                Some(local) => {
                    let local_flags = self.checker.symbol_flags(local);
                    let matches = local_flags.intersects(flags)
                        || local_flags.intersects(SymbolFlags::ALIAS)
                            && self
                                .checker
                                .get_symbol_flags_of(local)
                                .map_err(|abort| checker_abort_error(self.checker, context, abort))?
                                .intersects(flags);
                    matches.then_some(local)
                }
                None => None,
            };
            // Parameter locals in a synthesized signature scope are not in
            // scope for their own JSDoc type annotations. The parse-site
            // resolver can conservatively return that parameter in Rust;
            // recover the outer symbol that upstream resolved before it
            // installed the fake scope so the reference-mismatch arm fires.
            // Only a JSDoc-hosted reference is rejected upstream
            // (resolveNameHelper _tsc.js:19558, 19563-19568: `lastLocation`
            // is the JSDoc node). A reference in the parameter list or the
            // return type annotation (`(name: unknown): name is string`,
            // `typeof name`) keeps the parameter (`lastLocation.kind ===
            // Parameter || lastLocation === location.type`), so
            // trackExistingEntityName (_tsc.js:53555-53614) resolves the same
            // symbol at both sites regardless of a same-named outer binding
            // (DOM global `name`, an outer `const v`).
            if let (Some(original), Some(fake), Some(enclosing)) =
                (symbol, fake_scope_symbol, context.enclosing_declaration)
            {
                if original == fake
                    && self.checker.node_flags(leftmost) & tsc_types::NodeFlags::JS_DOC.bits() != 0
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
                // Upstream resolves from the fake scope Block whose parent is
                // the enclosing declaration (enterNewScope _tsc.js:52728-52730),
                // so after the fake locals (handled by `fake_scope_symbol`
                // above) the first real location is the enclosing declaration
                // ITSELF (resolveNameHelper _tsc.js:19553-19556: a namespace's
                // locals, a function's parameters). Hopping to its parent
                // skipped that scope (spurious mismatch + TS9039 for namespace
                // members and outer parameters).
                (true, None) => match context.enclosing_declaration {
                    Some(enclosing) => self
                        .checker
                        .resolve_entity_name_ex(leftmost, flags, true, Some(enclosing), true)
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
        let cloned = arena
            .factory()
            .clone_node_keeping_quote(node)
            .map_err(factory_error)?;
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

    /// tsgo-port: tryVisitTypeReference's gates @7.1 (nodecopy.go:416-435).
    ///
    /// A type reference is rebuilt from its type instead of reused when it
    /// is a `const` reference, names a type parameter the context's mapper
    /// replaces, or is a JavaScript reference whose written form does not
    /// say its type (`canReuseExistingJSTypeNode`). Unlike strada's
    /// canReuseTypeNode, a reference whose type the mapper changes is still
    /// reused: the visitor replaces only the type parameters inside it.
    fn can_reuse_type_reference_parse(
        &mut self,
        context: &mut NodeBuilderContext<'_>,
        existing: NodeId,
    ) -> BuildResult<bool> {
        if self.checker.is_const_type_reference_node(existing) {
            return Ok(false);
        }
        let declared_type = self
            .checker
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
            .type_of(declared_type)
            .flags
            .intersects(TypeFlags::TYPE_PARAMETER);
        if symbol.is_some_and(|symbol| {
            self.checker
                .symbol_flags(symbol)
                .intersects(SymbolFlags::TYPE_PARAMETER)
        }) || type_is_type_parameter
        {
            let declared = symbol.map_or(declared_type, |symbol| {
                self.checker.get_declared_type_of_type_parameter(symbol)
            });
            if let Some(mapper) = context.mapper {
                let mapped = self
                    .checker
                    .get_mapped_type(declared, mapper)
                    .map_err(|abort| checker_abort_error(self.checker, context, abort))?;
                if mapped != declared {
                    return Ok(false);
                }
            }
        }
        let Some(r#type) = get_type_from_type_node2(self.checker, context, existing, false)? else {
            return Ok(false);
        };
        if self.checker.is_jsdoc_type_reference(existing)
            && self
                .checker
                .get_intended_type_from_jsdoc_type_reference(existing)
                .map_err(|abort| checker_abort_error(self.checker, context, abort))?
                .is_some()
        {
            return Ok(false);
        }
        existing_type_node_is_not_reference_or_is_reference_with_compatible_type_argument_count(
            self.checker,
            existing,
            r#type,
            context,
        )
    }

    /// tsc-port: canReuseTypeNode @6.0.3
    /// tsc-hash: af141a7d202b5ffec61fda03caf2df8dbc2cd77d7eea3dc4e40383975bf30673
    /// tsc-span: _tsc.js:53675-53711
    fn can_reuse_type_node_parse(
        &mut self,
        context: &mut NodeBuilderContext<'_>,
        existing: NodeId,
    ) -> BuildResult<bool> {
        if self.checker.kind_of(existing) == SyntaxKind::TypeReference {
            return self.can_reuse_type_reference_parse(context, existing);
        }
        if get_type_from_type_node2(self.checker, context, existing, true)?.is_none() {
            return Ok(false);
        }
        // tsgo reuses a JavaScript import type whose type resolves, with no
        // check of the symbol it names (nodecopy.go:614-646).
        if self.checker.kind_of(existing) == SyntaxKind::ImportType {
            return Ok(true);
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

    /// tsc-port: serializeExistingTypeNode @6.0.3, without the addUndefined
    /// arm (the visitor passes false)
    /// tsc-hash: 433daa463f78335a63960c6658ccab7a037a667922af31e6eb4320cadafe30ff
    /// tsc-span: _tsc.js:53712-53721
    fn serialize_existing_type_node(
        &mut self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        type_node: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let type_node = arena
            .require_parse_tree_resolver_node(type_node)
            .map_err(factory_error)?
            .node();
        let Some(r#type) = get_type_from_type_node2(self.checker, context, type_node, false)?
        else {
            return Ok(None);
        };
        type_to_type_node_helper(self.checker, arena, target, r#type, context)
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

    fn enter_new_scope(
        &mut self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        node: TransformNode,
    ) -> Result<SyntacticScopeCleanup, EmitResolverError> {
        let node = self.parse_node(arena, node)?;
        let cleanup = SyntacticScopeCleanup::capture(context);
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
                        context
                            .synthetic_scope_locals
                            .get_or_insert_with(rustc_hash::FxHashMap::default)
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
                        context
                            .synthetic_scope_locals
                            .get_or_insert_with(rustc_hash::FxHashMap::default)
                            .insert(data.escaped_text, symbol);
                        if !context
                            .synthetic_type_param_names
                            .contains(&data.escaped_text)
                        {
                            context.synthetic_type_param_names.push(data.escaped_text);
                        }
                    }
                }
                if context.enclosing_declaration.is_some()
                    && signature
                        .type_parameters
                        .as_ref()
                        .is_some_and(|parameters| !parameters.is_empty())
                {
                    context.enclosing_declaration_is_synthetic = true;
                    context.synthetic_type_params_scope_active = true;
                    context.push_fake_type_params_scope();
                }
            }
        } else {
            // nodecopy.go:842-846: a conditional type's `infer` type
            // parameters, a mapped type's own type parameter.
            let type_parameters = if self.checker.kind_of(node) == SyntaxKind::ConditionalType {
                self.checker.get_infer_type_parameters(node)
            } else {
                match self.checker.data_of(node) {
                    NodeData::MappedType(data) => data
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
                        context
                            .synthetic_scope_locals
                            .get_or_insert_with(rustc_hash::FxHashMap::default)
                            .insert(data.escaped_text, symbol);
                        if !context
                            .synthetic_type_param_names
                            .contains(&data.escaped_text)
                        {
                            context.synthetic_type_param_names.push(data.escaped_text);
                        }
                    }
                }
                if context.enclosing_declaration.is_some() && !type_parameters.is_empty() {
                    context.enclosing_declaration_is_synthetic = true;
                    context.synthetic_type_params_scope_active = true;
                    context.push_fake_type_params_scope();
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

    fn get_module_specifier_override(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        context: &mut NodeBuilderContext<'_>,
        parent: TransformNode,
        literal: TransformNode,
    ) -> Result<Option<tsc_types::JsString>, EmitResolverError> {
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
}

#[cfg(test)]
#[path = "../../tests/unit/node_builder_serialize/tests.rs"]
mod tests;

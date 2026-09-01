use std::collections::{HashMap, HashSet};

use tsc_binder::{node_util, SymbolId, SymbolTable};
use tsc_emitter::{
    EmitFlags, EmitInternalNodeBuilderFlags, EmitNodeBuilderFlags, EmitResolverError,
    EmitSymbolAccessibility, EmitSymbolExpansionOut, EmitSymbolMeaning, SyntheticComment,
    SyntheticCommentKind, TransformArena, TransformNode, TransformSourceId,
};
use tsc_syntax::nodes::{
    ClassDeclarationData, ConstructorData, EmptyStatementData, EnumDeclarationData, EnumMemberData,
    ExportAssignmentData, ExportDeclarationData, ExportSpecifierData, ExpressionStatementData,
    ExpressionWithTypeArgumentsData, ExternalModuleReferenceData, GetAccessorData,
    HeritageClauseData, ImportClauseData, ImportDeclarationData, ImportEqualsDeclarationData,
    ImportSpecifierData, InterfaceDeclarationData, MethodSignatureData, ModuleBlockData,
    ModuleDeclarationData, NamedExportsData, NamedImportsData, NamespaceExportData,
    NamespaceExportDeclarationData, NamespaceImportData, NumericLiteralData, ParameterData,
    PrivateIdentifierData, PropertyDeclarationData, PropertySignatureData, SetAccessorData,
    StringLiteralData, TypeAliasDeclarationData, VariableDeclarationData,
    VariableDeclarationListData, VariableStatementData,
};
use tsc_syntax::{NodeArrayId, NodeData, NodeId, SyntaxKind};
use tsc_types::{
    IntersectionFlags, ModifierFlags, NodeFlags, ObjectFlags, SymbolFlags, TypeData, TypeFlags,
    TypeId,
};

use crate::evaluate::EvalValue;
use crate::state::{CheckerState, IndexInfo, SignatureId, SignatureKind};

use super::{
    add_approximate_length, chains_get_property_name_node_for_symbol,
    chains_symbol_to_entity_name_node, chains_symbol_to_expression,
    check_truncation_length_if_expanding, checker_abort_error, clone_node_builder_context,
    clone_parse_node, create_identifier, create_node, create_node_array, create_token,
    factory_error, get_declaration_with_type_annotation,
    index_info_to_index_signature_declaration_helper, project_parse_node,
    restore_cloned_node_builder_context, restore_flags, save_restore_flags,
    serialize_type_for_declaration_seam, set_text_range2,
    signature_to_signature_declaration_helper, specifier_for_module_symbol,
    type_parameter_to_declaration, type_to_type_node_helper, with_context, BuildResult,
    NodeBuilderContext, SignatureDeclarationOptions,
};

const ALLOW_ANONYMOUS_IDENTIFIER: u32 = 131_072;
const IN_TYPE_ALIAS: u32 = 8_388_608;
const IN_INITIAL_ENTITY_NAME: u32 = 16_777_216;

// h2-7a-m-3: every create_/update_ helper below is the statement-level
// NodeData spelling of an upstream factory face. Typed constructor and
// parenthesizer faces remain owned by m-3.5; the shared arena factory still
// validates children and applies its already-landed generic grammar rules.

fn array(
    arena: &mut TransformArena,
    target: TransformSourceId,
    nodes: Vec<TransformNode>,
) -> BuildResult<Option<NodeArrayId>> {
    (!nodes.is_empty())
        .then(|| create_node_array(arena, target, nodes))
        .transpose()
}

fn required_array(
    arena: &mut TransformArena,
    target: TransformSourceId,
    nodes: Vec<TransformNode>,
) -> BuildResult<NodeArrayId> {
    create_node_array(arena, target, nodes)
}

fn create_string_literal(
    arena: &mut TransformArena,
    target: TransformSourceId,
    text: impl Into<String>,
) -> BuildResult<TransformNode> {
    create_node(
        arena,
        target,
        NodeData::StringLiteral(StringLiteralData {
            text: text.into(),
            has_extended_unicode_escape: None,
        }),
    )
}

fn create_numeric_literal(
    arena: &mut TransformArena,
    target: TransformSourceId,
    value: f64,
) -> BuildResult<TransformNode> {
    create_node(
        arena,
        target,
        NodeData::NumericLiteral(NumericLiteralData {
            text: tsc_types::js_number_to_string(value),
        }),
    )
}

fn create_private_identifier(
    arena: &mut TransformArena,
    target: TransformSourceId,
    text: &str,
) -> BuildResult<TransformNode> {
    let text = if text.starts_with('#') {
        text.to_owned()
    } else {
        format!("#{text}")
    };
    create_node(
        arena,
        target,
        NodeData::PrivateIdentifier(PrivateIdentifierData {
            escaped_text: text.clone(),
            text,
        }),
    )
}

fn create_modifiers_from_flags(
    arena: &mut TransformArena,
    target: TransformSourceId,
    flags: ModifierFlags,
) -> BuildResult<Option<NodeArrayId>> {
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
        (ModifierFlags::ACCESSOR, SyntaxKind::AccessorKeyword),
        (ModifierFlags::ASYNC, SyntaxKind::AsyncKeyword),
        (ModifierFlags::IN, SyntaxKind::InKeyword),
        (ModifierFlags::OUT, SyntaxKind::OutKeyword),
        (ModifierFlags::CONST, SyntaxKind::ConstKeyword),
    ] {
        if flags.intersects(flag) {
            modifiers.push(create_token(arena, target, kind)?);
        }
    }
    array(arena, target, modifiers)
}

fn transform_modifier_flags(
    arena: &TransformArena,
    target: TransformSourceId,
    modifiers: Option<NodeArrayId>,
) -> BuildResult<ModifierFlags> {
    let Some(modifiers) = modifiers.and_then(|array| arena.node_array_ref(target, array)) else {
        return Ok(ModifierFlags::NONE);
    };
    let mut flags = ModifierFlags::NONE;
    for &modifier in &arena.node_array(modifiers).map_err(factory_error)?.nodes {
        let Some(modifier) = arena.node_ref(target, modifier) else {
            continue;
        };
        flags |= match arena.node(modifier).map_err(factory_error)?.kind {
            SyntaxKind::ExportKeyword => ModifierFlags::EXPORT,
            SyntaxKind::DefaultKeyword => ModifierFlags::DEFAULT,
            SyntaxKind::DeclareKeyword => ModifierFlags::AMBIENT,
            SyntaxKind::PublicKeyword => ModifierFlags::PUBLIC,
            SyntaxKind::ProtectedKeyword => ModifierFlags::PROTECTED,
            SyntaxKind::PrivateKeyword => ModifierFlags::PRIVATE,
            SyntaxKind::AbstractKeyword => ModifierFlags::ABSTRACT,
            SyntaxKind::StaticKeyword => ModifierFlags::STATIC,
            SyntaxKind::ReadonlyKeyword => ModifierFlags::READONLY,
            SyntaxKind::OverrideKeyword => ModifierFlags::OVERRIDE,
            SyntaxKind::AccessorKeyword => ModifierFlags::ACCESSOR,
            SyntaxKind::AsyncKeyword => ModifierFlags::ASYNC,
            SyntaxKind::InKeyword => ModifierFlags::IN,
            SyntaxKind::OutKeyword => ModifierFlags::OUT,
            SyntaxKind::ConstKeyword => ModifierFlags::CONST,
            _ => ModifierFlags::NONE,
        };
    }
    Ok(flags)
}

fn modifiers_of(data: &NodeData) -> Option<NodeArrayId> {
    match data {
        NodeData::ClassDeclaration(data) => data.modifiers,
        NodeData::EnumDeclaration(data) => data.modifiers,
        NodeData::ExportAssignment(data) => data.modifiers,
        NodeData::ExportDeclaration(data) => data.modifiers,
        NodeData::FunctionDeclaration(data) => data.modifiers,
        NodeData::ImportDeclaration(data) => data.modifiers,
        NodeData::ImportEqualsDeclaration(data) => data.modifiers,
        NodeData::InterfaceDeclaration(data) => data.modifiers,
        NodeData::ModuleDeclaration(data) => data.modifiers,
        NodeData::NamespaceExportDeclaration(data) => data.modifiers,
        NodeData::TypeAliasDeclaration(data) => data.modifiers,
        NodeData::VariableStatement(data) => data.modifiers,
        NodeData::Constructor(data) => data.modifiers,
        NodeData::GetAccessor(data) => data.modifiers,
        NodeData::SetAccessor(data) => data.modifiers,
        NodeData::MethodDeclaration(data) => data.modifiers,
        NodeData::MethodSignature(data) => data.modifiers,
        NodeData::PropertyDeclaration(data) => data.modifiers,
        NodeData::PropertySignature(data) => data.modifiers,
        _ => None,
    }
}

fn with_modifiers(mut data: NodeData, modifiers: Option<NodeArrayId>) -> NodeData {
    match &mut data {
        NodeData::ClassDeclaration(data) => data.modifiers = modifiers,
        NodeData::EnumDeclaration(data) => data.modifiers = modifiers,
        NodeData::ExportAssignment(data) => data.modifiers = modifiers,
        NodeData::ExportDeclaration(data) => data.modifiers = modifiers,
        NodeData::FunctionDeclaration(data) => data.modifiers = modifiers,
        NodeData::ImportDeclaration(data) => data.modifiers = modifiers,
        NodeData::ImportEqualsDeclaration(data) => data.modifiers = modifiers,
        NodeData::InterfaceDeclaration(data) => data.modifiers = modifiers,
        NodeData::ModuleDeclaration(data) => data.modifiers = modifiers,
        NodeData::NamespaceExportDeclaration(data) => data.modifiers = modifiers,
        NodeData::TypeAliasDeclaration(data) => data.modifiers = modifiers,
        NodeData::VariableStatement(data) => data.modifiers = modifiers,
        NodeData::Constructor(data) => data.modifiers = modifiers,
        NodeData::GetAccessor(data) => data.modifiers = modifiers,
        NodeData::SetAccessor(data) => data.modifiers = modifiers,
        NodeData::MethodDeclaration(data) => data.modifiers = modifiers,
        NodeData::MethodSignature(data) => data.modifiers = modifiers,
        NodeData::PropertyDeclaration(data) => data.modifiers = modifiers,
        NodeData::PropertySignature(data) => data.modifiers = modifiers,
        _ => {}
    }
    data
}

fn replace_modifiers(
    arena: &mut TransformArena,
    target: TransformSourceId,
    node: TransformNode,
    flags: ModifierFlags,
) -> BuildResult<TransformNode> {
    let modifiers = create_modifiers_from_flags(arena, target, flags)?;
    let data = with_modifiers(
        arena.node(node).map_err(factory_error)?.data.clone(),
        modifiers,
    );
    let transform_flags = arena.transform_flags(node);
    arena
        .factory()
        .update_node(node, data, transform_flags)
        .map_err(factory_error)
}

fn create_variable_statement(
    arena: &mut TransformArena,
    target: TransformSourceId,
    name: TransformNode,
    type_node: Option<TransformNode>,
    flags: NodeFlags,
) -> BuildResult<TransformNode> {
    let declaration = create_node(
        arena,
        target,
        NodeData::VariableDeclaration(VariableDeclarationData {
            name: Some(name.node()),
            exclamation_token: None,
            r#type: type_node.map(TransformNode::node),
            initializer: None,
        }),
    )?;
    let declarations = required_array(arena, target, vec![declaration])?;
    let list = create_node(
        arena,
        target,
        NodeData::VariableDeclarationList(VariableDeclarationListData {
            declarations: Some(declarations),
        }),
    )?;
    let list = arena
        .factory()
        .set_node_flags(list, flags)
        .map_err(factory_error)?;
    create_node(
        arena,
        target,
        NodeData::VariableStatement(VariableStatementData {
            modifiers: None,
            declaration_list: Some(list.node()),
        }),
    )
}

fn create_export_specifier_node(
    arena: &mut TransformArena,
    target: TransformSourceId,
    property_name: Option<TransformNode>,
    name: TransformNode,
) -> BuildResult<TransformNode> {
    create_node(
        arena,
        target,
        NodeData::ExportSpecifier(ExportSpecifierData {
            name: Some(name.node()),
            is_type_only: false,
            property_name: property_name.map(TransformNode::node),
        }),
    )
}

fn create_named_export_declaration(
    arena: &mut TransformArena,
    target: TransformSourceId,
    elements: Vec<TransformNode>,
    module_specifier: Option<TransformNode>,
) -> BuildResult<TransformNode> {
    let elements = required_array(arena, target, elements)?;
    let clause = create_node(
        arena,
        target,
        NodeData::NamedExports(NamedExportsData {
            elements: Some(elements),
        }),
    )?;
    create_node(
        arena,
        target,
        NodeData::ExportDeclaration(ExportDeclarationData {
            modifiers: None,
            is_type_only: false,
            export_clause: Some(clause.node()),
            module_specifier: module_specifier.map(TransformNode::node),
            attributes: None,
        }),
    )
}

fn create_export_assignment(
    arena: &mut TransformArena,
    target: TransformSourceId,
    is_export_equals: bool,
    expression: TransformNode,
) -> BuildResult<TransformNode> {
    create_node(
        arena,
        target,
        NodeData::ExportAssignment(ExportAssignmentData {
            modifiers: None,
            is_export_equals: Some(is_export_equals),
            expression: Some(expression.node()),
        }),
    )
}

fn create_import_declaration(
    arena: &mut TransformArena,
    target: TransformSourceId,
    default_name: Option<TransformNode>,
    named_binding: Option<TransformNode>,
    module_specifier: TransformNode,
    is_type_only: bool,
) -> BuildResult<TransformNode> {
    let clause = create_node(
        arena,
        target,
        NodeData::ImportClause(ImportClauseData {
            name: default_name.map(TransformNode::node),
            is_type_only,
            phase_modifier: is_type_only.then_some(SyntaxKind::TypeKeyword),
            named_bindings: named_binding.map(TransformNode::node),
        }),
    )?;
    create_node(
        arena,
        target,
        NodeData::ImportDeclaration(ImportDeclarationData {
            modifiers: None,
            import_clause: Some(clause.node()),
            module_specifier: Some(module_specifier.node()),
            attributes: None,
        }),
    )
}

fn create_module_declaration(
    arena: &mut TransformArena,
    target: TransformSourceId,
    name: TransformNode,
    statements: Vec<TransformNode>,
    namespace: bool,
) -> BuildResult<TransformNode> {
    let statements = required_array(arena, target, statements)?;
    let block = create_node(
        arena,
        target,
        NodeData::ModuleBlock(ModuleBlockData {
            statements: Some(statements),
        }),
    )?;
    let declaration = create_node(
        arena,
        target,
        NodeData::ModuleDeclaration(ModuleDeclarationData {
            name: Some(name.node()),
            modifiers: None,
            body: Some(block.node()),
        }),
    )?;
    if namespace {
        arena
            .factory()
            .set_node_flags(declaration, NodeFlags::NAMESPACE)
            .map_err(factory_error)
    } else {
        Ok(declaration)
    }
}

fn is_class_like(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
    )
}

fn is_statement_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ImportDeclaration
            | SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::ExportDeclaration
            | SyntaxKind::ExportAssignment
            | SyntaxKind::NamespaceExportDeclaration
            | SyntaxKind::VariableStatement
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::ClassDeclaration
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::EnumDeclaration
            | SyntaxKind::ModuleDeclaration
    )
}

fn declaration_ancestor(checker: &CheckerState<'_>, mut node: NodeId) -> NodeId {
    while !is_statement_kind(checker.kind_of(node)) {
        let Some(parent) = checker.parent_of(node) else {
            break;
        };
        node = parent;
    }
    node
}

fn declaration_name(checker: &CheckerState<'_>, declaration: NodeId) -> Option<NodeId> {
    node_util::get_name_of_declaration(checker.binder.source_of_node(declaration), declaration)
}

fn parse_modifier_flags(checker: &CheckerState<'_>, declaration: NodeId) -> ModifierFlags {
    node_util::get_effective_modifier_flags(checker.binder.source_of_node(declaration), declaration)
}

/// tsc-port: symbolToDeclarations @6.0.3
/// tsc-hash: 1140cbfb469dc4a43154c65f60a605702bf80a00af87053a27ee85eaeb436c85
/// tsc-span: _tsc.js:51136-51164
#[allow(clippy::too_many_arguments)]
pub(crate) fn symbol_to_declarations(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    symbol: SymbolId,
    meaning: EmitSymbolMeaning,
    flags: EmitNodeBuilderFlags,
    maximum_length: Option<u32>,
    verbosity_level: Option<i32>,
    out: Option<&mut EmitSymbolExpansionOut>,
) -> BuildResult<Vec<TransformNode>> {
    let nodes = with_context(
        checker,
        arena,
        target,
        None,
        Some(flags),
        Some(EmitInternalNodeBuilderFlags::NONE),
        None,
        maximum_length,
        verbosity_level,
        |checker, arena, target, context| {
            symbol_to_declarations_worker(checker, arena, target, symbol, context)
        },
        out,
    )?
    .unwrap_or_default();

    let mut simplified = Vec::new();
    for node in nodes {
        let kind = arena.node(node).map_err(factory_error)?.kind;
        let node = match kind {
            SyntaxKind::ClassDeclaration => Some(simplify_class_declaration(
                checker, arena, target, node, symbol,
            )?),
            SyntaxKind::EnumDeclaration => Some(simplify_modifiers(
                checker,
                arena,
                target,
                node,
                symbol,
                |kind| kind == SyntaxKind::EnumDeclaration,
            )?),
            SyntaxKind::InterfaceDeclaration => {
                simplify_interface_declaration(checker, arena, target, node, symbol, meaning)?
            }
            SyntaxKind::ModuleDeclaration => Some(simplify_modifiers(
                checker,
                arena,
                target,
                node,
                symbol,
                |kind| kind == SyntaxKind::ModuleDeclaration,
            )?),
            _ => None,
        };
        if let Some(node) = node {
            simplified.push(node);
        }
    }
    Ok(simplified)
}

/// tsc-port: simplifyClassDeclaration @6.0.3
/// tsc-hash: ce308c645c52fcb6d1d41a2f291e4fae1c63a92503fd8f6d13de634259544464
/// tsc-span: _tsc.js:51165-51182
fn simplify_class_declaration(
    checker: &CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    mut class_declaration: TransformNode,
    symbol: SymbolId,
) -> BuildResult<TransformNode> {
    let original = checker
        .binder
        .symbol(symbol)
        .declarations
        .iter()
        .copied()
        .find(|&declaration| is_class_like(checker.kind_of(declaration)));
    let modifiers = match original {
        Some(declaration) => parse_modifier_flags(checker, declaration),
        None => transform_modifier_flags(
            arena,
            class_declaration.source(),
            modifiers_of(&arena.node(class_declaration).map_err(factory_error)?.data),
        )?,
    };
    let modifiers = ModifierFlags::from_bits(
        modifiers.bits() & !(ModifierFlags::EXPORT.bits() | ModifierFlags::AMBIENT.bits()),
    );
    if original
        .is_some_and(|declaration| checker.kind_of(declaration) == SyntaxKind::ClassExpression)
    {
        let mut data = arena
            .node(class_declaration)
            .map_err(factory_error)?
            .data
            .clone();
        if let NodeData::ClassDeclaration(class) = &mut data {
            class.name = None;
        }
        let flags = arena.transform_flags(class_declaration);
        class_declaration = arena
            .factory()
            .update_node(class_declaration, data, flags)
            .map_err(factory_error)?;
    }
    replace_modifiers(arena, target, class_declaration, modifiers)
}

/// tsc-port: simplifyModifiers @6.0.3
/// tsc-hash: db500b5665d7b105b6b4397ddaf6529276ab02b3aff46e84c217415949f18103
/// tsc-span: _tsc.js:51183-51188
fn simplify_modifiers(
    checker: &CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    declaration: TransformNode,
    symbol: SymbolId,
    is_kind: impl Fn(SyntaxKind) -> bool,
) -> BuildResult<TransformNode> {
    let original = checker
        .binder
        .symbol(symbol)
        .declarations
        .iter()
        .copied()
        .find(|&declaration| is_kind(checker.kind_of(declaration)));
    let modifiers = match original {
        Some(declaration) => parse_modifier_flags(checker, declaration),
        None => transform_modifier_flags(
            arena,
            declaration.source(),
            modifiers_of(&arena.node(declaration).map_err(factory_error)?.data),
        )?,
    };
    let modifiers = ModifierFlags::from_bits(
        modifiers.bits() & !(ModifierFlags::EXPORT.bits() | ModifierFlags::AMBIENT.bits()),
    );
    replace_modifiers(arena, target, declaration, modifiers)
}

/// tsc-port: simplifyInterfaceDeclaration @6.0.3
/// tsc-hash: fa0dab87310ad15433a20e3c6f9ef76bcddaac8097ea6864801f634a36455350
/// tsc-span: _tsc.js:51189-51194
fn simplify_interface_declaration(
    checker: &CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    declaration: TransformNode,
    symbol: SymbolId,
    meaning: EmitSymbolMeaning,
) -> BuildResult<Option<TransformNode>> {
    if meaning.0 & SymbolFlags::INTERFACE.bits() as u32 == 0 {
        return Ok(None);
    }
    simplify_modifiers(checker, arena, target, declaration, symbol, |kind| {
        kind == SyntaxKind::InterfaceDeclaration
    })
    .map(Some)
}

/// tsc-port: symbolToDeclarationsWorker @6.0.3
/// tsc-hash: 68c41d0b6ecca70ee84e000981a6508625f2f62607ae0a96a648cbfbd26bbf5f
/// tsc-span: _tsc.js:51195-51204
fn symbol_to_declarations_worker(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    symbol: SymbolId,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<Vec<TransformNode>> {
    let r#type = checker
        .get_declared_type_of_symbol_slice(symbol)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    context.type_stack.push(Some(r#type));
    context.type_stack.push(None);
    let mut table = SymbolTable::default();
    table.insert(checker.binder.symbol(symbol).escaped_name.clone(), symbol);
    let result = symbol_table_to_declaration_statements(checker, arena, target, &table, context);
    context.type_stack.pop();
    context.type_stack.pop();
    result
}

/// tsc-port: symbolTableToDeclarationStatements @6.0.3
/// tsc-hash: 5868ee7bf2f8ed650d0238acace1253ecddcd0a05232cd8f2c1a0f1579b5323b
/// tsc-span: _tsc.js:53722-55438
pub(crate) fn symbol_table_to_declaration_statements(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    symbol_table: &SymbolTable,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<Vec<TransformNode>> {
    let old_used = context.used_symbol_names.clone();
    let old_remapped = context.remapped_symbol_names.clone();
    let old_references = context.remapped_symbol_references.clone();
    // Object-spread creates a distinct statement-serialization context in
    // upstream; primitive accumulator writes do not escape to `oldcontext`.
    let old_approximate_length = context.approximate_length;
    context.used_symbol_names = Some(old_used.clone().unwrap_or_default());
    context.remapped_symbol_names = Some(HashMap::new());
    context.remapped_symbol_references = Some(old_references.clone().unwrap_or_default());
    let statement_tracker_restore = context.tracker.begin_statement_tracking();

    let result = {
        let mut serializer = StatementSerializer::new(checker, arena, target, context);
        serializer.reserve_symbol_names(symbol_table);
        serializer.serialize_root_table(symbol_table)
    };

    context.used_symbol_names = old_used;
    context.remapped_symbol_names = old_remapped;
    context.remapped_symbol_references = old_references;
    context.approximate_length = old_approximate_length;
    context
        .tracker
        .end_statement_tracking(statement_tracker_restore);
    result
}

struct StatementSerializer<'state, 'program, 'tracker> {
    checker: &'state mut CheckerState<'program>,
    arena: &'state mut TransformArena,
    target: TransformSourceId,
    context: &'state mut NodeBuilderContext<'tracker>,
    enclosing_declaration: Option<NodeId>,
    results: Vec<TransformNode>,
    visited_symbols: HashSet<SymbolId>,
    deferred_privates_stack: Vec<Vec<SymbolId>>,
    emitted_parse_statements: HashSet<NodeId>,
    adding_declare: bool,
}

impl<'state, 'program, 'tracker> StatementSerializer<'state, 'program, 'tracker> {
    fn new(
        checker: &'state mut CheckerState<'program>,
        arena: &'state mut TransformArena,
        target: TransformSourceId,
        context: &'state mut NodeBuilderContext<'tracker>,
    ) -> Self {
        Self {
            enclosing_declaration: context.enclosing_declaration,
            adding_declare: !context.bundled,
            checker,
            arena,
            target,
            context,
            results: Vec::new(),
            visited_symbols: HashSet::new(),
            deferred_privates_stack: Vec::new(),
            emitted_parse_statements: HashSet::new(),
        }
    }

    fn reserve_symbol_names(&mut self, table: &SymbolTable) {
        for (&symbol, base_name) in table.values().zip(table.keys()) {
            let base_name = tsc_binder::unescape_leading_underscores(base_name);
            let _ = self.get_internal_symbol_name(symbol, base_name);
        }
    }

    fn serialize_root_table(&mut self, table: &SymbolTable) -> BuildResult<Vec<TransformNode>> {
        let export_equals = table
            .get(tsc_types::InternalSymbolName::EXPORT_EQUALS)
            .copied();
        if let Some(export_equals) = export_equals.filter(|_| table.len() > 1) {
            let flags = self.checker.symbol_flags(export_equals);
            if flags.intersects(SymbolFlags::ALIAS | SymbolFlags::MODULE) {
                let mut only_export_equals = SymbolTable::default();
                only_export_equals.insert(
                    tsc_types::InternalSymbolName::EXPORT_EQUALS.to_owned(),
                    export_equals,
                );
                self.visit_symbol_table(&only_export_equals, false, false)?;
                let results = std::mem::take(&mut self.results);
                return self.merge_redundant_statements(results);
            }
        }
        self.visit_symbol_table(table, false, false)?;
        let results = std::mem::take(&mut self.results);
        let results = self.merge_redundant_statements(results)?;
        Ok(results)
    }

    fn node_from_id(&self, id: NodeId) -> Option<TransformNode> {
        self.arena.node_ref(self.target, id)
    }

    fn array_nodes(&self, array: Option<NodeArrayId>) -> BuildResult<Vec<TransformNode>> {
        let Some(array) = array.and_then(|array| self.arena.node_array_ref(self.target, array))
        else {
            return Ok(Vec::new());
        };
        Ok(self
            .arena
            .node_array(array)
            .map_err(factory_error)?
            .nodes
            .iter()
            .filter_map(|&node| self.node_from_id(node))
            .collect())
    }

    fn identifier_text(&self, node: TransformNode) -> BuildResult<Option<&str>> {
        Ok(match &self.arena.node(node).map_err(factory_error)?.data {
            NodeData::Identifier(data) => Some(data.text.as_str()),
            _ => None,
        })
    }

    fn name_of_statement(&self, statement: TransformNode) -> BuildResult<Vec<TransformNode>> {
        let data = &self.arena.node(statement).map_err(factory_error)?.data;
        if let NodeData::VariableStatement(statement) = data {
            let Some(list) = statement
                .declaration_list
                .and_then(|node| self.node_from_id(node))
            else {
                return Ok(Vec::new());
            };
            let NodeData::VariableDeclarationList(list) =
                &self.arena.node(list).map_err(factory_error)?.data
            else {
                return Ok(Vec::new());
            };
            let mut names = Vec::new();
            for declaration in self.array_nodes(list.declarations)? {
                let NodeData::VariableDeclaration(declaration) =
                    &self.arena.node(declaration).map_err(factory_error)?.data
                else {
                    continue;
                };
                if let Some(name) = declaration.name.and_then(|name| self.node_from_id(name)) {
                    if self.is_identifier_and_not_undefined(Some(name))? {
                        names.push(name);
                    }
                }
            }
            return Ok(names);
        }
        let name = match data {
            NodeData::ClassDeclaration(data) => data.name,
            NodeData::EnumDeclaration(data) => data.name,
            NodeData::FunctionDeclaration(data) => data.name,
            NodeData::ImportEqualsDeclaration(data) => data.name,
            NodeData::InterfaceDeclaration(data) => data.name,
            NodeData::ModuleDeclaration(data) => data.name,
            NodeData::TypeAliasDeclaration(data) => data.name,
            _ => None,
        };
        Ok(name
            .and_then(|name| self.node_from_id(name))
            .filter(|&name| {
                self.is_identifier_and_not_undefined(Some(name))
                    .unwrap_or(false)
            })
            .into_iter()
            .collect())
    }

    fn node_has_name(
        &self,
        statement: TransformNode,
        expected: TransformNode,
    ) -> BuildResult<bool> {
        let Some(expected) = self.identifier_text(expected)? else {
            return Ok(false);
        };
        for name in self.name_of_statement(statement)? {
            if self.identifier_text(name)? == Some(expected) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// tsc-port: isIdentifierAndNotUndefined @6.0.3
    /// tsc-hash: 34162d2b8b0d4f4df2f4f19f6ad43a8ecf81200a72d86c61988fbbd1d3154726
    /// tsc-span: _tsc.js:53788-53790
    fn is_identifier_and_not_undefined(&self, node: Option<TransformNode>) -> BuildResult<bool> {
        Ok(node.is_some_and(|node| {
            self.arena
                .node(node)
                .is_ok_and(|record| record.kind == SyntaxKind::Identifier)
        }))
    }

    /// tsc-port: getNamesOfDeclaration @6.0.3
    /// tsc-hash: 9da8d702024be32a1b7bf53adf2025d30f97d277a6b7eae06494f90fdfd38da5
    /// tsc-span: _tsc.js:53791-53796
    fn get_names_of_declaration(
        &self,
        statement: TransformNode,
    ) -> BuildResult<Vec<TransformNode>> {
        self.name_of_statement(statement)
    }

    /// tsc-port: flattenExportAssignedNamespace @6.0.3
    /// tsc-hash: da3d3310a92243cec6a3d081625c56bc7071560af3d178e19e2b93f67273e7e6
    /// tsc-span: _tsc.js:53797-53844
    fn flatten_export_assigned_namespace(
        &mut self,
        mut statements: Vec<TransformNode>,
    ) -> BuildResult<Vec<TransformNode>> {
        let export_assignment = statements.iter().copied().find(|&statement| {
            matches!(
                self.arena.node(statement).map(|node| &node.data),
                Ok(NodeData::ExportAssignment(_))
            )
        });
        let Some((namespace_index, mut namespace)) =
            statements
                .iter()
                .copied()
                .enumerate()
                .find(|(_, statement)| {
                    self.arena
                        .node(*statement)
                        .is_ok_and(|node| node.kind == SyntaxKind::ModuleDeclaration)
                })
        else {
            return Ok(statements);
        };
        let Some(export_assignment) = export_assignment else {
            return Ok(statements);
        };
        let NodeData::ExportAssignment(export_data) = &self
            .arena
            .node(export_assignment)
            .map_err(factory_error)?
            .data
        else {
            return Ok(statements);
        };
        if export_data.is_export_equals != Some(true) {
            return Ok(statements);
        }
        let Some(expression) = export_data
            .expression
            .and_then(|expression| self.node_from_id(expression))
        else {
            return Ok(statements);
        };
        let NodeData::ModuleDeclaration(namespace_data) =
            &self.arena.node(namespace).map_err(factory_error)?.data
        else {
            return Ok(statements);
        };
        let Some(name) = namespace_data.name.and_then(|name| self.node_from_id(name)) else {
            return Ok(statements);
        };
        if self.identifier_text(name)? != self.identifier_text(expression)? {
            return Ok(statements);
        }
        let Some(body) = namespace_data.body.and_then(|body| self.node_from_id(body)) else {
            return Ok(statements);
        };
        let NodeData::ModuleBlock(body_data) = &self.arena.node(body).map_err(factory_error)?.data
        else {
            return Ok(statements);
        };
        let mut body_statements = self.array_nodes(body_data.statements)?;
        let excess_exports = statements
            .iter()
            .copied()
            .filter(|&statement| {
                self.effective_modifier_flags(statement)
                    .is_ok_and(|flags| flags.intersects(ModifierFlags::EXPORT))
            })
            .collect::<Vec<_>>();
        if !excess_exports.is_empty() {
            let mut elements = Vec::new();
            for declaration in excess_exports {
                for name in self.get_names_of_declaration(declaration)? {
                    elements.push(create_export_specifier_node(
                        self.arena,
                        self.target,
                        None,
                        name,
                    )?);
                }
            }
            if !elements.is_empty() {
                body_statements.push(create_named_export_declaration(
                    self.arena,
                    self.target,
                    elements,
                    None,
                )?);
                let statements_array = required_array(self.arena, self.target, body_statements)?;
                let body_flags = self.arena.transform_flags(body);
                let updated_body = self
                    .arena
                    .factory()
                    .update_node(
                        body,
                        NodeData::ModuleBlock(ModuleBlockData {
                            statements: Some(statements_array),
                        }),
                        body_flags,
                    )
                    .map_err(factory_error)?;
                let mut updated_namespace_data = self
                    .arena
                    .node(namespace)
                    .map_err(factory_error)?
                    .data
                    .clone();
                let NodeData::ModuleDeclaration(data) = &mut updated_namespace_data else {
                    return Ok(statements);
                };
                data.body = Some(updated_body.node());
                let namespace_flags = self.arena.transform_flags(namespace);
                namespace = self
                    .arena
                    .factory()
                    .update_node(namespace, updated_namespace_data, namespace_flags)
                    .map_err(factory_error)?;
                statements[namespace_index] = namespace;
                body_statements = self.array_nodes(Some(statements_array))?;
            }
        }
        let has_other_named = statements
            .iter()
            .copied()
            .filter(|&statement| statement != namespace)
            .any(|statement| self.node_has_name(statement, name).unwrap_or(false));
        if has_other_named {
            return Ok(statements);
        }
        let mixin_export = !body_statements.iter().copied().any(|statement| {
            self.effective_modifier_flags(statement)
                .is_ok_and(|flags| flags.intersects(ModifierFlags::EXPORT))
                || self.arena.node(statement).is_ok_and(|node| {
                    matches!(
                        node.kind,
                        SyntaxKind::ExportAssignment | SyntaxKind::ExportDeclaration
                    )
                })
        });
        let old_results = std::mem::take(&mut self.results);
        for statement in body_statements {
            self.add_result(
                statement,
                if mixin_export {
                    ModifierFlags::EXPORT
                } else {
                    ModifierFlags::NONE
                },
            )?;
        }
        let body_results = std::mem::replace(&mut self.results, old_results);
        let mut flattened = statements
            .into_iter()
            .filter(|&statement| statement != namespace && statement != export_assignment)
            .collect::<Vec<_>>();
        flattened.extend(body_results);
        Ok(flattened)
    }

    /// tsc-port: mergeExportDeclarations @6.0.3
    /// tsc-hash: 245412fef84953ba31a0faf8b3826834567267f2bac031fb267bb15125fd58d4
    /// tsc-span: _tsc.js:53845-53884
    fn merge_export_declarations(
        &mut self,
        statements: Vec<TransformNode>,
    ) -> BuildResult<Vec<TransformNode>> {
        let mut local = Vec::new();
        let mut reexport_groups: Vec<(String, Vec<usize>)> = Vec::new();
        for (index, &statement) in statements.iter().enumerate() {
            let NodeData::ExportDeclaration(data) =
                &self.arena.node(statement).map_err(factory_error)?.data
            else {
                continue;
            };
            let Some(clause) = data.export_clause.and_then(|node| self.node_from_id(node)) else {
                continue;
            };
            if self.arena.node(clause).map_err(factory_error)?.kind != SyntaxKind::NamedExports {
                continue;
            }
            let Some(module) = data
                .module_specifier
                .and_then(|node| self.node_from_id(node))
            else {
                local.push(index);
                continue;
            };
            let key = match &self.arena.node(module).map_err(factory_error)?.data {
                NodeData::StringLiteral(data) => format!(">{}", data.text),
                _ => ">".to_owned(),
            };
            if let Some((_, indices)) = reexport_groups
                .iter_mut()
                .find(|(candidate, _)| *candidate == key)
            {
                indices.push(index);
            } else {
                reexport_groups.push((key, vec![index]));
            }
        }

        let mut removed = HashSet::new();
        let mut additions = Vec::new();
        if local.len() > 1 {
            let mut elements = Vec::new();
            for &index in &local {
                removed.insert(index);
                let NodeData::ExportDeclaration(data) = &self
                    .arena
                    .node(statements[index])
                    .map_err(factory_error)?
                    .data
                else {
                    continue;
                };
                let Some(clause) = data.export_clause.and_then(|node| self.node_from_id(node))
                else {
                    continue;
                };
                let NodeData::NamedExports(data) =
                    &self.arena.node(clause).map_err(factory_error)?.data
                else {
                    continue;
                };
                elements.extend(self.array_nodes(data.elements)?);
            }
            additions.push(create_named_export_declaration(
                self.arena,
                self.target,
                elements,
                None,
            )?);
        }
        for (_, indices) in reexport_groups {
            if indices.len() <= 1 {
                continue;
            }
            let mut elements = Vec::new();
            let first = statements[indices[0]];
            let NodeData::ExportDeclaration(first_data) =
                &self.arena.node(first).map_err(factory_error)?.data
            else {
                continue;
            };
            let module = first_data
                .module_specifier
                .and_then(|node| self.node_from_id(node));
            for index in indices {
                removed.insert(index);
                let NodeData::ExportDeclaration(data) = &self
                    .arena
                    .node(statements[index])
                    .map_err(factory_error)?
                    .data
                else {
                    continue;
                };
                let Some(clause) = data.export_clause.and_then(|node| self.node_from_id(node))
                else {
                    continue;
                };
                let NodeData::NamedExports(data) =
                    &self.arena.node(clause).map_err(factory_error)?.data
                else {
                    continue;
                };
                elements.extend(self.array_nodes(data.elements)?);
            }
            additions.push(create_named_export_declaration(
                self.arena,
                self.target,
                elements,
                module,
            )?);
        }
        let mut result = statements
            .into_iter()
            .enumerate()
            .filter_map(|(index, statement)| (!removed.contains(&index)).then_some(statement))
            .collect::<Vec<_>>();
        result.extend(additions);
        Ok(result)
    }

    /// tsc-port: inlineExportModifiers @6.0.3
    /// tsc-hash: bbff5bd8204c8ec22138964d915fafdbb8888d49c7998d1b2b38d750a4c3bbb5
    /// tsc-span: _tsc.js:53885-53920
    fn inline_export_modifiers(
        &mut self,
        mut statements: Vec<TransformNode>,
    ) -> BuildResult<Vec<TransformNode>> {
        let export_index = statements.iter().position(|&statement| {
            let Ok(node) = self.arena.node(statement) else {
                return false;
            };
            let NodeData::ExportDeclaration(data) = &node.data else {
                return false;
            };
            data.module_specifier.is_none()
                && data.attributes.is_none()
                && data.export_clause.is_some_and(|clause| {
                    self.node_from_id(clause).is_some_and(|clause| {
                        self.arena
                            .node(clause)
                            .is_ok_and(|node| node.kind == SyntaxKind::NamedExports)
                    })
                })
        });
        let Some(export_index) = export_index else {
            return Ok(statements);
        };
        let export = statements[export_index];
        let NodeData::ExportDeclaration(export_data) =
            &self.arena.node(export).map_err(factory_error)?.data
        else {
            return Ok(statements);
        };
        let Some(clause) = export_data
            .export_clause
            .and_then(|clause| self.node_from_id(clause))
        else {
            return Ok(statements);
        };
        let NodeData::NamedExports(exports) = &self.arena.node(clause).map_err(factory_error)?.data
        else {
            return Ok(statements);
        };
        let mut replacements = Vec::new();
        for element in self.array_nodes(exports.elements)? {
            let NodeData::ExportSpecifier(specifier) =
                &self.arena.node(element).map_err(factory_error)?.data
            else {
                replacements.push(element);
                continue;
            };
            let Some(name) = specifier.name.and_then(|name| self.node_from_id(name)) else {
                replacements.push(element);
                continue;
            };
            if specifier.property_name.is_some()
                || self.arena.node(name).map_err(factory_error)?.kind == SyntaxKind::StringLiteral
            {
                replacements.push(element);
                continue;
            }
            let associated: Vec<usize> = statements
                .iter()
                .copied()
                .enumerate()
                .filter(|(index, statement)| {
                    *index != export_index && self.node_has_name(*statement, name).unwrap_or(false)
                })
                .map(|(index, _)| index)
                .collect();
            if associated.is_empty()
                || associated.iter().any(|&index| {
                    !self
                        .can_have_export_modifier(statements[index])
                        .unwrap_or(false)
                })
            {
                replacements.push(element);
                continue;
            }
            for index in associated {
                statements[index] = self.add_export_modifier(statements[index])?;
            }
        }
        if replacements.is_empty() {
            statements.remove(export_index);
        } else {
            let elements = required_array(self.arena, self.target, replacements)?;
            let clause_flags = self.arena.transform_flags(clause);
            let clause = self
                .arena
                .factory()
                .update_node(
                    clause,
                    NodeData::NamedExports(NamedExportsData {
                        elements: Some(elements),
                    }),
                    clause_flags,
                )
                .map_err(factory_error)?;
            let mut updated_data = self.arena.node(export).map_err(factory_error)?.data.clone();
            let NodeData::ExportDeclaration(export_data) = &mut updated_data else {
                return Ok(statements);
            };
            export_data.export_clause = Some(clause.node());
            let flags = self.arena.transform_flags(export);
            statements[export_index] = self
                .arena
                .factory()
                .update_node(export, updated_data, flags)
                .map_err(factory_error)?;
        }
        Ok(statements)
    }

    /// tsc-port: mergeRedundantStatements @6.0.3
    /// tsc-hash: c10a8b36307bead4131305fbd477eb4d4376af8e4d8c4b265775922dae96c037
    /// tsc-span: _tsc.js:53921-53929
    fn merge_redundant_statements(
        &mut self,
        statements: Vec<TransformNode>,
    ) -> BuildResult<Vec<TransformNode>> {
        let statements = self.flatten_export_assigned_namespace(statements)?;
        let statements = self.merge_export_declarations(statements)?;
        let mut statements = self.inline_export_modifiers(statements)?;
        let exporting_scope = self.enclosing_declaration.is_some_and(|node| {
            self.checker.kind_of(node) == SyntaxKind::ModuleDeclaration
                || self.checker.kind_of(node) == SyntaxKind::SourceFile
                    && self
                        .checker
                        .binder
                        .is_external_or_common_js_module_of_node(node)
        });
        let is_import_or_reexport = |kind| {
            matches!(
                kind,
                SyntaxKind::ImportDeclaration
                    | SyntaxKind::ImportEqualsDeclaration
                    | SyntaxKind::ExportDeclaration
            )
        };
        let has_external_indicator = statements.iter().copied().any(|statement| {
            self.arena.node(statement).is_ok_and(|node| {
                is_import_or_reexport(node.kind) || node.kind == SyntaxKind::ExportAssignment
            }) || self
                .effective_modifier_flags(statement)
                .is_ok_and(|flags| flags.intersects(ModifierFlags::EXPORT))
        });
        let has_scope_marker = statements.iter().copied().any(|statement| {
            self.arena.node(statement).is_ok_and(|node| {
                matches!(
                    node.kind,
                    SyntaxKind::ExportAssignment | SyntaxKind::ExportDeclaration
                )
            })
        });
        let needs_scope_marker = statements.iter().copied().any(|statement| {
            let Ok(node) = self.arena.node(statement) else {
                return false;
            };
            !is_import_or_reexport(node.kind)
                && node.kind != SyntaxKind::ExportAssignment
                && !self.effective_modifier_flags(statement).is_ok_and(|flags| {
                    flags.intersects(ModifierFlags::EXPORT)
                        || node.kind == SyntaxKind::ModuleDeclaration
                            && flags.intersects(ModifierFlags::AMBIENT)
                })
        });
        if exporting_scope && (!has_external_indicator || !has_scope_marker && needs_scope_marker) {
            statements.push(create_named_export_declaration(
                self.arena,
                self.target,
                Vec::new(),
                None,
            )?);
        }
        Ok(statements)
    }

    fn effective_modifier_flags(&self, node: TransformNode) -> BuildResult<ModifierFlags> {
        let data = &self.arena.node(node).map_err(factory_error)?.data;
        transform_modifier_flags(self.arena, self.target, modifiers_of(data))
    }

    fn can_have_export_modifier(&self, node: TransformNode) -> BuildResult<bool> {
        Ok(matches!(
            self.arena.node(node).map_err(factory_error)?.kind,
            SyntaxKind::VariableStatement
                | SyntaxKind::FunctionDeclaration
                | SyntaxKind::ClassDeclaration
                | SyntaxKind::InterfaceDeclaration
                | SyntaxKind::TypeAliasDeclaration
                | SyntaxKind::EnumDeclaration
                | SyntaxKind::ModuleDeclaration
                | SyntaxKind::ImportEqualsDeclaration
        ))
    }

    /// tsc-port: addExportModifier @6.0.3
    /// tsc-hash: 8702ee85cf5271a70f09c656c903925fdc88086ef8b4a857350eee0f599d62e9
    /// tsc-span: _tsc.js:53930-53933
    fn add_export_modifier(&mut self, node: TransformNode) -> BuildResult<TransformNode> {
        let old = self.effective_modifier_flags(node)?;
        let flags = ModifierFlags::from_bits(
            (old.bits() | ModifierFlags::EXPORT.bits()) & !ModifierFlags::AMBIENT.bits(),
        );
        replace_modifiers(self.arena, self.target, node, flags)
    }

    /// tsc-port: removeExportModifier @6.0.3
    /// tsc-hash: 061b1d8d4f6236c6aeed8c7f695a1bf9b9b4f325404dffb63f17f297d2f91bd3
    /// tsc-span: _tsc.js:53934-53937
    fn remove_export_modifier(&mut self, node: TransformNode) -> BuildResult<TransformNode> {
        let old = self.effective_modifier_flags(node)?;
        let flags = ModifierFlags::from_bits(old.bits() & !ModifierFlags::EXPORT.bits());
        replace_modifiers(self.arena, self.target, node, flags)
    }

    /// tsc-port: visitSymbolTable @6.0.3
    /// tsc-hash: 6c24132a883411f0dd4cce6dfc03ad19ff6f3cdfeaede3e283f075c90e325235
    /// tsc-span: _tsc.js:53938-53975
    fn visit_symbol_table(
        &mut self,
        table: &SymbolTable,
        suppress_new_private_context: bool,
        property_as_alias: bool,
    ) -> BuildResult<()> {
        if !suppress_new_private_context {
            self.deferred_privates_stack.push(Vec::new());
        }
        let symbols: Vec<SymbolId> = table.values().copied().collect();
        for (index, symbol) in symbols.iter().copied().enumerate() {
            let ordinal = index + 1;
            if check_truncation_length_if_expanding(self.context)
                && ordinal + 2 < table.len().saturating_sub(1)
            {
                self.context.out.truncated = true;
                let statement = self.create_truncation_statement(&format!(
                    "... ({} more ...)",
                    table.len().saturating_sub(ordinal)
                ))?;
                self.results.push(statement);
                if let Some(last) = symbols.last().copied() {
                    self.serialize_symbol(last, false, property_as_alias)?;
                }
                break;
            }
            self.serialize_symbol(symbol, false, property_as_alias)?;
            self.include_tracked_private_symbols()?;
        }
        if !suppress_new_private_context {
            let mut index = 0;
            while let Some(symbol) = self
                .deferred_privates_stack
                .last()
                .and_then(|deferred| deferred.get(index))
                .copied()
            {
                index += 1;
                self.serialize_symbol(symbol, true, property_as_alias)?;
                self.include_tracked_private_symbols()?;
            }
            self.deferred_privates_stack.pop();
        }
        Ok(())
    }

    /// The scoped tracker installed by `symbolTableToDeclarationStatements`
    /// calls `lookupSymbolChainWorker` and queues an accessible chain root via
    /// `includePrivateSymbol` before returning to the builder.
    fn include_tracked_private_symbols(&mut self) -> BuildResult<()> {
        loop {
            let pending = self.context.tracker.take_statement_symbols();
            if pending.is_empty() {
                return Ok(());
            }
            for (symbol, meaning) in pending {
                let chain = super::chains::lookup_symbol_chain_worker(
                    self.checker,
                    self.context,
                    symbol,
                    meaning,
                    false,
                )?;
                let Some(root) = chain.first().copied() else {
                    continue;
                };
                // Rust retains distinct local/export symbols for an exported
                // declaration. Upstream's statement serializer observes the
                // export-projected identity, so normalize before the visited
                // check queues a referenced same-file private. Otherwise an
                // already-emitted `export function b` is serialized again as
                // a private and paints its declaration/source visible.
                let root = self
                    .checker
                    .get_export_symbol_of_value_symbol_if_exported(root);
                if self
                    .checker
                    .symbol_flags(root)
                    .intersects(SymbolFlags::TYPE_PARAMETER)
                {
                    for declaration in self.checker.binder.symbol(root).declarations.clone() {
                        let _ = self
                            .checker
                            .emit_is_declaration_visible(declaration)
                            .map_err(|abort| {
                                checker_abort_error(self.checker, self.context, abort)
                            })?;
                    }
                }
                if self
                    .checker
                    .binder
                    .symbol(root)
                    .declarations
                    .iter()
                    .any(|&declaration| {
                        Some(self.checker.binder.source_of_node(declaration).root)
                            == self.context.enclosing_file
                    })
                {
                    self.include_private_symbol(root);
                }
            }
        }
    }

    /// tsc-port: serializeSymbol @6.0.3
    /// tsc-hash: 89a62d997aa6147fdb69cb375b6f5a5e4a24830189c28093f5d59dfefacc0647
    /// tsc-span: _tsc.js:53976-53991
    fn serialize_symbol(
        &mut self,
        symbol: SymbolId,
        is_private: bool,
        property_as_alias: bool,
    ) -> BuildResult<()> {
        if self
            .checker
            .symbol_flags(symbol)
            .intersects(SymbolFlags::VALUE | SymbolFlags::ALIAS)
        {
            let r#type = self
                .checker
                .get_type_of_symbol(symbol)
                .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
            let _ = self
                .checker
                .get_properties_of_type(r#type)
                .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
        }
        let merged = self.checker.get_merged_symbol(symbol);
        if !self.visited_symbols.insert(merged) {
            return Ok(());
        }
        if is_private && !self.private_symbol_belongs_to_scope(symbol) {
            return Ok(());
        }
        let restore = clone_node_builder_context(self.context);
        let fallback = self
            .checker
            .binder
            .symbol(symbol)
            .declarations
            .iter()
            .copied()
            .find(|&declaration| {
                Some(self.checker.binder.source_of_node(declaration).root)
                    == self.context.enclosing_file
            });
        self.context.tracker.push_error_fallback_node(fallback);
        let result = self.serialize_symbol_worker(symbol, is_private, property_as_alias, None);
        self.context.tracker.pop_error_fallback_node();
        restore_cloned_node_builder_context(self.context, restore);
        result
    }

    fn private_symbol_belongs_to_scope(&self, symbol: SymbolId) -> bool {
        let Some(enclosing) = self.enclosing_declaration else {
            return true;
        };
        self.checker
            .binder
            .symbol(symbol)
            .declarations
            .iter()
            .copied()
            .any(|declaration| {
                let mut current = Some(declaration);
                while let Some(node) = current {
                    if node == enclosing {
                        return true;
                    }
                    current = self.checker.parent_of(node);
                }
                false
            })
    }

    /// tsc-port: serializeSymbolWorker @6.0.3
    /// tsc-hash: dc9bf6e639d95e72cefd3e99de392150843321b4b1f785340d52751bbe4bbc38
    /// tsc-span: _tsc.js:53992-54179
    fn serialize_symbol_worker(
        &mut self,
        symbol: SymbolId,
        mut is_private: bool,
        property_as_alias: bool,
        escaped_symbol_name: Option<String>,
    ) -> BuildResult<()> {
        let symbol_data = self.checker.binder.symbol(symbol).clone();
        let escaped_symbol_name =
            escaped_symbol_name.unwrap_or_else(|| symbol_data.escaped_name.clone());
        let symbol_name = tsc_binder::unescape_leading_underscores(&escaped_symbol_name).to_owned();
        let is_default = escaped_symbol_name == tsc_types::InternalSymbolName::DEFAULT;
        if is_private
            && self.context.flags.0 & ALLOW_ANONYMOUS_IDENTIFIER == 0
            && is_string_a_non_contextual_keyword(&symbol_name)
            && !is_default
        {
            self.context.encountered_error = true;
            return Ok(());
        }

        let type_of_symbol = if symbol_data
            .flags
            .intersects(SymbolFlags::VALUE | SymbolFlags::ALIAS)
        {
            Some(
                self.checker
                    .get_type_of_symbol(symbol)
                    .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?,
            )
        } else {
            None
        };
        let has_function_properties = if symbol_data.flags.intersects(SymbolFlags::FUNCTION) {
            let function_type = match type_of_symbol {
                Some(r#type) => r#type,
                None => self
                    .checker
                    .get_type_of_symbol(symbol)
                    .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?,
            };
            !self
                .checker
                .get_properties_of_type(function_type)
                .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?
                .is_empty()
        } else {
            false
        };
        let mut needs_post_export_default = is_default
            && (symbol_data
                .flags
                .intersects(SymbolFlags::EXPORT_DOES_NOT_SUPPORT_DEFAULT_MODIFIER)
                || has_function_properties)
            && !symbol_data.flags.intersects(SymbolFlags::ALIAS);
        let mut needs_export_declaration = !needs_post_export_default
            && !is_private
            && is_string_a_non_contextual_keyword(&symbol_name)
            && !is_default;
        if needs_post_export_default || needs_export_declaration {
            is_private = true;
        }
        let modifier_flags = ModifierFlags::from_bits(
            if is_private {
                0
            } else {
                ModifierFlags::EXPORT.bits()
            } | if is_default && !needs_post_export_default {
                ModifierFlags::DEFAULT.bits()
            } else {
                0
            },
        );

        let const_merged_with_namespace = symbol_data.flags.intersects(SymbolFlags::MODULE)
            && symbol_data.flags.intersects(
                SymbolFlags::BLOCK_SCOPED_VARIABLE
                    | SymbolFlags::FUNCTION_SCOPED_VARIABLE
                    | SymbolFlags::PROPERTY,
            )
            && escaped_symbol_name != tsc_types::InternalSymbolName::EXPORT_EQUALS;
        let const_merge_is_function_namespace = match type_of_symbol {
            Some(r#type) if const_merged_with_namespace => {
                self.is_type_representable_as_function_namespace_merge(r#type, symbol)?
            }
            _ => false,
        };

        if symbol_data
            .flags
            .intersects(SymbolFlags::FUNCTION | SymbolFlags::METHOD)
            || const_merge_is_function_namespace
        {
            let local_name = self.get_internal_symbol_name(symbol, &symbol_name);
            let r#type = match type_of_symbol {
                Some(r#type) => r#type,
                None => self
                    .checker
                    .get_type_of_symbol(symbol)
                    .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?,
            };
            self.serialize_as_function_namespace_merge(
                r#type,
                symbol,
                &local_name,
                modifier_flags,
            )?;
        }
        if symbol_data.flags.intersects(SymbolFlags::TYPE_ALIAS) {
            self.serialize_type_alias(symbol, &symbol_name, modifier_flags)?;
        }

        let variable_like = symbol_data.flags.intersects(
            SymbolFlags::BLOCK_SCOPED_VARIABLE
                | SymbolFlags::FUNCTION_SCOPED_VARIABLE
                | SymbolFlags::PROPERTY
                | SymbolFlags::ACCESSOR,
        );
        if variable_like
            && escaped_symbol_name != tsc_types::InternalSymbolName::EXPORT_EQUALS
            && !symbol_data.flags.intersects(SymbolFlags::PROTOTYPE)
            && !symbol_data.flags.intersects(SymbolFlags::CLASS)
            && !symbol_data.flags.intersects(SymbolFlags::METHOD)
            && !const_merge_is_function_namespace
        {
            if property_as_alias {
                if self.serialize_maybe_alias_assignment(symbol)? {
                    needs_export_declaration = false;
                    needs_post_export_default = false;
                }
            } else {
                let r#type = type_of_symbol.unwrap_or(
                    self.checker
                        .get_type_of_symbol(symbol)
                        .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?,
                );
                let local_name = self.get_internal_symbol_name(symbol, &symbol_name);
                if !symbol_data.flags.intersects(SymbolFlags::FUNCTION)
                    && self.is_type_representable_as_function_namespace_merge(r#type, symbol)?
                {
                    self.serialize_as_function_namespace_merge(
                        r#type,
                        symbol,
                        &local_name,
                        modifier_flags,
                    )?;
                } else {
                    let list_flags = if symbol_data
                        .flags
                        .intersects(SymbolFlags::BLOCK_SCOPED_VARIABLE)
                    {
                        if self.checker.is_constant_variable(symbol) {
                            NodeFlags::CONST
                        } else {
                            NodeFlags::LET
                        }
                    } else if symbol_data
                        .parent
                        .and_then(|parent| self.checker.binder.symbol(parent).value_declaration)
                        .is_some_and(|declaration| {
                            self.checker.kind_of(declaration) == SyntaxKind::SourceFile
                        })
                    {
                        NodeFlags::CONST
                    } else {
                        NodeFlags::NONE
                    };
                    let name = if needs_post_export_default
                        || !symbol_data.flags.intersects(SymbolFlags::PROPERTY)
                    {
                        local_name.clone()
                    } else {
                        self.get_unused_name(&local_name, Some(symbol))
                    };
                    let type_node = serialize_type_for_declaration_seam(
                        self.checker,
                        self.arena,
                        self.target,
                        self.context,
                        None,
                        r#type,
                        Some(symbol),
                    )?;
                    let name_node = create_identifier(self.arena, self.target, &name)?;
                    let statement = create_variable_statement(
                        self.arena,
                        self.target,
                        name_node,
                        type_node,
                        list_flags,
                    )?;
                    let text_range = self.variable_statement_range(symbol);
                    let location = text_range
                        .map(|node| project_parse_node(self.checker, self.arena, node))
                        .transpose()?
                        .flatten();
                    let statement = set_text_range2(
                        self.checker,
                        self.arena,
                        self.context,
                        statement,
                        location,
                    )?;
                    add_approximate_length(self.context, 7 + name.encode_utf16().count());
                    let flags = if name == local_name {
                        modifier_flags
                    } else {
                        ModifierFlags::from_bits(
                            modifier_flags.bits() & !ModifierFlags::EXPORT.bits(),
                        )
                    };
                    self.add_result(statement, flags)?;
                    if name != local_name && !is_private {
                        add_approximate_length(
                            self.context,
                            16 + name.encode_utf16().count() + local_name.encode_utf16().count(),
                        );
                        self.serialize_export_specifier(&local_name, &name, None)?;
                        needs_export_declaration = false;
                        needs_post_export_default = false;
                    }
                }
            }
        }

        if symbol_data.flags.intersects(SymbolFlags::ENUM) {
            self.serialize_enum(symbol, &symbol_name, modifier_flags)?;
        }
        if symbol_data.flags.intersects(SymbolFlags::CLASS) {
            let local_name = self.get_internal_symbol_name(symbol, &symbol_name);
            self.serialize_as_class(symbol, &local_name, modifier_flags)?;
        }
        if symbol_data
            .flags
            .intersects(SymbolFlags::VALUE_MODULE | SymbolFlags::NAMESPACE_MODULE)
            && (!const_merged_with_namespace || self.is_type_only_namespace(symbol)?)
            || const_merge_is_function_namespace
        {
            self.serialize_module(symbol, &symbol_name, modifier_flags)?;
        }
        if symbol_data.flags.intersects(SymbolFlags::INTERFACE)
            && !symbol_data.flags.intersects(SymbolFlags::CLASS)
        {
            self.serialize_interface(symbol, &symbol_name, modifier_flags)?;
        }
        if symbol_data.flags.intersects(SymbolFlags::ALIAS) {
            let local_name = self.get_internal_symbol_name(symbol, &symbol_name);
            self.serialize_as_alias(symbol, &local_name, modifier_flags)?;
        }
        if symbol_data.flags.intersects(SymbolFlags::PROPERTY)
            && escaped_symbol_name == tsc_types::InternalSymbolName::EXPORT_EQUALS
        {
            let _ = self.serialize_maybe_alias_assignment(symbol)?;
        }
        if symbol_data.flags.intersects(SymbolFlags::EXPORT_STAR) {
            for declaration in symbol_data.declarations {
                let statement = declaration_ancestor(self.checker, declaration);
                if self.checker.kind_of(statement) == SyntaxKind::ExportDeclaration {
                    self.add_cloned_parse_statement(statement)?;
                }
            }
        }
        if needs_post_export_default {
            let internal = self.get_internal_symbol_name(symbol, &symbol_name);
            add_approximate_length(self.context, 16 + internal.encode_utf16().count());
            let expression = create_identifier(self.arena, self.target, &internal)?;
            let assignment = create_export_assignment(self.arena, self.target, false, expression)?;
            self.add_result(assignment, ModifierFlags::NONE)?;
        } else if needs_export_declaration {
            let internal = self.get_internal_symbol_name(symbol, &symbol_name);
            add_approximate_length(
                self.context,
                22 + symbol_name.encode_utf16().count() + internal.encode_utf16().count(),
            );
            self.serialize_export_specifier(&symbol_name, &internal, None)?;
        }
        Ok(())
    }

    fn variable_statement_range(&self, symbol: SymbolId) -> Option<NodeId> {
        let declaration = self
            .checker
            .binder
            .symbol(symbol)
            .declarations
            .iter()
            .copied()
            .find(|&declaration| {
                self.checker.kind_of(declaration) == SyntaxKind::VariableDeclaration
            })?;
        let list = self.checker.parent_of(declaration)?;
        let statement = self.checker.parent_of(list)?;
        let one_declaration = match self.checker.data_of(list) {
            NodeData::VariableDeclarationList(data) => {
                self.checker.nodes_of(data.declarations).len() == 1
            }
            _ => false,
        };
        Some(
            if one_declaration && self.checker.kind_of(statement) == SyntaxKind::VariableStatement {
                statement
            } else {
                declaration
            },
        )
    }

    fn add_cloned_parse_statement(&mut self, statement: NodeId) -> BuildResult<()> {
        if !self.emitted_parse_statements.insert(statement) {
            return Ok(());
        }
        if let Some(statement) = clone_parse_node(self.checker, self.arena, statement)? {
            self.add_result(statement, ModifierFlags::NONE)?;
        }
        Ok(())
    }

    /// tsc-port: includePrivateSymbol @6.0.3
    /// tsc-hash: 47e3cfac44d1da24d0576ff68a44b9145c0b4d6d090ec814e2fb751ef8a09906
    /// tsc-span: _tsc.js:54180-54186
    fn include_private_symbol(&mut self, symbol: SymbolId) {
        if self
            .checker
            .binder
            .symbol(symbol)
            .declarations
            .iter()
            .any(|&declaration| {
                node_util::is_part_of_parameter_declaration(
                    self.checker.binder.source_of_node(declaration),
                    declaration,
                )
            })
        {
            return;
        }
        let name = tsc_binder::unescape_leading_underscores(
            &self.checker.binder.symbol(symbol).escaped_name,
        )
        .to_owned();
        let _ = self.get_unused_name(&name, Some(symbol));
        let is_external_import_alias = self
            .checker
            .symbol_flags(symbol)
            .intersects(SymbolFlags::ALIAS)
            && !self
                .checker
                .binder
                .symbol(symbol)
                .declarations
                .iter()
                .copied()
                .any(|declaration| {
                    let statement = declaration_ancestor(self.checker, declaration);
                    matches!(
                        self.checker.kind_of(statement),
                        SyntaxKind::ExportDeclaration | SyntaxKind::NamespaceExportDeclaration
                    ) || self.checker.kind_of(statement) == SyntaxKind::ImportEqualsDeclaration
                        && match self.checker.data_of(statement) {
                            NodeData::ImportEqualsDeclaration(data) => {
                                data.module_reference.is_some_and(|reference| {
                                    self.checker.kind_of(reference)
                                        != SyntaxKind::ExternalModuleReference
                                })
                            }
                            _ => false,
                        }
                });
        let stack = if is_external_import_alias {
            self.deferred_privates_stack.first_mut()
        } else {
            self.deferred_privates_stack.last_mut()
        };
        let Some(stack) = stack else {
            return;
        };
        if !stack.contains(&symbol) {
            stack.push(symbol);
        }
    }

    /// tsc-port: isExportingScope @6.0.3
    /// tsc-hash: c5e869e20b2780c6401b7722dde00d9f5cc14acde0ac87489d655ea0ff07e416
    /// tsc-span: _tsc.js:54187-54189
    fn is_exporting_scope(&self, declaration: NodeId) -> bool {
        if self.checker.kind_of(declaration) == SyntaxKind::SourceFile {
            return self
                .checker
                .binder
                .is_external_or_common_js_module_of_node(declaration)
                || self
                    .checker
                    .binder
                    .source_of_node(declaration)
                    .file_name
                    .ends_with(".json");
        }
        self.checker.kind_of(declaration) == SyntaxKind::ModuleDeclaration
            && node_util::is_ambient_module(
                self.checker.binder.source_of_node(declaration),
                declaration,
            )
            && !node_util::is_global_scope_augmentation(
                self.checker.binder.source_of_node(declaration),
                declaration,
            )
    }

    /// tsc-port: addResult @6.0.3
    /// tsc-hash: 1d5bbaa5bc6714a9dcb56eb0347501e0ebd1c79db08073b2b679e32987b7bee1
    /// tsc-span: _tsc.js:54190-54210
    fn add_result(
        &mut self,
        mut node: TransformNode,
        additional_modifier_flags: ModifierFlags,
    ) -> BuildResult<()> {
        if modifiers_of(&self.arena.node(node).map_err(factory_error)?.data).is_some()
            || matches!(
                self.arena.node(node).map_err(factory_error)?.kind,
                SyntaxKind::ClassDeclaration
                    | SyntaxKind::EnumDeclaration
                    | SyntaxKind::FunctionDeclaration
                    | SyntaxKind::InterfaceDeclaration
                    | SyntaxKind::ModuleDeclaration
                    | SyntaxKind::TypeAliasDeclaration
                    | SyntaxKind::VariableStatement
            )
        {
            let old = self.effective_modifier_flags(node)?;
            let mut new = ModifierFlags::NONE;
            let enclosing = self.context.enclosing_declaration;
            if additional_modifier_flags.intersects(ModifierFlags::EXPORT)
                && enclosing.is_some_and(|enclosing| {
                    self.is_exporting_scope(enclosing)
                        || self.checker.kind_of(enclosing) == SyntaxKind::ModuleDeclaration
                })
                && self.can_have_export_modifier(node)?
            {
                new |= ModifierFlags::EXPORT;
            }
            let ambient_context = enclosing.is_some_and(|enclosing| {
                NodeFlags::from_bits(self.checker.node_flags(enclosing))
                    .intersects(NodeFlags::AMBIENT)
            });
            if self.adding_declare
                && !new.intersects(ModifierFlags::EXPORT)
                && !ambient_context
                && matches!(
                    self.arena.node(node).map_err(factory_error)?.kind,
                    SyntaxKind::EnumDeclaration
                        | SyntaxKind::VariableStatement
                        | SyntaxKind::FunctionDeclaration
                        | SyntaxKind::ClassDeclaration
                        | SyntaxKind::ModuleDeclaration
                )
            {
                new |= ModifierFlags::AMBIENT;
            }
            if additional_modifier_flags.intersects(ModifierFlags::DEFAULT)
                && matches!(
                    self.arena.node(node).map_err(factory_error)?.kind,
                    SyntaxKind::ClassDeclaration
                        | SyntaxKind::InterfaceDeclaration
                        | SyntaxKind::FunctionDeclaration
                )
            {
                new |= ModifierFlags::DEFAULT;
            }
            let combined = ModifierFlags::from_bits(new.bits() | old.bits());
            if !new.is_empty() {
                node = replace_modifiers(self.arena, self.target, node, combined)?;
            }
            add_approximate_length(self.context, modifiers_length(combined));
        }
        self.results.push(node);
        Ok(())
    }

    /// tsc-port: serializeTypeAlias @6.0.3
    /// tsc-hash: 3f680b229b027017d227c09fec93ac0536eb88d62202961ba0e4bc80a1aa2570
    /// tsc-span: _tsc.js:54211-54240
    fn serialize_type_alias(
        &mut self,
        symbol: SymbolId,
        symbol_name: &str,
        modifier_flags: ModifierFlags,
    ) -> BuildResult<()> {
        let alias_type = self
            .checker
            .get_declared_type_of_type_alias(symbol)
            .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
        let parameters = self
            .checker
            .get_local_type_parameters_of_class_or_interface_or_type_alias(symbol);
        let mut parameter_nodes = Vec::with_capacity(parameters.len());
        for parameter in parameters {
            parameter_nodes.push(type_parameter_to_declaration(
                self.checker,
                self.arena,
                self.target,
                parameter,
                self.context,
                None,
            )?);
        }
        let restore = save_restore_flags(self.context);
        self.context.flags.0 |= IN_TYPE_ALIAS;
        let type_node = type_to_type_node_helper(
            self.checker,
            self.arena,
            self.target,
            alias_type,
            self.context,
        )?;
        restore_flags(self.context, restore);
        let internal_name = self.get_internal_symbol_name(symbol, symbol_name);
        add_approximate_length(self.context, 8 + internal_name.encode_utf16().count());
        let name = create_identifier(self.arena, self.target, &internal_name)?;
        let type_parameters = array(self.arena, self.target, parameter_nodes)?;
        let declaration = create_node(
            self.arena,
            self.target,
            NodeData::TypeAliasDeclaration(TypeAliasDeclarationData {
                name: Some(name.node()),
                modifiers: None,
                type_parameters,
                r#type: type_node.map(TransformNode::node),
            }),
        )?;
        self.add_result(declaration, modifier_flags)
    }

    /// tsc-port: serializeInterface @6.0.3
    /// tsc-hash: e1fdd40dc220944b36b9f034eb5cb985249c346db44cdd1453db2d6f7431e901
    /// tsc-span: _tsc.js:54241-54270
    fn serialize_interface(
        &mut self,
        symbol: SymbolId,
        symbol_name: &str,
        modifier_flags: ModifierFlags,
    ) -> BuildResult<()> {
        let internal_name = self.get_internal_symbol_name(symbol, symbol_name);
        add_approximate_length(self.context, 14 + internal_name.encode_utf16().count());
        let interface_type = self
            .checker
            .get_declared_type_of_class_or_interface(symbol)
            .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
        let parameters = self
            .checker
            .get_local_type_parameters_of_class_or_interface_or_type_alias(symbol);
        let mut parameter_nodes = Vec::with_capacity(parameters.len());
        for parameter in parameters {
            parameter_nodes.push(type_parameter_to_declaration(
                self.checker,
                self.arena,
                self.target,
                parameter,
                self.context,
                None,
            )?);
        }
        let base_types = self
            .checker
            .get_base_types(interface_type)
            .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
        let base_type = match base_types.as_slice() {
            [] => None,
            [base] => Some(*base),
            bases => Some(
                self.checker
                    .get_intersection_type(bases, IntersectionFlags::NONE)
                    .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?,
            ),
        };
        let properties = self
            .checker
            .get_properties_of_type(interface_type)
            .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
        let members = self.serialize_property_symbols_for_class_or_interface(
            &properties,
            false,
            base_type,
            false,
        )?;
        let calls = self.serialize_signatures(
            SignatureKind::Call,
            interface_type,
            base_type,
            SyntaxKind::CallSignature,
        )?;
        let constructs = self.serialize_signatures(
            SignatureKind::Construct,
            interface_type,
            base_type,
            SyntaxKind::ConstructSignature,
        )?;
        let indexes = self.serialize_index_signatures(interface_type, base_type)?;
        let mut all_members = Vec::new();
        all_members.extend(indexes);
        all_members.extend(constructs);
        all_members.extend(calls);
        all_members.extend(members);
        let mut base_nodes = Vec::new();
        for base in base_types {
            if let Some(base) =
                self.try_serialize_as_type_reference(base, EmitSymbolMeaning::VALUE_EXPORT_VALUE)?
            {
                base_nodes.push(base);
            }
        }
        let heritage = if base_nodes.is_empty() {
            None
        } else {
            let types = required_array(self.arena, self.target, base_nodes)?;
            let clause = create_node(
                self.arena,
                self.target,
                NodeData::HeritageClause(HeritageClauseData {
                    token: SyntaxKind::ExtendsKeyword,
                    types: Some(types),
                }),
            )?;
            Some(required_array(self.arena, self.target, vec![clause])?)
        };
        let name = create_identifier(self.arena, self.target, &internal_name)?;
        let type_parameters = array(self.arena, self.target, parameter_nodes)?;
        let members = required_array(self.arena, self.target, all_members)?;
        let declaration = create_node(
            self.arena,
            self.target,
            NodeData::InterfaceDeclaration(InterfaceDeclarationData {
                name: Some(name.node()),
                modifiers: None,
                type_parameters,
                heritage_clauses: heritage,
                members: Some(members),
            }),
        )?;
        self.add_result(declaration, modifier_flags)
    }

    /// tsc-port: serializePropertySymbolsForClassOrInterface @6.0.3
    /// tsc-hash: 79ecea32341ad7dd8f48307e4197380fd9d764026f6043f7a288b9029b8d6616
    /// tsc-span: _tsc.js:54271-54297
    fn serialize_property_symbols_for_class_or_interface(
        &mut self,
        properties: &[SymbolId],
        is_class: bool,
        base_type: Option<TypeId>,
        is_static: bool,
    ) -> BuildResult<Vec<TransformNode>> {
        let mut elements = Vec::new();
        for (index, property) in properties.iter().copied().enumerate() {
            let ordinal = index + 1;
            if check_truncation_length_if_expanding(self.context)
                && ordinal + 2 < properties.len().saturating_sub(1)
            {
                self.context.out.truncated = true;
                elements.push(self.create_truncation_property(
                    &format!("... {} more ... ", properties.len().saturating_sub(ordinal)),
                    is_class,
                )?);
                if let Some(last) = properties.last().copied() {
                    elements.extend(self.make_serialize_property_symbol(
                        last, is_static, base_type, is_class, is_class,
                    )?);
                }
                break;
            }
            add_approximate_length(self.context, 1);
            elements.extend(self.make_serialize_property_symbol(
                property, is_static, base_type, is_class, is_class,
            )?);
        }
        Ok(elements)
    }

    /// tsc-port: createTruncationProperty @6.0.3
    /// tsc-hash: 8a285791d7a50780c9d3e8bb782603cd94b4f2b9cd4bc7b2863182f3383cdcc8
    /// tsc-span: _tsc.js:54298-54321
    fn create_truncation_property(
        &mut self,
        text: &str,
        is_class: bool,
    ) -> BuildResult<TransformNode> {
        let name = create_string_literal(self.arena, self.target, text)?;
        let node = if is_class {
            create_node(
                self.arena,
                self.target,
                NodeData::PropertyDeclaration(PropertyDeclarationData {
                    name: Some(name.node()),
                    modifiers: None,
                    question_token: None,
                    exclamation_token: None,
                    r#type: None,
                    initializer: None,
                }),
            )?
        } else {
            create_node(
                self.arena,
                self.target,
                NodeData::PropertySignature(PropertySignatureData {
                    name: Some(name.node()),
                    question_token: None,
                    modifiers: None,
                    r#type: None,
                    initializer: None,
                }),
            )?
        };
        if self
            .context
            .flags
            .contains(EmitNodeBuilderFlags::NO_TRUNCATION)
        {
            self.arena
                .metadata_mut(node)
                .add_leading_comment(SyntheticComment::new(
                    SyntheticCommentKind::MultiLine,
                    text,
                    false,
                    false,
                ));
        }
        Ok(node)
    }

    /// tsc-port: getNamespaceMembersForSerialization @6.0.3
    /// tsc-hash: 57ba9de7ff91ae7782eee4f92b553b7e8c63f36d3b49fcc011245075b9e46b02
    /// tsc-span: _tsc.js:54322-54335
    fn get_namespace_members_for_serialization(
        &mut self,
        symbol: SymbolId,
    ) -> BuildResult<Vec<SymbolId>> {
        let exports = self
            .checker
            .get_exports_of_symbol(symbol)
            .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
        let mut members = Vec::new();
        let mut seen = HashSet::new();
        for member in exports.values().copied() {
            if seen.insert(member)
                && self.is_namespace_member(member)
                && tsc_syntax::is_identifier_text(tsc_binder::unescape_leading_underscores(
                    &self.checker.binder.symbol(member).escaped_name,
                ))
            {
                members.push(member);
            }
        }
        let merged = self.checker.get_merged_symbol(symbol);
        if merged != symbol {
            let exports = self
                .checker
                .get_exports_of_symbol(merged)
                .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
            for member in exports.values().copied() {
                let resolved = if self
                    .checker
                    .symbol_flags(member)
                    .intersects(SymbolFlags::ALIAS)
                {
                    self.checker
                        .resolve_alias(member)
                        .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?
                } else {
                    member
                };
                if !self
                    .checker
                    .symbol_flags(resolved)
                    .intersects(SymbolFlags::VALUE)
                    && seen.insert(member)
                    && self.is_namespace_member(member)
                {
                    members.push(member);
                }
            }
        }
        Ok(members)
    }

    /// tsc-port: isTypeOnlyNamespace @6.0.3
    /// tsc-hash: 819f84ba477e18b4b2b2d616d7d3f47993999c3cda24f94900d82ad82e9f2e1b
    /// tsc-span: _tsc.js:54336-54338
    fn is_type_only_namespace(&mut self, symbol: SymbolId) -> BuildResult<bool> {
        for member in self.get_namespace_members_for_serialization(symbol)? {
            let resolved = if self
                .checker
                .symbol_flags(member)
                .intersects(SymbolFlags::ALIAS)
            {
                self.checker
                    .resolve_alias(member)
                    .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?
            } else {
                member
            };
            if self
                .checker
                .symbol_flags(resolved)
                .intersects(SymbolFlags::VALUE)
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// tsc-port: serializeModule @6.0.3
    /// tsc-hash: 1e332e6c96670e8c64f234b92e1ab6cf63589dbc1586e841b60e85960e0cd5e2
    /// tsc-span: _tsc.js:54339-54407
    fn serialize_module(
        &mut self,
        symbol: SymbolId,
        symbol_name: &str,
        modifier_flags: ModifierFlags,
    ) -> BuildResult<()> {
        let members = self.get_namespace_members_for_serialization(symbol)?;
        let expanding = is_expanding(self.context);
        let (mut real, mut merged) = (Vec::new(), Vec::new());
        for member in members {
            if self.checker.binder.symbol(member).parent == Some(symbol) || expanding {
                real.push(member);
            } else {
                merged.push(member);
            }
        }
        if !real.is_empty() || expanding {
            let local = self.get_internal_symbol_name(symbol, symbol_name);
            add_approximate_length(self.context, local.encode_utf16().count());
            let name = create_identifier(self.arena, self.target, &local)?;
            let suppress = self
                .checker
                .symbol_flags(symbol)
                .intersects(SymbolFlags::FUNCTION | SymbolFlags::ASSIGNMENT);
            self.serialize_as_namespace_declaration(&real, name, modifier_flags, suppress)?;
        }
        if !merged.is_empty() {
            let local = self.get_internal_symbol_name(symbol, symbol_name);
            let mut specifiers = Vec::new();
            for member in merged.drain(..) {
                if self.checker.binder.symbol(member).escaped_name
                    == tsc_types::InternalSymbolName::EXPORT_EQUALS
                {
                    continue;
                }
                let name = tsc_binder::unescape_leading_underscores(
                    &self.checker.binder.symbol(member).escaped_name,
                )
                .to_owned();
                let member_internal_name = self.get_internal_symbol_name(member, &name);
                let alias_declaration = self.checker.get_declaration_of_alias_symbol(member);
                let containing_file = self
                    .enclosing_declaration
                    .map(|declaration| self.checker.binder.source_of_node(declaration).root);
                let is_nonlocal = match alias_declaration {
                    Some(declaration) => containing_file.is_some_and(|file| {
                        self.checker.binder.source_of_node(declaration).root != file
                    }),
                    None => containing_file.is_some_and(|file| {
                        !self
                            .checker
                            .binder
                            .symbol(member)
                            .declarations
                            .iter()
                            .copied()
                            .any(|declaration| {
                                self.checker.binder.source_of_node(declaration).root == file
                            })
                    }),
                };
                if is_nonlocal {
                    if let Some(file) = containing_file {
                        self.context.tracker.report_nonlocal_augmentation(
                            &mut self.context.reported_diagnostic,
                            file,
                            symbol,
                            member,
                        );
                    }
                    continue;
                }
                let target = if self
                    .checker
                    .symbol_flags(member)
                    .intersects(SymbolFlags::ALIAS)
                {
                    self.checker
                        .get_immediate_aliased_symbol(member)
                        .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?
                        .unwrap_or(member)
                } else {
                    member
                };
                self.include_private_symbol(target);
                let target_symbol_name = tsc_binder::unescape_leading_underscores(
                    &self.checker.binder.symbol(target).escaped_name,
                )
                .to_owned();
                let target_name = if target == member {
                    member_internal_name
                } else {
                    self.get_internal_symbol_name(target, &target_symbol_name)
                };
                let property = (name != target_name)
                    .then(|| create_identifier(self.arena, self.target, &target_name))
                    .transpose()?;
                let name_node = create_identifier(self.arena, self.target, &name)?;
                specifiers.push(create_export_specifier_node(
                    self.arena,
                    self.target,
                    property,
                    name_node,
                )?);
            }
            let export =
                create_named_export_declaration(self.arena, self.target, specifiers, None)?;
            let name = create_identifier(self.arena, self.target, &local)?;
            let declaration =
                create_module_declaration(self.arena, self.target, name, vec![export], true)?;
            self.add_result(declaration, ModifierFlags::NONE)?;
        }
        Ok(())
    }

    /// tsc-port: serializeEnum @6.0.3
    /// tsc-hash: b325c26b84a08d8f70a33b8536557373d6bf5d264eb1dba4d0729161a12c3aed
    /// tsc-span: _tsc.js:54408-54457
    fn serialize_enum(
        &mut self,
        symbol: SymbolId,
        symbol_name: &str,
        modifier_flags: ModifierFlags,
    ) -> BuildResult<()> {
        let internal = self.get_internal_symbol_name(symbol, symbol_name);
        add_approximate_length(self.context, 9 + internal.encode_utf16().count());
        let mut member_symbols: Vec<SymbolId> = self
            .checker
            .binder
            .symbol(symbol)
            .exports
            .values()
            .copied()
            .filter(|&member| {
                self.checker
                    .symbol_flags(member)
                    .intersects(SymbolFlags::ENUM_MEMBER)
            })
            .collect();
        if member_symbols.is_empty() {
            member_symbols = self
                .checker
                .binder
                .symbol(symbol)
                .members
                .values()
                .copied()
                .filter(|&member| {
                    self.checker
                        .symbol_flags(member)
                        .intersects(SymbolFlags::ENUM_MEMBER)
                })
                .collect();
        }
        let mut members = Vec::new();
        let mut index = 0;
        while index < member_symbols.len() {
            let ordinal = index + 1;
            let mut serialized_last_after_truncation = false;
            if check_truncation_length_if_expanding(self.context)
                && ordinal + 2 < member_symbols.len().saturating_sub(1)
            {
                self.context.out.truncated = true;
                let name = create_string_literal(
                    self.arena,
                    self.target,
                    format!(
                        " ... {} more ... ",
                        member_symbols.len().saturating_sub(ordinal)
                    ),
                )?;
                members.push(create_node(
                    self.arena,
                    self.target,
                    NodeData::EnumMember(EnumMemberData {
                        name: Some(name.node()),
                        initializer: None,
                    }),
                )?);
                index = member_symbols.len() - 1;
                serialized_last_after_truncation = true;
            }
            let member = member_symbols[index];
            let data = self.checker.binder.symbol(member).clone();
            let declaration =
                data.declarations.iter().copied().find(|&declaration| {
                    self.checker.kind_of(declaration) == SyntaxKind::EnumMember
                });
            let name = chains_get_property_name_node_for_symbol(
                self.checker,
                self.arena,
                self.target,
                self.context,
                member,
            )?;
            let initializer = if is_expanding(self.context) {
                declaration
                    .and_then(|declaration| match self.checker.data_of(declaration) {
                        NodeData::EnumMember(data) => data.initializer,
                        _ => None,
                    })
                    .map(|initializer| clone_parse_node(self.checker, self.arena, initializer))
                    .transpose()?
                    .flatten()
            } else if let Some(declaration) = declaration {
                match self
                    .checker
                    .get_enum_member_value(declaration)
                    .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?
                    .value
                {
                    Some(EvalValue::Str(value)) => {
                        Some(create_string_literal(self.arena, self.target, value)?)
                    }
                    Some(EvalValue::Num(value)) => {
                        Some(create_numeric_literal(self.arena, self.target, value)?)
                    }
                    None => None,
                }
            } else {
                None
            };
            let member_name = tsc_binder::unescape_leading_underscores(&data.escaped_name);
            add_approximate_length(
                self.context,
                4 + member_name.encode_utf16().count()
                    + initializer
                        .and_then(|node| self.arena.node(node).ok())
                        .map_or(0, |node| match &node.data {
                            NodeData::StringLiteral(data) => data.text.encode_utf16().count(),
                            NodeData::NumericLiteral(data) => data.text.encode_utf16().count(),
                            _ => 0,
                        }),
            );
            members.push(create_node(
                self.arena,
                self.target,
                NodeData::EnumMember(EnumMemberData {
                    name: Some(name.node()),
                    initializer: initializer.map(TransformNode::node),
                }),
            )?);
            if serialized_last_after_truncation {
                break;
            }
            index += 1;
        }
        let name = create_identifier(self.arena, self.target, &internal)?;
        let const_modifier = if self
            .checker
            .symbol_flags(symbol)
            .intersects(SymbolFlags::CONST_ENUM)
        {
            ModifierFlags::CONST
        } else {
            ModifierFlags::NONE
        };
        let enum_modifiers = create_modifiers_from_flags(self.arena, self.target, const_modifier)?;
        let members = required_array(self.arena, self.target, members)?;
        let declaration = create_node(
            self.arena,
            self.target,
            NodeData::EnumDeclaration(EnumDeclarationData {
                name: Some(name.node()),
                modifiers: enum_modifiers,
                members: Some(members),
            }),
        )?;
        self.add_result(declaration, modifier_flags)
    }

    /// tsc-port: serializeAsFunctionNamespaceMerge @6.0.3
    /// tsc-hash: 31d4121617baea5146ed7516eaccff1fd2139d7f938bf5008f5b3516dfccd97e
    /// tsc-span: _tsc.js:54458-54476
    fn serialize_as_function_namespace_merge(
        &mut self,
        r#type: TypeId,
        symbol: SymbolId,
        local_name: &str,
        modifier_flags: ModifierFlags,
    ) -> BuildResult<()> {
        let signatures = self
            .checker
            .get_signatures_of_type(r#type, SignatureKind::Call)
            .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
        for signature in signatures {
            add_approximate_length(self.context, 1);
            let name = create_identifier(self.arena, self.target, local_name)?;
            let declaration = signature_to_signature_declaration_helper(
                self.checker,
                self.arena,
                self.target,
                signature,
                SyntaxKind::FunctionDeclaration,
                self.context,
                Some(SignatureDeclarationOptions {
                    name: Some(name),
                    ..SignatureDeclarationOptions::default()
                }),
            )?;
            let location = self
                .get_signature_text_range_location(signature)
                .map(|node| project_parse_node(self.checker, self.arena, node))
                .transpose()?
                .flatten();
            let declaration = set_text_range2(
                self.checker,
                self.arena,
                self.context,
                declaration,
                location,
            )?;
            self.add_result(declaration, modifier_flags)?;
        }
        let symbol_data = self.checker.binder.symbol(symbol).clone();
        if !symbol_data
            .flags
            .intersects(SymbolFlags::VALUE_MODULE | SymbolFlags::NAMESPACE_MODULE)
            || symbol_data.exports.is_empty()
        {
            let properties = self
                .checker
                .get_properties_of_type(r#type)
                .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
            let properties: Vec<SymbolId> = properties
                .into_iter()
                .filter(|&property| self.is_namespace_member(property))
                .collect();
            add_approximate_length(self.context, local_name.encode_utf16().count());
            let name = create_identifier(self.arena, self.target, local_name)?;
            self.serialize_as_namespace_declaration(&properties, name, modifier_flags, true)?;
        }
        Ok(())
    }

    /// tsc-port: createTruncationStatement @6.0.3
    /// tsc-hash: c90f0481088d42921c1c57477593ec039069294f731723eae2b9a776c254e6f9
    /// tsc-span: _tsc.js:54477-54482
    fn create_truncation_statement(&mut self, text: &str) -> BuildResult<TransformNode> {
        if self
            .context
            .flags
            .contains(EmitNodeBuilderFlags::NO_TRUNCATION)
        {
            let statement = create_node(
                self.arena,
                self.target,
                NodeData::EmptyStatement(EmptyStatementData {}),
            )?;
            self.arena
                .metadata_mut(statement)
                .add_leading_comment(SyntheticComment::new(
                    SyntheticCommentKind::MultiLine,
                    text,
                    false,
                    false,
                ));
            Ok(statement)
        } else {
            let expression = create_identifier(self.arena, self.target, text)?;
            create_node(
                self.arena,
                self.target,
                NodeData::ExpressionStatement(ExpressionStatementData {
                    expression: Some(expression.node()),
                }),
            )
        }
    }

    /// tsc-port: getSignatureTextRangeLocation @6.0.3
    /// tsc-hash: 73ccdbcf6a52d23d85ad0136f48e14849f39d1d31ed25842fb665495d0f1f4dc
    /// tsc-span: _tsc.js:54483-54493
    fn get_signature_text_range_location(&self, signature: SignatureId) -> Option<NodeId> {
        let declaration = self.checker.signature_of(signature).declaration?;
        let parent = self.checker.parent_of(declaration)?;
        if self.checker.kind_of(parent) == SyntaxKind::BinaryExpression {
            return Some(parent);
        }
        if self.checker.kind_of(parent) == SyntaxKind::VariableDeclaration {
            return self.checker.parent_of(parent).or(Some(parent));
        }
        Some(declaration)
    }

    /// tsc-port: serializeAsNamespaceDeclaration @6.0.3
    /// tsc-hash: 1911f6ac2b33198d3a9382ec2faf24b6cbf59aba22c50b2b5a2a41f607ec2cf9
    /// tsc-span: _tsc.js:54494-54561
    fn serialize_as_namespace_declaration(
        &mut self,
        properties: &[SymbolId],
        local_name: TransformNode,
        modifier_flags: ModifierFlags,
        suppress_new_private_context: bool,
    ) -> BuildResult<()> {
        let expanding = is_expanding(self.context);
        if properties.is_empty() && !expanding {
            return Ok(());
        }
        add_approximate_length(self.context, 14);
        let local: Vec<SymbolId> = properties
            .iter()
            .copied()
            .filter(|&property| {
                self.checker.binder.symbol(property).declarations.is_empty()
                    || self.context.enclosing_file.is_none()
                    || self
                        .checker
                        .binder
                        .symbol(property)
                        .declarations
                        .iter()
                        .any(|&declaration| {
                            Some(self.checker.binder.source_of_node(declaration).root)
                                == self.context.enclosing_file
                        })
                    || expanding
            })
            .collect();

        // Upstream installs a synthesized fakespace with setParent, locals,
        // and the owning property symbol before recursively visiting the
        // namespace table. Keep its identity and lookup overlay explicit.
        let mut table = SymbolTable::default();
        for property in local {
            table.insert(
                self.checker.binder.symbol(property).escaped_name.clone(),
                property,
            );
        }
        let old_results = std::mem::take(&mut self.results);
        let old_adding_declare = self.adding_declare;
        let old_enclosing = self.context.enclosing_declaration;
        let old_enclosing_is_synthetic = self.context.enclosing_declaration_is_synthetic;
        let old_synthetic_scope_locals = self.context.synthetic_scope_locals.clone();
        self.adding_declare = false;
        self.context.enclosing_declaration_is_synthetic = true;
        self.context.synthetic_scope_locals = Some(
            table
                .iter()
                .map(|(name, &symbol)| (name.clone(), symbol))
                .collect(),
        );
        let visit_result = self.visit_symbol_table(&table, suppress_new_private_context, true);
        self.context.enclosing_declaration = old_enclosing;
        self.context.enclosing_declaration_is_synthetic = old_enclosing_is_synthetic;
        self.context.synthetic_scope_locals = old_synthetic_scope_locals;
        self.adding_declare = old_adding_declare;
        let serialized_declarations = std::mem::replace(&mut self.results, old_results);
        visit_result?;
        let mut declarations = Vec::with_capacity(serialized_declarations.len());
        for declaration in serialized_declarations {
            let replacement_expression =
                match &self.arena.node(declaration).map_err(factory_error)?.data {
                    NodeData::ExportAssignment(data) if data.is_export_equals != Some(true) => data
                        .expression
                        .and_then(|expression| self.node_from_id(expression))
                        .filter(|&expression| {
                            self.arena
                                .node(expression)
                                .is_ok_and(|node| node.kind == SyntaxKind::Identifier)
                        }),
                    _ => None,
                };
            if let Some(expression) = replacement_expression {
                let name = create_identifier(
                    self.arena,
                    self.target,
                    tsc_types::InternalSymbolName::DEFAULT,
                )?;
                let specifier =
                    create_export_specifier_node(self.arena, self.target, Some(expression), name)?;
                declarations.push(create_named_export_declaration(
                    self.arena,
                    self.target,
                    vec![specifier],
                    None,
                )?);
            } else {
                declarations.push(declaration);
            }
        }
        if declarations.iter().all(|&declaration| {
            self.effective_modifier_flags(declaration)
                .is_ok_and(|flags| flags.intersects(ModifierFlags::EXPORT))
        }) {
            declarations = declarations
                .into_iter()
                .map(|declaration| self.remove_export_modifier(declaration))
                .collect::<BuildResult<Vec<_>>>()?;
        }
        let namespace =
            create_module_declaration(self.arena, self.target, local_name, declarations, true)?;
        self.add_result(namespace, modifier_flags)
    }

    /// tsc-port: isNamespaceMember @6.0.3
    /// tsc-hash: c67f99f0f91940f9483d5817d5ff9657c73fed0db9ff8ce36994e4ff4e2b5dec
    /// tsc-span: _tsc.js:54562-54564
    fn is_namespace_member(&self, property: SymbolId) -> bool {
        let data = self.checker.binder.symbol(property);
        if data
            .flags
            .intersects(SymbolFlags::TYPE | SymbolFlags::NAMESPACE | SymbolFlags::ALIAS)
        {
            return true;
        }
        if data.flags.intersects(SymbolFlags::PROTOTYPE) || data.escaped_name == "prototype" {
            return false;
        }
        !data.value_declaration.is_some_and(|declaration| {
            self.checker.has_static_modifier(declaration)
                && self
                    .checker
                    .parent_of(declaration)
                    .is_some_and(|parent| is_class_like(self.checker.kind_of(parent)))
        })
    }

    /// tsc-port: sanitizeJSDocImplements @6.0.3
    /// tsc-hash: 8ef6127728fc0f5d33791a61d92b478c788836a0a73be6f408bfb2cfe423d30e
    /// tsc-span: _tsc.js:54565-54599
    fn sanitize_jsdoc_implements(
        &mut self,
        declaration: NodeId,
    ) -> BuildResult<Vec<TransformNode>> {
        let clauses = match self.checker.data_of(declaration) {
            NodeData::ClassDeclaration(data) => data.heritage_clauses,
            NodeData::ClassExpression(data) => data.heritage_clauses,
            _ => None,
        };
        let mut result = Vec::new();
        for clause in self.checker.nodes_of(clauses) {
            let NodeData::HeritageClause(data) = self.checker.data_of(clause) else {
                continue;
            };
            if data.token != SyntaxKind::ImplementsKeyword {
                continue;
            }
            for element in self.checker.nodes_of(data.types) {
                let old = self.context.enclosing_declaration;
                self.context.enclosing_declaration = Some(element);
                let cloned = clone_parse_node(self.checker, self.arena, element)?;
                result.extend(self.cleanup(old, cloned));
            }
        }
        Ok(result)
    }

    /// tsc-port: sanitizeJSDocImplements.cleanup @6.0.3
    /// tsc-hash: d23a9e5db21b7795490fda5b4ff44840d5cf20e730948fa099e41b18395bf189
    /// tsc-span: _tsc.js:54590-54593
    fn cleanup<T>(&mut self, old_enclosing: Option<NodeId>, result: Option<T>) -> Option<T> {
        self.context.enclosing_declaration = old_enclosing;
        result
    }

    /// tsc-port: serializeAsClass @6.0.3
    /// tsc-hash: ccb4c10cb58d4154e83303d911464ee3af0f91f4b9cc33d3d43e98266e113bff
    /// tsc-span: _tsc.js:54600-54686
    fn serialize_as_class(
        &mut self,
        symbol: SymbolId,
        local_name: &str,
        modifier_flags: ModifierFlags,
    ) -> BuildResult<()> {
        add_approximate_length(self.context, 9 + local_name.encode_utf16().count());
        let original = self
            .checker
            .binder
            .symbol(symbol)
            .declarations
            .iter()
            .copied()
            .find(|&declaration| is_class_like(self.checker.kind_of(declaration)));
        let old_enclosing = self.context.enclosing_declaration;
        self.context.enclosing_declaration = original.or(old_enclosing);
        let parameters = self
            .checker
            .get_local_type_parameters_of_class_or_interface_or_type_alias(symbol);
        let mut parameter_nodes = Vec::new();
        for parameter in parameters {
            let parameter_symbol = self.checker.tables.type_of(parameter).symbol;
            if let Some(parameter_symbol) = parameter_symbol {
                add_approximate_length(
                    self.context,
                    self.checker
                        .symbol_display_name(parameter_symbol)
                        .encode_utf16()
                        .count(),
                );
            }
            parameter_nodes.push(type_parameter_to_declaration(
                self.checker,
                self.arena,
                self.target,
                parameter,
                self.context,
                None,
            )?);
        }
        let declared = self
            .checker
            .get_declared_type_of_class_or_interface(symbol)
            .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
        let class_type = self
            .checker
            .get_type_with_this_argument(declared, None, false)
            .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
        let base_types = self
            .checker
            .get_base_types(class_type)
            .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
        let implements = match original {
            Some(original) => self.sanitize_jsdoc_implements(original)?,
            None => Vec::new(),
        };
        let static_type = self
            .checker
            .get_type_of_symbol(symbol)
            .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
        let is_class = self
            .checker
            .tables
            .type_of(static_type)
            .symbol
            .is_some_and(|symbol| {
                self.checker
                    .binder
                    .symbol(symbol)
                    .value_declaration
                    .is_some_and(|declaration| is_class_like(self.checker.kind_of(declaration)))
            });
        let static_base = if is_class {
            self.checker
                .get_base_constructor_type_of_class(static_type)
                .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?
        } else {
            self.checker.tables.intrinsics.any
        };
        add_approximate_length(
            self.context,
            usize::from(!base_types.is_empty()) * 8 + usize::from(!implements.is_empty()) * 11,
        );
        let properties = self
            .checker
            .get_properties_of_type(class_type)
            .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
        let properties = self.get_non_inherited_properties(class_type, &base_types, properties)?;
        let public: Vec<SymbolId> = properties
            .iter()
            .copied()
            .filter(|&property| !is_hash_private(self.checker, property))
            .collect();
        let private: Vec<SymbolId> = properties
            .iter()
            .copied()
            .filter(|&property| is_hash_private(self.checker, property))
            .collect();
        let private_members = if private.is_empty() {
            Vec::new()
        } else if is_expanding(self.context) {
            self.serialize_property_symbols_for_class_or_interface(
                &private,
                true,
                base_types.first().copied(),
                false,
            )?
        } else {
            add_approximate_length(self.context, 9);
            let name = create_private_identifier(self.arena, self.target, "#private")?;
            vec![create_node(
                self.arena,
                self.target,
                NodeData::PropertyDeclaration(PropertyDeclarationData {
                    name: Some(name.node()),
                    modifiers: None,
                    question_token: None,
                    exclamation_token: None,
                    r#type: None,
                    initializer: None,
                }),
            )?]
        };
        let public_members = self.serialize_property_symbols_for_class_or_interface(
            &public,
            true,
            base_types.first().copied(),
            false,
        )?;
        let static_properties: Vec<SymbolId> = self
            .checker
            .get_properties_of_type(static_type)
            .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?
            .into_iter()
            .filter(|&property| {
                !self
                    .checker
                    .symbol_flags(property)
                    .intersects(SymbolFlags::PROTOTYPE)
                    && self.checker.binder.symbol(property).escaped_name != "prototype"
                    && !self.is_namespace_member(property)
            })
            .collect();
        let static_members = self.serialize_property_symbols_for_class_or_interface(
            &static_properties,
            true,
            Some(static_base),
            true,
        )?;
        let is_non_constructable_class_like_in_js = !is_class
            && self
                .checker
                .binder
                .symbol(symbol)
                .value_declaration
                .is_some_and(|declaration| self.checker.is_in_js_file(declaration))
            && self
                .checker
                .get_signatures_of_type(static_type, SignatureKind::Construct)
                .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?
                .is_empty();
        let constructors = if is_non_constructable_class_like_in_js {
            add_approximate_length(self.context, 21);
            let parameters = required_array(self.arena, self.target, Vec::new())?;
            let modifiers =
                create_modifiers_from_flags(self.arena, self.target, ModifierFlags::PRIVATE)?;
            vec![create_node(
                self.arena,
                self.target,
                NodeData::Constructor(ConstructorData {
                    name: None,
                    type_parameters: None,
                    parameters: Some(parameters),
                    r#type: None,
                    body: None,
                    modifiers,
                }),
            )?]
        } else {
            self.serialize_signatures(
                SignatureKind::Construct,
                static_type,
                Some(static_base),
                SyntaxKind::Constructor,
            )?
        };
        let indexes = self.serialize_index_signatures(class_type, base_types.first().copied())?;
        let mut heritage = Vec::new();
        if !base_types.is_empty() {
            let mut types = Vec::new();
            for base in &base_types {
                types.push(self.serialize_base_type(*base, static_base, local_name)?);
            }
            let types = required_array(self.arena, self.target, types)?;
            heritage.push(create_node(
                self.arena,
                self.target,
                NodeData::HeritageClause(HeritageClauseData {
                    token: SyntaxKind::ExtendsKeyword,
                    types: Some(types),
                }),
            )?);
        }
        if !implements.is_empty() {
            let types = required_array(self.arena, self.target, implements)?;
            heritage.push(create_node(
                self.arena,
                self.target,
                NodeData::HeritageClause(HeritageClauseData {
                    token: SyntaxKind::ImplementsKeyword,
                    types: Some(types),
                }),
            )?);
        }
        let mut members = Vec::new();
        members.extend(indexes);
        members.extend(static_members);
        members.extend(constructors);
        members.extend(public_members);
        members.extend(private_members);
        self.context.enclosing_declaration = old_enclosing;
        let name = create_identifier(self.arena, self.target, local_name)?;
        let type_parameters = array(self.arena, self.target, parameter_nodes)?;
        let heritage_clauses = array(self.arena, self.target, heritage)?;
        let members = required_array(self.arena, self.target, members)?;
        let declaration = create_node(
            self.arena,
            self.target,
            NodeData::ClassDeclaration(ClassDeclarationData {
                name: Some(name.node()),
                type_parameters,
                heritage_clauses,
                members: Some(members),
                modifiers: None,
            }),
        )?;
        let location = original
            .map(|node| project_parse_node(self.checker, self.arena, node))
            .transpose()?
            .flatten();
        let declaration = set_text_range2(
            self.checker,
            self.arena,
            self.context,
            declaration,
            location,
        )?;
        self.add_result(declaration, modifier_flags)
    }

    /// tsc-port: getNonInheritedProperties @6.0.3
    /// tsc-hash: a6e8a378715826c11077adab4c0c40bdc54b8f9013712625f418595c29b6f9f8
    /// tsc-span: _tsc.js:85420-85435
    fn get_non_inherited_properties(
        &mut self,
        class_type: TypeId,
        base_types: &[TypeId],
        mut properties: Vec<SymbolId>,
    ) -> BuildResult<Vec<SymbolId>> {
        if base_types.is_empty() {
            return Ok(properties);
        }
        let this_type = self.checker.this_type_of_class_or_interface(class_type);
        for &base_type in base_types {
            let base_with_this = self
                .checker
                .get_type_with_this_argument(base_type, this_type, false)
                .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
            let inherited = self
                .checker
                .get_properties_of_type(base_with_this)
                .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
            properties.retain(|&existing| {
                let existing = self.checker.binder.symbol(existing);
                !inherited.iter().copied().any(|property| {
                    let property = self.checker.binder.symbol(property);
                    property.escaped_name == existing.escaped_name
                        && property.parent == existing.parent
                })
            });
        }
        Ok(properties)
    }

    fn parse_declaration_name_text(&self, declaration: NodeId) -> Option<String> {
        let name = declaration_name(self.checker, declaration)?;
        match self.checker.data_of(name) {
            NodeData::Identifier(data) => Some(data.text.clone()),
            NodeData::StringLiteral(data) => Some(data.text.clone()),
            NodeData::NumericLiteral(data) => Some(data.text.clone()),
            _ => None,
        }
    }

    /// tsc-port: getSomeTargetNameFromDeclarations @6.0.3
    /// tsc-hash: 99649f147e539a79f374ae318992457f809ff16febf6e0e51244acade67ec29e
    /// tsc-span: _tsc.js:54687-54706
    fn get_some_target_name_from_declarations(&self, declarations: &[NodeId]) -> Option<String> {
        for &declaration in declarations {
            match self.checker.data_of(declaration) {
                NodeData::ImportSpecifier(data) => {
                    let name = data.property_name.or(data.name)?;
                    return self.parse_name_text(name);
                }
                NodeData::ExportSpecifier(data) => {
                    let name = data.property_name.or(data.name)?;
                    return self.parse_name_text(name);
                }
                NodeData::ExportAssignment(data) => {
                    if let Some(name) = data.expression.and_then(|expression| {
                        let NodeData::PropertyAccessExpression(data) =
                            self.checker.data_of(expression)
                        else {
                            return None;
                        };
                        data.name
                    }) {
                        return self.parse_name_text(name);
                    }
                }
                NodeData::BinaryExpression(data) => {
                    if let Some(name) = data.right.and_then(|expression| {
                        let NodeData::PropertyAccessExpression(data) =
                            self.checker.data_of(expression)
                        else {
                            return None;
                        };
                        data.name
                    }) {
                        return self.parse_name_text(name);
                    }
                }
                _ => {
                    if let Some(name) = self.parse_declaration_name_text(declaration) {
                        return Some(name);
                    }
                }
            }
        }
        None
    }

    fn parse_name_text(&self, name: NodeId) -> Option<String> {
        match self.checker.data_of(name) {
            NodeData::Identifier(data) => Some(data.text.clone()),
            NodeData::StringLiteral(data) => Some(data.text.clone()),
            NodeData::NumericLiteral(data) => Some(data.text.clone()),
            _ => None,
        }
    }

    /// tsc-port: serializeAsAlias @6.0.3
    /// tsc-hash: 60776812c24ded3bcf5a0336651b8c0726ab37d473a3a104a5495b4937227277
    /// tsc-span: _tsc.js:54707-54946
    fn serialize_as_alias(
        &mut self,
        symbol: SymbolId,
        local_name: &str,
        modifier_flags: ModifierFlags,
    ) -> BuildResult<()> {
        let Some(declaration) = self.checker.get_declaration_of_alias_symbol(symbol) else {
            return Ok(());
        };
        let Some(target) = self
            .checker
            .get_immediate_aliased_symbol(symbol)
            .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?
        else {
            return Ok(());
        };
        let target = self.checker.get_merged_symbol(target);
        if target == self.checker.unknown_symbol {
            return Ok(());
        }
        let target_data = self.checker.binder.symbol(target).clone();
        let mut verbatim_target_name = self
            .get_some_target_name_from_declarations(
                &self.checker.binder.symbol(symbol).declarations,
            )
            .unwrap_or_else(|| {
                tsc_binder::unescape_leading_underscores(&target_data.escaped_name).to_owned()
            });
        if verbatim_target_name == tsc_types::InternalSymbolName::EXPORT_EQUALS
            && self.checker.options.allow_synthetic_default_imports == Some(true)
        {
            verbatim_target_name = tsc_types::InternalSymbolName::DEFAULT.to_owned();
        }
        let target_name = self.get_internal_symbol_name(target, &verbatim_target_name);
        self.include_private_symbol(target);

        match self.checker.kind_of(declaration) {
            SyntaxKind::BindingElement | SyntaxKind::ImportSpecifier => {
                let module_symbol = target_data.parent.unwrap_or(target);
                let generated =
                    specifier_for_module_symbol(self.checker, self.context, module_symbol, None)?;
                let specifier = if self.checker.kind_of(declaration) == SyntaxKind::BindingElement
                    || self.context.bundled
                {
                    generated
                } else {
                    self.alias_module_specifier(declaration)
                        .unwrap_or(generated)
                };
                let property_name = match self.checker.data_of(declaration) {
                    NodeData::BindingElement(data) => data.property_name,
                    NodeData::ImportSpecifier(data) => data.property_name,
                    _ => None,
                }
                .and_then(|name| self.parse_name_text(name))
                .filter(|name| name != local_name && name != &verbatim_target_name)
                .or_else(|| {
                    (local_name != verbatim_target_name).then(|| verbatim_target_name.clone())
                });
                let property_name = property_name
                    .map(|name| create_identifier(self.arena, self.target, &name))
                    .transpose()?;
                let name = create_identifier(self.arena, self.target, local_name)?;
                let element = create_node(
                    self.arena,
                    self.target,
                    NodeData::ImportSpecifier(ImportSpecifierData {
                        name: Some(name.node()),
                        property_name: property_name.map(TransformNode::node),
                        is_type_only: false,
                    }),
                )?;
                let elements = required_array(self.arena, self.target, vec![element])?;
                let named = create_node(
                    self.arena,
                    self.target,
                    NodeData::NamedImports(NamedImportsData {
                        elements: Some(elements),
                    }),
                )?;
                let module = create_string_literal(self.arena, self.target, specifier)?;
                let import = create_import_declaration(
                    self.arena,
                    self.target,
                    None,
                    Some(named),
                    module,
                    false,
                )?;
                self.add_result(import, ModifierFlags::NONE)?;
            }
            SyntaxKind::ImportEqualsDeclaration | SyntaxKind::VariableDeclaration => {
                let is_local_import = !target_data.flags.intersects(SymbolFlags::VALUE_MODULE)
                    && self.checker.kind_of(declaration) != SyntaxKind::VariableDeclaration;
                let module_reference = if is_local_import {
                    chains_symbol_to_entity_name_node(
                        self.checker,
                        self.arena,
                        self.target,
                        self.context,
                        target,
                    )?
                } else {
                    let specifier =
                        specifier_for_module_symbol(self.checker, self.context, target, None)?;
                    let specifier = create_string_literal(self.arena, self.target, specifier)?;
                    create_node(
                        self.arena,
                        self.target,
                        NodeData::ExternalModuleReference(ExternalModuleReferenceData {
                            expression: Some(specifier.node()),
                        }),
                    )?
                };
                let is_type_only = match self.checker.data_of(declaration) {
                    NodeData::ImportEqualsDeclaration(data) => data.is_type_only,
                    _ => false,
                };
                let name = create_identifier(self.arena, self.target, local_name)?;
                let import = create_node(
                    self.arena,
                    self.target,
                    NodeData::ImportEqualsDeclaration(ImportEqualsDeclarationData {
                        name: Some(name.node()),
                        modifiers: None,
                        is_type_only,
                        module_reference: Some(module_reference.node()),
                    }),
                )?;
                add_approximate_length(
                    self.context,
                    11 + local_name.encode_utf16().count()
                        + tsc_binder::unescape_leading_underscores(&target_data.escaped_name)
                            .encode_utf16()
                            .count(),
                );
                self.add_result(
                    import,
                    if is_local_import {
                        modifier_flags
                    } else {
                        ModifierFlags::NONE
                    },
                )?;
            }
            SyntaxKind::ImportClause | SyntaxKind::NamespaceImport => {
                let module_symbol = target_data.parent.unwrap_or(target);
                let generated =
                    specifier_for_module_symbol(self.checker, self.context, module_symbol, None)?;
                let specifier = if self.context.bundled {
                    generated
                } else {
                    self.alias_module_specifier(declaration)
                        .unwrap_or(generated)
                };
                let module = create_string_literal(self.arena, self.target, specifier)?;
                let name = create_identifier(self.arena, self.target, local_name)?;
                let namespace = if self.checker.kind_of(declaration) == SyntaxKind::NamespaceImport
                {
                    Some(create_node(
                        self.arena,
                        self.target,
                        NodeData::NamespaceImport(NamespaceImportData {
                            name: Some(name.node()),
                        }),
                    )?)
                } else {
                    None
                };
                let import = create_import_declaration(
                    self.arena,
                    self.target,
                    (namespace.is_none()).then_some(name),
                    namespace,
                    module,
                    false,
                )?;
                self.add_result(import, ModifierFlags::NONE)?;
            }
            SyntaxKind::NamespaceExport => {
                let specifier =
                    specifier_for_module_symbol(self.checker, self.context, target, None)?;
                let name = create_identifier(self.arena, self.target, local_name)?;
                let clause = create_node(
                    self.arena,
                    self.target,
                    NodeData::NamespaceExport(NamespaceExportData {
                        name: Some(name.node()),
                    }),
                )?;
                let module = create_string_literal(self.arena, self.target, specifier)?;
                let export = create_node(
                    self.arena,
                    self.target,
                    NodeData::ExportDeclaration(ExportDeclarationData {
                        modifiers: None,
                        is_type_only: false,
                        export_clause: Some(clause.node()),
                        module_specifier: Some(module.node()),
                        attributes: None,
                    }),
                )?;
                self.add_result(export, ModifierFlags::NONE)?;
            }
            SyntaxKind::NamespaceExportDeclaration => {
                let name = create_identifier(self.arena, self.target, local_name)?;
                let declaration = create_node(
                    self.arena,
                    self.target,
                    NodeData::NamespaceExportDeclaration(NamespaceExportDeclarationData {
                        name: Some(name.node()),
                        modifiers: None,
                    }),
                )?;
                self.add_result(declaration, ModifierFlags::NONE)?;
            }
            SyntaxKind::ExportSpecifier => {
                let specifier = self
                    .alias_module_specifier(declaration)
                    .map(|specifier| create_string_literal(self.arena, self.target, specifier))
                    .transpose()?;
                self.serialize_export_specifier(
                    local_name,
                    if specifier.is_some() {
                        &verbatim_target_name
                    } else {
                        &target_name
                    },
                    specifier,
                )?;
            }
            SyntaxKind::ExportAssignment
            | SyntaxKind::BinaryExpression
            | SyntaxKind::PropertyAccessExpression
            | SyntaxKind::ElementAccessExpression => {
                let _ = self.serialize_maybe_alias_assignment(symbol)?;
            }
            SyntaxKind::ShorthandPropertyAssignment => {
                self.serialize_export_specifier(local_name, &target_name, None)?;
            }
            _ if self
                .checker
                .kind_of(declaration_ancestor(self.checker, declaration))
                == SyntaxKind::VariableStatement =>
            {
                let variable = declaration_ancestor(self.checker, declaration);
                if self.checker.is_in_js_file(variable) {
                    if self.alias_module_specifier(declaration).is_some() {
                        let module_symbol = target_data.parent.unwrap_or(target);
                        let specifier = specifier_for_module_symbol(
                            self.checker,
                            self.context,
                            module_symbol,
                            None,
                        )?;
                        let module = create_string_literal(self.arena, self.target, specifier)?;
                        let external = create_node(
                            self.arena,
                            self.target,
                            NodeData::ExternalModuleReference(ExternalModuleReferenceData {
                                expression: Some(module.node()),
                            }),
                        )?;
                        let name = create_identifier(self.arena, self.target, local_name)?;
                        let import = create_node(
                            self.arena,
                            self.target,
                            NodeData::ImportEqualsDeclaration(ImportEqualsDeclarationData {
                                name: Some(name.node()),
                                modifiers: None,
                                is_type_only: false,
                                module_reference: Some(external.node()),
                            }),
                        )?;
                        self.add_result(import, ModifierFlags::NONE)?;
                    } else {
                        self.serialize_export_specifier(local_name, &target_name, None)?;
                    }
                } else {
                    self.add_cloned_parse_statement(variable)?;
                }
            }
            _ => {
                self.serialize_export_specifier(local_name, &target_name, None)?;
            }
        }
        Ok(())
    }

    fn alias_module_specifier(&self, declaration: NodeId) -> Option<String> {
        let mut current = Some(declaration);
        while let Some(node) = current {
            match self.checker.data_of(node) {
                NodeData::ImportDeclaration(data) => {
                    let specifier = data.module_specifier?;
                    if let NodeData::StringLiteral(data) = self.checker.data_of(specifier) {
                        return Some(data.text.clone());
                    }
                }
                NodeData::ExportDeclaration(data) => {
                    let specifier = data.module_specifier?;
                    if let NodeData::StringLiteral(data) = self.checker.data_of(specifier) {
                        return Some(data.text.clone());
                    }
                }
                NodeData::ExternalModuleReference(data) => {
                    let specifier = data.expression?;
                    if let NodeData::StringLiteral(data) = self.checker.data_of(specifier) {
                        return Some(data.text.clone());
                    }
                }
                NodeData::CallExpression(data) => {
                    for argument in self.checker.nodes_of(data.arguments) {
                        if let NodeData::StringLiteral(data) = self.checker.data_of(argument) {
                            return Some(data.text.clone());
                        }
                    }
                }
                _ => {}
            }
            current = self.checker.parent_of(node);
        }
        None
    }

    /// tsc-port: serializeExportSpecifier @6.0.3
    /// tsc-hash: b3fb36e9d0e5bf03d237cacfbdc3a632a48530faab565d52fa493c0d7c88e32c
    /// tsc-span: _tsc.js:54947-54965
    fn serialize_export_specifier(
        &mut self,
        local_name: &str,
        target_name: &str,
        specifier: Option<TransformNode>,
    ) -> BuildResult<()> {
        add_approximate_length(
            self.context,
            16 + local_name.encode_utf16().count()
                + if local_name == target_name {
                    0
                } else {
                    target_name.encode_utf16().count()
                },
        );
        let property_name = if local_name != target_name {
            Some(create_identifier(self.arena, self.target, target_name)?)
        } else {
            None
        };
        let name = create_identifier(self.arena, self.target, local_name)?;
        let element = create_export_specifier_node(self.arena, self.target, property_name, name)?;
        let declaration =
            create_named_export_declaration(self.arena, self.target, vec![element], specifier)?;
        self.add_result(declaration, ModifierFlags::NONE)
    }

    /// tsc-port: serializeMaybeAliasAssignment @6.0.3
    /// tsc-hash: 76bea3c93c2aab13e158de586792f0b9e24d6840feaaf5ff7151b7d97ef249c0
    /// tsc-span: _tsc.js:54966-55082
    fn serialize_maybe_alias_assignment(&mut self, symbol: SymbolId) -> BuildResult<bool> {
        if self
            .checker
            .symbol_flags(symbol)
            .intersects(SymbolFlags::PROTOTYPE)
        {
            return Ok(false);
        }
        let name = tsc_binder::unescape_leading_underscores(
            &self.checker.binder.symbol(symbol).escaped_name,
        )
        .to_owned();
        let is_export_equals = name == tsc_types::InternalSymbolName::EXPORT_EQUALS;
        let is_default = name == tsc_types::InternalSymbolName::DEFAULT;
        let compatible = is_export_equals || is_default;
        let alias_declaration = self.checker.get_declaration_of_alias_symbol(symbol);
        let target = if self
            .checker
            .symbol_flags(symbol)
            .intersects(SymbolFlags::ALIAS)
        {
            self.checker
                .get_immediate_aliased_symbol(symbol)
                .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?
        } else {
            alias_declaration
                .map(|declaration| self.alias_like_assignment_target(declaration))
                .transpose()?
                .flatten()
        }
        .filter(|&target| target != self.checker.unknown_symbol);
        if let Some(target) = target.filter(|&target| {
            self.checker
                .binder
                .symbol(target)
                .declarations
                .iter()
                .any(|&declaration| {
                    self.enclosing_declaration.is_some_and(|enclosing| {
                        self.checker.binder.source_of_node(declaration).root
                            == self.checker.binder.source_of_node(enclosing).root
                    })
                })
        }) {
            self.include_private_symbol(target);
            let old_disable = self.context.tracker.disable_track_symbol;
            self.context.tracker.disable_track_symbol = true;
            if compatible {
                add_approximate_length(self.context, 10);
                let expression = chains_symbol_to_expression(
                    self.checker,
                    self.arena,
                    self.target,
                    self.context,
                    target,
                    EmitSymbolMeaning::ALIAS_RESOLVE,
                )?;
                let assignment = create_export_assignment(
                    self.arena,
                    self.target,
                    is_export_equals,
                    expression,
                )?;
                self.results.push(assignment);
            } else {
                let target_symbol_name = tsc_binder::unescape_leading_underscores(
                    &self.checker.binder.symbol(target).escaped_name,
                )
                .to_owned();
                let target_name = self.get_internal_symbol_name(target, &target_symbol_name);
                self.serialize_export_specifier(&name, &target_name, None)?;
            }
            self.context.tracker.disable_track_symbol = old_disable;
            return Ok(true);
        }

        let variable_name = self.get_unused_name(&name, Some(symbol));
        let r#type = self
            .checker
            .get_type_of_symbol(self.checker.get_merged_symbol(symbol))
            .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
        let r#type = self
            .checker
            .get_widened_type(r#type)
            .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
        if self.is_type_representable_as_function_namespace_merge(r#type, symbol)? {
            self.serialize_as_function_namespace_merge(
                r#type,
                symbol,
                &variable_name,
                if compatible {
                    ModifierFlags::NONE
                } else {
                    ModifierFlags::EXPORT
                },
            )?;
        } else {
            let type_node = serialize_type_for_declaration_seam(
                self.checker,
                self.arena,
                self.target,
                self.context,
                None,
                r#type,
                Some(symbol),
            )?;
            let name_node = create_identifier(self.arena, self.target, &variable_name)?;
            let statement = create_variable_statement(
                self.arena,
                self.target,
                name_node,
                type_node,
                if self
                    .context
                    .enclosing_declaration
                    .is_some_and(|declaration| {
                        self.checker.kind_of(declaration) == SyntaxKind::ModuleDeclaration
                    })
                    && (!self
                        .checker
                        .symbol_flags(symbol)
                        .intersects(SymbolFlags::ACCESSOR)
                        || self
                            .checker
                            .symbol_flags(symbol)
                            .intersects(SymbolFlags::SET_ACCESSOR))
                {
                    NodeFlags::LET
                } else {
                    NodeFlags::CONST
                },
            )?;
            add_approximate_length(self.context, variable_name.encode_utf16().count() + 5);
            self.add_result(
                statement,
                if target.is_some_and(|target| {
                    self.checker
                        .symbol_flags(target)
                        .intersects(SymbolFlags::PROPERTY)
                        && self.checker.binder.symbol(target).escaped_name
                            == tsc_types::InternalSymbolName::EXPORT_EQUALS
                }) {
                    ModifierFlags::AMBIENT
                } else if name == variable_name {
                    ModifierFlags::EXPORT
                } else {
                    ModifierFlags::NONE
                },
            )?;
        }
        if compatible {
            add_approximate_length(self.context, variable_name.encode_utf16().count() + 10);
            let identifier = create_identifier(self.arena, self.target, &variable_name)?;
            let assignment =
                create_export_assignment(self.arena, self.target, is_export_equals, identifier)?;
            self.results.push(assignment);
            Ok(true)
        } else if name != variable_name {
            self.serialize_export_specifier(&name, &variable_name, None)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// tsc-port: isTypeRepresentableAsFunctionNamespaceMerge @6.0.3
    /// tsc-hash: 59d447d2e6b4f724aecd7cea606cb5a4f3c66955eb99ec2305be8b639de4afc4
    /// tsc-span: _tsc.js:55083-55098
    fn is_type_representable_as_function_namespace_merge(
        &mut self,
        r#type: TypeId,
        host_symbol: SymbolId,
    ) -> BuildResult<bool> {
        let object_flags = self.checker.tables.object_flags_of(r#type);
        if !object_flags.intersects(ObjectFlags::ANONYMOUS | ObjectFlags::MAPPED)
            || object_flags.intersects(ObjectFlags::CLASS)
        {
            return Ok(false);
        }
        if !self
            .checker
            .get_index_infos_of_type(r#type)
            .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?
            .is_empty()
            || get_declaration_with_type_annotation(
                self.checker,
                host_symbol,
                self.enclosing_declaration,
                self.context,
            )?
            .is_some()
        {
            return Ok(false);
        }
        let context_root = self
            .enclosing_declaration
            .map(|declaration| self.checker.binder.source_of_node(declaration).root);
        if let Some(symbol) = self.checker.tables.type_of(r#type).symbol {
            if self
                .checker
                .binder
                .symbol(symbol)
                .declarations
                .iter()
                .copied()
                .any(|declaration| {
                    context_root.is_some_and(|root| {
                        self.checker.binder.source_of_node(declaration).root != root
                    })
                })
            {
                return Ok(false);
            }
        }
        let properties = self
            .checker
            .get_properties_of_type(r#type)
            .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
        let calls = self
            .checker
            .get_signatures_of_type(r#type, SignatureKind::Call)
            .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
        let constructs = self
            .checker
            .get_signatures_of_type(r#type, SignatureKind::Construct)
            .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
        if (properties
            .iter()
            .all(|&property| !self.is_namespace_member(property))
            && calls.is_empty())
            || !constructs.is_empty()
        {
            return Ok(false);
        }
        for property in properties {
            let property_data = self.checker.binder.symbol(property);
            let name = tsc_binder::unescape_leading_underscores(&property_data.escaped_name);
            if property_data.escaped_name.starts_with("__@")
                || !tsc_syntax::is_identifier_text(name)
                || property_data
                    .declarations
                    .iter()
                    .copied()
                    .any(|declaration| {
                        context_root.is_some_and(|root| {
                            self.checker.binder.source_of_node(declaration).root != root
                        })
                    })
            {
                return Ok(false);
            }
            if self
                .checker
                .symbol_flags(property)
                .intersects(SymbolFlags::ACCESSOR)
            {
                let read = self
                    .checker
                    .get_non_missing_type_of_symbol(property)
                    .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
                let write = self
                    .checker
                    .get_write_type_of_symbol(property)
                    .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
                if !self
                    .checker
                    .is_type_identical_to(read, write)
                    .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?
                {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    /// `getTargetOfAliasDeclaration(..., true)` for the checked-JS
    /// property-assignment shapes that do not carry `SymbolFlags::Alias`.
    fn alias_like_assignment_target(
        &mut self,
        declaration: NodeId,
    ) -> BuildResult<Option<SymbolId>> {
        let expression = match self.checker.data_of(declaration) {
            NodeData::BinaryExpression(data) => data.right,
            NodeData::PropertyAccessExpression(_) | NodeData::ElementAccessExpression(_) => self
                .checker
                .parent_of(declaration)
                .and_then(|parent| match self.checker.data_of(parent) {
                    NodeData::BinaryExpression(data) if data.left == Some(declaration) => {
                        data.right
                    }
                    _ => None,
                }),
            NodeData::PropertyAssignment(data) => data.initializer,
            NodeData::ShorthandPropertyAssignment(data) => data.name,
            _ => None,
        };
        let Some(expression) = expression else {
            return Ok(None);
        };
        if matches!(
            self.checker.kind_of(expression),
            SyntaxKind::ClassExpression | SyntaxKind::FunctionExpression
        ) {
            return self
                .checker
                .get_symbol_of_declaration(expression)
                .map(Some)
                .map_err(|abort| checker_abort_error(self.checker, self.context, abort));
        }
        self.checker
            .get_resolved_symbol(expression)
            .map_err(|abort| checker_abort_error(self.checker, self.context, abort))
    }

    fn property_in_base_type(
        &mut self,
        base_type: TypeId,
        property: SymbolId,
    ) -> BuildResult<Option<SymbolId>> {
        let name = self.checker.binder.symbol(property).escaped_name.clone();
        let properties = self
            .checker
            .get_properties_of_type(base_type)
            .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
        Ok(properties
            .into_iter()
            .find(|&candidate| self.checker.binder.symbol(candidate).escaped_name == name))
    }

    fn range_member(
        &mut self,
        node: TransformNode,
        declaration: Option<NodeId>,
    ) -> BuildResult<TransformNode> {
        let location = declaration
            .map(|declaration| project_parse_node(self.checker, self.arena, declaration))
            .transpose()?
            .flatten();
        set_text_range2(self.checker, self.arena, self.context, node, location)
    }

    /// tsc-port: makeSerializePropertySymbol @6.0.3
    /// tsc-hash: fab03df95bee26e871b1bc817932eb99b58a9c262f37009266cc1a5291f817a7
    /// tsc-span: _tsc.js:55099-55229
    #[allow(clippy::too_many_arguments)]
    fn make_serialize_property_symbol(
        &mut self,
        property: SymbolId,
        is_static: bool,
        base_type: Option<TypeId>,
        is_class: bool,
        use_accessors: bool,
    ) -> BuildResult<Vec<TransformNode>> {
        let property_data = self.checker.binder.symbol(property).clone();
        let modifier_flags = self
            .checker
            .get_declaration_modifier_flags_from_symbol(property);
        let omit_type =
            modifier_flags.intersects(ModifierFlags::PRIVATE) && !is_expanding(self.context);
        if is_static
            && property_data
                .flags
                .intersects(SymbolFlags::TYPE | SymbolFlags::NAMESPACE | SymbolFlags::ALIAS)
        {
            return Ok(Vec::new());
        }
        if property_data.flags.intersects(SymbolFlags::PROTOTYPE)
            || property_data.escaped_name == "constructor"
        {
            return Ok(Vec::new());
        }
        if let Some(base_property) = base_type
            .map(|base| self.property_in_base_type(base, property))
            .transpose()?
            .flatten()
        {
            let same_readonly = self.checker.is_readonly_symbol(base_property)
                == self.checker.is_readonly_symbol(property);
            let same_optional = self
                .checker
                .symbol_flags(base_property)
                .intersects(SymbolFlags::OPTIONAL)
                == property_data.flags.intersects(SymbolFlags::OPTIONAL);
            let property_type = self
                .checker
                .get_type_of_symbol(property)
                .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
            let base_property_type = self
                .checker
                .get_type_of_symbol(base_property)
                .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
            if same_readonly
                && same_optional
                && self
                    .checker
                    .is_type_identical_to(property_type, base_property_type)
                    .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?
            {
                return Ok(Vec::new());
            }
        }
        let flag = ModifierFlags::from_bits(
            modifier_flags.bits() & !ModifierFlags::ASYNC.bits()
                | if is_static {
                    ModifierFlags::STATIC.bits()
                } else {
                    0
                },
        );
        let name = chains_get_property_name_node_for_symbol(
            self.checker,
            self.arena,
            self.target,
            self.context,
            property,
        )?;
        let first_property_like = property_data
            .declarations
            .iter()
            .copied()
            .find(|&declaration| {
                matches!(
                    self.checker.kind_of(declaration),
                    SyntaxKind::PropertyDeclaration
                        | SyntaxKind::PropertySignature
                        | SyntaxKind::GetAccessor
                        | SyntaxKind::SetAccessor
                        | SyntaxKind::VariableDeclaration
                        | SyntaxKind::BinaryExpression
                        | SyntaxKind::PropertyAccessExpression
                )
            });

        if property_data.flags.intersects(SymbolFlags::ACCESSOR) && use_accessors {
            let mut result = Vec::new();
            if property_data.flags.intersects(SymbolFlags::SET_ACCESSOR) {
                let setter = property_data
                    .declarations
                    .iter()
                    .copied()
                    .find(|&declaration| {
                        self.checker.kind_of(declaration) == SyntaxKind::SetAccessor
                    });
                let write_type = self
                    .checker
                    .get_write_type_of_symbol(property)
                    .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
                let type_node = if omit_type {
                    None
                } else {
                    serialize_type_for_declaration_seam(
                        self.checker,
                        self.arena,
                        self.target,
                        self.context,
                        setter,
                        write_type,
                        Some(property),
                    )?
                };
                let parameter_name = setter
                    .and_then(|setter| match self.checker.data_of(setter) {
                        NodeData::SetAccessor(data) => self
                            .checker
                            .nodes_of(data.parameters)
                            .first()
                            .copied()
                            .and_then(|parameter| declaration_name(self.checker, parameter))
                            .and_then(|name| self.parse_name_text(name)),
                        _ => None,
                    })
                    .unwrap_or_else(|| "value".to_owned());
                add_approximate_length(
                    self.context,
                    modifiers_length(flag)
                        + 7
                        + parameter_name.encode_utf16().count()
                        + usize::from(!omit_type) * 2,
                );
                let parameter_name = create_identifier(self.arena, self.target, &parameter_name)?;
                let parameter = create_node(
                    self.arena,
                    self.target,
                    NodeData::Parameter(ParameterData {
                        name: Some(parameter_name.node()),
                        modifiers: None,
                        dot_dot_dot_token: None,
                        question_token: None,
                        r#type: type_node.map(TransformNode::node),
                        initializer: None,
                    }),
                )?;
                let parameters = required_array(self.arena, self.target, vec![parameter])?;
                let modifiers = create_modifiers_from_flags(self.arena, self.target, flag)?;
                let setter_node = create_node(
                    self.arena,
                    self.target,
                    NodeData::SetAccessor(SetAccessorData {
                        name: Some(name.node()),
                        type_parameters: None,
                        parameters: Some(parameters),
                        r#type: None,
                        body: None,
                        modifiers,
                    }),
                )?;
                result.push(self.range_member(setter_node, setter.or(first_property_like))?);
            }
            if property_data.flags.intersects(SymbolFlags::GET_ACCESSOR) {
                let getter = property_data
                    .declarations
                    .iter()
                    .copied()
                    .find(|&declaration| {
                        self.checker.kind_of(declaration) == SyntaxKind::GetAccessor
                    });
                let read_type = self
                    .checker
                    .get_type_of_symbol(property)
                    .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
                let type_node = if omit_type {
                    None
                } else {
                    serialize_type_for_declaration_seam(
                        self.checker,
                        self.arena,
                        self.target,
                        self.context,
                        getter,
                        read_type,
                        Some(property),
                    )?
                };
                add_approximate_length(
                    self.context,
                    modifiers_length(flag) + 8 + usize::from(!omit_type) * 2,
                );
                let parameters = required_array(self.arena, self.target, Vec::new())?;
                let modifiers = create_modifiers_from_flags(self.arena, self.target, flag)?;
                let getter_node = create_node(
                    self.arena,
                    self.target,
                    NodeData::GetAccessor(GetAccessorData {
                        name: Some(name.node()),
                        type_parameters: None,
                        parameters: Some(parameters),
                        r#type: type_node.map(TransformNode::node),
                        body: None,
                        modifiers,
                    }),
                )?;
                result.push(self.range_member(getter_node, getter.or(first_property_like))?);
            }
            return Ok(result);
        }

        if property_data
            .flags
            .intersects(SymbolFlags::PROPERTY | SymbolFlags::VARIABLE | SymbolFlags::ACCESSOR)
        {
            let modifier_flags = ModifierFlags::from_bits(
                flag.bits()
                    | if self.checker.is_readonly_symbol(property) {
                        ModifierFlags::READONLY.bits()
                    } else {
                        0
                    },
            );
            let property_type = self
                .checker
                .get_write_type_of_symbol(property)
                .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
            let type_node = if omit_type {
                None
            } else {
                serialize_type_for_declaration_seam(
                    self.checker,
                    self.arena,
                    self.target,
                    self.context,
                    property_data
                        .declarations
                        .iter()
                        .copied()
                        .find(|&declaration| {
                            self.checker.kind_of(declaration) == SyntaxKind::SetAccessor
                        }),
                    property_type,
                    Some(property),
                )?
            };
            let question = if property_data.flags.intersects(SymbolFlags::OPTIONAL) {
                Some(create_token(
                    self.arena,
                    self.target,
                    SyntaxKind::QuestionToken,
                )?)
            } else {
                None
            };
            add_approximate_length(
                self.context,
                2 + usize::from(!omit_type) * 2 + modifiers_length(modifier_flags),
            );
            let modifiers = create_modifiers_from_flags(self.arena, self.target, modifier_flags)?;
            let node = if is_class {
                create_node(
                    self.arena,
                    self.target,
                    NodeData::PropertyDeclaration(PropertyDeclarationData {
                        name: Some(name.node()),
                        modifiers,
                        question_token: question.map(TransformNode::node),
                        exclamation_token: None,
                        r#type: type_node.map(TransformNode::node),
                        initializer: None,
                    }),
                )?
            } else {
                create_node(
                    self.arena,
                    self.target,
                    NodeData::PropertySignature(PropertySignatureData {
                        name: Some(name.node()),
                        question_token: question.map(TransformNode::node),
                        modifiers,
                        r#type: type_node.map(TransformNode::node),
                        initializer: None,
                    }),
                )?
            };
            let range = property_data
                .declarations
                .iter()
                .copied()
                .find(|&declaration| {
                    matches!(
                        self.checker.kind_of(declaration),
                        SyntaxKind::PropertyDeclaration | SyntaxKind::VariableDeclaration
                    )
                })
                .or(first_property_like);
            return self.range_member(node, range).map(|node| vec![node]);
        }

        if property_data
            .flags
            .intersects(SymbolFlags::METHOD | SymbolFlags::FUNCTION)
        {
            let method_type = self
                .checker
                .get_type_of_symbol(property)
                .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
            if omit_type {
                let modifier_flags = ModifierFlags::from_bits(
                    flag.bits()
                        | if self.checker.is_readonly_symbol(property) {
                            ModifierFlags::READONLY.bits()
                        } else {
                            0
                        },
                );
                add_approximate_length(self.context, 1 + modifiers_length(modifier_flags));
                let modifiers =
                    create_modifiers_from_flags(self.arena, self.target, modifier_flags)?;
                let question = property_data
                    .flags
                    .intersects(SymbolFlags::OPTIONAL)
                    .then(|| create_token(self.arena, self.target, SyntaxKind::QuestionToken))
                    .transpose()?;
                let node = if is_class {
                    create_node(
                        self.arena,
                        self.target,
                        NodeData::PropertyDeclaration(PropertyDeclarationData {
                            name: Some(name.node()),
                            modifiers,
                            question_token: question.map(TransformNode::node),
                            exclamation_token: None,
                            r#type: None,
                            initializer: None,
                        }),
                    )?
                } else {
                    create_node(
                        self.arena,
                        self.target,
                        NodeData::PropertySignature(PropertySignatureData {
                            name: Some(name.node()),
                            question_token: question.map(TransformNode::node),
                            modifiers,
                            r#type: None,
                            initializer: None,
                        }),
                    )?
                };
                return self
                    .range_member(node, first_property_like)
                    .map(|node| vec![node]);
            }
            let signatures = self
                .checker
                .get_signatures_of_type(method_type, SignatureKind::Call)
                .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
            let mut nodes = Vec::new();
            for signature in signatures {
                add_approximate_length(self.context, 1);
                let question = property_data
                    .flags
                    .intersects(SymbolFlags::OPTIONAL)
                    .then(|| create_token(self.arena, self.target, SyntaxKind::QuestionToken))
                    .transpose()?;
                let modifiers = create_modifiers_from_flags(self.arena, self.target, flag)?
                    .map(|array| self.array_nodes(Some(array)).unwrap_or_default());
                let declaration = signature_to_signature_declaration_helper(
                    self.checker,
                    self.arena,
                    self.target,
                    signature,
                    if is_class {
                        SyntaxKind::MethodDeclaration
                    } else {
                        SyntaxKind::MethodSignature
                    },
                    self.context,
                    Some(SignatureDeclarationOptions {
                        modifiers,
                        name: Some(name),
                        question_token: question,
                    }),
                )?;
                let location = self
                    .checker
                    .signature_of(signature)
                    .declaration
                    .or(first_property_like);
                nodes.push(self.range_member(declaration, location)?);
            }
            return Ok(nodes);
        }
        Ok(Vec::new())
    }

    /// tsc-port: serializePropertySymbolForInterface @6.0.3
    /// tsc-hash: ed19b1ad9f7021cc8ccb32cdf599de799312d7f41457ac230e55bb49a160d5c9
    /// tsc-span: _tsc.js:55249-55256
    fn serialize_property_symbol_for_interface(
        &mut self,
        property: SymbolId,
        base_type: Option<TypeId>,
    ) -> BuildResult<Vec<TransformNode>> {
        self.make_serialize_property_symbol(property, false, base_type, false, false)
    }

    /// tsc-port: serializeSignatures @6.0.3
    /// tsc-hash: dc119e27712576259c6658a582ec1005c0ffac7c9227bc08ee5403a4893e3246
    /// tsc-span: _tsc.js:55257-55318
    fn serialize_signatures(
        &mut self,
        kind: SignatureKind,
        input: TypeId,
        base_type: Option<TypeId>,
        output_kind: SyntaxKind,
    ) -> BuildResult<Vec<TransformNode>> {
        let signatures = self
            .checker
            .get_signatures_of_type(input, kind)
            .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
        let mut output_signatures = signatures.clone();
        if kind == SignatureKind::Construct {
            let all_parameterless = signatures
                .iter()
                .all(|&signature| self.checker.signature_of(signature).parameters.is_empty());
            if base_type.is_none() && all_parameterless {
                return Ok(Vec::new());
            }
            if let Some(base_type) = base_type {
                let base_signatures = self
                    .checker
                    .get_signatures_of_type(base_type, SignatureKind::Construct)
                    .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
                if base_signatures.is_empty() && all_parameterless {
                    return Ok(Vec::new());
                }
                if base_signatures.len() == signatures.len() {
                    let mut identical = true;
                    for (&derived, &base) in signatures.iter().zip(&base_signatures) {
                        if !self
                            .checker
                            .compare_signatures_identical_at(derived, base, false, false, true)
                            .map_err(|abort| {
                                checker_abort_error(self.checker, self.context, abort)
                            })?
                        {
                            identical = false;
                            break;
                        }
                    }
                    if identical {
                        return Ok(Vec::new());
                    }
                }
                // A JS class with no explicit constructor inherits every
                // construct overload from its base.  The checker port can
                // currently expose only the first inherited signature on the
                // derived static type, while retaining the base declaration
                // as its provenance.  Upstream's `getSignaturesOfType(input)`
                // exposes the full overload set here, so restore that set for
                // serialization after the derived-vs-base elision decision.
                if signatures.len() == 1
                    && base_signatures.len() > 1
                    && self.checker.signature_of(signatures[0]).declaration
                        == self.checker.signature_of(base_signatures[0]).declaration
                {
                    let inherited_return = self
                        .checker
                        .get_return_type_of_signature(signatures[0])
                        .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
                    output_signatures.clear();
                    for signature in base_signatures {
                        // Default derived constructors erase a base
                        // constructor's own type parameters before replacing
                        // its return with the derived instance type.
                        let erased =
                            self.checker
                                .get_erased_signature(signature)
                                .map_err(|abort| {
                                    checker_abort_error(self.checker, self.context, abort)
                                })?;
                        let inherited = self.checker.clone_signature(erased);
                        self.checker.signature_mut(inherited).resolved_return_type =
                            crate::links::LinkSlot::Resolved(inherited_return);
                        output_signatures.push(inherited);
                    }
                }
            }
            let mut private_protected = ModifierFlags::NONE;
            for &signature in &output_signatures {
                if let Some(declaration) = self.checker.signature_of(signature).declaration {
                    let flags = parse_modifier_flags(self.checker, declaration);
                    private_protected |= ModifierFlags::from_bits(
                        flags.bits()
                            & (ModifierFlags::PRIVATE.bits() | ModifierFlags::PROTECTED.bits()),
                    );
                }
            }
            if !private_protected.is_empty() {
                let parameters = required_array(self.arena, self.target, Vec::new())?;
                let modifiers =
                    create_modifiers_from_flags(self.arena, self.target, private_protected)?;
                let declaration = create_node(
                    self.arena,
                    self.target,
                    NodeData::Constructor(ConstructorData {
                        name: None,
                        type_parameters: None,
                        parameters: Some(parameters),
                        r#type: None,
                        body: None,
                        modifiers,
                    }),
                )?;
                let location = output_signatures
                    .first()
                    .and_then(|&signature| self.checker.signature_of(signature).declaration);
                return self
                    .range_member(declaration, location)
                    .map(|node| vec![node]);
            }
        }
        let mut result = Vec::new();
        for signature in output_signatures {
            add_approximate_length(self.context, 1);
            let declaration = signature_to_signature_declaration_helper(
                self.checker,
                self.arena,
                self.target,
                signature,
                output_kind,
                self.context,
                None,
            )?;
            result.push(self.range_member(
                declaration,
                self.checker.signature_of(signature).declaration,
            )?);
        }
        Ok(result)
    }

    /// tsc-port: serializeIndexSignatures @6.0.3
    /// tsc-hash: 44122e501ca2ed0d63f8cccef0fb23425eeac979f359dc31fcbfc9e4bf6a318e
    /// tsc-span: _tsc.js:55319-55338
    fn serialize_index_signatures(
        &mut self,
        input: TypeId,
        base_type: Option<TypeId>,
    ) -> BuildResult<Vec<TransformNode>> {
        let infos = self
            .checker
            .get_index_infos_of_type(input)
            .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?;
        let mut result = Vec::new();
        for info in infos {
            if let Some(base_type) = base_type {
                if let Some(base_info) = self
                    .checker
                    .get_index_info_of_type(base_type, info.key_type)
                    .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?
                {
                    if self
                        .checker
                        .is_type_identical_to(info.value_type, base_info.value_type)
                        .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?
                    {
                        continue;
                    }
                }
            }
            result.push(index_info_to_index_signature_declaration_helper(
                self.checker,
                self.arena,
                self.target,
                &info,
                self.context,
                None,
            )?);
        }
        Ok(result)
    }

    /// tsc-port: serializeBaseType @6.0.3
    /// tsc-hash: 124e40de6a7801ccc545cc6c27da54f0c402d203e9a299886d38ec409c72d9d2
    /// tsc-span: _tsc.js:55339-55363
    fn serialize_base_type(
        &mut self,
        base: TypeId,
        static_type: TypeId,
        root_name: &str,
    ) -> BuildResult<TransformNode> {
        if let Some(reference) =
            self.try_serialize_as_type_reference(base, EmitSymbolMeaning::VALUE_EXPORT_VALUE)?
        {
            return Ok(reference);
        }
        let temporary = self.get_unused_name(&format!("{root_name}_base"), None);
        let type_node = type_to_type_node_helper(
            self.checker,
            self.arena,
            self.target,
            static_type,
            self.context,
        )?;
        let name = create_identifier(self.arena, self.target, &temporary)?;
        let statement =
            create_variable_statement(self.arena, self.target, name, type_node, NodeFlags::CONST)?;
        self.add_result(statement, ModifierFlags::NONE)?;
        let expression = create_identifier(self.arena, self.target, &temporary)?;
        create_node(
            self.arena,
            self.target,
            NodeData::ExpressionWithTypeArguments(ExpressionWithTypeArgumentsData {
                type_arguments: None,
                expression: Some(expression.node()),
            }),
        )
    }

    /// tsc-port: trySerializeAsTypeReference @6.0.3
    /// tsc-hash: 8bfdae2b2a0c562e812801b4a7e336101b31609d88d5ff82e630436b0d8023d8
    /// tsc-span: _tsc.js:55364-55376
    fn try_serialize_as_type_reference(
        &mut self,
        r#type: TypeId,
        meaning: EmitSymbolMeaning,
    ) -> BuildResult<Option<TransformNode>> {
        let type_data = self.checker.tables.type_of(r#type).clone();
        let (symbol, arguments) = match type_data.data {
            TypeData::Reference {
                target,
                resolved_type_arguments,
            } => (
                self.checker.tables.type_of(target).symbol,
                resolved_type_arguments.map(|arguments| arguments.to_vec()),
            ),
            TypeData::GenericType {
                ref type_parameters,
                ..
            } => (type_data.symbol, Some(type_parameters.to_vec())),
            _ => (type_data.symbol, None),
        };
        let Some(symbol) = symbol else {
            return Ok(None);
        };
        if let Some(enclosing) = self.enclosing_declaration {
            let accessible = self
                .checker
                .emit_is_symbol_accessible(symbol, enclosing, meaning, false)
                .map_err(|abort| checker_abort_error(self.checker, self.context, abort))?
                .accessibility
                == EmitSymbolAccessibility::Accessible;
            super::type_nodes::restore_direct_symbol_visibility(
                self.checker,
                symbol,
                enclosing,
                meaning,
                self.context,
            )?;
            if !accessible {
                return Ok(None);
            }
        }
        let expression = chains_symbol_to_expression(
            self.checker,
            self.arena,
            self.target,
            self.context,
            symbol,
            EmitSymbolMeaning::TYPE,
        )?;
        let mut argument_nodes = Vec::new();
        if let Some(arguments) = arguments {
            for argument in arguments {
                if let Some(argument) = type_to_type_node_helper(
                    self.checker,
                    self.arena,
                    self.target,
                    argument,
                    self.context,
                )? {
                    argument_nodes.push(argument);
                }
            }
        }
        let type_arguments = array(self.arena, self.target, argument_nodes)?;
        create_node(
            self.arena,
            self.target,
            NodeData::ExpressionWithTypeArguments(ExpressionWithTypeArgumentsData {
                type_arguments,
                expression: Some(expression.node()),
            }),
        )
        .map(Some)
    }

    /// tsc-port: serializeImplementedType @6.0.3
    /// tsc-hash: 8dc5d28d5c09c61f977c319e980df02034d39a52ed02f2d19919b9c9d13987b6
    /// tsc-span: _tsc.js:55377-55389
    fn serialize_implemented_type(&mut self, r#type: TypeId) -> BuildResult<Option<TransformNode>> {
        if let Some(reference) =
            self.try_serialize_as_type_reference(r#type, EmitSymbolMeaning::TYPE)?
        {
            return Ok(Some(reference));
        }
        let Some(symbol) = self.checker.tables.type_of(r#type).symbol else {
            return Ok(None);
        };
        let expression = chains_symbol_to_expression(
            self.checker,
            self.arena,
            self.target,
            self.context,
            symbol,
            EmitSymbolMeaning::TYPE,
        )?;
        create_node(
            self.arena,
            self.target,
            NodeData::ExpressionWithTypeArguments(ExpressionWithTypeArgumentsData {
                type_arguments: None,
                expression: Some(expression.node()),
            }),
        )
        .map(Some)
    }

    /// tsc-port: getUnusedName @6.0.3
    /// tsc-hash: 2546367acf7e260487b409700963b0f5415710fa780cf2dab270d0926af91ab0
    /// tsc-span: _tsc.js:55390-55412
    fn get_unused_name(&mut self, input: &str, symbol: Option<SymbolId>) -> String {
        if let Some(symbol) = symbol {
            if let Some(name) = self
                .context
                .remapped_symbol_names
                .as_ref()
                .and_then(|names| names.get(&symbol))
            {
                return name.clone();
            }
        }
        let mut input = symbol.map_or_else(
            || input.to_owned(),
            |symbol| self.get_name_candidate_worker(symbol, input),
        );
        let original = input.clone();
        let mut index = 0;
        while self
            .context
            .used_symbol_names
            .as_ref()
            .is_some_and(|names| names.contains(&input))
        {
            index += 1;
            input = format!("{original}_{index}");
        }
        self.context
            .used_symbol_names
            .get_or_insert_with(HashSet::new)
            .insert(input.clone());
        if let Some(symbol) = symbol {
            self.context
                .remapped_symbol_names
                .get_or_insert_with(HashMap::new)
                .insert(symbol, input.clone());
        }
        input
    }

    /// tsc-port: getNameCandidateWorker @6.0.3
    /// tsc-hash: b496a6d0abb70eff10421196e949b96fc10a935837e7a12b28b5094ccc2e3b92
    /// tsc-span: _tsc.js:55413-55428
    fn get_name_candidate_worker(&mut self, symbol: SymbolId, local_name: &str) -> String {
        let mut local_name = local_name.to_owned();
        if matches!(local_name.as_str(), "default" | "__class" | "__function") {
            let restore = save_restore_flags(self.context);
            self.context.flags.0 |= IN_INITIAL_ENTITY_NAME;
            let candidate = self.checker.entity_symbol_name_as_written_slice(
                symbol,
                true,
                true,
                self.context.enclosing_declaration,
            );
            restore_flags(self.context, restore);
            local_name = candidate
                .strip_prefix(['\'', '"'])
                .and_then(|value| value.strip_suffix(['\'', '"']))
                .unwrap_or(&candidate)
                .to_owned();
        }
        if local_name == tsc_types::InternalSymbolName::DEFAULT {
            local_name = "_default".to_owned();
        } else if local_name == tsc_types::InternalSymbolName::EXPORT_EQUALS {
            local_name = "_exports".to_owned();
        }
        if !tsc_syntax::is_identifier_text(&local_name)
            || is_string_a_non_contextual_keyword(&local_name)
        {
            let sanitized: String = local_name
                .chars()
                .map(|character| {
                    if character.is_ascii_alphanumeric() {
                        character
                    } else {
                        '_'
                    }
                })
                .collect();
            local_name = format!("_{sanitized}");
        }
        local_name
    }

    /// tsc-port: getInternalSymbolName @6.0.3
    /// tsc-hash: dabcf231a2d04cdc2cacbf91315a119bb0d9036f89eba69a2df05969003ddce2
    /// tsc-span: _tsc.js:55429-55437
    fn get_internal_symbol_name(&mut self, symbol: SymbolId, local_name: &str) -> String {
        if let Some(name) = self
            .context
            .remapped_symbol_names
            .as_ref()
            .and_then(|names| names.get(&symbol))
        {
            return name.clone();
        }
        let name = self.get_name_candidate_worker(symbol, local_name);
        self.context
            .remapped_symbol_names
            .get_or_insert_with(HashMap::new)
            .insert(symbol, name.clone());
        name
    }
}

/// tsc-port: modifiersLength @6.0.3
/// tsc-hash: 204830778286430e53a2919a82de20ccbc64f5c11717e3d11d32a332a9d04650
/// tsc-span: _tsc.js:55230-55248
fn modifiers_length(flags: ModifierFlags) -> usize {
    let mut result = 0;
    for (flag, length) in [
        (ModifierFlags::EXPORT, 7),
        (ModifierFlags::AMBIENT, 8),
        (ModifierFlags::DEFAULT, 8),
        (ModifierFlags::CONST, 6),
        (ModifierFlags::PUBLIC, 7),
        (ModifierFlags::PRIVATE, 8),
        (ModifierFlags::PROTECTED, 10),
        (ModifierFlags::ABSTRACT, 9),
        (ModifierFlags::STATIC, 7),
        (ModifierFlags::OVERRIDE, 9),
        (ModifierFlags::READONLY, 9),
        (ModifierFlags::ACCESSOR, 9),
        (ModifierFlags::ASYNC, 6),
        (ModifierFlags::IN, 3),
        (ModifierFlags::OUT, 4),
    ] {
        if flags.intersects(flag) {
            result += length;
        }
    }
    result
}

fn is_string_a_non_contextual_keyword(text: &str) -> bool {
    tsc_syntax::identifier_to_keyword_kind(text).is_some_and(|kind| {
        kind < SyntaxKind::FirstContextualKeyword || kind > SyntaxKind::LastContextualKeyword
    })
}

/// tsc-port: isExpanding @6.0.3
/// tsc-hash: f75ba19032417338711e7f3901b92e58442e7d99e9bf35a72b83cf5f4b7ad2ec
/// tsc-span: _tsc.js:55439-55441
fn is_expanding(context: &NodeBuilderContext<'_>) -> bool {
    context.max_expansion_depth != -1
}

/// tsc-port: isHashPrivate @6.0.3
/// tsc-hash: d800f94c5b928efcd160f651c3891f93eb3345b20f39a75cb105a2b1793e1601
/// tsc-span: _tsc.js:55442-55444
fn is_hash_private(checker: &CheckerState<'_>, symbol: SymbolId) -> bool {
    checker
        .binder
        .symbol(symbol)
        .value_declaration
        .and_then(|declaration| declaration_name(checker, declaration))
        .is_some_and(|name| checker.kind_of(name) == SyntaxKind::PrivateIdentifier)
}

/// tsc-port: getClonedHashPrivateName @6.0.3
/// tsc-hash: 890a0488f64c6dde886514350454bcb9fb08bce747e18b653a62ec09a6b13e87
/// tsc-span: _tsc.js:55445-55450
fn get_cloned_hash_private_name(
    checker: &CheckerState<'_>,
    arena: &mut TransformArena,
    symbol: SymbolId,
) -> BuildResult<Option<TransformNode>> {
    let Some(name) = checker
        .binder
        .symbol(symbol)
        .value_declaration
        .and_then(|declaration| declaration_name(checker, declaration))
        .filter(|&name| checker.kind_of(name) == SyntaxKind::PrivateIdentifier)
    else {
        return Ok(None);
    };
    clone_parse_node(checker, arena, name)
}

#[cfg(test)]
mod tests {
    use tsc_emitter::{SourceFileId, TransformArena, TransformNode, TransformSourceId};
    use tsc_syntax::{NodeData, NodeId, SyntaxKind};
    use tsc_types::CompilerOptions;

    use crate::state::test_support::with_program_state;

    use super::*;

    fn with_declaration_statements(
        files: &[(&str, &str)],
        target_index: usize,
        options: &CompilerOptions,
        verbosity: Option<i32>,
        run: impl FnOnce(
            &mut CheckerState<'_>,
            &mut TransformArena,
            TransformSourceId,
            Vec<TransformNode>,
        ),
    ) {
        with_program_state(files, options, |checker| {
            let root = checker.binder.source(target_index).root;
            let table = checker
                .binder
                .node_symbol(root)
                .map(|symbol| checker.binder.symbol(symbol).exports.clone())
                .or_else(|| checker.binder.locals_of(root).cloned())
                .expect("source-file symbol table");
            let mut arena = TransformArena::new();
            let targets = (0..checker.binder.file_count())
                .map(|index| {
                    arena.add_source(
                        checker.binder.source(index),
                        Some(SourceFileId::from_raw(index as u32)),
                    )
                })
                .collect::<Vec<_>>();
            let target = targets[target_index];
            let mut statements = None;
            let result = with_context(
                checker,
                &mut arena,
                target,
                Some(root),
                Some(EmitNodeBuilderFlags::NONE),
                Some(EmitInternalNodeBuilderFlags::NONE),
                None,
                None,
                verbosity,
                |checker, arena, target, context| {
                    statements = Some(symbol_table_to_declaration_statements(
                        checker, arena, target, &table, context,
                    )?);
                    Ok(())
                },
                None,
            )
            .expect("statement serialization succeeds");
            assert!(result.is_some(), "node-builder context remains valid");
            run(
                checker,
                &mut arena,
                target,
                statements.expect("serializer callback ran"),
            );
        });
    }

    fn node(arena: &TransformArena, node: TransformNode) -> &tsc_syntax::Node {
        arena.node(node).expect("transform node")
    }

    fn child(
        arena: &TransformArena,
        parent: TransformNode,
        child: Option<NodeId>,
    ) -> TransformNode {
        arena
            .node_ref(parent.source(), child.expect("child node"))
            .expect("child belongs to statement source")
    }

    fn array_nodes(
        arena: &TransformArena,
        parent: TransformNode,
        array: Option<NodeArrayId>,
    ) -> Vec<TransformNode> {
        let Some(array) = array.and_then(|array| arena.node_array_ref(parent.source(), array))
        else {
            return Vec::new();
        };
        arena
            .node_array(array)
            .expect("node array")
            .nodes
            .iter()
            .filter_map(|&node| arena.node_ref(parent.source(), node))
            .collect()
    }

    fn name_text(arena: &TransformArena, parent: TransformNode, name: Option<NodeId>) -> String {
        let name = child(arena, parent, name);
        match &node(arena, name).data {
            NodeData::Identifier(data) => data.text.clone(),
            NodeData::PrivateIdentifier(data) => data.text.clone(),
            NodeData::StringLiteral(data) => data.text.clone(),
            NodeData::NumericLiteral(data) => data.text.clone(),
            data => panic!("unexpected declaration name: {data:?}"),
        }
    }

    fn find_statement(
        statements: &[TransformNode],
        arena: &TransformArena,
        kind: SyntaxKind,
    ) -> TransformNode {
        statements
            .iter()
            .copied()
            .find(|&statement| node(arena, statement).kind == kind)
            .unwrap_or_else(|| {
                panic!(
                    "missing {kind:?} in {:#?}",
                    statements
                        .iter()
                        .map(|&statement| node(arena, statement).kind)
                        .collect::<Vec<_>>()
                )
            })
    }

    fn module_statements(arena: &TransformArena, module: TransformNode) -> Vec<TransformNode> {
        let NodeData::ModuleDeclaration(data) = &node(arena, module).data else {
            panic!("module declaration expected")
        };
        let body = child(arena, module, data.body);
        let NodeData::ModuleBlock(data) = &node(arena, body).data else {
            panic!("module block expected")
        };
        array_nodes(arena, body, data.statements)
    }

    #[test]
    fn js_exported_function_preserves_expando_namespace_shape() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        with_declaration_statements(
            &[(
                "/main.js",
                "/** @param {number} value @returns {string} */\nfunction api(value) { return String(value); }\napi.version = 1;\nmodule.exports = api;\n",
            )],
            0,
            &options,
            None,
            |_checker, arena, _target, statements| {
                assert!(statements.iter().any(|&statement| {
                    node(arena, statement).kind == SyntaxKind::FunctionDeclaration
                }));
                let namespace = find_statement(&statements, arena, SyntaxKind::ModuleDeclaration);
                let NodeData::ModuleDeclaration(namespace_data) = &node(arena, namespace).data
                else {
                    unreachable!()
                };
                assert_eq!(name_text(arena, namespace, namespace_data.name), "api");
                let members = module_statements(arena, namespace);
                assert!(members.iter().any(|&member| {
                    node(arena, member).kind == SyntaxKind::VariableStatement
                }));
                assert!(statements.iter().any(|&statement| {
                    node(arena, statement).kind == SyntaxKind::ExportAssignment
                }));
            },
        );
    }

    #[test]
    fn class_synthesis_emits_heritage_members_and_cloned_hash_private_name() {
        with_declaration_statements(
            &[(
                "/main.ts",
                "class Base { base = 0; }\nexport class Derived extends Base { #secret = 1; value = ''; method(x: number): string { return String(x); } get size() { return 1; } set size(value: number) {} }\n",
            )],
            0,
            &CompilerOptions::default(),
            Some(1),
            |_checker, arena, _target, statements| {
                let derived = statements
                    .iter()
                    .copied()
                    .find(|&statement| match &node(arena, statement).data {
                        NodeData::ClassDeclaration(data) => {
                            name_text(arena, statement, data.name) == "Derived"
                        }
                        _ => false,
                    })
                    .expect("Derived declaration");
                let NodeData::ClassDeclaration(data) = &node(arena, derived).data else {
                    unreachable!()
                };
                assert_ne!(arena.get_original_node(derived), derived);
                assert_eq!(array_nodes(arena, derived, data.heritage_clauses).len(), 1);
                let members = array_nodes(arena, derived, data.members);
                let member_kinds = members
                    .iter()
                    .map(|&member| node(arena, member).kind)
                    .collect::<Vec<_>>();
                assert!(member_kinds.contains(&SyntaxKind::MethodDeclaration));
                assert!(member_kinds.contains(&SyntaxKind::GetAccessor));
                assert!(member_kinds.contains(&SyntaxKind::SetAccessor));
                let private_member = members
                    .iter()
                    .copied()
                    .find(|&member| match &node(arena, member).data {
                        NodeData::PropertyDeclaration(data) => {
                            name_text(arena, member, data.name) == "#secret"
                        }
                        _ => false,
                    })
                    .expect("cloned hash-private member");
                let NodeData::PropertyDeclaration(private_data) =
                    &node(arena, private_member).data
                else {
                    unreachable!()
                };
                let private_name = child(arena, private_member, private_data.name);
                assert_ne!(arena.get_original_node(private_name), private_name);
            },
        );
    }

    #[test]
    fn nested_namespace_and_const_regular_enums_keep_statement_shapes() {
        with_declaration_statements(
            &[(
                "/main.ts",
                "export namespace Outer { export namespace Inner { export const value = 1; } }\nexport const enum ConstKind { A = 1, B = 3 }\nexport enum RegularKind { X = 'x', Y = 'y' }\n",
            )],
            0,
            &CompilerOptions::default(),
            None,
            |_checker, arena, _target, statements| {
                let outer = find_statement(&statements, arena, SyntaxKind::ModuleDeclaration);
                let outer_members = module_statements(arena, outer);
                let inner = find_statement(&outer_members, arena, SyntaxKind::ModuleDeclaration);
                assert!(module_statements(arena, inner).iter().any(|&member| {
                    node(arena, member).kind == SyntaxKind::VariableStatement
                }));

                let enums = statements
                    .iter()
                    .copied()
                    .filter(|&statement| {
                        node(arena, statement).kind == SyntaxKind::EnumDeclaration
                    })
                    .collect::<Vec<_>>();
                assert_eq!(enums.len(), 2);
                for declaration in enums {
                    let NodeData::EnumDeclaration(data) = &node(arena, declaration).data else {
                        unreachable!()
                    };
                    let members = array_nodes(arena, declaration, data.members);
                    assert_eq!(members.len(), 2);
                    assert!(members.iter().all(|&member| {
                        matches!(
                            &node(arena, member).data,
                            NodeData::EnumMember(data) if data.initializer.is_some()
                        )
                    }));
                    let flags = transform_modifier_flags(
                        arena,
                        declaration.source(),
                        data.modifiers,
                    )
                    .expect("enum modifiers");
                    if name_text(arena, declaration, data.name) == "ConstKind" {
                        assert!(flags.intersects(ModifierFlags::CONST));
                    } else {
                        assert!(!flags.intersects(ModifierFlags::CONST));
                    }
                }
            },
        );
    }

    #[test]
    fn import_equals_and_export_equals_are_composed() {
        with_declaration_statements(
            &[
                ("/dep.ts", "export class Item {}\n"),
                (
                    "/main.ts",
                    "import Dependency = require('./dep');\nexport = Dependency;\n",
                ),
            ],
            1,
            &CompilerOptions::default(),
            None,
            |_checker, arena, _target, statements| {
                assert!(
                    statements.iter().any(|&statement| {
                        node(arena, statement).kind == SyntaxKind::ImportEqualsDeclaration
                    }),
                    "statement kinds: {:?}",
                    statements
                        .iter()
                        .map(|&statement| node(arena, statement).kind)
                        .collect::<Vec<_>>()
                );
                let assignment = find_statement(&statements, arena, SyntaxKind::ExportAssignment);
                let NodeData::ExportAssignment(data) = &node(arena, assignment).data else {
                    unreachable!()
                };
                assert_eq!(data.is_export_equals, Some(true));
            },
        );
    }

    #[test]
    fn alias_reexport_keeps_module_specifier_and_named_export() {
        with_declaration_statements(
            &[
                ("/dep.ts", "export class Item {}\n"),
                ("/main.ts", "export { Item as Renamed } from './dep';\n"),
            ],
            1,
            &CompilerOptions::default(),
            None,
            |_checker, arena, _target, statements| {
                let declaration = find_statement(&statements, arena, SyntaxKind::ExportDeclaration);
                let NodeData::ExportDeclaration(data) = &node(arena, declaration).data else {
                    unreachable!()
                };
                let specifier = child(arena, declaration, data.module_specifier);
                let NodeData::StringLiteral(specifier) = &node(arena, specifier).data else {
                    panic!("string module specifier expected")
                };
                assert_eq!(specifier.text, "./dep");
                assert!(data.export_clause.is_some());
            },
        );
    }

    #[test]
    fn redundant_alias_reexports_are_merged_without_extra_specifiers() {
        with_declaration_statements(
            &[
                (
                    "/dep.ts",
                    "export class A {}\nexport interface B { value: number; }\n",
                ),
                ("/main.ts", "export { A as X, B as Y } from './dep';\n"),
            ],
            1,
            &CompilerOptions::default(),
            None,
            |_checker, arena, _target, statements| {
                let exports = statements
                    .iter()
                    .copied()
                    .filter(|&statement| {
                        node(arena, statement).kind == SyntaxKind::ExportDeclaration
                    })
                    .collect::<Vec<_>>();
                assert_eq!(exports.len(), 1);
                let declaration = exports[0];
                let NodeData::ExportDeclaration(data) = &node(arena, declaration).data else {
                    unreachable!()
                };
                let clause = child(arena, declaration, data.export_clause);
                let NodeData::NamedExports(data) = &node(arena, clause).data else {
                    panic!("named exports expected")
                };
                assert_eq!(array_nodes(arena, clause, data.elements).len(), 2);
            },
        );
    }

    #[test]
    fn interface_and_type_alias_are_synthesized_with_members_and_parameters() {
        with_declaration_statements(
            &[(
                "/main.ts",
                "export interface Box<T> { value: T; get(): T; }\nexport type Maybe<T> = T | undefined;\n",
            )],
            0,
            &CompilerOptions::default(),
            None,
            |_checker, arena, _target, statements| {
                let interface =
                    find_statement(&statements, arena, SyntaxKind::InterfaceDeclaration);
                let NodeData::InterfaceDeclaration(data) = &node(arena, interface).data else {
                    unreachable!()
                };
                assert_eq!(
                    array_nodes(arena, interface, data.type_parameters).len(),
                    1
                );
                assert_eq!(array_nodes(arena, interface, data.members).len(), 2);
                let alias = find_statement(&statements, arena, SyntaxKind::TypeAliasDeclaration);
                let NodeData::TypeAliasDeclaration(data) = &node(arena, alias).data else {
                    unreachable!()
                };
                assert_eq!(array_nodes(arena, alias, data.type_parameters).len(), 1);
                assert!(data.r#type.is_some());
            },
        );
    }

    #[test]
    fn unused_name_mangling_is_stable_for_colliding_authoring_names() {
        with_program_state(
            &[(
                "/main.ts",
                "class Taken {}\nclass Other {}\nexport { Taken, Other };\n",
            )],
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
                    None,
                    None,
                    None,
                    None,
                    None,
                    |checker, arena, target, context| {
                        context
                            .used_symbol_names
                            .get_or_insert_with(HashSet::new)
                            .insert("Taken".to_owned());
                        let mut serializer =
                            StatementSerializer::new(checker, arena, target, context);
                        assert_eq!(serializer.get_unused_name("Taken", None), "Taken_1");
                        assert_eq!(serializer.get_unused_name("Taken", None), "Taken_2");
                        Ok(())
                    },
                    None,
                )
                .expect("name generation succeeds");
                assert!(result.is_some());
            },
        );
    }

    #[test]
    fn symbol_to_declarations_simplifies_class_interface_enum_and_module_modifiers() {
        with_program_state(
            &[(
                "/main.ts",
                "export abstract class C { abstract value: number; }\nexport interface I { value: number; }\nexport const enum E { A }\nexport namespace N { export const value = 1; }\n",
            )],
            &CompilerOptions::default(),
            |checker| {
                let root = checker.binder.source(0).root;
                let exports = checker
                    .binder
                    .node_symbol(root)
                    .map(|symbol| checker.binder.symbol(symbol).exports.clone())
                    .expect("module exports");
                let mut arena = TransformArena::new();
                let target = arena.add_source(
                    checker.binder.source(0),
                    Some(SourceFileId::from_raw(0)),
                );
                for (name, meaning, expected_kind, retained) in [
                    (
                        "C",
                        EmitSymbolMeaning::TYPE,
                        SyntaxKind::ClassDeclaration,
                        ModifierFlags::ABSTRACT,
                    ),
                    (
                        "I",
                        EmitSymbolMeaning::TYPE,
                        SyntaxKind::InterfaceDeclaration,
                        ModifierFlags::NONE,
                    ),
                    (
                        "E",
                        EmitSymbolMeaning::TYPE,
                        SyntaxKind::EnumDeclaration,
                        ModifierFlags::CONST,
                    ),
                    (
                        "N",
                        EmitSymbolMeaning::NAMESPACE,
                        SyntaxKind::ModuleDeclaration,
                        ModifierFlags::NONE,
                    ),
                ] {
                    let symbol = exports
                        .get(name)
                        .copied()
                        .unwrap_or_else(|| panic!("missing {name}"));
                    let declarations = symbol_to_declarations(
                        checker,
                        &mut arena,
                        target,
                        symbol,
                        meaning,
                        EmitNodeBuilderFlags::NONE,
                        None,
                        None,
                        None,
                    )
                    .expect("symbol declaration synthesis");
                    let declaration = declarations
                        .iter()
                        .copied()
                        .find(|&declaration| node(&arena, declaration).kind == expected_kind)
                        .unwrap_or_else(|| panic!("missing simplified {expected_kind:?}"));
                    let flags = transform_modifier_flags(
                        &arena,
                        declaration.source(),
                        modifiers_of(&node(&arena, declaration).data),
                    )
                    .expect("simplified modifiers");
                    assert!(!flags.intersects(ModifierFlags::EXPORT | ModifierFlags::AMBIENT));
                    assert_eq!(flags.intersects(retained), !retained.is_empty());
                }
            },
        );
    }
}

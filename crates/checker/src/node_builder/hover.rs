//! tsgo's hover expansion of a symbol (checker/nodebuilder_hover.go and
//! `NodeBuilder.ExpandSymbolForHover`, nodebuilder.go:152-222): the enum,
//! class, interface and namespace declarations a hover shows at a higher
//! verbosity level. tsgo builds them directly rather than through
//! declaration emit's `symbolTableToDeclarationStatements`.

use tsc_binder::node_util;
use tsc_emitter::{
    EmitNodeBuilderFlags, EmitSymbolExpansionOut, TransformArena, TransformNode,
    TransformNodeArray, TransformSourceId,
};
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{ModifierFlags, NodeFlags, SymbolFlags, SymbolId, TypeId};

use crate::exports::ConstantValue;
use crate::state::CheckerState;
use crate::state::SignatureKind;

use super::chains::symbol_to_node;
use super::signatures::{
    index_info_to_index_signature_declaration_helper, signature_to_signature_declaration_helper,
    type_parameter_to_declaration, SignatureDeclarationOptions,
};
use super::type_nodes::{
    add_approximate_length, add_property_to_element_list, checker_abort_error,
    create_output_identifier, factory_error, project_parse_node, BuildResult,
};
use super::{check_truncation_length, with_context, NodeBuilderContext};

/// tsgo `nodebuilder.FlagsIgnoreErrors`.
const IGNORE_ERRORS: u32 = 70_221_824;
/// tsgo `nodebuilder.FlagsUseAliasDefinedOutsideCurrentScope`.
const USE_ALIAS_DEFINED_OUTSIDE_CURRENT_SCOPE: u32 = 16_384;
/// tsgo `nodebuilder.FlagsWriteTypeParametersInQualifiedName`.
const WRITE_TYPE_PARAMETERS_IN_QUALIFIED_NAME: u32 = 512;
/// tsgo `nodebuilder.Flags(SymbolFormatFlagsUseOnlyExternalAliasing)` (the
/// symbol format flag's value as a node builder flag, 2).
const SYMBOL_FORMAT_USE_ONLY_EXTERNAL_ALIASING: u32 = 2;
/// tsgo `nodebuilder.FlagsInTypeAlias`.
const IN_TYPE_ALIAS: u32 = 8_388_608;

/// tsgo `NodeBuilder.ExpandSymbolForHover`: the declarations a hover shows
/// for the symbol, each with its original declaration's modifiers (without
/// `export` or `declare`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn expand_symbol_for_hover(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    symbol: SymbolId,
    meaning: SymbolFlags,
    maximum_length: Option<u32>,
    verbosity_level: Option<i32>,
    out: Option<&mut EmitSymbolExpansionOut>,
) -> BuildResult<Vec<TransformNode>> {
    let flags = EmitNodeBuilderFlags(
        IGNORE_ERRORS
            | EmitNodeBuilderFlags::MULTILINE_OBJECT_LITERALS.0
            | USE_ALIAS_DEFINED_OUTSIDE_CURRENT_SCOPE,
    );
    with_context(
        checker,
        arena,
        target,
        None,
        Some(flags),
        None,
        None,
        maximum_length,
        verbosity_level,
        |checker, arena, target, context| {
            // The declared type goes on the type stack, under a sentinel
            // (isTypeOnStack skips the last entry), so it does not expand
            // again.
            let declared = checker
                .get_declared_type_of_symbol(symbol)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            context.type_stack.push(Some(declared));
            context.type_stack.push(None);
            let nodes = expand(checker, arena, target, context, symbol);
            context.type_stack.truncate(context.type_stack.len() - 2);
            let mut result = Vec::new();
            for node in nodes? {
                match arena.node(node).map_err(factory_error)?.kind {
                    SyntaxKind::ClassDeclaration => {
                        result.push(simplify_class_declaration(checker, arena, node, symbol)?);
                    }
                    SyntaxKind::EnumDeclaration => result.push(simplify_modifiers(
                        checker,
                        arena,
                        node,
                        symbol,
                        SyntaxKind::EnumDeclaration,
                    )?),
                    SyntaxKind::InterfaceDeclaration
                        if meaning.intersects(SymbolFlags::INTERFACE) =>
                    {
                        result.push(simplify_modifiers(
                            checker,
                            arena,
                            node,
                            symbol,
                            SyntaxKind::InterfaceDeclaration,
                        )?);
                    }
                    SyntaxKind::ModuleDeclaration => result.push(simplify_modifiers(
                        checker,
                        arena,
                        node,
                        symbol,
                        SyntaxKind::ModuleDeclaration,
                    )?),
                    _ => {}
                }
            }
            Ok(result)
        },
        out,
    )
    .map(Option::unwrap_or_default)
}

/// tsgo `simplifyClassDeclaration`: the first class-like declaration's
/// modifiers, and no name for a class expression.
fn simplify_class_declaration(
    checker: &CheckerState<'_>,
    arena: &mut TransformArena,
    class: TransformNode,
    symbol: SymbolId,
) -> BuildResult<TransformNode> {
    let original = checker
        .binder
        .symbol(symbol)
        .declarations
        .iter()
        .copied()
        .find(|&declaration| is_class_like(checker, declaration));
    let modifiers = original.map_or(ModifierFlags::NONE, |declaration| {
        syntactic_modifier_flags(checker, declaration)
    });
    let modifiers = without(modifiers, ModifierFlags::EXPORT | ModifierFlags::AMBIENT);
    let mut class = class;
    if original
        .is_some_and(|declaration| checker.kind_of(declaration) == SyntaxKind::ClassExpression)
    {
        let NodeData::ClassDeclaration(mut data) =
            arena.node(class).map_err(factory_error)?.data.clone()
        else {
            unreachable!("a class expansion is a class declaration");
        };
        data.name = None;
        class = arena
            .factory()
            .update_node(
                class,
                NodeData::ClassDeclaration(data),
                tsc_emitter::TransformFlags::NONE,
            )
            .map_err(factory_error)?;
    }
    replace_modifiers(arena, class, modifiers)
}

/// tsgo `simplifyModifiers`: the first declaration of the kind's modifiers.
fn simplify_modifiers(
    checker: &CheckerState<'_>,
    arena: &mut TransformArena,
    declaration: TransformNode,
    symbol: SymbolId,
    kind: SyntaxKind,
) -> BuildResult<TransformNode> {
    let modifiers = checker
        .binder
        .symbol(symbol)
        .declarations
        .iter()
        .copied()
        .find(|&declaration| checker.kind_of(declaration) == kind)
        .map_or(ModifierFlags::NONE, |declaration| {
            syntactic_modifier_flags(checker, declaration)
        });
    let modifiers = without(modifiers, ModifierFlags::EXPORT | ModifierFlags::AMBIENT);
    replace_modifiers(arena, declaration, modifiers)
}

/// tsgo `expandSymbolForHover`: an enum, a class, then a namespace (before
/// an interface, as Strada orders merged declarations), then an interface
/// that is no class.
fn expand(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    symbol: SymbolId,
) -> BuildResult<Vec<TransformNode>> {
    let flags = checker.symbol_flags(symbol);
    let mut results = Vec::new();
    if flags.intersects(SymbolFlags::ENUM) {
        results.push(expand_enum_declaration(
            checker, arena, target, context, symbol,
        )?);
    }
    if flags.intersects(SymbolFlags::CLASS) {
        results.push(expand_class_declaration(
            checker, arena, target, context, symbol,
        )?);
    }
    if flags.intersects(SymbolFlags::VALUE_MODULE | SymbolFlags::NAMESPACE_MODULE) {
        results.push(expand_module_declaration(
            checker, arena, target, context, symbol,
        )?);
    }
    if flags.intersects(SymbolFlags::INTERFACE) && !flags.intersects(SymbolFlags::CLASS) {
        results.push(expand_interface_declaration(
            checker, arena, target, context, symbol,
        )?);
    }
    Ok(results)
}

/// tsgo `checkTruncationLengthIfExpanding`.
fn check_truncation_length_if_expanding(context: &mut NodeBuilderContext<'_>) -> bool {
    if context.max_expansion_depth >= 0 && check_truncation_length(context) {
        context.out.truncated = true;
        return true;
    }
    false
}

/// tsgo `expandEnumDecl`: an enum declaration with every member.
fn expand_enum_declaration(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    symbol: SymbolId,
) -> BuildResult<TransformNode> {
    let name = name_text(checker, symbol);
    add_approximate_length(context, 9 + name.len());
    let ty = checker
        .get_type_of_symbol(symbol)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    let member_properties = checker
        .get_properties_of_type(ty)
        .map_err(|abort| checker_abort_error(checker, context, abort))?
        .into_iter()
        .filter(|&property| {
            checker
                .symbol_flags(property)
                .intersects(SymbolFlags::ENUM_MEMBER)
        })
        .collect::<Vec<_>>();
    let mut members = Vec::new();
    for (index, &property) in member_properties.iter().enumerate() {
        if check_truncation_length_if_expanding(context) && index + 3 < member_properties.len() - 1
        {
            context.out.truncated = true;
            let text = format!(" ... {} more ... ", member_properties.len() - index - 1);
            let literal = string_literal(arena, target, &text)?;
            members.push(
                arena
                    .factory()
                    .create_enum_member(target, literal, None)
                    .map_err(factory_error)?,
            );
            let last = member_properties[member_properties.len() - 1];
            let last_name = identifier(arena, target, &name_text(checker, last))?;
            let initializer = enum_member_initializer(checker, arena, target, context, last)?;
            members.push(
                arena
                    .factory()
                    .create_enum_member(target, last_name, initializer)
                    .map_err(factory_error)?,
            );
            break;
        }
        let source_initializer =
            enum_member_declaration(checker, property).and_then(|declaration| {
                match checker.data_of(declaration) {
                    NodeData::EnumMember(data) => data.initializer,
                    _ => None,
                }
            });
        let initializer = match source_initializer {
            Some(initializer) => Some(deep_clone_parse_node(checker, arena, target, initializer)?),
            None => enum_member_initializer(checker, arena, target, context, property)?,
        };
        let member_name = name_text(checker, property);
        add_approximate_length(context, 4 + member_name.len());
        if initializer.is_some() {
            // " = " and an estimate of the value.
            add_approximate_length(context, 5);
        }
        let member_name = identifier(arena, target, &member_name)?;
        members.push(
            arena
                .factory()
                .create_enum_member(target, member_name, initializer)
                .map_err(factory_error)?,
        );
    }
    let modifiers = if checker
        .symbol_flags(symbol)
        .intersects(SymbolFlags::CONST_ENUM)
    {
        arena
            .factory()
            .create_modifiers_from_modifier_flags(target, ModifierFlags::CONST)
            .map_err(factory_error)?
    } else {
        None
    };
    let name = identifier(arena, target, &name)?;
    let members = node_array(arena, target, members)?;
    arena
        .factory()
        .create_enum_declaration(target, modifiers, name, members)
        .map_err(factory_error)
}

fn enum_member_declaration(checker: &CheckerState<'_>, member: SymbolId) -> Option<NodeId> {
    checker
        .binder
        .symbol(member)
        .declarations
        .iter()
        .copied()
        .find(|&declaration| checker.kind_of(declaration) == SyntaxKind::EnumMember)
}

/// tsgo `enumMemberInitializer`: the member's constant value as a literal.
fn enum_member_initializer(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    member: SymbolId,
) -> BuildResult<Option<TransformNode>> {
    let Some(declaration) = enum_member_declaration(checker, member) else {
        return Ok(None);
    };
    let value = checker
        .get_constant_value(declaration)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    match value {
        Some(ConstantValue::String(text)) => {
            string_literal(arena, target, &text.to_string_lossy()).map(Some)
        }
        Some(ConstantValue::Number(value)) => arena
            .factory()
            .create_numeric_literal(target, tsc_types::js_number_to_string(value))
            .map(Some)
            .map_err(factory_error),
        None => Ok(None),
    }
}

/// tsgo `expandClassDecl`: a class declaration with its heritage clauses
/// and members.
fn expand_class_declaration(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    symbol: SymbolId,
) -> BuildResult<TransformNode> {
    let name = name_text(checker, symbol);
    add_approximate_length(context, 9 + name.len());
    let class_like_declarations = checker
        .binder
        .symbol(symbol)
        .declarations
        .iter()
        .copied()
        .filter(|&declaration| is_class_like(checker, declaration))
        .collect::<Vec<_>>();
    let old_enclosing = context.enclosing_declaration;
    if let Some(&original) = class_like_declarations.first() {
        context.enclosing_declaration = Some(original);
    }
    let result = (|| {
        let local_parameters =
            checker.get_local_type_parameters_of_class_or_interface_or_type_alias(symbol);
        let mut type_parameters = Vec::with_capacity(local_parameters.len());
        for parameter in local_parameters {
            type_parameters.push(type_parameter_to_declaration(
                checker, arena, target, parameter, context, None,
            )?);
        }
        let declared = checker
            .get_declared_type_of_symbol(symbol)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        let class_type = checker
            .get_type_with_this_argument(declared, None, false)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        let class_target = checker.get_target_type(class_type);
        let base_types = checker
            .get_base_types(class_target)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        let static_type = checker
            .get_type_of_symbol(symbol)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        let is_class = checker
            .tables
            .type_of(static_type)
            .symbol
            .and_then(|symbol| checker.binder.symbol(symbol).value_declaration)
            .is_some_and(|declaration| is_class_like(checker, declaration));
        let static_base_type = if is_class {
            checker
                .get_base_constructor_type_of_class(declared)
                .map_err(|abort| checker_abort_error(checker, context, abort))?
        } else {
            checker.tables.intrinsics.any
        };

        let heritage_clauses =
            hover_heritage_clauses(checker, arena, target, &class_like_declarations)?;

        // Instance members through addPropertyToElementList, then as class
        // elements with their class modifiers.
        let all_properties = checker
            .get_properties_of_type(class_type)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        let symbol_properties =
            filter_inherited_properties(checker, context, class_type, &base_types, all_properties)?;
        let public_properties = symbol_properties
            .iter()
            .copied()
            .filter(|&property| !is_hash_private(checker, property))
            .collect::<Vec<_>>();
        let has_private = symbol_properties
            .iter()
            .any(|&property| is_hash_private(checker, property));

        let mut instance_members = Vec::new();
        serialize_properties_with_truncation(
            checker,
            arena,
            target,
            context,
            &public_properties,
            &mut instance_members,
        )?;
        let instance_members = type_elements_to_class_elements(arena, target, instance_members)?;
        let instance_members =
            add_class_modifiers(checker, arena, target, instance_members, false)?;

        let static_properties = checker
            .get_properties_of_type(static_type)
            .map_err(|abort| checker_abort_error(checker, context, abort))?
            .into_iter()
            .filter(|&property| {
                !checker
                    .symbol_flags(property)
                    .intersects(SymbolFlags::PROTOTYPE)
                    && name_text(checker, property) != "prototype"
                    && !is_namespace_member(checker, property)
            })
            .collect::<Vec<_>>();
        let mut static_members = Vec::new();
        serialize_properties_with_truncation(
            checker,
            arena,
            target,
            context,
            &static_properties,
            &mut static_members,
        )?;
        let static_members = type_elements_to_class_elements(arena, target, static_members)?;
        let static_members = add_class_modifiers(checker, arena, target, static_members, true)?;

        let mut private_members = Vec::new();
        if has_private {
            let hash_private = symbol_properties
                .iter()
                .copied()
                .filter(|&property| is_hash_private(checker, property))
                .collect::<Vec<_>>();
            serialize_properties_with_truncation(
                checker,
                arena,
                target,
                context,
                &hash_private,
                &mut private_members,
            )?;
        }
        let private_members = type_elements_to_class_elements(arena, target, private_members)?;

        let constructors = serialize_constructors(
            checker,
            arena,
            target,
            context,
            static_type,
            Some(static_base_type),
            is_class,
            symbol,
        )?;
        let index_signatures = serialize_index_signatures_of_type(
            checker,
            arena,
            target,
            context,
            class_type,
            base_types.first().copied(),
        )?;

        let mut members = Vec::with_capacity(
            index_signatures.len()
                + static_members.len()
                + constructors.len()
                + instance_members.len()
                + private_members.len(),
        );
        members.extend(index_signatures);
        members.extend(static_members.into_iter().map(|member| member.node));
        members.extend(constructors);
        members.extend(instance_members.into_iter().map(|member| member.node));
        members.extend(private_members.into_iter().map(|member| member.node));

        let name = identifier(arena, target, &name)?;
        let type_parameters = optional_node_array(arena, target, type_parameters)?;
        let heritage_clauses = optional_node_array(arena, target, heritage_clauses)?;
        let members = node_array(arena, target, members)?;
        arena
            .factory()
            .create_class_declaration(
                target,
                None,
                Some(name),
                type_parameters,
                heritage_clauses,
                members,
            )
            .map_err(factory_error)
    })();
    context.enclosing_declaration = old_enclosing;
    result
}

/// A serialized member and, for a name tsgo's `newIdentifier` registered,
/// the symbol it names (tsgo's `idToSymbol`).
struct Member {
    node: TransformNode,
    symbol: Option<SymbolId>,
}

/// tsgo `addClassModifiers`: a member gets its symbol's declaration
/// modifiers (and `static`).
fn add_class_modifiers(
    checker: &CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    members: Vec<Member>,
    is_static: bool,
) -> BuildResult<Vec<Member>> {
    let mut result = Vec::with_capacity(members.len());
    for member in members {
        let Some(symbol) = member.symbol else {
            result.push(member);
            continue;
        };
        let mut flags = without(
            checker.get_declaration_modifier_flags_from_symbol(symbol),
            ModifierFlags::ASYNC,
        );
        if is_static {
            flags |= ModifierFlags::STATIC;
        }
        let existing = transform_modifier_flags(arena, member.node)?;
        let node = if flags != ModifierFlags::NONE
            && can_have_modifiers(arena, member.node)?
            && flags != existing
        {
            let modifiers = arena
                .factory()
                .create_modifiers_from_modifier_flags(target, flags | existing)
                .map_err(factory_error)?;
            arena
                .factory()
                .replace_modifiers(member.node, modifiers)
                .map_err(factory_error)?
        } else {
            member.node
        };
        result.push(Member {
            node,
            symbol: Some(symbol),
        });
    }
    Ok(result)
}

/// tsgo `typeElementsToClassElements`: property and method signatures as
/// property and method declarations.
fn type_elements_to_class_elements(
    arena: &mut TransformArena,
    target: TransformSourceId,
    members: Vec<Member>,
) -> BuildResult<Vec<Member>> {
    let mut result = Vec::with_capacity(members.len());
    for member in members {
        let source = member.node.source();
        let node = match arena.node(member.node).map_err(factory_error)?.data.clone() {
            NodeData::PropertySignature(data) => {
                let modifiers = data
                    .modifiers
                    .map(|array| TransformNodeArray::new(source, array));
                let name =
                    child(arena, source, data.name)?.expect("a property signature has a name");
                let question = child(arena, source, data.question_token)?;
                let r#type = child(arena, source, data.r#type)?;
                arena
                    .factory()
                    .create_property_declaration(target, modifiers, name, question, r#type, None)
                    .map_err(factory_error)?
            }
            NodeData::MethodSignature(data) => {
                let modifiers = data
                    .modifiers
                    .map(|array| TransformNodeArray::new(source, array));
                let name = child(arena, source, data.name)?.expect("a method signature has a name");
                let question = child(arena, source, data.question_token)?;
                let type_parameters = data
                    .type_parameters
                    .map(|array| TransformNodeArray::new(source, array));
                let parameters = match data.parameters {
                    Some(array) => TransformNodeArray::new(source, array),
                    None => node_array(arena, target, Vec::new())?,
                };
                let r#type = child(arena, source, data.r#type)?;
                arena
                    .factory()
                    .create_method_declaration(
                        target,
                        modifiers,
                        None,
                        name,
                        question,
                        type_parameters,
                        parameters,
                        r#type,
                        None,
                    )
                    .map_err(factory_error)?
            }
            _ => member.node,
        };
        result.push(Member {
            node,
            symbol: member.symbol,
        });
    }
    Ok(result)
}

/// tsgo `expandInterfaceDecl`: an interface declaration with its members.
fn expand_interface_declaration(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    symbol: SymbolId,
) -> BuildResult<TransformNode> {
    let name = name_text(checker, symbol);
    add_approximate_length(context, 14 + name.len());
    let interface_type = checker
        .get_declared_type_of_symbol(symbol)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    let interface_declarations = checker
        .binder
        .symbol(symbol)
        .declarations
        .iter()
        .copied()
        .filter(|&declaration| checker.kind_of(declaration) == SyntaxKind::InterfaceDeclaration)
        .collect::<Vec<_>>();
    let local_parameters =
        checker.get_local_type_parameters_of_class_or_interface_or_type_alias(symbol);
    let mut type_parameters = Vec::with_capacity(local_parameters.len());
    for parameter in local_parameters {
        type_parameters.push(type_parameter_to_declaration(
            checker, arena, target, parameter, context, None,
        )?);
    }
    let base_types = checker
        .get_base_types(interface_type)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    let base_type = if base_types.is_empty() {
        None
    } else {
        Some(
            checker
                .get_intersection_type(&base_types, Default::default())
                .map_err(|abort| checker_abort_error(checker, context, abort))?,
        )
    };

    let members_id = checker
        .resolve_structured_type_members(interface_type)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    let resolved = checker.members_of(members_id).clone();
    let mut members = serialize_index_signatures_of_type(
        checker,
        arena,
        target,
        context,
        interface_type,
        base_type,
    )?;
    // Construct signatures (not abstract ones), then call signatures.
    for &signature in resolved.construct_signatures.iter() {
        if checker
            .signature_of(signature)
            .flags
            .intersects(tsc_types::SignatureFlags::ABSTRACT)
        {
            continue;
        }
        members.push(signature_to_signature_declaration_helper(
            checker,
            arena,
            target,
            signature,
            SyntaxKind::ConstructSignature,
            context,
            None,
        )?);
    }
    for &signature in resolved.call_signatures.iter() {
        members.push(signature_to_signature_declaration_helper(
            checker,
            arena,
            target,
            signature,
            SyntaxKind::CallSignature,
            context,
            None,
        )?);
    }
    let properties = filter_inherited_properties(
        checker,
        context,
        interface_type,
        &base_types,
        resolved.properties.to_vec(),
    )?;
    let mut property_members = members
        .into_iter()
        .map(|node| Member { node, symbol: None })
        .collect::<Vec<_>>();
    serialize_properties_with_truncation(
        checker,
        arena,
        target,
        context,
        &properties,
        &mut property_members,
    )?;

    let heritage_clauses = hover_heritage_clauses(checker, arena, target, &interface_declarations)?;
    let name = identifier(arena, target, &name)?;
    let type_parameters = optional_node_array(arena, target, type_parameters)?;
    let heritage_clauses = optional_node_array(arena, target, heritage_clauses)?;
    let members = node_array(
        arena,
        target,
        property_members
            .into_iter()
            .map(|member| member.node)
            .collect(),
    )?;
    arena
        .factory()
        .create_interface_declaration(
            target,
            None,
            name,
            type_parameters,
            heritage_clauses,
            members,
        )
        .map_err(factory_error)
}

/// tsgo `hoverHeritageClauses`: the declarations' `extends` and
/// `implements` elements as written.
fn hover_heritage_clauses(
    checker: &CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    declarations: &[NodeId],
) -> BuildResult<Vec<TransformNode>> {
    let mut extends_types = Vec::new();
    let mut implements_types = Vec::new();
    for &declaration in declarations {
        for element in heritage_clause_elements(checker, declaration, SyntaxKind::ExtendsKeyword) {
            extends_types.push(deep_clone_parse_node(checker, arena, target, element)?);
        }
        for element in heritage_clause_elements(checker, declaration, SyntaxKind::ImplementsKeyword)
        {
            implements_types.push(deep_clone_parse_node(checker, arena, target, element)?);
        }
    }
    let mut clauses = Vec::new();
    if !extends_types.is_empty() {
        let types = node_array(arena, target, extends_types)?;
        clauses.push(
            arena
                .factory()
                .create_heritage_clause(target, SyntaxKind::ExtendsKeyword, types)
                .map_err(factory_error)?,
        );
    }
    if !implements_types.is_empty() {
        let types = node_array(arena, target, implements_types)?;
        clauses.push(
            arena
                .factory()
                .create_heritage_clause(target, SyntaxKind::ImplementsKeyword, types)
                .map_err(factory_error)?,
        );
    }
    Ok(clauses)
}

/// tsgo `ast.GetExtendsHeritageClauseElements` and
/// `GetImplementsHeritageClauseElements`.
fn heritage_clause_elements(
    checker: &CheckerState<'_>,
    declaration: NodeId,
    token: SyntaxKind,
) -> Vec<NodeId> {
    let clauses = match checker.data_of(declaration) {
        NodeData::ClassDeclaration(data) => data.heritage_clauses,
        NodeData::ClassExpression(data) => data.heritage_clauses,
        NodeData::InterfaceDeclaration(data) => data.heritage_clauses,
        _ => None,
    };
    checker
        .nodes_of(clauses)
        .iter()
        .copied()
        .find_map(|clause| match checker.data_of(clause) {
            NodeData::HeritageClause(data) if data.token == token => {
                Some(checker.nodes_of(data.types).to_vec())
            }
            _ => None,
        })
        .unwrap_or_default()
}

/// tsgo `serializePropertiesWithTruncation`: the properties through
/// addPropertyToElementList, cut as Strada's createTypeNodesFromResolvedType
/// cuts them.
fn serialize_properties_with_truncation(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    properties: &[SymbolId],
    elements: &mut Vec<Member>,
) -> BuildResult<()> {
    let properties = properties
        .iter()
        .copied()
        .filter(|&property| {
            !checker
                .symbol_flags(property)
                .intersects(SymbolFlags::PROTOTYPE)
        })
        .collect::<Vec<_>>();
    for (index, &property) in properties.iter().enumerate() {
        if check_truncation_length_if_expanding(context) && index + 3 < properties.len() - 1 {
            context.out.truncated = true;
            let text = format!("... {} more ...", properties.len() - index - 1);
            let name = identifier(arena, target, &text)?;
            let signature = arena
                .factory()
                .create_property_signature(target, None, name, None, None)
                .map_err(factory_error)?;
            elements.push(Member {
                node: signature,
                symbol: None,
            });
            add_property(
                checker,
                arena,
                target,
                context,
                properties[properties.len() - 1],
                elements,
            )?;
            break;
        }
        add_property(checker, arena, target, context, property, elements)?;
    }
    Ok(())
}

/// addPropertyToElementList, each element naming `property` when its name
/// is an identifier (tsgo's `newIdentifier` registers those).
fn add_property(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    property: SymbolId,
    elements: &mut Vec<Member>,
) -> BuildResult<()> {
    let mut nodes = Vec::new();
    add_property_to_element_list(checker, arena, target, property, context, &mut nodes)?;
    for node in nodes {
        let named = member_name(arena, node)?.is_some_and(|name| {
            arena
                .node(name)
                .is_ok_and(|record| record.kind == SyntaxKind::Identifier)
        });
        elements.push(Member {
            node,
            symbol: named.then_some(property),
        });
    }
    Ok(())
}

/// tsgo `serializeConstructors`: a class's constructors, none when they
/// match the base's.
#[allow(clippy::too_many_arguments)]
fn serialize_constructors(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    static_type: TypeId,
    static_base_type: Option<TypeId>,
    is_class: bool,
    symbol: SymbolId,
) -> BuildResult<Vec<TransformNode>> {
    let signatures = checker
        .get_signatures_of_type(static_type, SignatureKind::Construct)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    let is_non_constructable = !is_class
        && checker
            .binder
            .symbol(symbol)
            .value_declaration
            .is_some_and(|declaration| checker.is_in_js_file(declaration))
        && signatures.is_empty();
    if is_non_constructable {
        add_approximate_length(context, 21);
        let modifiers = arena
            .factory()
            .create_modifiers_from_modifier_flags(target, ModifierFlags::PRIVATE)
            .map_err(factory_error)?;
        let parameters = node_array(arena, target, Vec::new())?;
        return Ok(vec![arena
            .factory()
            .create_constructor_declaration(target, modifiers, parameters, None)
            .map_err(factory_error)?]);
    }
    let no_parameters = |checker: &CheckerState<'_>| {
        signatures
            .iter()
            .all(|&signature| checker.signature_of(signature).parameters.is_empty())
    };
    if let Some(static_base_type) = static_base_type {
        let base_signatures = checker
            .get_signatures_of_type(static_base_type, SignatureKind::Construct)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        if base_signatures.is_empty() && no_parameters(checker) {
            return Ok(Vec::new());
        }
        if base_signatures.len() == signatures.len() {
            let mut all_match = true;
            for (&signature, &base) in signatures.iter().zip(base_signatures.iter()) {
                // tsgo compares with compareTypesIdentical, ignoring the
                // return types.
                let identical = checker
                    .compare_signatures_identical_at(signature, base, false, false, true)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?;
                if !identical {
                    all_match = false;
                    break;
                }
            }
            if all_match {
                return Ok(Vec::new());
            }
        }
        let mut private_protected = ModifierFlags::NONE;
        for &signature in signatures.iter() {
            if let Some(declaration) = checker.signature_of(signature).declaration {
                private_protected |= syntactic_modifier_flags(checker, declaration)
                    & (ModifierFlags::PRIVATE | ModifierFlags::PROTECTED);
            }
        }
        if private_protected != ModifierFlags::NONE {
            let modifiers = arena
                .factory()
                .create_modifiers_from_modifier_flags(target, private_protected)
                .map_err(factory_error)?;
            let parameters = node_array(arena, target, Vec::new())?;
            return Ok(vec![arena
                .factory()
                .create_constructor_declaration(target, modifiers, parameters, None)
                .map_err(factory_error)?]);
        }
    } else if no_parameters(checker) {
        return Ok(Vec::new());
    }
    let mut result = Vec::with_capacity(signatures.len());
    for &signature in signatures.iter() {
        add_approximate_length(context, 1);
        result.push(signature_to_signature_declaration_helper(
            checker,
            arena,
            target,
            signature,
            SyntaxKind::Constructor,
            context,
            None,
        )?);
    }
    Ok(result)
}

/// tsgo `serializeIndexSignaturesOfType`: the type's index signatures but
/// those identical to the base type's.
fn serialize_index_signatures_of_type(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    input: TypeId,
    base_type: Option<TypeId>,
) -> BuildResult<Vec<TransformNode>> {
    let infos = checker
        .get_index_infos_of_type(input)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    let mut result = Vec::new();
    for info in infos {
        if let Some(base_type) = base_type {
            let base_info = checker
                .get_index_info_of_type(base_type, info.key_type)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            if let Some(base_info) = base_info {
                if checker
                    .is_type_identical_to(info.value_type, base_info.value_type)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?
                {
                    continue;
                }
            }
        }
        result.push(index_info_to_index_signature_declaration_helper(
            checker, arena, target, &info, context, None,
        )?);
    }
    Ok(result)
}

/// tsgo `serializeNamespaceMember`: a namespace member's declaration by
/// its kind (a variable for anything else).
fn serialize_namespace_member(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    resolved: SymbolId,
    name: &str,
) -> BuildResult<TransformNode> {
    let flags = checker.symbol_flags(resolved);
    if flags.intersects(SymbolFlags::TYPE_ALIAS) {
        return serialize_type_alias_for_namespace(checker, arena, target, context, resolved, name);
    }
    if flags.intersects(SymbolFlags::ENUM) {
        return expand_enum_declaration(checker, arena, target, context, resolved);
    }
    if flags.intersects(SymbolFlags::CLASS) {
        return expand_class_declaration(checker, arena, target, context, resolved);
    }
    if flags.intersects(SymbolFlags::INTERFACE) {
        return expand_interface_declaration(checker, arena, target, context, resolved);
    }
    if flags.intersects(SymbolFlags::VALUE_MODULE | SymbolFlags::NAMESPACE_MODULE) {
        return expand_module_declaration(checker, arena, target, context, resolved);
    }
    let ty = checker
        .get_type_of_symbol(resolved)
        .and_then(|ty| checker.get_widened_type(ty))
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    add_approximate_length(context, name.len() + 5);
    let_statement(checker, arena, target, context, name, ty, resolved)
}

/// `let name: T;` with the type tsgo's serializeTypeForDeclaration writes.
fn let_statement(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    name: &str,
    ty: TypeId,
    symbol: SymbolId,
) -> BuildResult<TransformNode> {
    let type_node = super::serialize::serialize_type_for_declaration_in_context(
        checker,
        arena,
        target,
        context,
        None,
        Some(ty),
        Some(symbol),
        true,
    )?;
    let name = identifier(arena, target, name)?;
    let declaration = arena
        .factory()
        .create_variable_declaration(target, name, None, Some(type_node), None)
        .map_err(factory_error)?;
    let declarations = node_array(arena, target, vec![declaration])?;
    let list = arena
        .factory()
        .create_variable_declaration_list(target, declarations, NodeFlags::LET)
        .map_err(factory_error)?;
    arena
        .factory()
        .create_variable_statement(target, None, list)
        .map_err(factory_error)
}

/// A namespace body statement; a local declaration (an alias's target)
/// gets no `export`.
struct HoverStatement {
    node: TransformNode,
    is_local: bool,
}

/// tsgo `expandModuleDecl`: a namespace declaration with its exported
/// members.
fn expand_module_declaration(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    symbol: SymbolId,
) -> BuildResult<TransformNode> {
    let exports = checker
        .get_exports_of_symbol(symbol)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    let mut members = exports
        .values()
        .copied()
        .filter(|&member| {
            is_namespace_member(checker, member)
                && tsc_syntax::is_identifier_text(&name_text(checker, member))
        })
        .collect::<Vec<_>>();
    checker.sort_symbols(&mut members);
    add_approximate_length(context, 14);

    // The name as the symbol display writes it.
    let old_flags = context.flags;
    context.flags.0 |=
        WRITE_TYPE_PARAMETERS_IN_QUALIFIED_NAME | SYMBOL_FORMAT_USE_ONLY_EXTERNAL_ALIASING;
    let local_name = symbol_to_node(
        checker,
        arena,
        target,
        context,
        symbol,
        tsc_emitter::EmitSymbolMeaning(SymbolFlags::ALL.bits() as u32),
    );
    context.flags = old_flags;
    let local_name = local_name?;

    let mut statements: Vec<HoverStatement> = Vec::new();
    let mut emitted_locals: Vec<SymbolId> = Vec::new();
    let mut index = 0;
    while index < members.len() {
        let member = members[index];
        if check_truncation_length_if_expanding(context) && index + 3 < members.len() - 1 {
            context.out.truncated = true;
            let text = format!("... ({} more) ...", members.len() - index - 1);
            let expression = identifier(arena, target, &text)?;
            statements.push(HoverStatement {
                node: arena
                    .factory()
                    .create_expression_statement(target, expression)
                    .map_err(factory_error)?,
                is_local: false,
            });
            // On to the last member.
            index = members.len() - 1;
            continue;
        }
        index += 1;

        // An alias or re-export.
        if checker.symbol_flags(member).intersects(SymbolFlags::ALIAS) {
            let alias_target = match checker.get_declaration_of_alias_symbol(member) {
                Some(declaration) => checker
                    .get_target_of_alias_declaration(declaration)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?
                    .map(|target| checker.get_merged_symbol(target)),
                None => None,
            };
            if let Some(alias_target) = alias_target {
                // A local target (no export itself) is declared first.
                if checker.symbol_flags(alias_target).intersects(
                    SymbolFlags::BLOCK_SCOPED_VARIABLE
                        | SymbolFlags::FUNCTION_SCOPED_VARIABLE
                        | SymbolFlags::PROPERTY,
                ) && !emitted_locals.contains(&alias_target)
                {
                    emitted_locals.push(alias_target);
                    let local_type = checker
                        .get_type_of_symbol(alias_target)
                        .and_then(|ty| checker.get_widened_type(ty))
                        .map_err(|abort| checker_abort_error(checker, context, abort))?;
                    let local_name = name_text(checker, alias_target);
                    add_approximate_length(context, local_name.len() + 5);
                    statements.push(HoverStatement {
                        node: let_statement(
                            checker,
                            arena,
                            target,
                            context,
                            &local_name,
                            local_type,
                            alias_target,
                        )?,
                        is_local: true,
                    });
                }
                let target_name = name_text(checker, alias_target);
                let member_name = name_text(checker, member);
                add_approximate_length(context, 16 + member_name.len());
                let property_name = if member_name != target_name {
                    Some(identifier(arena, target, &target_name)?)
                } else {
                    None
                };
                let name = identifier(arena, target, &member_name)?;
                let specifier = arena
                    .factory()
                    .create_export_specifier(target, false, property_name, name)
                    .map_err(factory_error)?;
                let specifiers = node_array(arena, target, vec![specifier])?;
                let named = arena
                    .factory()
                    .create_named_exports(target, specifiers)
                    .map_err(factory_error)?;
                statements.push(HoverStatement {
                    node: arena
                        .factory()
                        .create_export_declaration(target, None, false, Some(named), None, None)
                        .map_err(factory_error)?,
                    is_local: false,
                });
                continue;
            }
        }

        let resolved = checker
            .resolve_symbol_ex(Some(member), false)
            .map_err(|abort| checker_abort_error(checker, context, abort))?
            .unwrap_or(member);
        let member_name = name_text(checker, member);

        // A function as function declarations.
        if checker
            .symbol_flags(resolved)
            .intersects(SymbolFlags::FUNCTION | SymbolFlags::METHOD)
        {
            let ty = checker
                .get_type_of_symbol(resolved)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            let signatures = checker
                .get_signatures_of_type(ty, SignatureKind::Call)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            for &signature in signatures.iter() {
                add_approximate_length(context, 1);
                let name = identifier(arena, target, &member_name)?;
                let declaration = signature_to_signature_declaration_helper(
                    checker,
                    arena,
                    target,
                    signature,
                    SyntaxKind::FunctionDeclaration,
                    context,
                    Some(SignatureDeclarationOptions {
                        modifiers: None,
                        name: Some(name),
                        question_token: None,
                    }),
                )?;
                statements.push(HoverStatement {
                    node: declaration,
                    is_local: false,
                });
            }
            // A function with namespace characteristics but no exports
            // also gets an empty namespace.
            let merged = checker.get_merged_symbol(resolved);
            let has_module_exports = checker
                .symbol_flags(merged)
                .intersects(SymbolFlags::VALUE_MODULE | SymbolFlags::NAMESPACE_MODULE)
                && !checker.binder.symbol(merged).exports().is_empty();
            if !has_module_exports {
                let name = identifier(arena, target, &member_name)?;
                let statements_array = node_array(arena, target, Vec::new())?;
                let block = arena
                    .factory()
                    .create_module_block(target, statements_array)
                    .map_err(factory_error)?;
                statements.push(HoverStatement {
                    node: arena
                        .factory()
                        .create_module_declaration(
                            target,
                            None,
                            name,
                            None,
                            Some(block),
                            NodeFlags::NAMESPACE,
                        )
                        .map_err(factory_error)?,
                    is_local: false,
                });
            }
            continue;
        }

        statements.push(HoverStatement {
            node: serialize_namespace_member(
                checker,
                arena,
                target,
                context,
                resolved,
                &member_name,
            )?,
            is_local: false,
        });
    }

    // Exported statements get `export` (not local declarations or export
    // declarations).
    for statement in &mut statements {
        if statement.is_local
            || arena.node(statement.node).map_err(factory_error)?.kind
                == SyntaxKind::ExportDeclaration
        {
            continue;
        }
        if can_have_modifiers(arena, statement.node)? {
            let flags = transform_modifier_flags(arena, statement.node)? | ModifierFlags::EXPORT;
            statement.node = replace_modifiers(arena, statement.node, flags)?;
        }
    }

    // When every statement is exported, none says so.
    let mut body = statements
        .into_iter()
        .map(|statement| statement.node)
        .collect::<Vec<_>>();
    let mut all_exported = !body.is_empty();
    for &statement in &body {
        if !transform_modifier_flags(arena, statement)?.intersects(ModifierFlags::EXPORT) {
            all_exported = false;
            break;
        }
    }
    if all_exported {
        for statement in &mut body {
            if can_have_modifiers(arena, *statement)? {
                let flags = without(
                    transform_modifier_flags(arena, *statement)?,
                    ModifierFlags::EXPORT,
                );
                *statement = replace_modifiers(arena, *statement, flags)?;
            }
        }
    }

    let is_identifier_name =
        arena.node(local_name).map_err(factory_error)?.kind == SyntaxKind::Identifier;
    let node_flags = if is_identifier_name {
        NodeFlags::NAMESPACE
    } else {
        NodeFlags::NONE
    };
    let attributes = checker
        .binder
        .symbol(symbol)
        .declarations
        .iter()
        .find_map(|&declaration| match checker.data_of(declaration) {
            NodeData::ModuleDeclaration(data) => data.attributes,
            _ => None,
        });
    let attributes = match attributes {
        Some(attributes) => {
            let attributes = deep_clone_parse_node(checker, arena, target, attributes)?;
            arena
                .metadata_mut(attributes)
                .add_flags(tsc_emitter::EmitFlags::SINGLE_LINE);
            Some(attributes)
        }
        None => None,
    };
    let body = node_array(arena, target, body)?;
    let block = arena
        .factory()
        .create_module_block(target, body)
        .map_err(factory_error)?;
    arena
        .factory()
        .create_module_declaration(
            target,
            None,
            local_name,
            attributes,
            Some(block),
            node_flags,
        )
        .map_err(factory_error)
}

/// tsgo `serializeTypeAliasForNamespace`: a type alias in a namespace
/// body.
fn serialize_type_alias_for_namespace(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
    symbol: SymbolId,
    name: &str,
) -> BuildResult<TransformNode> {
    let alias_type = checker
        .get_declared_type_of_type_alias(symbol)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    let parameters = checker.get_local_type_parameters_of_class_or_interface_or_type_alias(symbol);
    let mut type_parameters = Vec::with_capacity(parameters.len());
    for parameter in parameters {
        type_parameters.push(type_parameter_to_declaration(
            checker, arena, target, parameter, context, None,
        )?);
    }
    let old_flags = context.flags;
    context.flags.0 |= IN_TYPE_ALIAS;
    let type_node =
        super::type_nodes::type_to_type_node_helper(checker, arena, target, alias_type, context);
    context.flags = old_flags;
    let type_node = type_node?.expect("a type alias's type has a type node");
    add_approximate_length(context, 8 + name.len());
    let name = identifier(arena, target, name)?;
    let type_parameters = optional_node_array(arena, target, type_parameters)?;
    arena
        .factory()
        .create_type_alias_declaration(target, None, name, type_parameters, type_node)
        .map_err(factory_error)
}

/// tsgo `filterInheritedProperties`: without the properties inherited
/// unchanged from a base type.
fn filter_inherited_properties(
    checker: &mut CheckerState<'_>,
    context: &mut NodeBuilderContext<'_>,
    ty: TypeId,
    base_types: &[TypeId],
    properties: Vec<SymbolId>,
) -> BuildResult<Vec<SymbolId>> {
    if base_types.is_empty() {
        return Ok(properties);
    }
    let this_type = checker.this_type_of_class_or_interface(checker.get_target_type(ty));
    let mut inherited: Vec<tsc_types::EscapedName> = Vec::new();
    for &base in base_types {
        let base_with_this = checker
            .get_type_with_this_argument(base, this_type, false)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        let base_properties = checker
            .get_properties_of_type(base_with_this)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        for property in base_properties {
            let name = checker.binder.symbol(property).escaped_name;
            let existing = properties
                .iter()
                .copied()
                .find(|&candidate| checker.binder.symbol(candidate).escaped_name == name);
            if existing.is_some_and(|existing| {
                checker.binder.symbol(property).parent == checker.binder.symbol(existing).parent
            }) && !inherited.contains(&name)
            {
                inherited.push(name);
            }
        }
    }
    if inherited.is_empty() {
        return Ok(properties);
    }
    Ok(properties
        .into_iter()
        .filter(|&property| !inherited.contains(&checker.binder.symbol(property).escaped_name))
        .collect())
}

/// tsgo `isNamespaceMember`.
fn is_namespace_member(checker: &CheckerState<'_>, property: SymbolId) -> bool {
    let flags = checker.symbol_flags(property);
    if flags.intersects(SymbolFlags::TYPE | SymbolFlags::NAMESPACE | SymbolFlags::ALIAS) {
        return true;
    }
    let static_class_member = checker
        .binder
        .symbol(property)
        .value_declaration
        .is_some_and(|declaration| {
            syntactic_modifier_flags(checker, declaration).intersects(ModifierFlags::STATIC)
                && checker
                    .parent_of(declaration)
                    .is_some_and(|parent| is_class_like(checker, parent))
        });
    !(flags.intersects(SymbolFlags::PROTOTYPE)
        || name_text(checker, property) == "prototype"
        || static_class_member)
}

/// tsgo `isHashPrivate`: declared with a `#private` name.
fn is_hash_private(checker: &CheckerState<'_>, symbol: SymbolId) -> bool {
    checker
        .binder
        .symbol(symbol)
        .value_declaration
        .and_then(|declaration| {
            node_util::name_field_of(checker.binder.source_of_node(declaration), declaration)
        })
        .is_some_and(|name| checker.kind_of(name) == SyntaxKind::PrivateIdentifier)
}

// -- helpers ------------------------------------------------------------------

/// `flags &^ removed`.
fn without(flags: ModifierFlags, removed: ModifierFlags) -> ModifierFlags {
    ModifierFlags::from_bits(flags.bits() & !removed.bits())
}

/// tsgo `ast.SymbolName`.
fn name_text(checker: &CheckerState<'_>, symbol: SymbolId) -> String {
    checker.symbol_name(symbol).to_string_lossy().into_owned()
}

fn is_class_like(checker: &CheckerState<'_>, node: NodeId) -> bool {
    matches!(
        checker.kind_of(node),
        SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
    )
}

/// tsgo `node.ModifierFlags()`: the flags of the node's modifier list.
fn syntactic_modifier_flags(checker: &CheckerState<'_>, node: NodeId) -> ModifierFlags {
    node_util::get_syntactic_modifier_flags(checker.binder.source_of_node(node), node)
}

/// tsgo `ModifierFlags()` of a built node: its modifiers' flags.
fn transform_modifier_flags(
    arena: &TransformArena,
    node: TransformNode,
) -> BuildResult<ModifierFlags> {
    let Some(modifiers) = modifiers_of(arena, node)? else {
        return Ok(ModifierFlags::NONE);
    };
    let mut flags = ModifierFlags::NONE;
    for modifier in arena
        .node_array(TransformNodeArray::new(node.source(), modifiers))
        .map_err(factory_error)?
        .nodes
        .to_vec()
    {
        let kind = arena
            .node_ref(node.source(), modifier)
            .map(|modifier| arena.node(modifier).map(|record| record.kind))
            .transpose()
            .map_err(factory_error)?;
        if let Some(kind) = kind {
            flags |= node_util::modifier_to_flag(kind);
        }
    }
    Ok(flags)
}

fn modifiers_of(
    arena: &TransformArena,
    node: TransformNode,
) -> BuildResult<Option<tsc_syntax::NodeArrayId>> {
    Ok(match &arena.node(node).map_err(factory_error)?.data {
        NodeData::ClassDeclaration(data) => data.modifiers,
        NodeData::InterfaceDeclaration(data) => data.modifiers,
        NodeData::EnumDeclaration(data) => data.modifiers,
        NodeData::ModuleDeclaration(data) => data.modifiers,
        NodeData::TypeAliasDeclaration(data) => data.modifiers,
        NodeData::FunctionDeclaration(data) => data.modifiers,
        NodeData::VariableStatement(data) => data.modifiers,
        NodeData::PropertyDeclaration(data) => data.modifiers,
        NodeData::MethodDeclaration(data) => data.modifiers,
        NodeData::Constructor(data) => data.modifiers,
        NodeData::PropertySignature(data) => data.modifiers,
        NodeData::MethodSignature(data) => data.modifiers,
        NodeData::IndexSignature(data) => data.modifiers,
        NodeData::ExportDeclaration(data) => data.modifiers,
        _ => None,
    })
}

/// tsgo `ast.CanHaveModifiers` of a built member or statement.
fn can_have_modifiers(arena: &TransformArena, node: TransformNode) -> BuildResult<bool> {
    Ok(matches!(
        arena.node(node).map_err(factory_error)?.kind,
        SyntaxKind::ClassDeclaration
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::EnumDeclaration
            | SyntaxKind::ModuleDeclaration
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::VariableStatement
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::Constructor
            | SyntaxKind::PropertySignature
            | SyntaxKind::MethodSignature
            | SyntaxKind::IndexSignature
            | SyntaxKind::ExportDeclaration
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
    ))
}

/// tsgo `ast.ReplaceModifiers` with `CreateModifiersFromModifierFlags`.
fn replace_modifiers(
    arena: &mut TransformArena,
    node: TransformNode,
    flags: ModifierFlags,
) -> BuildResult<TransformNode> {
    let modifiers = arena
        .factory()
        .create_modifiers_from_modifier_flags(node.source(), flags)
        .map_err(factory_error)?;
    arena
        .factory()
        .replace_modifiers(node, modifiers)
        .map_err(factory_error)
}

/// The name node of a built member.
fn member_name(arena: &TransformArena, node: TransformNode) -> BuildResult<Option<TransformNode>> {
    let name = match &arena.node(node).map_err(factory_error)?.data {
        NodeData::PropertySignature(data) => data.name,
        NodeData::MethodSignature(data) => data.name,
        NodeData::PropertyDeclaration(data) => data.name,
        NodeData::MethodDeclaration(data) => data.name,
        NodeData::GetAccessor(data) => data.name,
        NodeData::SetAccessor(data) => data.name,
        _ => None,
    };
    Ok(name.and_then(|name| arena.node_ref(node.source(), name)))
}

fn child(
    arena: &TransformArena,
    source: TransformSourceId,
    node: Option<NodeId>,
) -> BuildResult<Option<TransformNode>> {
    Ok(node.and_then(|node| arena.node_ref(source, node)))
}

fn identifier(
    arena: &mut TransformArena,
    target: TransformSourceId,
    text: &str,
) -> BuildResult<TransformNode> {
    create_output_identifier(arena, target, text)
}

/// tsgo `NewStringLiteral(text, 0)`: a double-quoted string literal.
fn string_literal(
    arena: &mut TransformArena,
    target: TransformSourceId,
    text: &str,
) -> BuildResult<TransformNode> {
    arena
        .factory()
        .create_string_literal(target, text, false)
        .map_err(factory_error)
}

/// tsgo `DeepCloneNode` of a parsed node, in the target source.
fn deep_clone_parse_node(
    checker: &CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    node: NodeId,
) -> BuildResult<TransformNode> {
    let projected = project_parse_node(checker, arena, node)?.expect("a parsed node projects");
    let clone = arena
        .factory()
        .deep_clone_node_without_positions(projected)
        .map_err(factory_error)?;
    if clone.source() == target {
        return Ok(clone);
    }
    arena
        .factory()
        .clone_node_to_source(clone, target)
        .map_err(factory_error)
}

fn node_array(
    arena: &mut TransformArena,
    target: TransformSourceId,
    nodes: Vec<TransformNode>,
) -> BuildResult<TransformNodeArray> {
    arena
        .factory()
        .create_node_array(target, nodes)
        .map_err(factory_error)
}

fn optional_node_array(
    arena: &mut TransformArena,
    target: TransformSourceId,
    nodes: Vec<TransformNode>,
) -> BuildResult<Option<TransformNodeArray>> {
    if nodes.is_empty() {
        return Ok(None);
    }
    node_array(arena, target, nodes).map(Some)
}

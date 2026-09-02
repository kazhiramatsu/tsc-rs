use std::collections::HashMap;

use tsc_binder::{node_util, SymbolId};
use tsc_emitter::{
    CommentRange, EmitFlags, EmitNodeBuilderFlags, EmitResolverError, EmitResolverMethod,
    EmitResolverNode, EmitSymbolAccessibility, EmitSymbolMeaning, SourceFileId, SourceRange,
    SyntheticComment, SyntheticCommentKind, TransformArena, TransformError, TransformFlags,
    TransformNode, TransformNodeArray, TransformSourceId,
};
use tsc_syntax::nodes::{
    ArrayTypeData, BigIntLiteralData, ConditionalTypeData, IdentifierData, IndexedAccessTypeData,
    InferTypeData, IntersectionTypeData, LiteralTypeData, MappedTypeData, NamedTupleMemberData,
    NumericLiteralData, OptionalTypeData, PrefixUnaryExpressionData, PropertySignatureData,
    QualifiedNameData, RestTypeData, StringLiteralData, TemplateLiteralTypeData,
    TemplateLiteralTypeSpanData, TupleTypeData, TypeLiteralData, TypeOperatorData,
    TypeParameterData, TypeQueryData, TypeReferenceData, UnionTypeData,
};
use tsc_syntax::{NodeArrayId, NodeData, NodeId, SyntaxKind};
use tsc_types::{
    CheckFlags, ElementFlags, IntersectionFlags, LiteralValue, ModifierFlags, NodeFlags,
    ObjectFlags, SignatureFlags, SymbolFlags, TypeData, TypeFlags, TypeId,
};

use crate::annotate::is_reserved_member_name;
use crate::links::LinkSlot;
use crate::state::{CheckAbort, CheckerState, ResolvedMembers};

use super::signatures::{
    index_info_to_index_signature_declaration_helper, prime_type_parameter_names_for_scope,
    signature_to_signature_declaration_helper, type_parameter_to_declaration_with_constraint,
    SignatureDeclarationOptions,
};
use super::{
    chains_get_property_name_node_for_symbol, chains_symbol_to_entity_name_node,
    chains_symbol_to_type_node, check_truncation_length, restore_flags, save_restore_flags,
    should_expand_type, symbol_to_node, type_parameter_to_name, with_context, NodeBuilderContext,
};

const METHOD: EmitResolverMethod = EmitResolverMethod::CreateTypeOfDeclaration;

const WRITE_ARRAY_AS_GENERIC_TYPE: u32 = 2;
const USE_STRUCTURAL_FALLBACK: u32 = 8;
const FORBID_INDEXED_ACCESS_SYMBOL_REFERENCES: u32 = 16;
const USE_FULLY_QUALIFIED_TYPE: u32 = 64;
const ALLOW_ANONYMOUS_IDENTIFIER: u32 = 131_072;
const ALLOW_EMPTY_UNION_OR_INTERSECTION: u32 = 262_144;
const ALLOW_EMPTY_TUPLE: u32 = 524_288;
const ALLOW_UNIQUE_ES_SYMBOL_TYPE: u32 = 1_048_576;
const IN_OBJECT_TYPE_LITERAL: u32 = 4_194_304;
const IN_TYPE_ALIAS: u32 = 8_388_608;
const USE_ALIAS_DEFINED_OUTSIDE_CURRENT_SCOPE: u32 = 16_384;
const ALLOW_THIS_IN_OBJECT_LITERAL: u32 = 32_768;
const WRITE_CLASS_EXPRESSION_AS_TYPE_LITERAL: u32 = 2_048;
const USE_TYPE_OF_FUNCTION: u32 = 4_096;
const NO_TYPE_REDUCTION: u32 = 536_870_912;
const USE_SINGLE_QUOTES_FOR_STRING_LITERAL_TYPE: u32 = 268_435_456;
const IGNORE_ERRORS: EmitNodeBuilderFlags = EmitNodeBuilderFlags(70_221_824);

pub(super) type BuildResult<T> = Result<T, EmitResolverError>;

pub(super) fn checker_abort_error(
    checker: &CheckerState<'_>,
    context: &NodeBuilderContext<'_>,
    abort: CheckAbort,
) -> EmitResolverError {
    let node = context
        .enclosing_declaration
        .unwrap_or_else(|| checker.binder.source(0).root);
    let source = u32::try_from(checker.binder.file_index_of_node(node)).unwrap_or(0);
    EmitResolverError::CheckerAborted {
        method: METHOD,
        node: EmitResolverNode::new(SourceFileId::from_raw(source), node),
        reason: abort.description(),
    }
}

pub(super) fn factory_error(error: TransformError) -> EmitResolverError {
    EmitResolverError::Factory {
        method: METHOD,
        error: Box::new(error),
    }
}

fn node_array_or_empty(
    arena: &mut TransformArena,
    target: TransformSourceId,
    array: Option<NodeArrayId>,
) -> BuildResult<TransformNodeArray> {
    match array {
        Some(array) => Ok(TransformNodeArray::new(target, array)),
        None => arena
            .factory()
            .create_node_array(target, Vec::new())
            .map_err(factory_error),
    }
}

/// tsrs-native: shared NodeData-spelling seam for the typed NodeFactory faces
/// (h2-7a-m-3.5 §5.8) — routes a face-built node through the arena factory
/// with the face's exact transform-flag word; no upstream counterpart.
pub(crate) fn create_factory_node(
    arena: &mut TransformArena,
    target: TransformSourceId,
    data: NodeData,
    fallback_flags: TransformFlags,
) -> BuildResult<TransformNode> {
    let child = |id: Option<NodeId>| id.map(|id| TransformNode::new(target, id));
    let array = |id: Option<NodeArrayId>| id.map(|id| TransformNodeArray::new(target, id));
    let required_child = |id: Option<NodeId>, parent, field| {
        child(id)
            .ok_or_else(|| factory_error(TransformError::RequiredChildRemoved { parent, field }))
    };
    macro_rules! with_array_or_empty {
        ($array:expr, $name:ident, $create:expr) => {{
            let $name = node_array_or_empty(arena, target, $array)?;
            $create
        }};
    }

    let result = match data {
        NodeData::Identifier(data) => arena.factory().create_identifier(target, data.text),
        NodeData::PrivateIdentifier(data) => {
            arena.factory().create_private_identifier(target, data.text)
        }
        NodeData::StringLiteral(data) => arena
            .factory()
            .create_string_literal(target, data.text, false),
        NodeData::NumericLiteral(data) => arena.factory().create_numeric_literal(target, data.text),
        NodeData::BigIntLiteral(data) => arena.factory().create_big_int_literal(target, data.text),
        NodeData::TemplateHead(data) => {
            arena
                .factory()
                .create_template_head(target, data.text, data.raw_text)
        }
        NodeData::QualifiedName(data) => arena.factory().create_qualified_name(
            target,
            required_child(data.left, SyntaxKind::QualifiedName, "left")?,
            required_child(data.right, SyntaxKind::QualifiedName, "right")?,
        ),
        NodeData::ComputedPropertyName(data) => arena.factory().create_computed_property_name(
            target,
            required_child(
                data.expression,
                SyntaxKind::ComputedPropertyName,
                "expression",
            )?,
        ),
        NodeData::TypeParameter(data) => arena.factory().create_type_parameter_declaration(
            target,
            array(data.modifiers),
            required_child(data.name, SyntaxKind::TypeParameter, "name")?,
            child(data.constraint),
            child(data.r#default),
        ),
        NodeData::Parameter(data) => arena.factory().create_parameter_declaration(
            target,
            array(data.modifiers),
            child(data.dot_dot_dot_token),
            required_child(data.name, SyntaxKind::Parameter, "name")?,
            child(data.question_token),
            child(data.r#type),
            child(data.initializer),
        ),
        NodeData::PropertySignature(data) => arena.factory().create_property_signature(
            target,
            array(data.modifiers),
            required_child(data.name, SyntaxKind::PropertySignature, "name")?,
            child(data.question_token),
            child(data.r#type),
        ),
        NodeData::PropertyDeclaration(data) => arena.factory().create_property_declaration(
            target,
            array(data.modifiers),
            required_child(data.name, SyntaxKind::PropertyDeclaration, "name")?,
            child(data.question_token.or(data.exclamation_token)),
            child(data.r#type),
            child(data.initializer),
        ),
        NodeData::MethodSignature(data) => with_array_or_empty!(
            data.parameters,
            parameters,
            arena.factory().create_method_signature(
                target,
                array(data.modifiers),
                required_child(data.name, SyntaxKind::MethodSignature, "name")?,
                child(data.question_token),
                array(data.type_parameters),
                parameters,
                child(data.r#type),
            )
        ),
        NodeData::MethodDeclaration(data) => with_array_or_empty!(
            data.parameters,
            parameters,
            arena.factory().create_method_declaration(
                target,
                array(data.modifiers),
                child(data.asterisk_token),
                required_child(data.name, SyntaxKind::MethodDeclaration, "name")?,
                child(data.question_token),
                array(data.type_parameters),
                parameters,
                child(data.r#type),
                child(data.body),
            )
        ),
        NodeData::Constructor(data) => with_array_or_empty!(
            data.parameters,
            parameters,
            arena.factory().create_constructor_declaration(
                target,
                array(data.modifiers),
                parameters,
                child(data.body),
            )
        ),
        NodeData::GetAccessor(data) => with_array_or_empty!(
            data.parameters,
            parameters,
            arena.factory().create_get_accessor_declaration(
                target,
                array(data.modifiers),
                required_child(data.name, SyntaxKind::GetAccessor, "name")?,
                parameters,
                child(data.r#type),
                child(data.body),
            )
        ),
        NodeData::SetAccessor(data) => with_array_or_empty!(
            data.parameters,
            parameters,
            arena.factory().create_set_accessor_declaration(
                target,
                array(data.modifiers),
                required_child(data.name, SyntaxKind::SetAccessor, "name")?,
                parameters,
                child(data.body),
            )
        ),
        NodeData::CallSignature(data) => with_array_or_empty!(
            data.parameters,
            parameters,
            arena.factory().create_call_signature(
                target,
                array(data.type_parameters),
                parameters,
                child(data.r#type),
            )
        ),
        NodeData::ConstructSignature(data) => with_array_or_empty!(
            data.parameters,
            parameters,
            arena.factory().create_construct_signature(
                target,
                array(data.type_parameters),
                parameters,
                child(data.r#type),
            )
        ),
        NodeData::IndexSignature(data) => with_array_or_empty!(
            data.parameters,
            parameters,
            arena.factory().create_index_signature(
                target,
                array(data.modifiers),
                parameters,
                required_child(data.r#type, SyntaxKind::IndexSignature, "type")?,
            )
        ),
        NodeData::TypePredicate(data) => arena.factory().create_type_predicate_node(
            target,
            child(data.asserts_modifier),
            required_child(
                data.parameter_name,
                SyntaxKind::TypePredicate,
                "parameterName",
            )?,
            child(data.r#type),
        ),
        NodeData::TypeReference(data) => arena.factory().create_type_reference_node(
            target,
            required_child(data.type_name, SyntaxKind::TypeReference, "typeName")?,
            array(data.type_arguments),
        ),
        NodeData::FunctionType(data) => with_array_or_empty!(
            data.parameters,
            parameters,
            arena.factory().create_function_type_node(
                target,
                array(data.type_parameters),
                parameters,
                required_child(data.r#type, SyntaxKind::FunctionType, "type")?,
            )
        ),
        NodeData::ConstructorType(data) => with_array_or_empty!(
            data.parameters,
            parameters,
            arena.factory().create_constructor_type_node(
                target,
                array(data.modifiers),
                array(data.type_parameters),
                parameters,
                required_child(data.r#type, SyntaxKind::ConstructorType, "type")?,
            )
        ),
        NodeData::TypeQuery(data) => arena.factory().create_type_query_node(
            target,
            required_child(data.expr_name, SyntaxKind::TypeQuery, "exprName")?,
            array(data.type_arguments),
        ),
        NodeData::TypeLiteral(data) => with_array_or_empty!(
            data.members,
            members,
            arena.factory().create_type_literal_node(target, members)
        ),
        NodeData::ArrayType(data) => arena.factory().create_array_type_node(
            target,
            required_child(data.element_type, SyntaxKind::ArrayType, "elementType")?,
        ),
        NodeData::TupleType(data) => with_array_or_empty!(
            data.elements,
            elements,
            arena.factory().create_tuple_type_node(target, elements)
        ),
        NodeData::NamedTupleMember(data) => arena.factory().create_named_tuple_member(
            target,
            child(data.dot_dot_dot_token),
            required_child(data.name, SyntaxKind::NamedTupleMember, "name")?,
            child(data.question_token),
            required_child(data.r#type, SyntaxKind::NamedTupleMember, "type")?,
        ),
        NodeData::OptionalType(data) => arena.factory().create_optional_type_node(
            target,
            required_child(data.r#type, SyntaxKind::OptionalType, "type")?,
        ),
        NodeData::RestType(data) => arena.factory().create_rest_type_node(
            target,
            required_child(data.r#type, SyntaxKind::RestType, "type")?,
        ),
        NodeData::UnionType(data) => with_array_or_empty!(
            data.types,
            types,
            arena.factory().create_union_type_node(target, types)
        ),
        NodeData::IntersectionType(data) => with_array_or_empty!(
            data.types,
            types,
            arena.factory().create_intersection_type_node(target, types)
        ),
        NodeData::ConditionalType(data) => arena.factory().create_conditional_type_node(
            target,
            required_child(data.check_type, SyntaxKind::ConditionalType, "checkType")?,
            required_child(
                data.extends_type,
                SyntaxKind::ConditionalType,
                "extendsType",
            )?,
            required_child(data.true_type, SyntaxKind::ConditionalType, "trueType")?,
            required_child(data.false_type, SyntaxKind::ConditionalType, "falseType")?,
        ),
        NodeData::InferType(data) => arena.factory().create_infer_type_node(
            target,
            required_child(data.type_parameter, SyntaxKind::InferType, "typeParameter")?,
        ),
        NodeData::TemplateLiteralType(data) => with_array_or_empty!(
            data.template_spans,
            template_spans,
            arena.factory().create_template_literal_type(
                target,
                required_child(data.head, SyntaxKind::TemplateLiteralType, "head")?,
                template_spans,
            )
        ),
        NodeData::ImportType(data) => arena.factory().create_import_type_node(
            target,
            required_child(data.argument, SyntaxKind::ImportType, "argument")?,
            child(data.attributes),
            child(data.qualifier),
            array(data.type_arguments),
            data.is_type_of,
        ),
        NodeData::ParenthesizedType(data) => arena.factory().create_parenthesized_type(
            target,
            required_child(data.r#type, SyntaxKind::ParenthesizedType, "type")?,
        ),
        NodeData::TypeOperator(data) => arena.factory().create_type_operator_node(
            target,
            data.operator,
            required_child(data.r#type, SyntaxKind::TypeOperator, "type")?,
        ),
        NodeData::IndexedAccessType(data) => arena.factory().create_indexed_access_type_node(
            target,
            required_child(
                data.object_type,
                SyntaxKind::IndexedAccessType,
                "objectType",
            )?,
            required_child(data.index_type, SyntaxKind::IndexedAccessType, "indexType")?,
        ),
        NodeData::MappedType(data) => arena.factory().create_mapped_type_node(
            target,
            child(data.readonly_token),
            required_child(data.type_parameter, SyntaxKind::MappedType, "typeParameter")?,
            child(data.name_type),
            child(data.question_token),
            child(data.r#type),
            array(data.members),
        ),
        NodeData::LiteralType(data) => arena.factory().create_literal_type_node(
            target,
            required_child(data.literal, SyntaxKind::LiteralType, "literal")?,
        ),
        NodeData::TemplateLiteralTypeSpan(data) => {
            arena.factory().create_template_literal_type_span(
                target,
                required_child(data.r#type, SyntaxKind::TemplateLiteralTypeSpan, "type")?,
                required_child(data.literal, SyntaxKind::TemplateLiteralTypeSpan, "literal")?,
            )
        }
        NodeData::ExpressionWithTypeArguments(data) => {
            arena.factory().create_expression_with_type_arguments(
                target,
                required_child(
                    data.expression,
                    SyntaxKind::ExpressionWithTypeArguments,
                    "expression",
                )?,
                array(data.type_arguments),
            )
        }
        NodeData::JSDocFunctionType(data) => with_array_or_empty!(
            data.parameters,
            parameters,
            arena
                .factory()
                .create_jsdoc_function_type(target, parameters, child(data.r#type))
        ),
        NodeData::PrefixUnaryExpression(data) => arena.factory().create_prefix_unary_expression(
            target,
            data.operator,
            required_child(data.operand, SyntaxKind::PrefixUnaryExpression, "operand")?,
        ),
        NodeData::PropertyAccessExpression(data) if data.question_dot_token.is_none() => {
            arena.factory().create_property_access_expression(
                target,
                required_child(
                    data.expression,
                    SyntaxKind::PropertyAccessExpression,
                    "expression",
                )?,
                required_child(data.name, SyntaxKind::PropertyAccessExpression, "name")?,
            )
        }
        NodeData::ElementAccessExpression(data) if data.question_dot_token.is_none() => {
            arena.factory().create_element_access_expression(
                target,
                required_child(
                    data.expression,
                    SyntaxKind::ElementAccessExpression,
                    "expression",
                )?,
                required_child(
                    data.argument_expression,
                    SyntaxKind::ElementAccessExpression,
                    "argumentExpression",
                )?,
            )
        }
        NodeData::FunctionDeclaration(data) => with_array_or_empty!(
            data.parameters,
            parameters,
            arena.factory().create_function_declaration(
                target,
                array(data.modifiers),
                child(data.asterisk_token),
                child(data.name),
                array(data.type_parameters),
                parameters,
                child(data.r#type),
                child(data.body),
            )
        ),
        NodeData::FunctionExpression(data) => with_array_or_empty!(
            data.parameters,
            parameters,
            arena.factory().create_function_expression(
                target,
                array(data.modifiers),
                child(data.asterisk_token),
                child(data.name),
                array(data.type_parameters),
                parameters,
                child(data.r#type),
                required_child(data.body, SyntaxKind::FunctionExpression, "body")?,
            )
        ),
        NodeData::ArrowFunction(data) => with_array_or_empty!(
            data.parameters,
            parameters,
            arena.factory().create_arrow_function(
                target,
                array(data.modifiers),
                array(data.type_parameters),
                parameters,
                child(data.r#type),
                child(data.equals_greater_than_token),
                required_child(data.body, SyntaxKind::ArrowFunction, "body")?,
            )
        ),
        NodeData::Block(data) => with_array_or_empty!(
            data.statements,
            statements,
            arena.factory().create_block(target, statements, false)
        ),
        NodeData::VariableDeclaration(data) => arena.factory().create_variable_declaration(
            target,
            required_child(data.name, SyntaxKind::VariableDeclaration, "name")?,
            child(data.exclamation_token),
            child(data.r#type),
            child(data.initializer),
        ),
        NodeData::VariableDeclarationList(data) => with_array_or_empty!(
            data.declarations,
            declarations,
            arena
                .factory()
                .create_variable_declaration_list(target, declarations, NodeFlags::NONE,)
        ),
        NodeData::VariableStatement(data) => arena.factory().create_variable_statement(
            target,
            array(data.modifiers),
            required_child(
                data.declaration_list,
                SyntaxKind::VariableStatement,
                "declarationList",
            )?,
        ),
        NodeData::EmptyStatement(_) => arena.factory().create_empty_statement(target),
        NodeData::ExpressionStatement(data) => arena.factory().create_expression_statement(
            target,
            required_child(
                data.expression,
                SyntaxKind::ExpressionStatement,
                "expression",
            )?,
        ),
        NodeData::ClassDeclaration(data) => with_array_or_empty!(
            data.members,
            members,
            arena.factory().create_class_declaration(
                target,
                array(data.modifiers),
                child(data.name),
                array(data.type_parameters),
                array(data.heritage_clauses),
                members,
            )
        ),
        NodeData::InterfaceDeclaration(data) => with_array_or_empty!(
            data.members,
            members,
            arena.factory().create_interface_declaration(
                target,
                array(data.modifiers),
                required_child(data.name, SyntaxKind::InterfaceDeclaration, "name")?,
                array(data.type_parameters),
                array(data.heritage_clauses),
                members,
            )
        ),
        NodeData::TypeAliasDeclaration(data) => arena.factory().create_type_alias_declaration(
            target,
            array(data.modifiers),
            required_child(data.name, SyntaxKind::TypeAliasDeclaration, "name")?,
            array(data.type_parameters),
            required_child(data.r#type, SyntaxKind::TypeAliasDeclaration, "type")?,
        ),
        NodeData::EnumDeclaration(data) => with_array_or_empty!(
            data.members,
            members,
            arena.factory().create_enum_declaration(
                target,
                array(data.modifiers),
                required_child(data.name, SyntaxKind::EnumDeclaration, "name")?,
                members,
            )
        ),
        NodeData::ModuleDeclaration(data) => arena.factory().create_module_declaration(
            target,
            array(data.modifiers),
            required_child(data.name, SyntaxKind::ModuleDeclaration, "name")?,
            child(data.body),
            NodeFlags::NONE,
        ),
        NodeData::ModuleBlock(data) => with_array_or_empty!(
            data.statements,
            statements,
            arena.factory().create_module_block(target, statements)
        ),
        NodeData::NamespaceExportDeclaration(data) => {
            arena.factory().create_namespace_export_declaration(
                target,
                required_child(data.name, SyntaxKind::NamespaceExportDeclaration, "name")?,
            )
        }
        NodeData::ImportEqualsDeclaration(data) => {
            arena.factory().create_import_equals_declaration(
                target,
                array(data.modifiers),
                data.is_type_only,
                required_child(data.name, SyntaxKind::ImportEqualsDeclaration, "name")?,
                required_child(
                    data.module_reference,
                    SyntaxKind::ImportEqualsDeclaration,
                    "moduleReference",
                )?,
            )
        }
        NodeData::ImportDeclaration(data) => arena.factory().create_import_declaration(
            target,
            array(data.modifiers),
            child(data.import_clause),
            required_child(
                data.module_specifier,
                SyntaxKind::ImportDeclaration,
                "moduleSpecifier",
            )?,
            child(data.attributes),
        ),
        NodeData::ImportClause(data) => arena.factory().create_import_clause(
            target,
            data.phase_modifier,
            child(data.name),
            child(data.named_bindings),
        ),
        NodeData::ImportAttributes(data) => with_array_or_empty!(
            data.elements,
            elements,
            arena.factory().create_import_attributes(
                target,
                elements,
                data.multi_line,
                Some(data.token),
            )
        ),
        NodeData::ImportAttribute(data) => arena.factory().create_import_attribute(
            target,
            required_child(data.name, SyntaxKind::ImportAttribute, "name")?,
            required_child(data.value, SyntaxKind::ImportAttribute, "value")?,
        ),
        NodeData::NamespaceImport(data) => arena.factory().create_namespace_import(
            target,
            required_child(data.name, SyntaxKind::NamespaceImport, "name")?,
        ),
        NodeData::NamespaceExport(data) => arena.factory().create_namespace_export(
            target,
            required_child(data.name, SyntaxKind::NamespaceExport, "name")?,
        ),
        NodeData::NamedImports(data) => with_array_or_empty!(
            data.elements,
            elements,
            arena.factory().create_named_imports(target, elements)
        ),
        NodeData::ImportSpecifier(data) => arena.factory().create_import_specifier(
            target,
            data.is_type_only,
            child(data.property_name),
            required_child(data.name, SyntaxKind::ImportSpecifier, "name")?,
        ),
        NodeData::ExportAssignment(data) => arena.factory().create_export_assignment(
            target,
            array(data.modifiers),
            data.is_export_equals.unwrap_or(false),
            required_child(data.expression, SyntaxKind::ExportAssignment, "expression")?,
        ),
        NodeData::ExportDeclaration(data) => arena.factory().create_export_declaration(
            target,
            array(data.modifiers),
            data.is_type_only,
            child(data.export_clause),
            child(data.module_specifier),
            child(data.attributes),
        ),
        NodeData::NamedExports(data) => with_array_or_empty!(
            data.elements,
            elements,
            arena.factory().create_named_exports(target, elements)
        ),
        NodeData::ExportSpecifier(data) => arena.factory().create_export_specifier(
            target,
            data.is_type_only,
            child(data.property_name),
            required_child(data.name, SyntaxKind::ExportSpecifier, "name")?,
        ),
        NodeData::ExternalModuleReference(data) => {
            arena.factory().create_external_module_reference(
                target,
                required_child(
                    data.expression,
                    SyntaxKind::ExternalModuleReference,
                    "expression",
                )?,
            )
        }
        NodeData::HeritageClause(data) => with_array_or_empty!(
            data.types,
            types,
            arena
                .factory()
                .create_heritage_clause(target, data.token, types)
        ),
        NodeData::EnumMember(data) => arena.factory().create_enum_member(
            target,
            required_child(data.name, SyntaxKind::EnumMember, "name")?,
            child(data.initializer),
        ),
        other => arena.factory().create_node(target, other, fallback_flags),
    };
    result.map_err(factory_error)
}

pub(super) fn create_node(
    arena: &mut TransformArena,
    target: TransformSourceId,
    data: NodeData,
) -> BuildResult<TransformNode> {
    create_factory_node(arena, target, data, TransformFlags::CONTAINS_TYPE_SCRIPT)
}

/// tsrs-native: shared update seam for the typed NodeFactory update faces
/// (h2-7a-m-3.5 §5.8) — same-node identity when unchanged, otherwise a fresh
/// node with original provenance; no upstream counterpart.
pub(crate) fn update_factory_node(
    arena: &mut TransformArena,
    original: TransformNode,
    data: NodeData,
) -> BuildResult<TransformNode> {
    let source = original.source();
    let original_flags = arena.transform_flags(original);
    let child = |id: Option<NodeId>| id.map(|id| TransformNode::new(source, id));
    let array = |id: Option<NodeArrayId>| id.map(|id| TransformNodeArray::new(source, id));
    let required_child = |id: Option<NodeId>, parent, field| {
        child(id)
            .ok_or_else(|| factory_error(TransformError::RequiredChildRemoved { parent, field }))
    };
    let required_array = |id: Option<NodeArrayId>, parent, field| {
        array(id)
            .ok_or_else(|| factory_error(TransformError::RequiredChildRemoved { parent, field }))
    };
    let result = match data {
        NodeData::QualifiedName(data) => arena.factory().update_qualified_name(
            original,
            required_child(data.left, SyntaxKind::QualifiedName, "left")?,
            required_child(data.right, SyntaxKind::QualifiedName, "right")?,
        ),
        NodeData::ComputedPropertyName(data) => arena.factory().update_computed_property_name(
            original,
            required_child(
                data.expression,
                SyntaxKind::ComputedPropertyName,
                "expression",
            )?,
        ),
        NodeData::TypeParameter(data) => arena.factory().update_type_parameter_declaration(
            original,
            array(data.modifiers),
            required_child(data.name, SyntaxKind::TypeParameter, "name")?,
            child(data.constraint),
            child(data.r#default),
        ),
        NodeData::Parameter(data) => arena.factory().update_parameter_declaration(
            original,
            array(data.modifiers),
            child(data.dot_dot_dot_token),
            required_child(data.name, SyntaxKind::Parameter, "name")?,
            child(data.question_token),
            child(data.r#type),
            child(data.initializer),
        ),
        NodeData::TypePredicate(data) => arena.factory().update_type_predicate_node(
            original,
            child(data.asserts_modifier),
            required_child(
                data.parameter_name,
                SyntaxKind::TypePredicate,
                "parameterName",
            )?,
            child(data.r#type),
        ),
        NodeData::TypeReference(data) => arena.factory().update_type_reference_node(
            original,
            required_child(data.type_name, SyntaxKind::TypeReference, "typeName")?,
            array(data.type_arguments),
        ),
        NodeData::TypeQuery(data) => arena.factory().update_type_query_node(
            original,
            required_child(data.expr_name, SyntaxKind::TypeQuery, "exprName")?,
            array(data.type_arguments),
        ),
        NodeData::ConditionalType(data) => arena.factory().update_conditional_type_node(
            original,
            required_child(data.check_type, SyntaxKind::ConditionalType, "checkType")?,
            required_child(
                data.extends_type,
                SyntaxKind::ConditionalType,
                "extendsType",
            )?,
            required_child(data.true_type, SyntaxKind::ConditionalType, "trueType")?,
            required_child(data.false_type, SyntaxKind::ConditionalType, "falseType")?,
        ),
        NodeData::ImportType(data) => arena.factory().update_import_type_node(
            original,
            required_child(data.argument, SyntaxKind::ImportType, "argument")?,
            child(data.attributes),
            child(data.qualifier),
            array(data.type_arguments),
            data.is_type_of,
        ),
        NodeData::TypeOperator(data) => arena.factory().update_type_operator_node(
            original,
            required_child(data.r#type, SyntaxKind::TypeOperator, "type")?,
        ),
        NodeData::IndexedAccessType(data) => arena.factory().update_indexed_access_type_node(
            original,
            required_child(
                data.object_type,
                SyntaxKind::IndexedAccessType,
                "objectType",
            )?,
            required_child(data.index_type, SyntaxKind::IndexedAccessType, "indexType")?,
        ),
        NodeData::BindingElement(data) => arena.factory().update_binding_element(
            original,
            child(data.dot_dot_dot_token),
            child(data.property_name),
            required_child(data.name, SyntaxKind::BindingElement, "name")?,
            child(data.initializer),
        ),
        NodeData::ClassDeclaration(data) => arena.factory().update_class_declaration(
            original,
            array(data.modifiers),
            child(data.name),
            array(data.type_parameters),
            array(data.heritage_clauses),
            required_array(data.members, SyntaxKind::ClassDeclaration, "members")?,
        ),
        NodeData::ModuleDeclaration(data) => arena.factory().update_module_declaration(
            original,
            array(data.modifiers),
            required_child(data.name, SyntaxKind::ModuleDeclaration, "name")?,
            child(data.body),
        ),
        NodeData::ModuleBlock(data) => arena.factory().update_module_block(
            original,
            required_array(data.statements, SyntaxKind::ModuleBlock, "statements")?,
        ),
        NodeData::ExportDeclaration(data) => arena.factory().update_export_declaration(
            original,
            array(data.modifiers),
            data.is_type_only,
            child(data.export_clause),
            child(data.module_specifier),
            child(data.attributes),
        ),
        NodeData::NamedExports(data) => arena.factory().update_named_exports(
            original,
            required_array(data.elements, SyntaxKind::NamedExports, "elements")?,
        ),
        NodeData::Constructor(data) => arena.factory().update_constructor_declaration(
            original,
            data.modifiers,
            data.parameters,
            data.body,
            original_flags,
        ),
        NodeData::GetAccessor(data) => arena.factory().update_get_accessor_declaration(
            original,
            data.modifiers,
            data.name,
            data.parameters,
            data.r#type,
            data.body,
            original_flags,
        ),
        NodeData::SetAccessor(data) => arena.factory().update_set_accessor_declaration(
            original,
            data.modifiers,
            data.name,
            data.parameters,
            data.body,
            original_flags,
        ),
        other => {
            let flags = arena.transform_flags(original);
            arena.factory().update_node(original, other, flags)
        }
    };
    result.map_err(factory_error)
}

pub(super) fn create_token(
    arena: &mut TransformArena,
    target: TransformSourceId,
    kind: SyntaxKind,
) -> BuildResult<TransformNode> {
    let mut factory = arena.factory();
    let result = match kind {
        SyntaxKind::NullKeyword => factory.create_null(target),
        SyntaxKind::TrueKeyword => factory.create_true(target),
        SyntaxKind::FalseKeyword => factory.create_false(target),
        SyntaxKind::ThisType => factory.create_this_type_node(target),
        SyntaxKind::NotEmittedTypeElement => factory.create_not_emitted_type_element(target),
        SyntaxKind::AnyKeyword
        | SyntaxKind::BigIntKeyword
        | SyntaxKind::BooleanKeyword
        | SyntaxKind::IntrinsicKeyword
        | SyntaxKind::NeverKeyword
        | SyntaxKind::NumberKeyword
        | SyntaxKind::ObjectKeyword
        | SyntaxKind::StringKeyword
        | SyntaxKind::SymbolKeyword
        | SyntaxKind::UndefinedKeyword
        | SyntaxKind::UnknownKeyword
        | SyntaxKind::VoidKeyword => factory.create_keyword_type_node(target, kind),
        _ => factory.create_modifier(target, kind),
    };
    result.map_err(factory_error)
}

pub(super) fn create_identifier(
    arena: &mut TransformArena,
    target: TransformSourceId,
    text: &str,
) -> BuildResult<TransformNode> {
    create_node(
        arena,
        target,
        NodeData::Identifier(IdentifierData {
            escaped_text: tsc_syntax::escape_leading_underscores(text),
            text: text.to_owned(),
        }),
    )
}

pub(super) fn create_node_array(
    arena: &mut TransformArena,
    target: TransformSourceId,
    nodes: Vec<TransformNode>,
) -> BuildResult<tsc_syntax::NodeArrayId> {
    arena
        .factory()
        .create_node_array(target, nodes)
        .map(|array| array.array())
        .map_err(factory_error)
}

pub(super) fn set_single_line(arena: &mut TransformArena, node: TransformNode) -> TransformNode {
    arena.metadata_mut(node).add_flags(EmitFlags::SINGLE_LINE);
    node
}

pub(super) fn set_no_ascii_escaping(
    arena: &mut TransformArena,
    node: TransformNode,
) -> TransformNode {
    arena
        .metadata_mut(node)
        .add_flags(EmitFlags::NO_ASCII_ESCAPING);
    node
}

fn add_synthetic_leading_comment(
    arena: &mut TransformArena,
    node: TransformNode,
    text: impl Into<Box<str>>,
) -> TransformNode {
    arena
        .metadata_mut(node)
        .add_leading_comment(SyntheticComment::new(
            SyntheticCommentKind::MultiLine,
            text,
            false,
            false,
        ));
    node
}

fn add_synthetic_trailing_comment(
    arena: &mut TransformArena,
    node: TransformNode,
    text: impl Into<Box<str>>,
) -> TransformNode {
    arena
        .metadata_mut(node)
        .add_trailing_comment(SyntheticComment::new(
            SyntheticCommentKind::MultiLine,
            text,
            false,
            false,
        ));
    node
}

pub(super) fn project_parse_node(
    checker: &CheckerState<'_>,
    arena: &TransformArena,
    node: NodeId,
) -> BuildResult<Option<TransformNode>> {
    let source = u32::try_from(checker.binder.file_index_of_node(node)).unwrap_or(0);
    arena
        .parse_tree_transform_node(EmitResolverNode::new(SourceFileId::from_raw(source), node))
        .map_err(factory_error)
}

pub(super) fn clone_parse_node(
    checker: &CheckerState<'_>,
    arena: &mut TransformArena,
    node: NodeId,
) -> BuildResult<Option<TransformNode>> {
    let Some(original) = project_parse_node(checker, arena, node)? else {
        return Ok(None);
    };
    // clone_node performs set_original_node before returning. The explicit
    // range write is second; callers attach metadata only after this helper.
    let clone = arena
        .factory()
        .clone_node(original)
        .map_err(factory_error)?;
    arena
        .factory()
        .set_text_range(clone, original)
        .map_err(factory_error)
        .map(Some)
}

pub(super) fn clone_parse_node_to_source(
    checker: &CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    node: NodeId,
) -> BuildResult<Option<TransformNode>> {
    let Some(original) = project_parse_node(checker, arena, node)? else {
        return Ok(None);
    };
    if original.source() != target {
        return arena
            .factory()
            .clone_node_to_source(original, target)
            .map(Some)
            .map_err(factory_error);
    }
    clone_parse_node(checker, arena, node)
}

pub(super) fn range_synthesized_node_to_parse(
    checker: &CheckerState<'_>,
    arena: &mut TransformArena,
    node: TransformNode,
    original_node: NodeId,
) -> BuildResult<TransformNode> {
    let Some(original) = project_parse_node(checker, arena, original_node)? else {
        return Ok(node);
    };
    // Provenance order is load-bearing: original, range, then metadata.
    arena
        .set_original_node(node, Some(original))
        .map_err(factory_error)?;
    arena
        .factory()
        .set_text_range(node, original)
        .map_err(factory_error)
}

fn has_flag(context: &NodeBuilderContext<'_>, flag: u32) -> bool {
    context.flags.0 & flag != 0
}

fn clear_flag(context: &mut NodeBuilderContext<'_>, flag: u32) {
    context.flags.0 &= !flag;
}

fn set_flag(context: &mut NodeBuilderContext<'_>, flag: u32) {
    context.flags.0 |= flag;
}

pub(super) fn add_approximate_length(context: &mut NodeBuilderContext<'_>, amount: usize) {
    context.approximate_length += u32::try_from(amount).unwrap_or(u32::MAX);
}

fn js_len(text: &str) -> usize {
    text.encode_utf16().count()
}

fn require_node(
    context: &mut NodeBuilderContext<'_>,
    node: Option<TransformNode>,
) -> Option<TransformNode> {
    if node.is_none() {
        context.encountered_error = true;
    }
    node
}

fn create_keyword_type_node(
    arena: &mut TransformArena,
    target: TransformSourceId,
    kind: SyntaxKind,
) -> BuildResult<TransformNode> {
    arena
        .factory()
        .create_keyword_type_node(target, kind)
        .map_err(factory_error)
}

fn create_literal_type_node(
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

fn create_type_reference_node(
    arena: &mut TransformArena,
    target: TransformSourceId,
    name: TransformNode,
    arguments: Option<Vec<TransformNode>>,
) -> BuildResult<TransformNode> {
    let type_arguments = match arguments {
        Some(arguments) => Some(create_node_array(arena, target, arguments)?),
        None => None,
    };
    create_node(
        arena,
        target,
        NodeData::TypeReference(TypeReferenceData {
            type_arguments,
            type_name: Some(name.node()),
        }),
    )
}

fn create_named_type_reference(
    arena: &mut TransformArena,
    target: TransformSourceId,
    name: &str,
    arguments: Option<Vec<TransformNode>>,
) -> BuildResult<TransformNode> {
    let name = create_identifier(arena, target, name)?;
    create_type_reference_node(arena, target, name, arguments)
}

fn create_array_type_node(
    arena: &mut TransformArena,
    target: TransformSourceId,
    element: TransformNode,
) -> BuildResult<TransformNode> {
    create_node(
        arena,
        target,
        NodeData::ArrayType(ArrayTypeData {
            element_type: Some(element.node()),
        }),
    )
}

fn create_type_operator_node(
    arena: &mut TransformArena,
    target: TransformSourceId,
    operator: SyntaxKind,
    r#type: TransformNode,
) -> BuildResult<TransformNode> {
    create_node(
        arena,
        target,
        NodeData::TypeOperator(TypeOperatorData {
            operator,
            r#type: Some(r#type.node()),
        }),
    )
}

fn create_union_or_intersection_node(
    arena: &mut TransformArena,
    target: TransformSourceId,
    types: Vec<TransformNode>,
    union: bool,
) -> BuildResult<TransformNode> {
    let types = Some(create_node_array(arena, target, types)?);
    create_node(
        arena,
        target,
        if union {
            NodeData::UnionType(UnionTypeData { types })
        } else {
            NodeData::IntersectionType(IntersectionTypeData { types })
        },
    )
}

fn is_value_symbol_accessible(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    symbol: SymbolId,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<bool> {
    is_symbol_accessible_with_error_names(
        checker,
        arena,
        target,
        symbol,
        // checker.ts:isValueSymbolAccessible uses the plain Value face.  The
        // wider Value|ExportValue mask is for name resolution, not this
        // accessibility decision (and is observable by the replay tracker).
        EmitSymbolMeaning(SymbolFlags::VALUE.bits() as u32),
        context,
    )
}

fn is_type_symbol_accessible(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    symbol: SymbolId,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<bool> {
    is_symbol_accessible_with_error_names(
        checker,
        arena,
        target,
        symbol,
        EmitSymbolMeaning::TYPE,
        context,
    )
}

fn is_symbol_accessible_with_error_names(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    symbol: SymbolId,
    meaning: EmitSymbolMeaning,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<bool> {
    let Some(enclosing) = context.enclosing_declaration else {
        return Ok(true);
    };
    let symbol_flags = checker.symbol_flags(symbol);
    if context.enclosing_declaration_is_synthetic
        && context.tracker.is_statement_tracking()
        && symbol_flags.intersects(SymbolFlags::ASSIGNMENT)
        && symbol_flags.intersects(SymbolFlags::FUNCTION)
        && super::is_statement_symbol_remapped(checker, context, symbol)
    {
        return Ok(true);
    }
    let result = checker
        .emit_is_symbol_accessible(symbol, enclosing, meaning, false)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    restore_direct_symbol_visibility(checker, symbol, enclosing, meaning, context)?;
    let nested_enclosing = (!context.enclosing_declaration_is_synthetic).then_some(enclosing);
    if result.error_symbol_name.is_some() {
        with_context(
            checker,
            arena,
            target,
            nested_enclosing,
            Some(IGNORE_ERRORS),
            None,
            None,
            None,
            None,
            |checker, arena, target, nested| {
                symbol_to_node(checker, arena, target, nested, symbol, meaning)
            },
            None,
        )?;
    }
    if let Some(error_module_name) = result.error_module_name.as_deref() {
        let mut module_symbol = None;
        let mut parent = checker.binder.symbol(symbol).parent;
        while let Some(candidate) = parent {
            if checker.symbol_display_name(candidate) == error_module_name {
                module_symbol = Some(candidate);
                break;
            }
            parent = checker.binder.symbol(candidate).parent;
        }
        if module_symbol.is_none() {
            for declaration in checker.binder.symbol(symbol).declarations.clone() {
                if let Some(candidate) = checker
                    .get_external_module_container(declaration)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?
                {
                    if checker.symbol_display_name(candidate) == error_module_name {
                        module_symbol = Some(candidate);
                        break;
                    }
                }
            }
        }
        if let Some(module_symbol) = module_symbol {
            let module_meaning = if result.accessibility == EmitSymbolAccessibility::NotAccessible {
                EmitSymbolMeaning::NAMESPACE
            } else {
                EmitSymbolMeaning(0)
            };
            // isAnySymbolAccessible's qualified-chain diagnostic renders its
            // module with the enclosing declaration, but the CannotBeNamed
            // arm calls `symbolToString(symbolExternalModule)` without one.
            // That distinction selects a full source-file name rather than a
            // relative module specifier in the latter case.
            let module_enclosing = if result.accessibility == EmitSymbolAccessibility::NotAccessible
            {
                nested_enclosing
            } else {
                None
            };
            with_context(
                checker,
                arena,
                target,
                module_enclosing,
                Some(IGNORE_ERRORS),
                None,
                None,
                None,
                None,
                |checker, arena, target, nested| {
                    symbol_to_node(
                        checker,
                        arena,
                        target,
                        nested,
                        module_symbol,
                        module_meaning,
                    )
                },
                None,
            )?;
        }
    }
    Ok(result.accessibility == EmitSymbolAccessibility::Accessible)
}

/// `getAccessibleSymbolChain` returns the symbol itself when its written name
/// resolves directly at the enclosing declaration. The declaration-emitter
/// slice can conservatively return an enclosing module chain instead; retain
/// the upstream `hasVisibleDeclarations(chain[0])` visibility side effect for
/// the direct-name case used by NodeBuilder.
pub(super) fn restore_direct_symbol_visibility(
    checker: &mut CheckerState<'_>,
    symbol: SymbolId,
    enclosing: NodeId,
    meaning: EmitSymbolMeaning,
    context: &NodeBuilderContext<'_>,
) -> BuildResult<()> {
    // A fake signature scope is not an upstream declaration node. Names
    // resolved in that synthetic scope are already accepted by the tracker;
    // walking their real declarations here would spuriously paint enclosing
    // parameters/functions visible (notably anonymous class `__class`).
    if context.enclosing_declaration_is_synthetic {
        return Ok(());
    }
    let name = checker.binder.symbol(symbol).escaped_name.clone();
    let meaning_flags = SymbolFlags::from_bits(meaning.0 as i32);
    let resolved = checker
        .resolve_name(Some(enclosing), &name, meaning_flags, None, false, false)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    let Some(resolved) = resolved else {
        return Ok(());
    };
    let resolved = checker.get_export_symbol_of_value_symbol_if_exported(resolved);
    if checker.get_merged_symbol(resolved) != checker.get_merged_symbol(symbol) {
        return Ok(());
    }
    let _ = checker
        .has_visible_declarations_with_aliases(symbol, false)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    Ok(())
}

/// tsc-port: typeToTypeNodeHelper @6.0.3
/// tsc-hash: 69fd0744f9ba5217b598eaa6816f1650532dca59a9044240e3738d2b3e8ff86f
/// tsc-span: _tsc.js:51311-51318
pub(crate) fn type_to_type_node_helper(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    r#type: TypeId,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<Option<TransformNode>> {
    type_to_type_node_helper_optional(checker, arena, target, Some(r#type), context)
}

fn type_to_type_node_helper_optional(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    r#type: Option<TypeId>,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<Option<TransformNode>> {
    let restore = save_restore_flags(context);
    if let Some(r#type) = r#type {
        context.type_stack.push(Some(r#type));
    }
    let result = type_to_type_node_worker(checker, arena, target, r#type, context);
    if r#type.is_some() {
        context.type_stack.pop();
    }
    restore_flags(context, restore);
    result
}

/// tsc-port: typeToTypeNodeWorker @6.0.3
/// tsc-hash: dc4c98a69ce409185da50c7cdd11a363802eb83ddbdd89bac3166716de8e8322
/// tsc-span: _tsc.js:51319-52211
fn type_to_type_node_worker(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    r#type: Option<TypeId>,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<Option<TransformNode>> {
    // The Rust checker has no cancellation token (the same explicit elision
    // used by the existing checker workers).
    let in_type_alias = has_flag(context, IN_TYPE_ALIAS);
    clear_flag(context, IN_TYPE_ALIAS);
    let Some(mut r#type) = r#type else {
        if !has_flag(context, ALLOW_EMPTY_UNION_OR_INTERSECTION) {
            context.encountered_error = true;
            return Ok(None);
        }
        add_approximate_length(context, 3);
        return create_keyword_type_node(arena, target, SyntaxKind::AnyKeyword).map(Some);
    };

    if !has_flag(context, NO_TYPE_REDUCTION) {
        r#type = checker
            .get_reduced_type(r#type)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
    }
    let mut ty = checker.tables.type_of(r#type).clone();
    let mut flags = ty.flags;

    if flags.intersects(TypeFlags::ANY) {
        if let Some(alias) = ty.alias_symbol {
            let name = chains_symbol_to_entity_name_node(checker, arena, target, context, alias)?;
            let arguments = match ty.alias_type_arguments.as_deref() {
                Some(arguments) => {
                    map_to_type_nodes(checker, arena, target, arguments, context, false)?
                }
                None => None,
            };
            return create_type_reference_node(arena, target, name, arguments).map(Some);
        }
        if r#type == checker.tables.intrinsics.unresolved {
            let node = create_keyword_type_node(arena, target, SyntaxKind::AnyKeyword)?;
            return Ok(Some(add_synthetic_leading_comment(
                arena,
                node,
                "unresolved",
            )));
        }
        add_approximate_length(context, 3);
        let kind = if r#type == checker.tables.intrinsics.intrinsic_marker {
            SyntaxKind::IntrinsicKeyword
        } else {
            SyntaxKind::AnyKeyword
        };
        return create_keyword_type_node(arena, target, kind).map(Some);
    }
    if flags.intersects(TypeFlags::UNKNOWN) {
        return create_keyword_type_node(arena, target, SyntaxKind::UnknownKeyword).map(Some);
    }
    if flags.intersects(TypeFlags::STRING) {
        add_approximate_length(context, 6);
        return create_keyword_type_node(arena, target, SyntaxKind::StringKeyword).map(Some);
    }
    if flags.intersects(TypeFlags::NUMBER) {
        add_approximate_length(context, 6);
        return create_keyword_type_node(arena, target, SyntaxKind::NumberKeyword).map(Some);
    }
    if flags.intersects(TypeFlags::BIG_INT) {
        add_approximate_length(context, 6);
        return create_keyword_type_node(arena, target, SyntaxKind::BigIntKeyword).map(Some);
    }
    if flags.intersects(TypeFlags::BOOLEAN) && ty.alias_symbol.is_none() {
        add_approximate_length(context, 7);
        return create_keyword_type_node(arena, target, SyntaxKind::BooleanKeyword).map(Some);
    }

    let mut expanding_enum = false;
    if flags.intersects(TypeFlags::ENUM_LIKE) {
        if let Some(symbol) = ty.symbol {
            if checker
                .symbol_flags(symbol)
                .intersects(SymbolFlags::ENUM_MEMBER)
            {
                let parent = checker
                    .get_parent_of_symbol(symbol)
                    .expect("enum member symbols have a parent");
                let parent_name = chains_symbol_to_type_node(
                    checker,
                    arena,
                    target,
                    context,
                    parent,
                    EmitSymbolMeaning::TYPE,
                    None,
                )?;
                let declared = checker
                    .get_declared_type_of_enum(parent)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?;
                if declared == r#type {
                    return Ok(Some(parent_name));
                }
                let member_name = checker.symbol_display_name(symbol);
                if tsc_syntax::is_identifier_text(&member_name) {
                    let member = create_named_type_reference(arena, target, &member_name, None)?;
                    return append_reference_to_type(arena, target, parent_name, member).map(Some);
                }
                let literal = create_node(
                    arena,
                    target,
                    NodeData::StringLiteral(StringLiteralData {
                        text: member_name,
                        has_extended_unicode_escape: None,
                    }),
                )?;
                let literal = create_literal_type_node(arena, target, literal)?;
                let parent_data = arena.node(parent_name).map_err(factory_error)?.data.clone();
                let object = match parent_data {
                    NodeData::TypeReference(data) => create_node(
                        arena,
                        target,
                        NodeData::TypeQuery(TypeQueryData {
                            type_arguments: None,
                            expr_name: data.type_name,
                        }),
                    )?,
                    NodeData::ImportType(mut data) => {
                        data.is_type_of = true;
                        create_node(arena, target, NodeData::ImportType(data))?
                    }
                    _ => {
                        context.encountered_error = true;
                        return Ok(None);
                    }
                };
                return create_node(
                    arena,
                    target,
                    NodeData::IndexedAccessType(IndexedAccessTypeData {
                        object_type: Some(object.node()),
                        index_type: Some(literal.node()),
                    }),
                )
                .map(Some);
            }
            if !should_expand_type(checker, r#type, context, false) {
                return chains_symbol_to_type_node(
                    checker,
                    arena,
                    target,
                    context,
                    symbol,
                    EmitSymbolMeaning::TYPE,
                    None,
                )
                .map(Some);
            }
            expanding_enum = true;
        }
    }

    if flags.intersects(TypeFlags::STRING_LITERAL) {
        let TypeData::Literal {
            value: LiteralValue::String(value),
        } = ty.data
        else {
            unreachable!("StringLiteral flag implies string literal payload")
        };
        add_approximate_length(context, value.len() + 2);
        let single_quote = has_flag(context, USE_SINGLE_QUOTES_FOR_STRING_LITERAL_TYPE);
        let literal = arena
            .factory()
            .create_string_literal_from_code_units(target, value.units(), single_quote)
            .map_err(factory_error)?;
        let metadata = arena.metadata_mut(literal);
        metadata.add_flags(EmitFlags::NO_ASCII_ESCAPING);
        return create_literal_type_node(arena, target, literal).map(Some);
    }
    if flags.intersects(TypeFlags::NUMBER_LITERAL) {
        let TypeData::Literal {
            value: LiteralValue::Number(value),
        } = ty.data
        else {
            unreachable!("NumberLiteral flag implies number literal payload")
        };
        let text = tsc_types::js_number_to_string(value);
        add_approximate_length(context, js_len(&text));
        let magnitude = if value < 0.0 {
            tsc_types::js_number_to_string(-value)
        } else {
            text
        };
        let literal = create_node(
            arena,
            target,
            NodeData::NumericLiteral(NumericLiteralData { text: magnitude }),
        )?;
        let literal = if value < 0.0 {
            create_node(
                arena,
                target,
                NodeData::PrefixUnaryExpression(PrefixUnaryExpressionData {
                    operator: SyntaxKind::MinusToken,
                    operand: Some(literal.node()),
                }),
            )?
        } else {
            literal
        };
        return create_literal_type_node(arena, target, literal).map(Some);
    }
    if flags.intersects(TypeFlags::BIG_INT_LITERAL) {
        let TypeData::Literal {
            value: LiteralValue::BigInt(value),
        } = ty.data
        else {
            unreachable!("BigIntLiteral flag implies bigint literal payload")
        };
        let text = value.to_base10_string();
        add_approximate_length(context, js_len(&text) + 1);
        let literal = create_node(
            arena,
            target,
            NodeData::BigIntLiteral(BigIntLiteralData {
                text: format!("{text}n"),
            }),
        )?;
        return create_literal_type_node(arena, target, literal).map(Some);
    }
    if flags.intersects(TypeFlags::BOOLEAN_LITERAL) {
        let TypeData::Intrinsic { name, .. } = ty.data else {
            unreachable!("BooleanLiteral flag implies intrinsic payload")
        };
        add_approximate_length(context, js_len(name));
        let literal = create_token(
            arena,
            target,
            if name == "true" {
                SyntaxKind::TrueKeyword
            } else {
                SyntaxKind::FalseKeyword
            },
        )?;
        return create_literal_type_node(arena, target, literal).map(Some);
    }
    if flags.intersects(TypeFlags::UNIQUE_ES_SYMBOL) {
        let symbol = ty.symbol.expect("unique symbol type carries a symbol");
        if !has_flag(context, ALLOW_UNIQUE_ES_SYMBOL_TYPE) {
            let accessible = is_value_symbol_accessible(checker, arena, target, symbol, context)?;
            if accessible {
                add_approximate_length(context, 6);
                return chains_symbol_to_type_node(
                    checker,
                    arena,
                    target,
                    context,
                    symbol,
                    EmitSymbolMeaning(SymbolFlags::VALUE.bits() as u32),
                    None,
                )
                .map(Some);
            }
            context
                .tracker
                .report_inaccessible_unique_symbol_error(&mut context.reported_diagnostic);
        }
        add_approximate_length(context, 13);
        let symbol = create_keyword_type_node(arena, target, SyntaxKind::SymbolKeyword)?;
        return create_type_operator_node(arena, target, SyntaxKind::UniqueKeyword, symbol)
            .map(Some);
    }
    if flags.intersects(TypeFlags::VOID) {
        add_approximate_length(context, 4);
        return create_keyword_type_node(arena, target, SyntaxKind::VoidKeyword).map(Some);
    }
    if flags.intersects(TypeFlags::UNDEFINED) {
        add_approximate_length(context, 9);
        return create_keyword_type_node(arena, target, SyntaxKind::UndefinedKeyword).map(Some);
    }
    if flags.intersects(TypeFlags::NULL) {
        add_approximate_length(context, 4);
        let literal = create_token(arena, target, SyntaxKind::NullKeyword)?;
        return create_literal_type_node(arena, target, literal).map(Some);
    }
    if flags.intersects(TypeFlags::NEVER) {
        add_approximate_length(context, 5);
        return create_keyword_type_node(arena, target, SyntaxKind::NeverKeyword).map(Some);
    }
    if flags.intersects(TypeFlags::ES_SYMBOL) {
        add_approximate_length(context, 6);
        return create_keyword_type_node(arena, target, SyntaxKind::SymbolKeyword).map(Some);
    }
    if flags.intersects(TypeFlags::NON_PRIMITIVE) {
        add_approximate_length(context, 6);
        return create_keyword_type_node(arena, target, SyntaxKind::ObjectKeyword).map(Some);
    }

    if matches!(
        ty.data,
        TypeData::TypeParameter {
            is_this_type: true,
            ..
        }
    ) {
        if has_flag(context, IN_OBJECT_TYPE_LITERAL) {
            if !context.encountered_error && !has_flag(context, ALLOW_THIS_IN_OBJECT_LITERAL) {
                context.encountered_error = true;
            }
            context
                .tracker
                .report_inaccessible_this_error(&mut context.reported_diagnostic);
        }
        add_approximate_length(context, 4);
        return create_token(arena, target, SyntaxKind::ThisType).map(Some);
    }

    if !in_type_alias {
        if let Some(alias) = ty.alias_symbol {
            let accessible = has_flag(context, USE_ALIAS_DEFINED_OUTSIDE_CURRENT_SCOPE)
                || is_type_symbol_accessible(checker, arena, target, alias, context)?;
            if accessible {
                if !should_expand_type(checker, r#type, context, true) {
                    let arguments = match ty.alias_type_arguments.as_deref() {
                        Some(arguments) => {
                            map_to_type_nodes(checker, arena, target, arguments, context, false)?
                        }
                        None => None,
                    };
                    if is_reserved_member_name(&checker.symbol_display_name(alias))
                        && !checker.symbol_flags(alias).intersects(SymbolFlags::CLASS)
                    {
                        let empty = create_identifier(arena, target, "")?;
                        return create_type_reference_node(arena, target, empty, arguments)
                            .map(Some);
                    }
                    if arguments
                        .as_ref()
                        .is_some_and(|arguments| arguments.len() == 1)
                    {
                        let global_array = checker
                            .global_array_type()
                            .map_err(|abort| checker_abort_error(checker, context, abort))?;
                        if checker.tables.type_of(global_array).symbol == Some(alias) {
                            let element = arguments
                                .expect("checked as a single type argument")
                                .into_iter()
                                .next()
                                .expect("checked as a single type argument");
                            return create_array_type_node(arena, target, element).map(Some);
                        }
                    }
                    return chains_symbol_to_type_node(
                        checker,
                        arena,
                        target,
                        context,
                        alias,
                        EmitSymbolMeaning::TYPE,
                        arguments,
                    )
                    .map(Some);
                }
                context.depth += 1;
            }
        }
    }

    let object_flags = ty.object_flags;
    if object_flags.intersects(ObjectFlags::REFERENCE) {
        if should_expand_type(checker, r#type, context, false) {
            context.depth += 1;
            return create_anonymous_type_node(checker, arena, target, r#type, context, true, true)
                .map(Some);
        }
        let has_node = checker.links.ty(r#type).deferred_node.is_some();
        return if has_node {
            visit_and_transform_type(
                checker,
                arena,
                target,
                r#type,
                context,
                TypeTransform::Reference,
            )
        } else {
            type_reference_to_type_node(checker, arena, target, r#type, context)
        };
    }

    if flags.intersects(TypeFlags::TYPE_PARAMETER)
        || object_flags.intersects(ObjectFlags::CLASS_OR_INTERFACE)
    {
        if flags.intersects(TypeFlags::TYPE_PARAMETER)
            && context
                .infer_type_parameters
                .as_ref()
                .is_some_and(|parameters| parameters.contains(&r#type))
        {
            let name_len = ty
                .symbol
                .map(|symbol| js_len(&checker.symbol_display_name(symbol)))
                .unwrap_or(1);
            add_approximate_length(context, name_len + 6);
            let constraint = checker
                .get_constraint_of_type_parameter(r#type)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            let constraint_node = match constraint {
                Some(constraint)
                    if match checker
                        .get_inferred_type_parameter_constraint(r#type, true)
                        .map_err(|abort| checker_abort_error(checker, context, abort))?
                    {
                        Some(inferred) => !checker
                            .is_type_identical_to(constraint, inferred)
                            .map_err(|abort| checker_abort_error(checker, context, abort))?,
                        None => true,
                    } =>
                {
                    add_approximate_length(context, 9);
                    type_to_type_node_helper(checker, arena, target, constraint, context)?
                }
                _ => None,
            };
            let parameter = type_parameter_to_declaration_with_constraint(
                checker,
                arena,
                target,
                r#type,
                context,
                constraint_node,
            )?;
            return create_node(
                arena,
                target,
                NodeData::InferType(InferTypeData {
                    type_parameter: Some(parameter.node()),
                }),
            )
            .map(Some);
        }
        if context
            .flags
            .contains(tsc_emitter::EmitNodeBuilderFlags::GENERATE_NAMES_FOR_SHADOWED_TYPE_PARAMS)
            && flags.intersects(TypeFlags::TYPE_PARAMETER)
        {
            let name = type_parameter_to_name(checker, arena, target, r#type, context)?;
            let text = identifier_text(arena, name).unwrap_or("?");
            add_approximate_length(context, js_len(text));
            return create_type_reference_node(arena, target, name, None).map(Some);
        }
        if object_flags.intersects(ObjectFlags::CLASS_OR_INTERFACE)
            && should_expand_type(checker, r#type, context, false)
        {
            context.depth += 1;
            return create_anonymous_type_node(checker, arena, target, r#type, context, true, true)
                .map(Some);
        }
        if let Some(symbol) = ty.symbol {
            return chains_symbol_to_type_node(
                checker,
                arena,
                target,
                context,
                symbol,
                EmitSymbolMeaning::TYPE,
                None,
            )
            .map(Some);
        }
        let marker_prefix = if r#type == checker.marker_sub_type_for_check {
            Some("sub-")
        } else if r#type == checker.marker_super_type_for_check {
            Some("super-")
        } else {
            None
        };
        let name = marker_prefix
            .zip(
                checker
                    .variance_type_parameter
                    .and_then(|parameter| checker.tables.type_of(parameter).symbol),
            )
            .map(|(prefix, symbol)| format!("{prefix}{}", checker.symbol_display_name(symbol)))
            .unwrap_or_else(|| "?".to_owned());
        return create_named_type_reference(arena, target, &name, None).map(Some);
    }

    // Upstream replaces `type` with a union's denormalized origin before
    // dispatching the remaining worker arms (:51542-51545). Origins are not
    // necessarily unions: `keyof O` can retain an Index origin, which must
    // continue into the Index arm below rather than fall through as unknown.
    if flags.intersects(TypeFlags::UNION) {
        let origin = match &ty.data {
            TypeData::Union {
                origin: Some(origin),
                ..
            } => Some(*origin),
            _ => None,
        };
        if let Some(origin) = origin {
            r#type = origin;
            ty = checker.tables.type_of(origin).clone();
            flags = ty.flags;
        }
    }
    if flags.intersects(TypeFlags::UNION_OR_INTERSECTION) {
        let mut types = match ty.data {
            TypeData::Union { types, .. } | TypeData::Intersection { types } => types.to_vec(),
            _ => unreachable!("union/intersection flags imply list payload"),
        };
        if flags.intersects(TypeFlags::UNION) {
            types = format_union_types(checker, &types, expanding_enum)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
        }
        if types.len() == 1 {
            return type_to_type_node_helper(checker, arena, target, types[0], context);
        }
        let type_nodes = map_to_type_nodes(checker, arena, target, &types, context, true)?;
        if let Some(type_nodes) = type_nodes.filter(|nodes| !nodes.is_empty()) {
            return create_union_or_intersection_node(
                arena,
                target,
                type_nodes,
                flags.intersects(TypeFlags::UNION),
            )
            .map(Some);
        }
        if !context.encountered_error && !has_flag(context, ALLOW_EMPTY_UNION_OR_INTERSECTION) {
            context.encountered_error = true;
        }
        return Ok(None);
    }

    if object_flags.intersects(ObjectFlags::ANONYMOUS | ObjectFlags::MAPPED) {
        return create_anonymous_type_node(checker, arena, target, r#type, context, false, false)
            .map(Some);
    }
    if flags.intersects(TypeFlags::INDEX) {
        let TypeData::Index { ty: indexed, .. } = ty.data else {
            unreachable!("Index flag implies Index payload")
        };
        add_approximate_length(context, 6);
        let Some(indexed) = type_to_type_node_helper(checker, arena, target, indexed, context)?
        else {
            return Ok(None);
        };
        return create_type_operator_node(arena, target, SyntaxKind::KeyOfKeyword, indexed)
            .map(Some);
    }
    if flags.intersects(TypeFlags::TEMPLATE_LITERAL) {
        let TypeData::TemplateLiteral { texts, types } = ty.data else {
            unreachable!("TemplateLiteral flag implies template payload")
        };
        let head = arena
            .factory()
            .create_template_literal_like_from_code_units(
                target,
                SyntaxKind::TemplateHead,
                texts[0].units(),
                None,
            )
            .map_err(factory_error)?;
        let mut spans = Vec::with_capacity(types.len());
        for (index, span_type) in types.iter().copied().enumerate() {
            let Some(span_type) =
                type_to_type_node_helper(checker, arena, target, span_type, context)?
            else {
                return Ok(None);
            };
            let kind = if index + 1 == types.len() {
                SyntaxKind::TemplateTail
            } else {
                SyntaxKind::TemplateMiddle
            };
            let literal = arena
                .factory()
                .create_template_literal_like_from_code_units(
                    target,
                    kind,
                    texts[index + 1].units(),
                    None,
                )
                .map_err(factory_error)?;
            spans.push(create_node(
                arena,
                target,
                NodeData::TemplateLiteralTypeSpan(TemplateLiteralTypeSpanData {
                    r#type: Some(span_type.node()),
                    literal: Some(literal.node()),
                }),
            )?);
        }
        let spans = create_node_array(arena, target, spans)?;
        add_approximate_length(context, 2);
        return create_node(
            arena,
            target,
            NodeData::TemplateLiteralType(TemplateLiteralTypeData {
                head: Some(head.node()),
                template_spans: Some(spans),
            }),
        )
        .map(Some);
    }
    if flags.intersects(TypeFlags::STRING_MAPPING) {
        let TypeData::StringMapping { ty: inner } = ty.data else {
            unreachable!("StringMapping flag implies mapping payload")
        };
        let Some(inner) = type_to_type_node_helper(checker, arena, target, inner, context)? else {
            return Ok(None);
        };
        let symbol = ty
            .symbol
            .expect("string mappings carry an intrinsic symbol");
        return chains_symbol_to_type_node(
            checker,
            arena,
            target,
            context,
            symbol,
            EmitSymbolMeaning::TYPE,
            Some(vec![inner]),
        )
        .map(Some);
    }
    if flags.intersects(TypeFlags::INDEXED_ACCESS) {
        let TypeData::IndexedAccess {
            object_type,
            index_type,
            ..
        } = ty.data
        else {
            unreachable!("IndexedAccess flag implies indexed payload")
        };
        let Some(object) = type_to_type_node_helper(checker, arena, target, object_type, context)?
        else {
            return Ok(None);
        };
        let Some(index) = type_to_type_node_helper(checker, arena, target, index_type, context)?
        else {
            return Ok(None);
        };
        add_approximate_length(context, 2);
        return create_node(
            arena,
            target,
            NodeData::IndexedAccessType(IndexedAccessTypeData {
                object_type: Some(object.node()),
                index_type: Some(index.node()),
            }),
        )
        .map(Some);
    }
    if flags.intersects(TypeFlags::CONDITIONAL) {
        return visit_and_transform_type(
            checker,
            arena,
            target,
            r#type,
            context,
            TypeTransform::Conditional,
        );
    }
    if flags.intersects(TypeFlags::SUBSTITUTION) {
        let TypeData::Substitution(data) = ty.data else {
            unreachable!("Substitution flag implies substitution payload")
        };
        let Some(base) = type_to_type_node_helper(checker, arena, target, data.base_type, context)?
        else {
            return Ok(None);
        };
        if checker.tables.is_no_infer_type(r#type) {
            let symbol = checker
                .get_global_type_symbol("NoInfer", false)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            if let Some(symbol) = symbol {
                return chains_symbol_to_type_node(
                    checker,
                    arena,
                    target,
                    context,
                    symbol,
                    EmitSymbolMeaning::TYPE,
                    Some(vec![base]),
                )
                .map(Some);
            }
        }
        return Ok(Some(base));
    }

    context.encountered_error = true;
    Ok(None)
}

fn resolver_node(checker: &CheckerState<'_>, context: &NodeBuilderContext<'_>) -> EmitResolverNode {
    let node = context
        .enclosing_declaration
        .unwrap_or_else(|| checker.binder.source(0).root);
    EmitResolverNode::from_raw_source(
        u32::try_from(checker.binder.file_index_of_node(node)).unwrap_or(0),
        node,
    )
}

fn identifier_text(arena: &TransformArena, node: TransformNode) -> Option<&str> {
    let NodeData::Identifier(data) = &arena.node(node).ok()?.data else {
        return None;
    };
    Some(&data.text)
}

fn create_synthetic_type_parameter(checker: &mut CheckerState<'_>, name: &str) -> TypeId {
    let symbol = checker
        .binder
        .create_symbol(SymbolFlags::TYPE_PARAMETER, name.to_owned());
    let parameter = checker.tables.create_synthesized_type_parameter(None);
    checker.tables.type_mut(parameter).symbol = Some(symbol);
    parameter
}

fn create_infer_type_node(
    arena: &mut TransformArena,
    target: TransformSourceId,
    name: &str,
    constraint: Option<TransformNode>,
) -> BuildResult<TransformNode> {
    let name = create_identifier(arena, target, name)?;
    let parameter = create_node(
        arena,
        target,
        NodeData::TypeParameter(TypeParameterData {
            name: Some(name.node()),
            modifiers: None,
            constraint: constraint.map(TransformNode::node),
            r#default: None,
            expression: None,
        }),
    )?;
    create_node(
        arena,
        target,
        NodeData::InferType(InferTypeData {
            type_parameter: Some(parameter.node()),
        }),
    )
}

/// tsc-port: conditionalTypeToTypeNode @6.0.3
/// tsc-hash: 74c886f42fadc4445e15dde9c8986a422f2cc6f6bcc793838624387706cda790
/// tsc-span: _tsc.js:51611-51649
fn conditional_type_to_type_node(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    r#type: TypeId,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<Option<TransformNode>> {
    let TypeData::Conditional(data) = checker.tables.type_of(r#type).data.clone() else {
        unreachable!("conditional worker receives a conditional type")
    };
    let root = checker.tables.conditional_root(data.root).clone();
    let Some(check_type) =
        type_to_type_node_helper(checker, arena, target, data.check_type, context)?
    else {
        return Ok(None);
    };
    add_approximate_length(context, 15);

    if context
        .flags
        .contains(tsc_emitter::EmitNodeBuilderFlags::GENERATE_NAMES_FOR_SHADOWED_TYPE_PARAMS)
        && root.is_distributive
        && !checker
            .tables
            .flags_of(data.check_type)
            .intersects(TypeFlags::TYPE_PARAMETER)
    {
        let new_parameter = create_synthetic_type_parameter(checker, "T");
        let name = type_parameter_to_name(checker, arena, target, new_parameter, context)?;
        let name = identifier_text(arena, name).unwrap_or("T").to_owned();
        let new_variable = create_named_type_reference(arena, target, &name, None)?;
        add_approximate_length(context, 37);
        let mapper = checker.prepend_type_mapping(root.check_type, new_parameter, data.mapper);
        let old_infer = context
            .infer_type_parameters
            .replace(root.infer_type_parameters.to_vec());
        let extends = (|| {
            let instantiated_extends = checker
                .instantiate_type(root.extends_type, Some(mapper))
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            type_to_type_node_helper(checker, arena, target, instantiated_extends, context)
        })();
        context.infer_type_parameters = old_infer;
        let Some(extends) = extends? else {
            return Ok(None);
        };
        let NodeData::ConditionalType(root_node) = checker.data_of(NodeId(root.node)).clone()
        else {
            context.encountered_error = true;
            return Ok(None);
        };
        let (Some(root_true_node), Some(root_false_node)) =
            (root_node.true_type, root_node.false_type)
        else {
            context.encountered_error = true;
            return Ok(None);
        };
        let root_true_type = checker
            .get_type_from_type_node(root_true_node)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        let root_true_type = checker
            .instantiate_type(root_true_type, Some(mapper))
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        let root_false_type = checker
            .get_type_from_type_node(root_false_node)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        let root_false_type = checker
            .instantiate_type(root_false_type, Some(mapper))
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        let Some(true_type) = type_to_type_node_or_circularity_elision(
            checker,
            arena,
            target,
            root_true_type,
            context,
        )?
        else {
            return Ok(None);
        };
        let Some(false_type) = type_to_type_node_or_circularity_elision(
            checker,
            arena,
            target,
            root_false_type,
            context,
        )?
        else {
            return Ok(None);
        };
        let Some(original_check) =
            type_to_type_node_helper(checker, arena, target, data.check_type, context)?
        else {
            return Ok(None);
        };
        let inner = create_node(
            arena,
            target,
            NodeData::ConditionalType(ConditionalTypeData {
                check_type: Some(new_variable.node()),
                extends_type: Some(extends.node()),
                true_type: Some(true_type.node()),
                false_type: Some(false_type.node()),
            }),
        )?;
        let guard_name = create_named_type_reference(arena, target, &name, None)?;
        let never = create_keyword_type_node(arena, target, SyntaxKind::NeverKeyword)?;
        let guard = create_node(
            arena,
            target,
            NodeData::ConditionalType(ConditionalTypeData {
                check_type: Some(guard_name.node()),
                extends_type: Some(original_check.node()),
                true_type: Some(inner.node()),
                false_type: Some(never.node()),
            }),
        )?;
        let infer = create_infer_type_node(arena, target, &name, None)?;
        let never = create_keyword_type_node(arena, target, SyntaxKind::NeverKeyword)?;
        return create_node(
            arena,
            target,
            NodeData::ConditionalType(ConditionalTypeData {
                check_type: Some(check_type.node()),
                extends_type: Some(infer.node()),
                true_type: Some(guard.node()),
                false_type: Some(never.node()),
            }),
        )
        .map(Some);
    }

    let old_infer = context
        .infer_type_parameters
        .replace(root.infer_type_parameters.to_vec());
    let extends = type_to_type_node_helper(checker, arena, target, data.extends_type, context);
    context.infer_type_parameters = old_infer;
    let Some(extends) = extends? else {
        return Ok(None);
    };
    let true_type = checker
        .get_true_type_from_conditional_type(r#type)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    let false_type = checker
        .get_false_type_from_conditional_type(r#type)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    let Some(true_type) =
        type_to_type_node_or_circularity_elision(checker, arena, target, true_type, context)?
    else {
        return Ok(None);
    };
    let Some(false_type) =
        type_to_type_node_or_circularity_elision(checker, arena, target, false_type, context)?
    else {
        return Ok(None);
    };
    create_node(
        arena,
        target,
        NodeData::ConditionalType(ConditionalTypeData {
            check_type: Some(check_type.node()),
            extends_type: Some(extends.node()),
            true_type: Some(true_type.node()),
            false_type: Some(false_type.node()),
        }),
    )
    .map(Some)
}

/// tsc-port: typeToTypeNodeOrCircularityElision @6.0.3
/// tsc-hash: 464a3d6ae7a211969352a7ca61dd2ace7edeea7e6205af8e5b9e06156df36245
/// tsc-span: _tsc.js:51650-51663
fn type_to_type_node_or_circularity_elision(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    r#type: TypeId,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<Option<TransformNode>> {
    if checker.tables.flags_of(r#type).intersects(TypeFlags::UNION) {
        if context
            .visited_types
            .as_ref()
            .is_some_and(|visited| visited.contains(&r#type))
        {
            if !has_flag(context, ALLOW_ANONYMOUS_IDENTIFIER) {
                context.encountered_error = true;
                context
                    .tracker
                    .report_cyclic_structure_error(&mut context.reported_diagnostic);
            }
            return create_elided_information_placeholder(arena, target, context).map(Some);
        }
        return visit_and_transform_type(
            checker,
            arena,
            target,
            r#type,
            context,
            TypeTransform::Identity,
        );
    }
    type_to_type_node_helper(checker, arena, target, r#type, context)
}

/// tsc-port: isMappedTypeHomomorphic @6.0.3
/// tsc-hash: da183bfd8a801f09ee48ce763a48df9fa39cd1ca81a7d6d63b7fcb8482838015
/// tsc-span: _tsc.js:51664-51666
fn is_mapped_type_homomorphic(
    checker: &mut CheckerState<'_>,
    r#type: TypeId,
) -> Result<bool, CheckAbort> {
    Ok(checker.get_homomorphic_type_variable(r#type)?.is_some())
}

/// tsc-port: isHomomorphicMappedTypeWithNonHomomorphicInstantiation @6.0.3
/// tsc-hash: 56f8993304455056f686e1c66813f21f9af2152767090b879ba1921dcbcd4dfd
/// tsc-span: _tsc.js:51667-51669
fn is_homomorphic_mapped_type_with_non_homomorphic_instantiation(
    checker: &mut CheckerState<'_>,
    r#type: TypeId,
) -> Result<bool, CheckAbort> {
    let target = checker.mapped_type_data(r#type).target;
    Ok(match target {
        Some(target) => {
            is_mapped_type_homomorphic(checker, target)?
                && !is_mapped_type_homomorphic(checker, r#type)?
        }
        None => false,
    })
}

/// tsc-port: createMappedTypeNodeFromType @6.0.3
/// tsc-hash: d1d68d3b7b80854c8eadd15e019c23a5fd036153a28e083a152594a780aeb335
/// tsc-span: _tsc.js:51670-51749
fn create_mapped_type_node_from_type(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    r#type: TypeId,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<TransformNode> {
    let declaration = checker.mapped_type_declaration(r#type);
    let NodeData::MappedType(declaration_data) = checker.data_of(declaration).clone() else {
        unreachable!("mapped type declaration has MappedType data")
    };
    let readonly_token = match declaration_data.readonly_token {
        Some(token) => Some(create_token(arena, target, checker.kind_of(token))?),
        None => None,
    };
    let question_token = match declaration_data.question_token {
        Some(token) => Some(create_token(arena, target, checker.kind_of(token))?),
        None => None,
    };
    let mut template_type = checker
        .get_template_type_from_mapped_type(r#type)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    let parameter = checker
        .get_type_parameter_from_mapped_type(r#type)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    let constraint = checker
        .get_constraint_type_from_mapped_type(r#type)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    let modifiers_type = checker
        .get_modifiers_type_from_mapped_type(r#type)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    let homomorphic_wrapper =
        is_homomorphic_mapped_type_with_non_homomorphic_instantiation(checker, r#type)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
    let generate_names = context
        .flags
        .contains(tsc_emitter::EmitNodeBuilderFlags::GENERATE_NAMES_FOR_SHADOWED_TYPE_PARAMS);
    let constraint_parameter_constraint = if checker
        .tables
        .flags_of(constraint)
        .intersects(TypeFlags::TYPE_PARAMETER)
    {
        checker
            .get_constraint_of_type_parameter(constraint)
            .map_err(|abort| checker_abort_error(checker, context, abort))?
    } else {
        None
    };
    let needs_modifier_preserving_wrapper = !checker
        .is_mapped_type_with_keyof_constraint_declaration(r#type)
        && !checker
            .tables
            .flags_of(modifiers_type)
            .intersects(TypeFlags::UNKNOWN)
        && generate_names
        && !constraint_parameter_constraint.is_some_and(|constraint| {
            checker
                .tables
                .flags_of(constraint)
                .intersects(TypeFlags::INDEX)
        });

    let mut generated_name = None;
    let is_keyof_declaration = checker.is_mapped_type_with_keyof_constraint_declaration(r#type);
    let constraint_node = if is_keyof_declaration {
        if homomorphic_wrapper && generate_names {
            let parameter_for_modifiers = create_synthetic_type_parameter(checker, "T");
            let name =
                type_parameter_to_name(checker, arena, target, parameter_for_modifiers, context)?;
            let name = identifier_text(arena, name).unwrap_or("T").to_owned();
            generated_name = Some(name.clone());
            let mapped = checker.mapped_type_data(r#type);
            if let Some(mapped_target) = mapped.target {
                let target_template = checker
                    .get_template_type_from_mapped_type(mapped_target)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?;
                let target_parameter =
                    checker
                        .get_type_parameter_from_mapped_type(mapped_target)
                        .map_err(|abort| checker_abort_error(checker, context, abort))?;
                let target_modifiers =
                    checker
                        .get_modifiers_type_from_mapped_type(mapped_target)
                        .map_err(|abort| checker_abort_error(checker, context, abort))?;
                let mapper = checker.make_array_type_mapper(
                    vec![target_parameter, target_modifiers],
                    Some(vec![parameter, parameter_for_modifiers]),
                );
                template_type = checker
                    .instantiate_type(target_template, Some(mapper))
                    .map_err(|abort| checker_abort_error(checker, context, abort))?;
            }
        }
        let operand = match generated_name.as_deref() {
            Some(name) => create_named_type_reference(arena, target, name, None)?,
            None => {
                let Some(node) =
                    type_to_type_node_helper(checker, arena, target, modifiers_type, context)?
                else {
                    context.encountered_error = true;
                    return create_keyword_type_node(arena, target, SyntaxKind::AnyKeyword);
                };
                node
            }
        };
        Some(create_type_operator_node(
            arena,
            target,
            SyntaxKind::KeyOfKeyword,
            operand,
        )?)
    } else if needs_modifier_preserving_wrapper {
        let parameter_for_modifiers = create_synthetic_type_parameter(checker, "T");
        let name =
            type_parameter_to_name(checker, arena, target, parameter_for_modifiers, context)?;
        let name = identifier_text(arena, name).unwrap_or("T").to_owned();
        generated_name = Some(name.clone());
        Some(create_named_type_reference(arena, target, &name, None)?)
    } else {
        type_to_type_node_helper(checker, arena, target, constraint, context)?
    };
    let parameter_node = type_parameter_to_declaration_with_constraint(
        checker,
        arena,
        target,
        parameter,
        context,
        constraint_node,
    )?;

    let scope_parameter = match declaration_data.type_parameter {
        Some(parameter_declaration) => checker
            .get_symbol_of_declaration(parameter_declaration)
            .map(|symbol| checker.get_declared_type_of_type_parameter(symbol))
            .map_err(|abort| checker_abort_error(checker, context, abort))?,
        None => parameter,
    };
    let scope = super::signatures::enter_new_scope(
        context,
        Some(declaration),
        None,
        Some(&[scope_parameter]),
        None,
        None,
    );
    prime_type_parameter_names_for_scope(checker, arena, target, context, &[scope_parameter])?;
    let scoped_nodes = (|| -> BuildResult<_> {
        let name_type = if declaration_data.name_type.is_some() {
            match checker
                .get_name_type_from_mapped_type(r#type)
                .map_err(|abort| checker_abort_error(checker, context, abort))?
            {
                Some(name) => type_to_type_node_helper(checker, arena, target, name, context)?,
                None => None,
            }
        } else {
            None
        };
        let include_optional = checker
            .get_mapped_type_modifiers(r#type)
            .intersects(tsc_types::MappedTypeModifiers::INCLUDE_OPTIONAL);
        let template_type = checker.remove_missing_type(template_type, include_optional);
        let template = type_to_type_node_helper(checker, arena, target, template_type, context)?;
        Ok((name_type, template))
    })();
    super::signatures::exit_new_scope(context, scope);
    let (name_type, template) = scoped_nodes?;

    let node = create_node(
        arena,
        target,
        NodeData::MappedType(MappedTypeData {
            readonly_token: readonly_token.map(TransformNode::node),
            type_parameter: Some(parameter_node.node()),
            name_type: name_type.map(TransformNode::node),
            question_token: question_token.map(TransformNode::node),
            r#type: template.map(TransformNode::node),
            members: None,
        }),
    )?;
    add_approximate_length(context, 10);
    let result = set_single_line(arena, node);

    if homomorphic_wrapper && generate_names {
        let Some(name) = generated_name.as_deref() else {
            return Ok(result);
        };
        let mapped = checker.mapped_type_data(r#type);
        let mut original_constraint = checker.tables.intrinsics.unknown;
        if let Some(operand) = declaration_data
            .type_parameter
            .and_then(|parameter| match checker.data_of(parameter) {
                NodeData::TypeParameter(data) => data.constraint,
                _ => None,
            })
            .and_then(|constraint| match checker.data_of(constraint) {
                NodeData::TypeOperator(data) => data.r#type,
                _ => None,
            })
        {
            let declared_parameter = checker
                .get_type_from_type_node(operand)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            original_constraint = checker
                .get_constraint_of_type_parameter(declared_parameter)
                .map_err(|abort| checker_abort_error(checker, context, abort))?
                .unwrap_or(checker.tables.intrinsics.unknown);
        }
        original_constraint = checker
            .instantiate_type(original_constraint, mapped.mapper)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        let infer_constraint = if checker
            .tables
            .flags_of(original_constraint)
            .intersects(TypeFlags::UNKNOWN)
        {
            None
        } else {
            type_to_type_node_helper(checker, arena, target, original_constraint, context)?
        };
        let infer = create_infer_type_node(arena, target, name, infer_constraint)?;
        let Some(check) =
            type_to_type_node_helper(checker, arena, target, modifiers_type, context)?
        else {
            return Ok(result);
        };
        let never = create_keyword_type_node(arena, target, SyntaxKind::NeverKeyword)?;
        return create_node(
            arena,
            target,
            NodeData::ConditionalType(ConditionalTypeData {
                check_type: Some(check.node()),
                extends_type: Some(infer.node()),
                true_type: Some(result.node()),
                false_type: Some(never.node()),
            }),
        );
    } else if needs_modifier_preserving_wrapper {
        let Some(name) = generated_name.as_deref() else {
            return Ok(result);
        };
        let Some(modifiers) =
            type_to_type_node_helper(checker, arena, target, modifiers_type, context)?
        else {
            return Ok(result);
        };
        let keyof_modifiers =
            create_type_operator_node(arena, target, SyntaxKind::KeyOfKeyword, modifiers)?;
        let infer = create_infer_type_node(arena, target, name, Some(keyof_modifiers))?;
        let Some(check) = type_to_type_node_helper(checker, arena, target, constraint, context)?
        else {
            return Ok(result);
        };
        let never = create_keyword_type_node(arena, target, SyntaxKind::NeverKeyword)?;
        return create_node(
            arena,
            target,
            NodeData::ConditionalType(ConditionalTypeData {
                check_type: Some(check.node()),
                extends_type: Some(infer.node()),
                true_type: Some(result.node()),
                false_type: Some(never.node()),
            }),
        );
    }
    Ok(result)
}

/// tsc-port: createAnonymousTypeNode @6.0.3
/// tsc-hash: 0c4bd387aaaa40e88a957f74d475f7dc797d65b59c06df516af17e933713bf98
/// tsc-span: _tsc.js:51750-51810
fn create_anonymous_type_node(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    r#type: TypeId,
    context: &mut NodeBuilderContext<'_>,
    force_class_expansion: bool,
    force_expansion: bool,
) -> BuildResult<TransformNode> {
    let ty = checker.tables.type_of(r#type).clone();
    if ty
        .object_flags
        .intersects(ObjectFlags::INSTANTIATION_EXPRESSION_TYPE)
    {
        if let Some(existing) = checker.links.ty(r#type).deferred_node {
            if checker.kind_of(existing) == SyntaxKind::TypeQuery
                && checker
                    .get_type_from_type_node(existing)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?
                    == r#type
            {
                if let Some(reused) = super::syntactic_try_reuse_existing_type_node(
                    checker, arena, target, context, existing,
                )? {
                    return Ok(reused);
                }
            }
        }
        if context
            .visited_types
            .as_ref()
            .is_some_and(|visited| visited.contains(&r#type))
        {
            return create_elided_information_placeholder(arena, target, context);
        }
        return visit_and_transform_type(
            checker,
            arena,
            target,
            r#type,
            context,
            TypeTransform::Object,
        )
        .map(|node| node.expect("object transform always creates a node"));
    }

    if let Some(symbol) = ty.symbol {
        let symbol_flags = checker.symbol_flags(symbol);
        let value_declaration = checker.binder.symbol(symbol).value_declaration;
        let is_class_instance = if symbol_flags.intersects(SymbolFlags::CLASS) {
            checker
                .get_declared_type_of_class_or_interface(symbol)
                .map_err(|abort| checker_abort_error(checker, context, abort))?
                == r#type
                || ty
                    .object_flags
                    .intersects(ObjectFlags::IS_CLASS_INSTANCE_CLONE)
        } else {
            false
        };
        let symbol_meaning = if is_class_instance {
            EmitSymbolMeaning::TYPE
        } else {
            // createAnonymousTypeNode passes `SymbolFlags.Value` for the
            // anonymous/static face.  `ExportValue` is deliberately not part
            // of this meaning: symbolToTypeNode's tracker observes the exact
            // mask, and the typeof/anonymous-class path depends on that face.
            EmitSymbolMeaning(SymbolFlags::VALUE.bits() as u32)
        };
        if value_declaration.is_some_and(|declaration| checker.is_js_constructor(declaration)) {
            return chains_symbol_to_type_node(
                checker,
                arena,
                target,
                context,
                symbol,
                symbol_meaning,
                None,
            );
        }
        let should_write_function =
            should_write_type_of_function_symbol(checker, arena, target, r#type, symbol, context)?;
        let has_base_type_variable = if symbol_flags.intersects(SymbolFlags::CLASS) {
            let class_type = checker
                .get_declared_type_of_class_or_interface(symbol)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            let base_constructor = checker
                .get_base_constructor_type_of_class(class_type)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            let base_flags = checker.tables.flags_of(base_constructor);
            base_flags.intersects(TypeFlags::TYPE_VARIABLE)
                || if let TypeData::Intersection { types } =
                    &checker.tables.type_of(base_constructor).data
                {
                    types.iter().any(|&ty| {
                        checker
                            .tables
                            .flags_of(ty)
                            .intersects(TypeFlags::TYPE_VARIABLE)
                    })
                } else {
                    false
                }
        } else {
            false
        };
        let write_class_expression_as_literal = match value_declaration {
            Some(declaration)
                if matches!(
                    checker.kind_of(declaration),
                    SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
                ) && has_flag(context, WRITE_CLASS_EXPRESSION_AS_TYPE_LITERAL) =>
            {
                if checker.kind_of(declaration) != SyntaxKind::ClassDeclaration {
                    true
                } else {
                    !is_symbol_accessible_with_error_names(
                        checker,
                        arena,
                        target,
                        symbol,
                        symbol_meaning,
                        context,
                    )?
                }
            }
            _ => false,
        };
        let named_class = symbol_flags.intersects(SymbolFlags::CLASS)
            && !force_class_expansion
            && !has_base_type_variable
            && !write_class_expression_as_literal;
        let named_value = symbol_flags.intersects(
            SymbolFlags::REGULAR_ENUM | SymbolFlags::CONST_ENUM | SymbolFlags::VALUE_MODULE,
        ) || should_write_function;
        if !force_expansion && (named_class || named_value) {
            if should_expand_type(checker, r#type, context, false) {
                context.depth += 1;
            } else {
                return chains_symbol_to_type_node(
                    checker,
                    arena,
                    target,
                    context,
                    symbol,
                    symbol_meaning,
                    None,
                );
            }
        }
        if context
            .visited_types
            .as_ref()
            .is_some_and(|visited| visited.contains(&r#type))
        {
            if symbol_flags.intersects(SymbolFlags::TYPE_LITERAL) {
                if let Some(mut node) = checker
                    .binder
                    .symbol(symbol)
                    .declarations
                    .first()
                    .copied()
                    .and_then(|declaration| checker.parent_of(declaration))
                {
                    while checker.kind_of(node) == SyntaxKind::ParenthesizedType {
                        let Some(parent) = checker.parent_of(node) else {
                            break;
                        };
                        node = parent;
                    }
                    if checker.kind_of(node) == SyntaxKind::TypeAliasDeclaration {
                        let alias = checker
                            .get_symbol_of_declaration(node)
                            .map_err(|abort| checker_abort_error(checker, context, abort))?;
                        return chains_symbol_to_type_node(
                            checker,
                            arena,
                            target,
                            context,
                            alias,
                            EmitSymbolMeaning::TYPE,
                            None,
                        );
                    }
                }
            }
            return create_elided_information_placeholder(arena, target, context);
        }
        return visit_and_transform_type(
            checker,
            arena,
            target,
            r#type,
            context,
            TypeTransform::Object,
        )
        .map(|node| node.expect("object transform always creates a node"));
    }
    create_type_node_from_object_type(checker, arena, target, r#type, context)
}

/// tsc-port: shouldWriteTypeOfFunctionSymbol @6.0.3
/// tsc-hash: c613afc58096a6ced8cbbaf0463b9eb7009d996d87842cfac380b4d1753d085a
/// tsc-span: _tsc.js:51799-51809
fn should_write_type_of_function_symbol(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    r#type: TypeId,
    symbol: SymbolId,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<bool> {
    let (flags, parent, declarations) = {
        let data = checker.binder.symbol(symbol);
        (data.flags, data.parent, data.declarations.clone())
    };
    let is_static_method = flags.intersects(SymbolFlags::METHOD)
        && declarations.iter().copied().any(|declaration| {
            checker.is_static_element(declaration)
                && !checker
                    .has_late_bindable_index_signature(declaration)
                    .unwrap_or(true)
        });
    let is_non_local_function = flags.intersects(SymbolFlags::FUNCTION)
        && (parent.is_some()
            || declarations.iter().copied().any(|declaration| {
                checker.parent_of(declaration).is_some_and(|parent| {
                    matches!(
                        checker.kind_of(parent),
                        SyntaxKind::SourceFile | SyntaxKind::ModuleBlock
                    )
                })
            }));
    if !(is_static_method || is_non_local_function) {
        return Ok(false);
    }
    let requested = has_flag(context, USE_TYPE_OF_FUNCTION)
        || context
            .visited_types
            .as_ref()
            .is_some_and(|visited| visited.contains(&r#type));
    if !requested {
        return Ok(false);
    }
    Ok(!has_flag(context, USE_STRUCTURAL_FALLBACK)
        || is_value_symbol_accessible(checker, arena, target, symbol, context)?)
}

#[derive(Clone, Copy)]
enum TypeTransform {
    Identity,
    Conditional,
    Reference,
    Object,
}

/// tsc-port: visitAndTransformType @6.0.3
/// tsc-hash: cc6d118a4cf40da3a03c25b3ace1bc14075081d20dca260f01d6b744760523ee
/// tsc-span: _tsc.js:51811-51893
fn visit_and_transform_type(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    r#type: TypeId,
    context: &mut NodeBuilderContext<'_>,
    transform: TypeTransform,
) -> BuildResult<Option<TransformNode>> {
    context.visited_types.get_or_insert_with(Default::default);
    let symbol = checker.tables.type_of(r#type).symbol;
    let depth = symbol.map(|symbol| {
        let old = context
            .symbol_depth
            .as_ref()
            .and_then(|depths| depths.get(&symbol).copied())
            .unwrap_or(0);
        (symbol, old)
    });
    if depth.is_some_and(|(_, old)| old > 10) {
        return create_elided_information_placeholder(arena, target, context).map(Some);
    }
    if let Some((symbol, old)) = depth {
        context
            .symbol_depth
            .get_or_insert_with(HashMap::new)
            .insert(symbol, old + 1);
    }
    context
        .visited_types
        .as_mut()
        .expect("initialized above")
        .insert(r#type);
    let old_tracked = context.tracked_symbols.take();
    let result = match transform {
        TypeTransform::Identity => {
            type_to_type_node_helper(checker, arena, target, r#type, context)
        }
        TypeTransform::Conditional => {
            conditional_type_to_type_node(checker, arena, target, r#type, context)
        }
        TypeTransform::Reference => {
            type_reference_to_type_node(checker, arena, target, r#type, context)
        }
        TypeTransform::Object => {
            create_type_node_from_object_type(checker, arena, target, r#type, context).map(Some)
        }
    };
    context
        .visited_types
        .as_mut()
        .expect("initialized above")
        .remove(&r#type);
    if let Some((symbol, old)) = depth {
        context
            .symbol_depth
            .as_mut()
            .expect("initialized with symbol")
            .insert(symbol, old);
    }
    context.tracked_symbols = old_tracked;
    result
}

/// tsc-port: deepCloneOrReuseNode @6.0.3
/// tsc-hash: b6ecd18580cc61281a9d25bc83c371362dec6a1184ddeab9382e2046710fbb89
/// tsc-span: _tsc.js:51870-51882
#[allow(dead_code)]
fn deep_clone_or_reuse_node(
    arena: &mut TransformArena,
    node: TransformNode,
) -> BuildResult<TransformNode> {
    if arena.is_parsed_node(node).map_err(factory_error)? {
        return Ok(node);
    }
    let clone = arena.factory().clone_node(node).map_err(factory_error)?;
    arena
        .factory()
        .set_text_range(clone, node)
        .map_err(factory_error)
}

/// tsc-port: deepCloneOrReuseNodes @6.0.3
/// tsc-hash: 4bf00b5c2df59930e6c881a869b984027bbb1eb1ae7ac46fd8354ea08b0852f3
/// tsc-span: _tsc.js:51883-51892
#[allow(dead_code)]
fn deep_clone_or_reuse_nodes(
    arena: &mut TransformArena,
    nodes: &[TransformNode],
) -> BuildResult<Vec<TransformNode>> {
    nodes
        .iter()
        .copied()
        .map(|node| deep_clone_or_reuse_node(arena, node))
        .collect()
}

/// tsc-port: createTypeNodeFromObjectType @6.0.3
/// tsc-hash: 90bfabc8231e6c1bdfdcd7d13da8b3d098a7b07a0c74aa778b109d30bdd4347b
/// tsc-span: _tsc.js:51894-51937
fn create_type_node_from_object_type(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    r#type: TypeId,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<TransformNode> {
    if checker
        .tables
        .object_flags_of(r#type)
        .intersects(ObjectFlags::MAPPED)
        && (checker
            .is_generic_mapped_type_state(r#type)
            .map_err(|abort| checker_abort_error(checker, context, abort))?
            || checker.links.ty(r#type).mapped_contains_error)
    {
        return create_mapped_type_node_from_type(checker, arena, target, r#type, context);
    }
    let members = checker
        .resolve_structured_type_members(r#type)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    let resolved = checker.members_of(members).clone();
    if resolved.properties.is_empty() && resolved.index_infos.is_empty() {
        if resolved.call_signatures.is_empty() && resolved.construct_signatures.is_empty() {
            add_approximate_length(context, 2);
            let node = create_node(
                arena,
                target,
                NodeData::TypeLiteral(TypeLiteralData { members: None }),
            )?;
            return Ok(set_single_line(arena, node));
        }
        if resolved.call_signatures.len() == 1 && resolved.construct_signatures.is_empty() {
            return signature_to_signature_declaration_helper(
                checker,
                arena,
                target,
                resolved.call_signatures[0],
                SyntaxKind::FunctionType,
                context,
                None,
            );
        }
        if resolved.construct_signatures.len() == 1 && resolved.call_signatures.is_empty() {
            return signature_to_signature_declaration_helper(
                checker,
                arena,
                target,
                resolved.construct_signatures[0],
                SyntaxKind::ConstructorType,
                context,
                None,
            );
        }
    }
    let has_abstract = resolved.construct_signatures.iter().any(|signature| {
        checker
            .signature_of(*signature)
            .flags
            .intersects(SignatureFlags::ABSTRACT)
    });
    if has_abstract {
        let abstract_signatures: Vec<_> = resolved
            .construct_signatures
            .iter()
            .copied()
            .filter(|signature| {
                checker
                    .signature_of(*signature)
                    .flags
                    .intersects(SignatureFlags::ABSTRACT)
            })
            .collect();
        let mut types = Vec::with_capacity(abstract_signatures.len() + 1);
        for signature in abstract_signatures.iter().copied() {
            types.push(
                checker
                    .get_or_create_type_from_signature(signature)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?,
            );
        }
        let non_abstract_construct_signatures = resolved
            .construct_signatures
            .iter()
            .copied()
            .filter(|signature| !abstract_signatures.contains(signature))
            .collect::<Vec<_>>();
        let property_count = if has_flag(context, WRITE_CLASS_EXPRESSION_AS_TYPE_LITERAL) {
            resolved
                .properties
                .iter()
                .filter(|&&property| {
                    !checker
                        .symbol_flags(property)
                        .intersects(SymbolFlags::PROTOTYPE)
                })
                .count()
        } else {
            resolved.properties.len()
        };
        let type_element_count = resolved.call_signatures.len()
            + non_abstract_construct_signatures.len()
            + resolved.index_infos.len()
            + property_count;
        if type_element_count != 0 {
            let remainder = checker
                .tables
                .create_type(TypeFlags::OBJECT, TypeData::Object);
            checker.tables.type_mut(remainder).object_flags = ObjectFlags::ANONYMOUS;
            checker.tables.type_mut(remainder).symbol = checker.tables.type_of(r#type).symbol;
            let members = checker.alloc_members(ResolvedMembers {
                members: resolved.members,
                properties: resolved.properties,
                call_signatures: resolved.call_signatures,
                construct_signatures: non_abstract_construct_signatures,
                index_infos: resolved.index_infos,
            });
            checker
                .links
                .set_fresh_type_members(remainder, LinkSlot::Resolved(members));
            types.push(remainder);
        }
        let intersection = checker
            .get_intersection_type(&types, IntersectionFlags::NONE)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        return type_to_type_node_helper(checker, arena, target, intersection, context)?
            .ok_or_else(|| EmitResolverError::CheckerAborted {
                method: METHOD,
                node: resolver_node(checker, context),
                reason: "abstract constructor intersection did not produce a type node",
            });
    }
    let restore = save_restore_flags(context);
    set_flag(context, IN_OBJECT_TYPE_LITERAL);
    let object_flags = checker.tables.object_flags_of(r#type);
    let members = create_type_nodes_from_resolved_type(
        checker,
        arena,
        target,
        r#type,
        &resolved,
        object_flags,
        context,
    );
    restore_flags(context, restore);
    let members = members?;
    let members = match members {
        Some(members) => Some(create_node_array(arena, target, members)?),
        None => None,
    };
    let node = create_node(
        arena,
        target,
        NodeData::TypeLiteral(TypeLiteralData { members }),
    )?;
    add_approximate_length(context, 2);
    if !context
        .flags
        .contains(tsc_emitter::EmitNodeBuilderFlags::MULTILINE_OBJECT_LITERALS)
    {
        arena.metadata_mut(node).add_flags(EmitFlags::SINGLE_LINE);
    }
    Ok(node)
}

/// tsc-port: getParentSymbolOfTypeParameter @6.0.3
/// tsc-hash: c6c6439ef9269ecc33487047b46a90e24f5781fb5e1ee2548429866d84d7e57e
/// tsc-span: _tsc.js:60123-60127
fn parent_symbol_of_type_parameter(
    checker: &CheckerState<'_>,
    parameter: TypeId,
) -> Option<SymbolId> {
    let symbol = checker.tables.type_of(parameter).symbol?;
    let declaration = checker
        .binder
        .symbol(symbol)
        .declarations
        .iter()
        .copied()
        .find(|&declaration| checker.kind_of(declaration) == SyntaxKind::TypeParameter)?;
    let parent = checker.parent_of(declaration)?;
    let host = if checker.kind_of(parent) == SyntaxKind::JSDocTemplateTag {
        checker.effective_container_for_jsdoc_template_tag(parent)?
    } else {
        parent
    };
    checker.get_symbol_of_declaration_opt(host)
}

/// tsc-port: typeReferenceToTypeNode @6.0.3
/// tsc-hash: 6f7e7c3e97acfbc74e98257d4c884b6393eaa62eebe33e0826b5e6296bfcb325
/// tsc-span: _tsc.js:51938-52042
fn type_reference_to_type_node(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    r#type: TypeId,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<Option<TransformNode>> {
    let target_type = checker.tables.reference_target(r#type);
    let mut arguments = checker
        .get_type_arguments(r#type)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    let is_array = checker
        .is_array_type(r#type)
        .map_err(|abort| checker_abort_error(checker, context, abort))?;
    if is_array {
        let readonly = checker
            .is_readonly_array_type(r#type)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        let element = arguments
            .first()
            .copied()
            .unwrap_or(checker.tables.intrinsics.any);
        let Some(element) = type_to_type_node_helper(checker, arena, target, element, context)?
        else {
            context.encountered_error = true;
            return Ok(None);
        };
        if has_flag(context, WRITE_ARRAY_AS_GENERIC_TYPE) {
            return create_named_type_reference(
                arena,
                target,
                if readonly { "ReadonlyArray" } else { "Array" },
                Some(vec![element]),
            )
            .map(Some);
        }
        let array = create_array_type_node(arena, target, element)?;
        let result = if readonly {
            create_type_operator_node(arena, target, SyntaxKind::ReadonlyKeyword, array)
        } else {
            Ok(array)
        }?;
        return Ok(Some(result));
    }

    if checker.tables.is_tuple_type(r#type) {
        let TypeData::TupleTarget(tuple) = checker.tables.type_of(target_type).data.clone() else {
            unreachable!("tuple reference targets have tuple payload")
        };
        for (index, argument) in arguments.clone().iter().copied().enumerate() {
            let optional = tuple
                .element_flags
                .get(index)
                .is_some_and(|flags| flags.intersects(ElementFlags::OPTIONAL));
            arguments[index] = checker.remove_missing_type(argument, optional);
        }
        let arity = checker
            .get_type_reference_arity(r#type)
            .min(arguments.len());
        let nodes = map_to_type_nodes(checker, arena, target, &arguments[..arity], context, false)?;
        if let Some(mut nodes) = nodes.filter(|nodes| !nodes.is_empty()) {
            for (index, node) in nodes.iter_mut().enumerate() {
                let flags = tuple.element_flags[index];
                let label = tuple
                    .labeled_element_declarations
                    .as_ref()
                    .and_then(|labels| labels.get(index).copied())
                    .flatten();
                *node = if let Some(label) = label {
                    let name = checker
                        .tuple_element_label(NodeId(label))
                        .map_err(|abort| checker_abort_error(checker, context, abort))?;
                    let name = create_identifier(arena, target, &name)?;
                    let dot = if flags.intersects(ElementFlags::VARIABLE) {
                        Some(create_token(arena, target, SyntaxKind::DotDotDotToken)?)
                    } else {
                        None
                    };
                    let question = if flags.intersects(ElementFlags::OPTIONAL) {
                        Some(create_token(arena, target, SyntaxKind::QuestionToken)?)
                    } else {
                        None
                    };
                    let r#type = if flags.intersects(ElementFlags::REST) {
                        create_array_type_node(arena, target, *node)?
                    } else {
                        *node
                    };
                    create_node(
                        arena,
                        target,
                        NodeData::NamedTupleMember(NamedTupleMemberData {
                            dot_dot_dot_token: dot.map(TransformNode::node),
                            name: Some(name.node()),
                            question_token: question.map(TransformNode::node),
                            r#type: Some(r#type.node()),
                        }),
                    )?
                } else if flags.intersects(ElementFlags::VARIABLE) {
                    let r#type = if flags.intersects(ElementFlags::REST) {
                        create_array_type_node(arena, target, *node)?
                    } else {
                        *node
                    };
                    create_node(
                        arena,
                        target,
                        NodeData::RestType(RestTypeData {
                            r#type: Some(r#type.node()),
                        }),
                    )?
                } else if flags.intersects(ElementFlags::OPTIONAL) {
                    create_node(
                        arena,
                        target,
                        NodeData::OptionalType(OptionalTypeData {
                            r#type: Some(node.node()),
                        }),
                    )?
                } else {
                    *node
                };
            }
            let elements = create_node_array(arena, target, nodes)?;
            let tuple_node = create_node(
                arena,
                target,
                NodeData::TupleType(TupleTypeData {
                    elements: Some(elements),
                }),
            )?;
            let tuple_node = set_single_line(arena, tuple_node);
            let result = if tuple.readonly {
                create_type_operator_node(arena, target, SyntaxKind::ReadonlyKeyword, tuple_node)
            } else {
                Ok(tuple_node)
            }?;
            return Ok(Some(result));
        }
        if context.encountered_error || has_flag(context, ALLOW_EMPTY_TUPLE) {
            let elements = create_node_array(arena, target, Vec::new())?;
            let tuple_node = create_node(
                arena,
                target,
                NodeData::TupleType(TupleTypeData {
                    elements: Some(elements),
                }),
            )?;
            let tuple_node = set_single_line(arena, tuple_node);
            let result = if tuple.readonly {
                create_type_operator_node(arena, target, SyntaxKind::ReadonlyKeyword, tuple_node)
            } else {
                Ok(tuple_node)
            }?;
            return Ok(Some(result));
        }
        context.encountered_error = true;
        return Ok(None);
    }

    let reference_symbol = checker
        .tables
        .type_of(r#type)
        .symbol
        .or(checker.tables.type_of(target_type).symbol)
        .expect("non-tuple references carry a symbol");
    if has_flag(context, WRITE_CLASS_EXPRESSION_AS_TYPE_LITERAL) {
        if let Some(value_declaration) = checker.binder.symbol(reference_symbol).value_declaration {
            if matches!(
                checker.kind_of(value_declaration),
                SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
            ) && !is_value_symbol_accessible(checker, arena, target, reference_symbol, context)?
            {
                return create_anonymous_type_node(
                    checker, arena, target, r#type, context, false, false,
                )
                .map(Some);
            }
        }
    }

    let target_data = checker.tables.type_of(target_type).clone();
    let (type_parameters, outer_count) = match target_data.data {
        TypeData::GenericType {
            type_parameters,
            outer_type_parameter_count,
            ..
        } => (type_parameters.to_vec(), outer_type_parameter_count),
        _ => (Vec::new(), 0),
    };
    let mut argument_start = 0;
    let mut result_type = None;
    while argument_start < outer_count {
        let group_start = argument_start;
        let Some(parent) = type_parameters
            .get(argument_start)
            .and_then(|&parameter| parent_symbol_of_type_parameter(checker, parameter))
        else {
            return Err(EmitResolverError::CheckerAborted {
                method: METHOD,
                node: resolver_node(checker, context),
                reason: "outer type parameter has no parent symbol",
            });
        };
        argument_start += 1;
        while argument_start < outer_count
            && type_parameters
                .get(argument_start)
                .and_then(|&parameter| parent_symbol_of_type_parameter(checker, parameter))
                == Some(parent)
        {
            argument_start += 1;
        }
        let unchanged = arguments.get(group_start..argument_start)
            == type_parameters.get(group_start..argument_start);
        if !unchanged {
            let argument_group = arguments
                .get(group_start..argument_start)
                .unwrap_or_default();
            let argument_nodes =
                map_to_type_nodes(checker, arena, target, argument_group, context, false)?;
            let restore = save_restore_flags(context);
            set_flag(context, FORBID_INDEXED_ACCESS_SYMBOL_REFERENCES);
            let reference = chains_symbol_to_type_node(
                checker,
                arena,
                target,
                context,
                parent,
                EmitSymbolMeaning::TYPE,
                argument_nodes,
            );
            restore_flags(context, restore);
            let reference = reference?;
            result_type = Some(match result_type {
                Some(root) => append_reference_to_type(arena, target, root, reference)?,
                None => reference,
            });
        }
    }

    let mut type_parameter_count = type_parameters.len().min(arguments.len());
    if type_parameter_count > 0 {
        let iterable = checker
            .get_global_iterable_type(false)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        let iterable_iterator = checker
            .get_global_iterable_iterator_type(false)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        let async_iterable = checker
            .get_global_async_iterable_type(false)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        let async_iterable_iterator = checker
            .get_global_async_iterable_iterator_type(false)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        let is_iterable_protocol = [
            iterable,
            iterable_iterator,
            async_iterable,
            async_iterable_iterator,
        ]
        .into_iter()
        .any(|protocol| checker.is_reference_to_type(r#type, protocol));
        if is_iterable_protocol {
            let written_argument_count = checker.links.ty(r#type).deferred_node.and_then(|node| {
                match checker.data_of(node) {
                    NodeData::TypeReference(data) => {
                        Some(checker.nodes_of(data.type_arguments).len())
                    }
                    _ => None,
                }
            });
            if written_argument_count.is_none_or(|count| count < type_parameter_count) {
                while type_parameter_count > 0 {
                    let argument = arguments[type_parameter_count - 1];
                    let parameter = type_parameters[type_parameter_count - 1];
                    let default = checker
                        .get_default_from_type_parameter(parameter)
                        .map_err(|abort| checker_abort_error(checker, context, abort))?;
                    let Some(default) = default else {
                        break;
                    };
                    if !checker
                        .is_type_identical_to(argument, default)
                        .map_err(|abort| checker_abort_error(checker, context, abort))?
                    {
                        break;
                    }
                    type_parameter_count -= 1;
                }
            }
        }
    }
    let argument_start = argument_start.min(type_parameter_count);
    let argument_nodes = if arguments.is_empty() {
        None
    } else {
        map_to_type_nodes(
            checker,
            arena,
            target,
            &arguments[argument_start..type_parameter_count],
            context,
            false,
        )?
    };
    let restore = save_restore_flags(context);
    set_flag(context, FORBID_INDEXED_ACCESS_SYMBOL_REFERENCES);
    let final_reference = chains_symbol_to_type_node(
        checker,
        arena,
        target,
        context,
        reference_symbol,
        EmitSymbolMeaning::TYPE,
        argument_nodes,
    );
    restore_flags(context, restore);
    let final_reference = final_reference?;
    match result_type {
        Some(root) => append_reference_to_type(arena, target, root, final_reference),
        None => Ok(final_reference),
    }
    .map(Some)
}

/// tsc-port: appendReferenceToType @6.0.3
/// tsc-hash: 7474bfd1eabcc9ae559767d7e5beefc274c15b7564cbce62b022aee95eb058fd
/// tsc-span: _tsc.js:52043-52094
fn append_reference_to_type(
    arena: &mut TransformArena,
    target: TransformSourceId,
    root: TransformNode,
    reference: TransformNode,
) -> BuildResult<TransformNode> {
    let access = get_access_stack(arena, reference)?;
    let reference_arguments = match &arena.node(reference).map_err(factory_error)?.data {
        NodeData::TypeReference(data) => data.type_arguments,
        _ => None,
    };
    let root_data = arena.node(root).map_err(factory_error)?.data.clone();
    let data = match root_data {
        NodeData::TypeReference(mut data) => {
            let mut name = data.type_name;
            for identifier in access {
                let Some(left) = name else {
                    break;
                };
                let qualified = create_node(
                    arena,
                    target,
                    NodeData::QualifiedName(QualifiedNameData {
                        left: Some(left),
                        right: Some(identifier.node()),
                    }),
                )?;
                name = Some(qualified.node());
            }
            data.type_name = name;
            data.type_arguments = reference_arguments;
            NodeData::TypeReference(data)
        }
        NodeData::ImportType(mut data) => {
            let mut qualifier = data.qualifier;
            for identifier in access {
                qualifier = Some(match qualifier {
                    Some(left) => create_node(
                        arena,
                        target,
                        NodeData::QualifiedName(QualifiedNameData {
                            left: Some(left),
                            right: Some(identifier.node()),
                        }),
                    )?
                    .node(),
                    None => identifier.node(),
                });
            }
            data.qualifier = qualifier;
            data.type_arguments = reference_arguments;
            NodeData::ImportType(data)
        }
        _ => return Ok(root),
    };
    let updated = create_node(arena, target, data)?;
    arena
        .set_original_node(updated, Some(root))
        .map_err(factory_error)?;
    arena
        .factory()
        .set_text_range(updated, root)
        .map_err(factory_error)
}

/// tsc-port: getAccessStack @6.0.3
/// tsc-hash: e00399c8443e6ccc933717740ae1fae2e6c3e4d9a3d0824119218e112a0d5e54
/// tsc-span: _tsc.js:52095-52104
fn get_access_stack(
    arena: &TransformArena,
    reference: TransformNode,
) -> BuildResult<Vec<TransformNode>> {
    let NodeData::TypeReference(data) = &arena.node(reference).map_err(factory_error)?.data else {
        return Ok(Vec::new());
    };
    let mut state = data.type_name;
    let mut result = Vec::new();
    while let Some(node) = state.and_then(|node| arena.node_ref(reference.source(), node)) {
        match &arena.node(node).map_err(factory_error)?.data {
            NodeData::Identifier(_) => {
                result.insert(0, node);
                break;
            }
            NodeData::QualifiedName(data) => {
                if let Some(right) = data
                    .right
                    .and_then(|right| arena.node_ref(reference.source(), right))
                {
                    result.insert(0, right);
                }
                state = data.left;
            }
            _ => break,
        }
    }
    Ok(result)
}

/// tsc-port: indexInfoToObjectComputedNamesOrSignatureDeclaration @6.0.3
/// tsc-hash: a2224f118c3e076d970f0aa3db99bb2da997457d1c73dd290b1007e9ef567be7
/// tsc-span: _tsc.js:52105-52136
fn index_info_to_object_computed_names_or_signature_declaration(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    info: &crate::state::IndexInfo,
    recovered_components: Option<&[NodeId]>,
    context: &mut NodeBuilderContext<'_>,
    type_node: Option<TransformNode>,
) -> BuildResult<Vec<TransformNode>> {
    let components = info.components.as_deref().or(recovered_components);
    if let (Some(components), Some(enclosing)) = (components, context.enclosing_declaration) {
        let mut serializable = true;
        for component in components {
            let Some(name) = checker.name_of_named_declaration(*component) else {
                serializable = false;
                break;
            };
            let NodeData::ComputedPropertyName(data) = checker.data_of(name) else {
                serializable = false;
                break;
            };
            let Some(expression) = data.expression else {
                serializable = false;
                break;
            };
            if !checker.is_entity_name_expression(expression)
                || checker
                    .emit_is_entity_name_visible(expression, enclosing, false)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?
                    .accessibility
                    != EmitSymbolAccessibility::Accessible
            {
                serializable = false;
                break;
            }
        }
        if serializable {
            let mut result = Vec::new();
            for component in components {
                if checker
                    .has_late_bindable_name(*component)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?
                {
                    continue;
                }
                let Some(name_id) = checker.name_of_named_declaration(*component) else {
                    continue;
                };
                if let NodeData::ComputedPropertyName(data) = checker.data_of(name_id) {
                    if let Some(expression) = data.expression {
                        super::signatures::track_computed_name(checker, expression, context)?;
                    }
                }
                let Some(name) = clone_parse_node(checker, arena, name_id)? else {
                    continue;
                };
                let symbol = checker
                    .get_symbol_of_declaration(*component)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?;
                let component_type = match type_node {
                    Some(node) => Some(node),
                    None => {
                        let ty = checker
                            .get_type_of_symbol(symbol)
                            .map_err(|abort| checker_abort_error(checker, context, abort))?;
                        type_to_type_node_helper(checker, arena, target, ty, context)?
                    }
                };
                let modifiers = if info.is_readonly {
                    let readonly = create_token(arena, target, SyntaxKind::ReadonlyKeyword)?;
                    Some(create_node_array(arena, target, vec![readonly])?)
                } else {
                    None
                };
                let question_token = match checker.data_of(*component) {
                    NodeData::PropertySignature(data) => data.question_token,
                    NodeData::PropertyDeclaration(data) => data.question_token,
                    NodeData::MethodSignature(data) => data.question_token,
                    NodeData::MethodDeclaration(data) => data.question_token,
                    _ => None,
                }
                .map(|_| create_token(arena, target, SyntaxKind::QuestionToken))
                .transpose()?;
                let property = create_node(
                    arena,
                    target,
                    NodeData::PropertySignature(PropertySignatureData {
                        name: Some(name.node()),
                        question_token: question_token.map(TransformNode::node),
                        modifiers,
                        r#type: component_type.map(TransformNode::node),
                        initializer: None,
                    }),
                )?;
                result.push(range_synthesized_node_to_parse(
                    checker, arena, property, *component,
                )?);
            }
            return Ok(result);
        }
    }
    Ok(vec![index_info_to_index_signature_declaration_helper(
        checker, arena, target, info, context, type_node,
    )?])
}

/// tsc-port: createTypeNodesFromResolvedType @6.0.3
/// tsc-hash: e5b153cf9ea662a3cd1b911916a466bb28fdae8630757f9e99c3a2e7ad6624ef
/// tsc-span: _tsc.js:52137-52210
fn create_type_nodes_from_resolved_type(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    r#type: TypeId,
    resolved: &ResolvedMembers,
    object_flags: ObjectFlags,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<Option<Vec<TransformNode>>> {
    if check_truncation_length(context) {
        context.out.truncated = true;
        let property = create_property_signature_with_name(arena, target, "...", None, None, None)?;
        let property = if context
            .flags
            .contains(tsc_emitter::EmitNodeBuilderFlags::NO_TRUNCATION)
        {
            add_synthetic_trailing_comment(arena, property, "elided")
        } else {
            property
        };
        return Ok(Some(vec![property]));
    }
    context.type_stack.push(None);
    let result = (|| -> BuildResult<Option<Vec<TransformNode>>> {
        // The checker port can lose `IndexInfo.components` for the instance
        // side of an anonymous class while retaining it for the static side.
        // Upstream's component list is the class's non-static computed
        // members in source order; recover that provenance so `every(...)`
        // still performs its visibility walk before falling back to an index
        // signature.
        let recovered_instance_components =
            if has_flag(context, WRITE_CLASS_EXPRESSION_AS_TYPE_LITERAL) {
                checker
                    .tables
                    .type_of(r#type)
                    .symbol
                    .and_then(|symbol| checker.binder.symbol(symbol).value_declaration)
                    .filter(|&declaration| {
                        matches!(
                            checker.kind_of(declaration),
                            SyntaxKind::ClassExpression | SyntaxKind::ClassDeclaration
                        )
                    })
                    .map(|declaration| {
                        let members = match checker.data_of(declaration) {
                            NodeData::ClassExpression(data) => data.members,
                            NodeData::ClassDeclaration(data) => data.members,
                            _ => None,
                        };
                        checker
                            .nodes_of(members)
                            .into_iter()
                            .filter(|&member| {
                                let source = checker.binder.source_of_node(member);
                                !node_util::get_syntactic_modifier_flags(source, member)
                                    .intersects(ModifierFlags::STATIC)
                                    && checker.name_of_named_declaration(member).is_some_and(
                                        |name| {
                                            checker.kind_of(name)
                                                == SyntaxKind::ComputedPropertyName
                                        },
                                    )
                            })
                            .collect::<Vec<_>>()
                    })
                    .filter(|components| !components.is_empty())
            } else {
                None
            };
        let mut elements = Vec::new();
        for signature in &resolved.call_signatures {
            elements.push(signature_to_signature_declaration_helper(
                checker,
                arena,
                target,
                *signature,
                SyntaxKind::CallSignature,
                context,
                None,
            )?);
        }
        for signature in &resolved.construct_signatures {
            if checker
                .signature_of(*signature)
                .flags
                .intersects(SignatureFlags::ABSTRACT)
            {
                continue;
            }
            elements.push(signature_to_signature_declaration_helper(
                checker,
                arena,
                target,
                *signature,
                SyntaxKind::ConstructSignature,
                context,
                None,
            )?);
        }
        for info in &resolved.index_infos {
            let type_node = if object_flags.intersects(ObjectFlags::REVERSE_MAPPED) {
                Some(create_elided_information_placeholder(
                    arena, target, context,
                )?)
            } else {
                None
            };
            elements.extend(
                index_info_to_object_computed_names_or_signature_declaration(
                    checker,
                    arena,
                    target,
                    info,
                    info.components
                        .is_none()
                        .then_some(recovered_instance_components.as_deref())
                        .flatten(),
                    context,
                    type_node,
                )?,
            );
        }
        let mut property_index = 0;
        for property in resolved.properties.iter().copied() {
            let is_prototype = checker
                .symbol_flags(property)
                .intersects(SymbolFlags::PROTOTYPE);
            if context.max_expansion_depth != -1 && is_prototype {
                continue;
            }
            property_index += 1;
            if has_flag(context, WRITE_CLASS_EXPRESSION_AS_TYPE_LITERAL) {
                if is_prototype {
                    continue;
                }
                if checker
                    .get_declaration_modifier_flags_from_symbol(property)
                    .intersects(
                        tsc_types::ModifierFlags::PRIVATE | tsc_types::ModifierFlags::PROTECTED,
                    )
                {
                    context.tracker.report_private_in_base_of_class_expression(
                        &mut context.reported_diagnostic,
                        &checker.symbol_display_name(property),
                    );
                }
                if let Some(value_declaration) = checker.binder.symbol(property).value_declaration {
                    let source = checker.binder.source_of_node(value_declaration);
                    if let Some(name) =
                        node_util::get_name_of_declaration(source, value_declaration)
                    {
                        if let NodeData::PrivateIdentifier(data) = checker.data_of(name) {
                            context.tracker.report_private_in_base_of_class_expression(
                                &mut context.reported_diagnostic,
                                &data.text,
                            );
                        }
                    }
                }
            }
            if check_truncation_length(context)
                && property_index + 2 < resolved.properties.len().saturating_sub(1)
            {
                context.out.truncated = true;
                let remaining = resolved.properties.len() - property_index;
                if context
                    .flags
                    .contains(tsc_emitter::EmitNodeBuilderFlags::NO_TRUNCATION)
                {
                    if let Some(last) = elements.pop() {
                        elements.push(add_synthetic_trailing_comment(
                            arena,
                            last,
                            format!("... {remaining} more elided ..."),
                        ));
                    }
                } else {
                    let name = format!("... {remaining} more ...");
                    elements.push(create_property_signature_with_name(
                        arena, target, &name, None, None, None,
                    )?);
                }
                if let Some(last) = resolved.properties.last().copied() {
                    add_property_to_element_list(
                        checker,
                        arena,
                        target,
                        last,
                        context,
                        &mut elements,
                    )?;
                }
                break;
            }
            add_property_to_element_list(checker, arena, target, property, context, &mut elements)?;
        }
        Ok((!elements.is_empty()).then_some(elements))
    })();
    context.type_stack.pop();
    result
}

/// tsc-port: createElidedInformationPlaceholder @6.0.3
/// tsc-hash: 9fe24796b9c8dc49e718e88a66c16cac0341b79590fdec1e7b8edc49122e169f
/// tsc-span: _tsc.js:52212-52222
fn create_elided_information_placeholder(
    arena: &mut TransformArena,
    target: TransformSourceId,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<TransformNode> {
    add_approximate_length(context, 3);
    if !context
        .flags
        .contains(tsc_emitter::EmitNodeBuilderFlags::NO_TRUNCATION)
    {
        return create_named_type_reference(arena, target, "...", None);
    }
    let node = create_keyword_type_node(arena, target, SyntaxKind::AnyKeyword)?;
    Ok(add_synthetic_leading_comment(arena, node, "elided"))
}

/// tsc-port: shouldUsePlaceholderForProperty @6.0.3
/// tsc-hash: 6216ae17f4795783d5b0c85fe0c09dd1c1b7fb7ccc3940cd203890b7a4dc7822
/// tsc-span: _tsc.js:52223-52240
fn should_use_placeholder_for_property(
    checker: &CheckerState<'_>,
    property: SymbolId,
    context: &NodeBuilderContext<'_>,
) -> bool {
    if !checker
        .get_check_flags(property)
        .intersects(CheckFlags::REVERSE_MAPPED)
    {
        return false;
    }
    let Some(stack) = context.reverse_mapped_stack.as_ref() else {
        return false;
    };
    stack.contains(&property)
        || !stack.is_empty()
            && stack.last().is_some_and(|last| {
                checker.links.symbol(*last).property_type.is_some_and(|ty| {
                    !checker
                        .tables
                        .object_flags_of(ty)
                        .intersects(ObjectFlags::ANONYMOUS)
                })
            })
        || is_deeply_nested_reverse_mapped_type_property(checker, property, stack)
}

/// tsc-port: isDeeplyNestedReverseMappedTypeProperty @6.0.3
/// tsc-hash: df3584e30d1c13c85520b98687a965cf29813b15a6bbce950b45d32825a366a1
/// tsc-span: _tsc.js:52227-52239
fn is_deeply_nested_reverse_mapped_type_property(
    checker: &CheckerState<'_>,
    property: SymbolId,
    stack: &[SymbolId],
) -> bool {
    const DEPTH: usize = 3;
    if stack.len() < DEPTH {
        return false;
    }
    let property_mapped_symbol = checker
        .links
        .symbol(property)
        .mapped_type
        .and_then(|ty| checker.tables.type_of(ty).symbol);
    stack.iter().rev().take(DEPTH).all(|entry| {
        checker
            .links
            .symbol(*entry)
            .mapped_type
            .and_then(|ty| checker.tables.type_of(ty).symbol)
            == property_mapped_symbol
    })
}

fn create_property_name_for_symbol(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    symbol: SymbolId,
    context: &mut NodeBuilderContext<'_>,
) -> BuildResult<TransformNode> {
    chains_get_property_name_node_for_symbol(checker, arena, target, context, symbol)
}

fn create_property_signature_with_name(
    arena: &mut TransformArena,
    target: TransformSourceId,
    name: &str,
    modifiers: Option<Vec<TransformNode>>,
    question: Option<TransformNode>,
    r#type: Option<TransformNode>,
) -> BuildResult<TransformNode> {
    let name = if tsc_syntax::is_identifier_text(name) {
        create_identifier(arena, target, name)?
    } else {
        create_node(
            arena,
            target,
            NodeData::StringLiteral(StringLiteralData {
                text: name.to_owned(),
                has_extended_unicode_escape: None,
            }),
        )?
    };
    let modifiers = match modifiers {
        Some(modifiers) => Some(create_node_array(arena, target, modifiers)?),
        None => None,
    };
    create_node(
        arena,
        target,
        NodeData::PropertySignature(PropertySignatureData {
            name: Some(name.node()),
            question_token: question.map(TransformNode::node),
            modifiers,
            r#type: r#type.map(TransformNode::node),
            initializer: None,
        }),
    )
}

/// tsc-port: addPropertyToElementList @6.0.3
/// tsc-hash: 6c6b916aa8ce07acd5ac0ba435bbb827fea9ee957fdaf3e2843643414cba7580
/// tsc-span: _tsc.js:52241-52397
fn add_property_to_element_list(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    property: SymbolId,
    context: &mut NodeBuilderContext<'_>,
    elements: &mut Vec<TransformNode>,
) -> BuildResult<()> {
    let reverse = checker
        .get_check_flags(property)
        .intersects(CheckFlags::REVERSE_MAPPED);
    let placeholder = should_use_placeholder_for_property(checker, property, context);
    let property_type = if placeholder {
        checker.tables.intrinsics.any
    } else {
        checker
            .get_non_missing_type_of_symbol(property)
            .map_err(|abort| checker_abort_error(checker, context, abort))?
    };
    let old_enclosing = context.enclosing_declaration;
    let property_data = checker.binder.symbol(property);
    let late_bound_name = property_data.escaped_name.starts_with("__@");
    if context.tracker.can_track_symbol && late_bound_name {
        if let Some(&declaration) = property_data.declarations.first() {
            if checker.has_late_bindable_ast_name(declaration) {
                let source = checker.binder.source_of_node(declaration);
                if let Some(name) = node_util::get_name_of_declaration(source, declaration) {
                    let expression = match checker.data_of(name) {
                        NodeData::ComputedPropertyName(data) => data.expression,
                        NodeData::ElementAccessExpression(data) => data.argument_expression,
                        _ => None,
                    };
                    if let Some(expression) = expression {
                        super::signatures::track_computed_name(checker, expression, context)?;
                    }
                }
            }
        } else {
            context.tracker.report_non_serializable_property(
                &mut context.reported_diagnostic,
                &checker.symbol_display_name(property),
            );
        }
    }
    context.enclosing_declaration = checker
        .binder
        .symbol(property)
        .value_declaration
        .or_else(|| {
            checker
                .binder
                .symbol(property)
                .declarations
                .first()
                .copied()
        })
        .or(old_enclosing);
    let name = create_property_name_for_symbol(checker, arena, target, property, context);
    context.enclosing_declaration = old_enclosing;
    let name = name?;
    let display_name = checker.symbol_display_name(property);
    let approximate_name_length = if late_bound_name {
        // Upstream late-bound names embed a small, program-local symbol id
        // (`__@name@N`). Rust's globally allocated SymbolId is commonly five
        // digits in this replay, but that allocator detail must not inflate
        // NodeBuilder's truncation accounting. The probe's corresponding
        // program-local suffix is two digits.
        display_name
            .rsplit_once('@')
            .filter(|(_, suffix)| suffix.bytes().all(|byte| byte.is_ascii_digit()))
            .map_or_else(
                || js_len(&display_name),
                |(prefix, suffix)| js_len(prefix) + 1 + js_len(suffix).min(2),
            )
    } else {
        js_len(&display_name)
    };
    add_approximate_length(context, approximate_name_length + 1);

    if checker
        .symbol_flags(property)
        .intersects(SymbolFlags::ACCESSOR)
    {
        let write_type = checker
            .get_write_type_of_symbol(property)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        if !checker.tables.is_error_type(property_type) && !checker.tables.is_error_type(write_type)
        {
            let symbol = checker.binder.symbol(property);
            let mapper = checker.links.symbol(property).mapper;
            let property_declaration =
                checker.get_declaration_of_kind(property, SyntaxKind::PropertyDeclaration);
            let parent_is_class = symbol
                .parent
                .is_some_and(|parent| checker.symbol_flags(parent).intersects(SymbolFlags::CLASS));
            if property_type != write_type || parent_is_class && property_declaration.is_none() {
                if let Some(getter_declaration) =
                    checker.get_declaration_of_kind(property, SyntaxKind::GetAccessor)
                {
                    let mut getter = checker
                        .get_signature_from_declaration(getter_declaration)
                        .map_err(|abort| checker_abort_error(checker, context, abort))?;
                    if let Some(mapper) = mapper {
                        getter = checker
                            .instantiate_signature(getter, mapper, false)
                            .map_err(|abort| checker_abort_error(checker, context, abort))?;
                    }
                    let getter = signature_to_signature_declaration_helper(
                        checker,
                        arena,
                        target,
                        getter,
                        SyntaxKind::GetAccessor,
                        context,
                        Some(SignatureDeclarationOptions {
                            modifiers: None,
                            name: Some(name),
                            question_token: None,
                        }),
                    )?;
                    elements.push(set_comment_range_2(
                        checker,
                        arena,
                        getter,
                        getter_declaration,
                        context,
                    )?);
                }
                if let Some(setter_declaration) =
                    checker.get_declaration_of_kind(property, SyntaxKind::SetAccessor)
                {
                    let mut setter = checker
                        .get_signature_from_declaration(setter_declaration)
                        .map_err(|abort| checker_abort_error(checker, context, abort))?;
                    if let Some(mapper) = mapper {
                        setter = checker
                            .instantiate_signature(setter, mapper, false)
                            .map_err(|abort| checker_abort_error(checker, context, abort))?;
                    }
                    let setter = signature_to_signature_declaration_helper(
                        checker,
                        arena,
                        target,
                        setter,
                        SyntaxKind::SetAccessor,
                        context,
                        Some(SignatureDeclarationOptions {
                            modifiers: None,
                            name: Some(name),
                            question_token: None,
                        }),
                    )?;
                    elements.push(set_comment_range_2(
                        checker,
                        arena,
                        setter,
                        setter_declaration,
                        context,
                    )?);
                }
                return Ok(());
            }
            let is_auto_accessor = property_declaration.is_some_and(|declaration| {
                let source = checker.binder.source_of_node(declaration);
                node_util::modifiers_of(source, declaration).is_some_and(|modifiers| {
                    checker
                        .nodes_of(Some(modifiers))
                        .iter()
                        .any(|&modifier| checker.kind_of(modifier) == SyntaxKind::AccessorKeyword)
                })
            });
            if parent_is_class && is_auto_accessor {
                let getter_signature = checker.alloc_signature(crate::state::Signature {
                    declaration: None,
                    flags: SignatureFlags::NONE,
                    type_parameters: None,
                    parameters: Vec::new(),
                    this_parameter: None,
                    min_argument_count: 0,
                    resolved_return_type: crate::links::LinkSlot::Resolved(property_type),
                    from_method: false,
                    target: None,
                    mapper: None,
                    instantiations: HashMap::new(),
                    erased_signature_cache: None,
                    canonical_signature_cache: None,
                    base_signature_cache: None,
                    composite_kind: None,
                    composite_signatures: None,
                    optional_call_signature_cache: (None, None),
                    isolated_signature_kind: None,
                    isolated_signature_type: None,
                });
                let getter = signature_to_signature_declaration_helper(
                    checker,
                    arena,
                    target,
                    getter_signature,
                    SyntaxKind::GetAccessor,
                    context,
                    Some(SignatureDeclarationOptions {
                        modifiers: None,
                        name: Some(name),
                        question_token: None,
                    }),
                )?;
                elements.push(set_comment_range_2(
                    checker,
                    arena,
                    getter,
                    property_declaration.expect("auto-accessor declaration"),
                    context,
                )?);

                let setter_parameter = checker
                    .binder
                    .create_symbol(SymbolFlags::FUNCTION_SCOPED_VARIABLE, "arg".to_owned());
                checker.links.set_fresh_symbol_type(
                    setter_parameter,
                    crate::links::LinkSlot::Resolved(write_type),
                );
                let setter_signature = checker.alloc_signature(crate::state::Signature {
                    declaration: None,
                    flags: SignatureFlags::NONE,
                    type_parameters: None,
                    parameters: vec![setter_parameter],
                    this_parameter: None,
                    min_argument_count: 0,
                    resolved_return_type: crate::links::LinkSlot::Resolved(
                        checker.tables.intrinsics.void,
                    ),
                    from_method: false,
                    target: None,
                    mapper: None,
                    instantiations: HashMap::new(),
                    erased_signature_cache: None,
                    canonical_signature_cache: None,
                    base_signature_cache: None,
                    composite_kind: None,
                    composite_signatures: None,
                    optional_call_signature_cache: (None, None),
                    isolated_signature_kind: None,
                    isolated_signature_type: None,
                });
                elements.push(signature_to_signature_declaration_helper(
                    checker,
                    arena,
                    target,
                    setter_signature,
                    SyntaxKind::SetAccessor,
                    context,
                    Some(SignatureDeclarationOptions {
                        modifiers: None,
                        name: Some(name),
                        question_token: None,
                    }),
                )?);
                return Ok(());
            }
        }
    }

    let optional = checker
        .symbol_flags(property)
        .intersects(SymbolFlags::OPTIONAL)
        .then(|| create_token(arena, target, SyntaxKind::QuestionToken))
        .transpose()?;
    if checker
        .symbol_flags(property)
        .intersects(SymbolFlags::FUNCTION | SymbolFlags::METHOD)
        && checker
            .get_properties_of_type_full(property_type)
            .map_err(|abort| checker_abort_error(checker, context, abort))?
            .is_empty()
        && !checker.is_readonly_symbol(property)
    {
        let callable_type = checker.tables.filter_type(property_type, |tables, ty| {
            !tables.flags_of(ty).intersects(TypeFlags::UNDEFINED)
        });
        let signatures = checker
            .get_signatures_of_type(callable_type, crate::state::SignatureKind::Call)
            .map_err(|abort| checker_abort_error(checker, context, abort))?;
        for signature in &signatures {
            let method = signature_to_signature_declaration_helper(
                checker,
                arena,
                target,
                *signature,
                SyntaxKind::MethodSignature,
                context,
                Some(SignatureDeclarationOptions {
                    modifiers: None,
                    name: Some(name),
                    question_token: optional,
                }),
            )?;
            elements.push(preserve_comments_on(
                checker,
                arena,
                method,
                checker
                    .signature_of(*signature)
                    .declaration
                    .or(checker.binder.symbol(property).value_declaration),
                property,
                context,
            )?);
        }
        if !signatures.is_empty() || optional.is_none() {
            return Ok(());
        }
    }
    let type_node = if placeholder {
        create_elided_information_placeholder(arena, target, context)?
    } else {
        if reverse {
            context
                .reverse_mapped_stack
                .get_or_insert_with(Vec::new)
                .push(property);
        }
        let node = (|| match super::serialize_type_for_declaration_seam(
            checker,
            arena,
            target,
            context,
            None,
            property_type,
            Some(property),
        )? {
            Some(node) => Ok(Some(node)),
            None => type_to_type_node_helper(checker, arena, target, property_type, context),
        })();
        if reverse {
            context
                .reverse_mapped_stack
                .as_mut()
                .expect("pushed above")
                .pop();
        }
        let node = node?;
        require_node(context, node).unwrap_or(create_keyword_type_node(
            arena,
            target,
            SyntaxKind::AnyKeyword,
        )?)
    };
    let modifiers = if checker.is_readonly_symbol(property) {
        add_approximate_length(context, 9);
        Some(vec![create_token(
            arena,
            target,
            SyntaxKind::ReadonlyKeyword,
        )?])
    } else {
        None
    };
    let modifiers = match modifiers {
        Some(modifiers) => Some(create_node_array(arena, target, modifiers)?),
        None => None,
    };
    let property_node = create_node(
        arena,
        target,
        NodeData::PropertySignature(PropertySignatureData {
            name: Some(name.node()),
            question_token: optional.map(TransformNode::node),
            modifiers,
            r#type: Some(type_node.node()),
            initializer: None,
        }),
    )?;
    elements.push(preserve_comments_on(
        checker,
        arena,
        property_node,
        checker.binder.symbol(property).value_declaration,
        property,
        context,
    )?);
    Ok(())
}

/// tsc-port: preserveCommentsOn @6.0.3
/// tsc-hash: 151533253991304ca0ea7538723109604fab53e462040768f2ee1a7cc26a0bda
/// tsc-span: _tsc.js:52384-52396
fn preserve_comments_on(
    checker: &CheckerState<'_>,
    arena: &mut TransformArena,
    node: TransformNode,
    range: Option<NodeId>,
    property: SymbolId,
    context: &NodeBuilderContext<'_>,
) -> BuildResult<TransformNode> {
    if let Some(tag) = checker
        .binder
        .symbol(property)
        .declarations
        .iter()
        .copied()
        .find(|&declaration| checker.kind_of(declaration) == SyntaxKind::JSDocPropertyTag)
    {
        if let NodeData::JSDocPropertyTag(data) = checker.data_of(tag) {
            let comment_text = match data.comment.as_ref() {
                Some(tsc_syntax::nodes::JSDocComment::Text(text)) => Some(text.clone()),
                Some(tsc_syntax::nodes::JSDocComment::Nodes(nodes)) => {
                    let text = checker
                        .nodes_of(Some(*nodes))
                        .into_iter()
                        .filter_map(|node| match checker.data_of(node) {
                            NodeData::JSDocText(data) => Some(data.text.as_str()),
                            _ => None,
                        })
                        .collect::<String>();
                    (!text.is_empty()).then_some(text)
                }
                None => None,
            };
            if let Some(comment_text) = comment_text.filter(|text| !text.is_empty()) {
                let text = format!("*\n * {}\n ", comment_text.replace('\n', "\n * "));
                arena
                    .metadata_mut(node)
                    .add_leading_comment(SyntheticComment::new(
                        SyntheticCommentKind::MultiLine,
                        text,
                        false,
                        true,
                    ));
                return Ok(node);
            }
        }
        return Ok(node);
    }
    match range {
        Some(range) => set_comment_range_2(checker, arena, node, range, context),
        None => Ok(node),
    }
}

/// tsc-port: setCommentRange2 @6.0.3
/// tsc-hash: 73d8d78104ed1ab5c2753b4f79c5644f542f81a0241b6842f3d9695959e120a6
/// tsc-span: _tsc.js:52398-52403
fn set_comment_range_2(
    checker: &CheckerState<'_>,
    arena: &mut TransformArena,
    node: TransformNode,
    range: NodeId,
    context: &NodeBuilderContext<'_>,
) -> BuildResult<TransformNode> {
    if context.enclosing_file != Some(checker.binder.source_of_node(range).root) {
        return Ok(node);
    }
    let Some(original) = project_parse_node(checker, arena, range)? else {
        return Ok(node);
    };
    let record = arena.node(original).map_err(factory_error)?;
    let source = arena.source(original.source()).map_err(factory_error)?;
    let source_range = SourceRange::from_raw(record.pos, record.end, source.syntax().positions())
        .map_err(|error| {
        factory_error(TransformError::InvalidSourceRange {
            node: original,
            error,
        })
    })?;
    arena
        .metadata_mut(node)
        .set_comment_range(CommentRange::new(original.source(), source_range));
    Ok(node)
}

/// tsc-port: mapToTypeNodes @6.0.3
/// tsc-hash: a385aa0049c7141c5be2128f906eee6191ceb8679fc524625f46ebab2808d59a
/// tsc-span: _tsc.js:52404-52472
pub(crate) fn map_to_type_nodes(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    types: &[TypeId],
    context: &mut NodeBuilderContext<'_>,
    is_bare_list: bool,
) -> BuildResult<Option<Vec<TransformNode>>> {
    if types.is_empty() {
        return Ok(None);
    }
    if check_truncation_length(context) {
        context.out.truncated = true;
        if !is_bare_list {
            return Ok(Some(vec![create_elided_information_placeholder(
                arena, target, context,
            )?]));
        }
        if types.len() > 2 {
            let mut result = Vec::with_capacity(3);
            if let Some(first) =
                type_to_type_node_helper(checker, arena, target, types[0], context)?
            {
                result.push(first);
            }
            let placeholder = if context
                .flags
                .contains(tsc_emitter::EmitNodeBuilderFlags::NO_TRUNCATION)
            {
                let any = create_keyword_type_node(arena, target, SyntaxKind::AnyKeyword)?;
                add_synthetic_leading_comment(
                    arena,
                    any,
                    format!("... {} more elided ...", types.len() - 2),
                )
            } else {
                create_named_type_reference(
                    arena,
                    target,
                    &format!("... {} more ...", types.len() - 2),
                    None,
                )?
            };
            result.push(placeholder);
            if let Some(last) =
                type_to_type_node_helper(checker, arena, target, types[types.len() - 1], context)?
            {
                result.push(last);
            }
            return Ok(Some(result));
        }
    }
    let may_have_name_collisions = !has_flag(context, USE_FULLY_QUALIFIED_TYPE);
    let mut seen_names: HashMap<String, Vec<(TypeId, usize)>> = HashMap::new();
    let mut result = Vec::with_capacity(types.len());
    for (index, r#type) in types.iter().copied().enumerate() {
        if check_truncation_length(context) && index + 4 < types.len() {
            context.out.truncated = true;
            let remaining = types.len() - index - 1;
            let placeholder = if context
                .flags
                .contains(tsc_emitter::EmitNodeBuilderFlags::NO_TRUNCATION)
            {
                let any = create_keyword_type_node(arena, target, SyntaxKind::AnyKeyword)?;
                add_synthetic_leading_comment(
                    arena,
                    any,
                    format!("... {remaining} more elided ..."),
                )
            } else {
                create_named_type_reference(
                    arena,
                    target,
                    &format!("... {remaining} more ..."),
                    None,
                )?
            };
            result.push(placeholder);
            if let Some(last) =
                type_to_type_node_helper(checker, arena, target, types[types.len() - 1], context)?
            {
                result.push(last);
            }
            break;
        }
        add_approximate_length(context, 2);
        if let Some(node) = type_to_type_node_helper(checker, arena, target, r#type, context)? {
            if may_have_name_collisions {
                if let NodeData::TypeReference(data) =
                    &arena.node(node).map_err(factory_error)?.data
                {
                    if let Some(type_name) = data.type_name {
                        if let NodeData::Identifier(identifier) = &arena
                            .source(target)
                            .map_err(factory_error)?
                            .syntax()
                            .arena
                            .node(type_name)
                            .data
                        {
                            seen_names
                                .entry(identifier.text.clone())
                                .or_default()
                                .push((r#type, result.len()));
                        }
                    }
                }
            }
            result.push(node);
        }
    }
    if may_have_name_collisions {
        let restore = save_restore_flags(context);
        set_flag(context, USE_FULLY_QUALIFIED_TYPE);
        let mut collision_groups = seen_names.values().collect::<Vec<_>>();
        collision_groups.sort_by_key(|collisions| {
            collisions
                .first()
                .map(|(_, index)| *index)
                .unwrap_or(usize::MAX)
        });
        for collisions in collision_groups {
            let homogeneous = collisions.first().is_none_or(|(first, _)| {
                collisions
                    .iter()
                    .skip(1)
                    .all(|(other, _)| types_are_same_reference(checker, *first, *other))
            });
            if !homogeneous {
                for &(r#type, index) in collisions {
                    let replacement =
                        type_to_type_node_helper(checker, arena, target, r#type, context);
                    restore_flags(context, restore);
                    if let Some(replacement) = replacement? {
                        result[index] = replacement;
                    }
                    set_flag(context, USE_FULLY_QUALIFIED_TYPE);
                }
            }
        }
        restore_flags(context, restore);
    }
    Ok(Some(result))
}

/// tsc-port: typesAreSameReference @6.0.3
/// tsc-hash: d9ee2b44342848a14d7e49485aed022879cec63e4b385d12538235e872085bcb
/// tsc-span: _tsc.js:52473-52475
#[allow(dead_code)]
fn types_are_same_reference(checker: &CheckerState<'_>, left: TypeId, right: TypeId) -> bool {
    let left_type = checker.tables.type_of(left);
    let right_type = checker.tables.type_of(right);
    left == right
        || left_type.symbol.is_some() && left_type.symbol == right_type.symbol
        || left_type.alias_symbol.is_some() && left_type.alias_symbol == right_type.alias_symbol
}

fn format_union_types(
    checker: &mut CheckerState<'_>,
    types: &[TypeId],
    expanding_enum: bool,
) -> Result<Vec<TypeId>, CheckAbort> {
    let mut result = Vec::new();
    let mut combined = TypeFlags::from_bits(0);
    let mut index = 0;
    while index < types.len() {
        let current = types[index];
        let flags = checker.tables.flags_of(current);
        combined = TypeFlags::from_bits(combined.bits() | flags.bits());
        if !flags.intersects(TypeFlags::NULLABLE) {
            if flags.intersects(TypeFlags::BOOLEAN_LITERAL) || !expanding_enum {
                let base = if flags.intersects(TypeFlags::BOOLEAN_LITERAL) {
                    checker.tables.intrinsics.boolean
                } else {
                    checker.get_base_type_of_enum_like_type(current)?
                };
                if let TypeData::Union {
                    types: base_types, ..
                } = checker.tables.type_of(base).data.clone()
                {
                    let count = base_types.len();
                    if count > 0 && index + count <= types.len() {
                        let run_last = checker
                            .tables
                            .get_regular_type_of_literal_type(types[index + count - 1]);
                        let base_last = checker
                            .tables
                            .get_regular_type_of_literal_type(base_types[count - 1]);
                        if run_last == base_last {
                            result.push(base);
                            index += count;
                            continue;
                        }
                    }
                }
            }
            result.push(current);
        }
        index += 1;
    }
    if combined.intersects(TypeFlags::NULL) {
        result.push(checker.tables.intrinsics.null);
    }
    if combined.intersects(TypeFlags::UNDEFINED) {
        result.push(checker.tables.intrinsics.undefined);
    }
    Ok(result)
}

#[cfg(test)]
#[path = "../../tests/unit/node_builder_type_nodes/tests.rs"]
mod tests;

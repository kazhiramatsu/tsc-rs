use tsc_emitter::{
    EmitInternalNodeBuilderFlags, EmitNodeBuilderFlags, EmitSymbolMeaning, SourceFileId,
    TransformArena, TransformNode, TransformSourceId,
};
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::CompilerOptions;

use crate::node_builder::with_context;
use crate::state::test_support::with_program_state;

use super::super::type_nodes::create_token;
use super::*;

fn with_builder_files(
    files: &[(&str, &str)],
    target_index: usize,
    flags: EmitNodeBuilderFlags,
    internal_flags: EmitInternalNodeBuilderFlags,
    run: impl FnOnce(
        &mut CheckerState<'_>,
        &mut TransformArena,
        TransformSourceId,
        &mut NodeBuilderContext<'_>,
    ) -> BuildResult<()>,
) {
    with_program_state(files, &CompilerOptions::default(), |checker| {
        let root = checker.binder.source(target_index).root;
        let mut arena = TransformArena::new();
        let targets = (0..checker.binder.file_count())
            .map(|index| {
                arena.add_source(
                    checker.binder.source(index),
                    Some(program_source_id(checker, index)),
                )
            })
            .collect::<Vec<_>>();
        let result = with_context(
            checker,
            &mut arena,
            targets[target_index],
            Some(root),
            Some(flags),
            Some(internal_flags),
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
    });
}

fn declaration_symbol(
    checker: &CheckerState<'_>,
    file_index: usize,
    kind: SyntaxKind,
    expected_name: &str,
) -> SymbolId {
    checker
        .binder
        .source(file_index)
        .arena
        .node_ids()
        .filter(|&node| checker.kind_of(node) == kind)
        .find_map(|node| {
            let name =
                node_util::get_name_of_declaration(checker.binder.source_of_node(node), node)?;
            (declaration_name_text(checker, name).as_deref() == Some(expected_name))
                .then(|| checker.binder.node_symbol(node))
                .flatten()
        })
        .unwrap_or_else(|| panic!("missing {kind:?} symbol {expected_name}"))
}

fn declaration_name_text(checker: &CheckerState<'_>, name: NodeId) -> Option<String> {
    match checker.data_of(name) {
        NodeData::Identifier(data) => Some(data.text.clone()),
        NodeData::StringLiteral(data) => Some(data.text.clone()),
        NodeData::NumericLiteral(data) => Some(data.text.clone()),
        _ => None,
    }
}

fn declarations_of_kind(
    checker: &CheckerState<'_>,
    file_index: usize,
    kind: SyntaxKind,
) -> Vec<NodeId> {
    checker
        .binder
        .source(file_index)
        .arena
        .node_ids()
        .filter(|&node| checker.kind_of(node) == kind)
        .collect()
}

fn child(arena: &TransformArena, parent: TransformNode, node: Option<NodeId>) -> TransformNode {
    arena
        .node_ref(parent.source(), node.expect("child identity"))
        .expect("child belongs to its parent source")
}

fn array_nodes(
    arena: &TransformArena,
    target: TransformSourceId,
    array: tsc_syntax::NodeArrayId,
) -> Vec<TransformNode> {
    arena
        .source(target)
        .expect("transform source")
        .syntax()
        .arena
        .node_array(array)
        .nodes
        .iter()
        .copied()
        .map(|node| arena.node_ref(target, node).expect("array child"))
        .collect()
}

#[test]
fn nested_namespace_chain_selects_parent_qualification_and_distinct_node_forms() {
    let source = "namespace Outer {\n\
                  export namespace Inner {\n\
                    export const value = 1;\n\
                  }\n\
                }";
    with_builder_files(
        &[("/project/main.ts", source)],
        0,
        EmitNodeBuilderFlags::NONE,
        EmitInternalNodeBuilderFlags::NONE,
        |checker, arena, target, context| {
            let value = declaration_symbol(checker, 0, SyntaxKind::VariableDeclaration, "value");
            let chain = chains_lookup_symbol_chain(
                checker,
                context,
                value,
                EmitSymbolMeaning::VALUE_EXPORT_VALUE,
            )?;
            let names = chain
                .iter()
                .map(|&symbol| checker.symbol_display_name(symbol))
                .collect::<Vec<_>>();
            assert_eq!(names, ["Outer", "Inner", "value"]);

            let entity = chains_symbol_to_entity_name_node(checker, arena, target, context, value)?;
            assert_eq!(
                arena.node(entity).map_err(factory_error)?.kind,
                SyntaxKind::QualifiedName
            );

            let expression = chains_symbol_to_expression(
                checker,
                arena,
                target,
                context,
                value,
                EmitSymbolMeaning::VALUE_EXPORT_VALUE,
            )?;
            let NodeData::PropertyAccessExpression(outer) =
                &arena.node(expression).map_err(factory_error)?.data
            else {
                panic!("property-access expression expected")
            };
            let receiver = child(arena, expression, outer.expression);
            assert_eq!(
                arena.node(receiver).map_err(factory_error)?.kind,
                SyntaxKind::PropertyAccessExpression
            );
            Ok(())
        },
    );
}

#[test]
fn qualified_type_reference_threads_override_arguments_and_parameter_declarations() {
    let source = "namespace Outer {\n\
                  export namespace Inner {\n\
                    export interface Box<T extends string = 'fallback'> {}\n\
                  }\n\
                }";
    with_builder_files(
        &[("/project/main.ts", source)],
        0,
        EmitNodeBuilderFlags::NONE,
        EmitInternalNodeBuilderFlags::NONE,
        |checker, arena, target, context| {
            let symbol = declaration_symbol(checker, 0, SyntaxKind::InterfaceDeclaration, "Box");
            let argument = create_token(arena, target, SyntaxKind::NumberKeyword)?;
            let node = chains_symbol_to_type_node(
                checker,
                arena,
                target,
                context,
                symbol,
                EmitSymbolMeaning::TYPE,
                Some(vec![argument]),
            )?;
            let NodeData::TypeReference(data) = &arena.node(node).map_err(factory_error)?.data
            else {
                panic!("type reference expected")
            };
            let name = child(arena, node, data.type_name);
            assert_eq!(
                arena.node(name).map_err(factory_error)?.kind,
                SyntaxKind::QualifiedName
            );
            let arguments = array_nodes(
                arena,
                target,
                data.type_arguments.expect("override type arguments"),
            );
            assert_eq!(arguments, [argument]);

            let declarations = type_parameters_to_type_parameter_declarations(
                checker, arena, target, symbol, context,
            )?
            .expect("generic interface parameters");
            assert_eq!(declarations.len(), 1);
            let NodeData::TypeParameter(parameter) =
                &arena.node(declarations[0]).map_err(factory_error)?.data
            else {
                panic!("type parameter expected")
            };
            let constraint = child(arena, declarations[0], parameter.constraint);
            assert_eq!(
                arena.node(constraint).map_err(factory_error)?.kind,
                SyntaxKind::StringKeyword
            );
            let default = child(arena, declarations[0], parameter.r#default);
            assert_eq!(
                arena.node(default).map_err(factory_error)?.kind,
                SyntaxKind::LiteralType
            );
            Ok(())
        },
    );
}

#[test]
fn external_symbol_uses_lane_h_relative_import_type_and_threads_arguments() {
    let files = [
        (
            "/project/lib.ts",
            "export namespace Api { export interface Box<T> {} }",
        ),
        ("/project/main.ts", "export {};"),
    ];
    with_builder_files(
        &files,
        1,
        EmitNodeBuilderFlags::NONE,
        EmitInternalNodeBuilderFlags::NONE,
        |checker, arena, target, context| {
            let symbol = declaration_symbol(checker, 0, SyntaxKind::InterfaceDeclaration, "Box");
            let argument = create_token(arena, target, SyntaxKind::StringKeyword)?;
            let node = chains_symbol_to_type_node(
                checker,
                arena,
                target,
                context,
                symbol,
                EmitSymbolMeaning::TYPE,
                Some(vec![argument]),
            )?;
            let NodeData::ImportType(data) = &arena.node(node).map_err(factory_error)?.data else {
                panic!(
                    "import type expected, got {:?}",
                    arena.node(node).unwrap().kind
                )
            };
            let literal_type = child(arena, node, data.argument);
            let NodeData::LiteralType(literal_type_data) =
                &arena.node(literal_type).map_err(factory_error)?.data
            else {
                panic!("literal import argument expected")
            };
            let literal = child(arena, literal_type, literal_type_data.literal);
            assert!(matches!(
                &arena.node(literal).map_err(factory_error)?.data,
                NodeData::StringLiteral(data) if data.text == "./lib"
            ));
            let qualifier = child(arena, node, data.qualifier);
            assert_eq!(
                arena.node(qualifier).map_err(factory_error)?.kind,
                SyntaxKind::QualifiedName
            );
            let arguments = array_nodes(
                arena,
                target,
                data.type_arguments.expect("import type arguments"),
            );
            assert_eq!(arguments, [argument]);
            Ok(())
        },
    );
}

#[test]
fn computed_entity_chain_and_write_computed_props_keep_computed_shapes() {
    let source = "declare const key: unique symbol;\n\
                  class Container { static [key]: number; }";
    with_builder_files(
        &[("/project/main.ts", source)],
        0,
        EmitNodeBuilderFlags::NONE,
        EmitInternalNodeBuilderFlags::WRITE_COMPUTED_PROPS,
        |checker, arena, target, context| {
            let key = declaration_symbol(checker, 0, SyntaxKind::VariableDeclaration, "key");
            let key_type = checker
                .get_type_of_symbol(key)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            assert!(checker
                .tables
                .flags_of(key_type)
                .intersects(TypeFlags::UNIQUE_ES_SYMBOL));
            let computed_symbol = checker
                .binder
                .create_symbol(SymbolFlags::PROPERTY, "__computed".to_owned());
            checker
                .links
                .set_symbol_name_type(0, computed_symbol, Some(key_type));
            let property = declarations_of_kind(checker, 0, SyntaxKind::PropertyDeclaration)
                .into_iter()
                .next()
                .and_then(|declaration| checker.binder.node_symbol(declaration))
                .expect("computed property symbol");
            let computed = symbol_to_node(
                checker,
                arena,
                target,
                context,
                computed_symbol,
                EmitSymbolMeaning::VALUE_EXPORT_VALUE,
            )?;
            let NodeData::ComputedPropertyName(data) =
                &arena.node(computed).map_err(factory_error)?.data
            else {
                panic!("computed property name expected")
            };
            let expression = child(arena, computed, data.expression);
            assert_eq!(
                arena.node(expression).map_err(factory_error)?.kind,
                SyntaxKind::Identifier
            );

            context.internal_flags = EmitInternalNodeBuilderFlags::NONE;
            let access = chains_symbol_to_type_node(
                checker,
                arena,
                target,
                context,
                property,
                EmitSymbolMeaning::VALUE_EXPORT_VALUE,
                None,
            )?;
            let NodeData::IndexedAccessType(data) =
                &arena.node(access).map_err(factory_error)?.data
            else {
                panic!("computed symbol access must be indexed")
            };
            let object = child(arena, access, data.object_type);
            let index = child(arena, access, data.index_type);
            assert_eq!(
                arena.node(object).map_err(factory_error)?.kind,
                SyntaxKind::ParenthesizedType
            );
            assert_eq!(
                arena.node(index).map_err(factory_error)?.kind,
                SyntaxKind::TypeQuery
            );
            Ok(())
        },
    );
}

#[test]
fn expression_worker_uses_string_and_numeric_element_access_literals() {
    let source = "class Names {\n\
                    static 'x-y' = 1;\n\
                    static 42 = 2;\n\
                  }";
    with_builder_files(
        &[("/project/main.ts", source)],
        0,
        EmitNodeBuilderFlags::NONE,
        EmitInternalNodeBuilderFlags::NONE,
        |checker, arena, target, context| {
            let properties = declarations_of_kind(checker, 0, SyntaxKind::PropertyDeclaration);
            assert_eq!(properties.len(), 2);
            let symbols = properties
                .iter()
                .map(|&declaration| {
                    checker
                        .binder
                        .node_symbol(declaration)
                        .expect("property symbol")
                })
                .collect::<Vec<_>>();
            let string_access = chains_symbol_to_expression(
                checker,
                arena,
                target,
                context,
                symbols[0],
                EmitSymbolMeaning::VALUE_EXPORT_VALUE,
            )?;
            let numeric_access = chains_symbol_to_expression(
                checker,
                arena,
                target,
                context,
                symbols[1],
                EmitSymbolMeaning::VALUE_EXPORT_VALUE,
            )?;
            for (access, expected) in [
                (string_access, SyntaxKind::StringLiteral),
                (numeric_access, SyntaxKind::NumericLiteral),
            ] {
                let NodeData::ElementAccessExpression(data) =
                    &arena.node(access).map_err(factory_error)?.data
                else {
                    panic!("element access expected")
                };
                let argument = child(arena, access, data.argument_expression);
                assert_eq!(arena.node(argument).map_err(factory_error)?.kind, expected);
            }
            Ok(())
        },
    );
}

#[test]
fn set_text_range2_replaces_provenance_before_copying_the_range() {
    let source = "type First = string; type Second = number;";
    with_builder_files(
        &[("/project/main.ts", source)],
        0,
        EmitNodeBuilderFlags::NONE,
        EmitInternalNodeBuilderFlags::NONE,
        |checker, arena, target, context| {
            let aliases = declarations_of_kind(checker, 0, SyntaxKind::TypeAliasDeclaration);
            assert_eq!(aliases.len(), 2);
            let names = aliases
                .iter()
                .map(|&declaration| {
                    node_util::get_name_of_declaration(
                        checker.binder.source_of_node(declaration),
                        declaration,
                    )
                    .expect("alias name")
                })
                .collect::<Vec<_>>();
            let first = project_parse_node(checker, arena, names[0])?.expect("first alias mounted");
            let second =
                project_parse_node(checker, arena, names[1])?.expect("second alias mounted");
            let range = create_identifier(arena, target, "synthetic")?;
            arena
                .set_original_node(range, Some(second))
                .map_err(factory_error)?;

            let result = set_text_range2(checker, arena, context, range, Some(first))?;
            assert_eq!(result, range, "same-source synthesized node is reused");
            assert_eq!(
                arena
                    .metadata(result)
                    .and_then(|metadata| metadata.original()),
                Some(first)
            );
            assert_eq!(
                arena
                    .parse_tree_resolver_node(result)
                    .map_err(factory_error)?,
                Some(enclosing_resolver_node(checker, names[0]))
            );
            Ok(())
        },
    );
}

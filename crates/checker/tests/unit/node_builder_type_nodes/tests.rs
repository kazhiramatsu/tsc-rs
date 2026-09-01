use tsc_emitter::{EmitNodeBuilderFlags, SourceFileId, TransformArena};
use tsc_syntax::{Node, NodeData, NodeId, SyntaxKind};
use tsc_types::{CompilerOptions, ElementFlags, ObjectFlags, PseudoBigInt, TypeData, TypeFlags};

use crate::state::test_support::with_program_state;

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

fn alias_rhs_nodes(checker: &CheckerState<'_>) -> Vec<NodeId> {
    let root = checker.binder.source(0).root;
    let NodeData::SourceFile(data) = checker.data_of(root) else {
        unreachable!()
    };
    checker
        .nodes_of(data.statements)
        .into_iter()
        .filter_map(|statement| match checker.data_of(statement) {
            NodeData::TypeAliasDeclaration(data) => data.r#type,
            _ => None,
        })
        .collect()
}

#[test]
fn primitive_and_literal_arms_preserve_shapes_and_upstream_length_deltas() {
    with_builder(
        "export {};",
        EmitNodeBuilderFlags::NONE,
        |checker, arena, target, context| {
            let primitive_cases = [
                (checker.tables.intrinsics.any, SyntaxKind::AnyKeyword, 3),
                (
                    checker.tables.intrinsics.string,
                    SyntaxKind::StringKeyword,
                    6,
                ),
                (
                    checker.tables.intrinsics.number,
                    SyntaxKind::NumberKeyword,
                    6,
                ),
                (
                    checker.tables.intrinsics.bigint,
                    SyntaxKind::BigIntKeyword,
                    6,
                ),
                (checker.tables.intrinsics.void, SyntaxKind::VoidKeyword, 4),
            ];
            for (r#type, expected_kind, expected_delta) in primitive_cases {
                let before = context.approximate_length;
                let node = type_to_type_node_helper(checker, arena, target, r#type, context)?
                    .expect("primitive node");
                assert_eq!(arena.node(node).map_err(factory_error)?.kind, expected_kind);
                assert_eq!(context.approximate_length - before, expected_delta);
            }

            let regular_string = checker.tables.get_string_literal_type("hi");
            let fresh_string = checker
                .tables
                .get_fresh_type_of_literal_type(regular_string);
            let negative_number = checker.tables.get_number_literal_type(-12.0);
            let negative_bigint = checker.tables.get_bigint_literal_type(PseudoBigInt {
                negative: true,
                base10_value: "42".to_owned(),
            });
            let true_type = checker.tables.intrinsics.true_regular;
            let literal_cases = [
                (fresh_string, SyntaxKind::StringLiteral, "hi", 4),
                (negative_number, SyntaxKind::PrefixUnaryExpression, "", 3),
                (negative_bigint, SyntaxKind::BigIntLiteral, "-42n", 4),
                (true_type, SyntaxKind::TrueKeyword, "", 4),
            ];
            for (r#type, expected_literal_kind, expected_text, expected_delta) in literal_cases {
                let before = context.approximate_length;
                let node = type_to_type_node_helper(checker, arena, target, r#type, context)?
                    .expect("literal node");
                let NodeData::LiteralType(data) = &arena.node(node).map_err(factory_error)?.data
                else {
                    panic!("literal type expected")
                };
                let literal = emitted_node(arena, target, data.literal.expect("literal payload"));
                assert_eq!(literal.kind, expected_literal_kind);
                match &literal.data {
                    NodeData::StringLiteral(data) => assert_eq!(data.text, expected_text),
                    NodeData::BigIntLiteral(data) => assert_eq!(data.text, expected_text),
                    NodeData::PrefixUnaryExpression(data) => {
                        assert_eq!(data.operator, SyntaxKind::MinusToken);
                        let operand = emitted_node(arena, target, data.operand.expect("operand"));
                        assert!(
                            matches!(&operand.data, NodeData::NumericLiteral(data) if data.text == "12")
                        );
                    }
                    NodeData::Token => {}
                    other => panic!("unexpected literal payload: {other:?}"),
                }
                assert_eq!(context.approximate_length - before, expected_delta);
            }
            Ok(())
        },
    );
}

#[test]
fn union_and_intersection_arms_emit_plain_constituent_lists() {
    with_builder(
        "export {};",
        EmitNodeBuilderFlags::NONE,
        |checker, arena, target, context| {
            let string = checker.tables.intrinsics.string;
            let number = checker.tables.intrinsics.number;
            let union = checker.tables.create_type(
                TypeFlags::UNION,
                TypeData::Union {
                    types: vec![string, number].into_boxed_slice(),
                    origin: None,
                },
            );
            let intersection = checker.tables.create_intersection_type(
                vec![string, number],
                ObjectFlags::from_bits(0),
                None,
                None,
            );
            for (r#type, expected) in [
                (union, SyntaxKind::UnionType),
                (intersection, SyntaxKind::IntersectionType),
            ] {
                let node = type_to_type_node_helper(checker, arena, target, r#type, context)?
                    .expect("list node");
                assert_eq!(arena.node(node).map_err(factory_error)?.kind, expected);
                let types = match &arena.node(node).map_err(factory_error)?.data {
                    NodeData::UnionType(data) => data.types,
                    NodeData::IntersectionType(data) => data.types,
                    _ => unreachable!(),
                };
                assert_eq!(emitted_array(arena, target, types.expect("types")).len(), 2);
            }
            Ok(())
        },
    );
}

#[test]
fn array_and_tuple_arms_cover_readonly_optional_and_rest_elements() {
    let source = "interface Array<T> {}\ninterface ReadonlyArray<T> {}";
    with_builder(
        source,
        EmitNodeBuilderFlags::NONE,
        |checker, arena, target, context| {
            let string = checker.tables.intrinsics.string;
            let array = checker
                .create_array_type(string, false)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            let readonly_array = checker
                .create_array_type(string, true)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            let array_node = type_to_type_node_helper(checker, arena, target, array, context)?
                .expect("array node");
            assert_eq!(
                arena.node(array_node).map_err(factory_error)?.kind,
                SyntaxKind::ArrayType
            );
            let readonly_node =
                type_to_type_node_helper(checker, arena, target, readonly_array, context)?
                    .expect("readonly array node");
            assert!(matches!(
                &arena.node(readonly_node).map_err(factory_error)?.data,
                NodeData::TypeOperator(data) if data.operator == SyntaxKind::ReadonlyKeyword
            ));

            let tuple = checker
                .create_tuple_type_forced(
                    &[
                        string,
                        checker.tables.intrinsics.number,
                        checker.tables.intrinsics.boolean,
                    ],
                    Some(&[
                        ElementFlags::REQUIRED,
                        ElementFlags::OPTIONAL,
                        ElementFlags::REST,
                    ]),
                    false,
                    None,
                )
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            let tuple_node = type_to_type_node_helper(checker, arena, target, tuple, context)?
                .expect("tuple node");
            let NodeData::TupleType(data) = &arena.node(tuple_node).map_err(factory_error)?.data
            else {
                panic!("tuple node expected")
            };
            let elements = emitted_array(arena, target, data.elements.expect("tuple elements"));
            assert_eq!(elements.len(), 3);
            assert_eq!(
                emitted_node(arena, target, elements[1]).kind,
                SyntaxKind::OptionalType
            );
            assert_eq!(
                emitted_node(arena, target, elements[2]).kind,
                SyntaxKind::RestType
            );
            Ok(())
        },
    );
}

#[test]
fn mapped_conditional_and_this_type_arms_emit_semantic_shapes() {
    let source = "type M<T> = { readonly [K in keyof T]?: T[K] };\n\
                  type C<T> = T extends string ? number : bigint;";
    with_builder(
        source,
        EmitNodeBuilderFlags::GENERATE_NAMES_FOR_SHADOWED_TYPE_PARAMS,
        |checker, arena, target, context| {
            let rhs = alias_rhs_nodes(checker);
            assert_eq!(rhs.len(), 2);
            for (node, expected) in rhs
                .into_iter()
                .zip([SyntaxKind::MappedType, SyntaxKind::ConditionalType])
            {
                let r#type = checker
                    .get_type_from_type_node(node)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?;
                checker.tables.type_mut(r#type).alias_symbol = None;
                checker.tables.type_mut(r#type).alias_type_arguments = None;
                let emitted = type_to_type_node_helper(checker, arena, target, r#type, context)?
                    .expect("mapped/conditional node");
                assert_eq!(arena.node(emitted).map_err(factory_error)?.kind, expected);
            }

            let this_type = checker.tables.create_type(
                TypeFlags::TYPE_PARAMETER,
                TypeData::TypeParameter {
                    is_this_type: true,
                    constraint: None,
                },
            );
            let this_node = type_to_type_node_helper(checker, arena, target, this_type, context)?
                .expect("this type node");
            assert_eq!(
                arena.node(this_node).map_err(factory_error)?.kind,
                SyntaxKind::ThisType
            );
            Ok(())
        },
    );
}

#[test]
fn object_index_info_arm_emits_readonly_index_signature() {
    let source = "type Indexed = { readonly [key: string]: number };";
    with_builder(
        source,
        EmitNodeBuilderFlags::NONE,
        |checker, arena, target, context| {
            let rhs = alias_rhs_nodes(checker);
            let indexed = checker
                .get_type_from_type_node(rhs[0])
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            checker.tables.type_mut(indexed).alias_symbol = None;
            checker.tables.type_mut(indexed).alias_type_arguments = None;
            let node = type_to_type_node_helper(checker, arena, target, indexed, context)?
                .expect("indexed type literal");
            let NodeData::TypeLiteral(data) = &arena.node(node).map_err(factory_error)?.data else {
                panic!("type literal expected")
            };
            let members = emitted_array(arena, target, data.members.expect("members"));
            assert_eq!(members.len(), 1);
            let NodeData::IndexSignature(data) = &emitted_node(arena, target, members[0]).data
            else {
                panic!("index signature expected")
            };
            let modifiers = emitted_array(arena, target, data.modifiers.expect("readonly"));
            assert_eq!(
                emitted_node(arena, target, modifiers[0]).kind,
                SyntaxKind::ReadonlyKeyword
            );
            Ok(())
        },
    );
}

#[test]
fn empty_anonymous_object_emits_an_empty_type_literal() {
    with_builder(
        "export {};",
        EmitNodeBuilderFlags::NONE,
        |checker, arena, target, context| {
            let anonymous = checker.create_resolved_empty_anonymous_type(None);
            let node = type_to_type_node_helper(checker, arena, target, anonymous, context)?
                .expect("anonymous type literal");
            let NodeData::TypeLiteral(data) = &arena.node(node).map_err(factory_error)?.data else {
                panic!("type literal expected")
            };
            assert!(data
                .members
                .map(|members| emitted_array(arena, target, members).is_empty())
                .unwrap_or(true));
            Ok(())
        },
    );
}

#[test]
fn bare_type_list_truncation_keeps_ends_and_marks_the_result() {
    with_builder(
        "export {};",
        EmitNodeBuilderFlags::NONE,
        |checker, arena, target, context| {
            context.max_truncation_length = 10;
            context.approximate_length = 11;
            let types = [
                checker.tables.intrinsics.string,
                checker.tables.intrinsics.number,
                checker.tables.intrinsics.bigint,
                checker.tables.intrinsics.boolean,
                checker.tables.intrinsics.unknown,
                checker.tables.intrinsics.void,
            ];
            let nodes = map_to_type_nodes(checker, arena, target, &types, context, true)?
                .expect("truncated bare list");
            assert_eq!(nodes.len(), 3);
            assert!(context.out.truncated);
            assert!(context.truncating);
            assert_eq!(
                arena.node(nodes[0]).map_err(factory_error)?.kind,
                SyntaxKind::StringKeyword
            );
            assert_eq!(
                arena.node(nodes[2]).map_err(factory_error)?.kind,
                SyntaxKind::VoidKeyword
            );
            Ok(())
        },
    );
}

#[test]
fn type_reference_and_typeof_faces_serialize_through_the_chains_cluster() {
    // Pre-lane-C this asserted the typed pending error; the chains
    // cluster now serializes both faces to real nodes.
    let source = "interface Box<T> {}\n\
                  function f(value: string): number { return 1; }\n\
                  type Reference = Box<string>;\n\
                  type Query = typeof f;";
    with_builder(
        source,
        EmitNodeBuilderFlags::USE_TYPE_OF_FUNCTION,
        |checker, arena, target, context| {
            let rhs = alias_rhs_nodes(checker);
            assert_eq!(rhs.len(), 2);
            let expected = [SyntaxKind::TypeReference, SyntaxKind::TypeQuery];
            for (rhs, expected_kind) in rhs.into_iter().zip(expected) {
                let r#type = checker
                    .get_type_from_type_node(rhs)
                    .map_err(|abort| checker_abort_error(checker, context, abort))?;
                let node = type_to_type_node_helper(checker, arena, target, r#type, context)?
                    .expect("chains cluster serializes the face");
                assert_eq!(arena.node(node).map_err(factory_error)?.kind, expected_kind,);
            }
            Ok(())
        },
    );
}

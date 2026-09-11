use std::collections::{HashMap, HashSet};

use tsc_emitter::{EmitNodeBuilderFlags, SourceFileId, TransformArena};
use tsc_syntax::{Node, NodeData, NodeId, SyntaxKind};
use tsc_types::{CompilerOptions, MapperId};

use crate::state::test_support::with_program_state;
use crate::state::SignatureKind;

use super::*;
use crate::node_builder::with_context;

#[test]
fn binding_name_clone_mounts_its_origin_and_remaps_children_to_another_source() {
    with_program_state(
        &[
            (
                "/input.ts",
                "function f({ value: [first = 1, ...rest] }) {}",
            ),
            ("/output.ts", "export {};"),
        ],
        &CompilerOptions::default(),
        |checker| {
            let input_root = checker.binder.source(0).root;
            let f = checker.binder.locals_of(input_root).unwrap()["f"];
            let declaration = checker.binder.symbol(f).value_declaration.unwrap();
            let parameter = checker.parameters_of_function(declaration)[0];
            let NodeData::Parameter(parameter) = checker.data_of(parameter) else {
                unreachable!()
            };
            let original_name = parameter.name.unwrap();
            let output_root = checker.binder.source(1).root;
            let mut arena = TransformArena::new();
            let target =
                arena.add_source(checker.binder.source(1), Some(SourceFileId::from_raw(1)));
            with_context(
                checker,
                &mut arena,
                target,
                Some(output_root),
                None,
                None,
                None,
                None,
                None,
                |checker, arena, target, context| {
                    let name = elide_initializer_and_set_emit_flags(
                        checker,
                        arena,
                        target,
                        original_name,
                        context,
                    )?;
                    assert_eq!(name.source(), target);
                    assert_eq!(
                        arena.node(name).unwrap().kind,
                        SyntaxKind::ObjectBindingPattern
                    );
                    let mut pending = vec![name.node()];
                    let mut count = 0;
                    while let Some(id) = pending.pop() {
                        let current = TransformNode::new(target, id);
                        let record = arena.node(current).unwrap();
                        assert_eq!((record.pos, record.end), (u32::MAX, u32::MAX));
                        let origin = arena.parse_tree_resolver_node(current).unwrap().unwrap();
                        assert_eq!(origin.source(), SourceFileId::from_raw(0));
                        assert_eq!(
                            arena.metadata(current).unwrap().flags(),
                            EmitFlags::SINGLE_LINE | EmitFlags::NO_ASCII_ESCAPING
                        );
                        if let NodeData::BindingElement(data) = &record.data {
                            assert!(data.initializer.is_none());
                        }
                        tsc_syntax::for_each_child(
                            &arena.source(target).unwrap().syntax().arena,
                            record,
                            |child| {
                                pending.push(child);
                                false
                            },
                        );
                        count += 1;
                    }
                    assert!(count > 6);
                    Ok(())
                },
                None,
            )
            .unwrap();
        },
    );
}

#[test]
fn binding_name_clone_propagates_an_unknown_target_as_a_factory_error() {
    with_builder(
        "function f({ value }) {}",
        EmitNodeBuilderFlags::NONE,
        |checker, arena, _, context| {
            let root = checker.binder.source(0).root;
            let f = checker.binder.locals_of(root).unwrap()["f"];
            let declaration = checker.binder.symbol(f).value_declaration.unwrap();
            let parameter = checker.parameters_of_function(declaration)[0];
            let NodeData::Parameter(parameter) = checker.data_of(parameter) else {
                unreachable!()
            };
            let mut other_arena = TransformArena::new();
            other_arena.add_source(checker.binder.source(0), None);
            let invalid = other_arena.add_source(checker.binder.source(0), None);
            let result = elide_initializer_and_set_emit_flags(
                checker,
                arena,
                invalid,
                parameter.name.unwrap(),
                context,
            );
            assert!(
                matches!(result, Err(EmitResolverError::Factory { error, .. }) if matches!(*error, tsc_emitter::TransformError::UnknownSource(source) if source == invalid))
            );
            Ok(())
        },
    );
}

#[test]
fn binding_name_clones_keep_origins_without_mapping_parsed_child_ranges() {
    with_builder(
        "function f({ value: [, first = 1, ...rest], [\"key\"]: renamed, 0: digit }) {}",
        EmitNodeBuilderFlags::NONE,
        |checker, arena, target, context| {
            let root = checker.binder.source(0).root;
            let f = checker.binder.locals_of(root).unwrap()["f"];
            let declaration = checker.binder.symbol(f).value_declaration.unwrap();
            let parameter = checker.parameters_of_function(declaration)[0];
            let NodeData::Parameter(parameter) = checker.data_of(parameter) else {
                unreachable!();
            };
            let original_name = parameter.name.unwrap();
            let original_pos = checker.binder.source(0).arena.node(original_name).pos;
            let name = elide_initializer_and_set_emit_flags(
                checker,
                arena,
                target,
                original_name,
                context,
            )?;
            let mut pending = vec![name.node()];
            let mut kinds = HashSet::new();
            while let Some(id) = pending.pop() {
                let current = TransformNode::new(target, id);
                let record = arena.node(current).unwrap();
                assert_eq!(
                    (record.pos, record.end),
                    (u32::MAX, u32::MAX),
                    "{:?}",
                    record.kind
                );
                assert_eq!(
                    arena.metadata(current).unwrap().flags(),
                    EmitFlags::SINGLE_LINE | EmitFlags::NO_ASCII_ESCAPING
                );
                assert!(arena.parse_tree_resolver_node(current).unwrap().is_some());
                if let NodeData::BindingElement(data) = &record.data {
                    assert!(data.initializer.is_none());
                }
                kinds.insert(record.kind);
                let syntax = arena.source(target).unwrap().syntax();
                tsc_syntax::for_each_child(&syntax.arena, record, |child| {
                    pending.push(child);
                    false
                });
            }
            for kind in [
                SyntaxKind::ObjectBindingPattern,
                SyntaxKind::ArrayBindingPattern,
                SyntaxKind::BindingElement,
                SyntaxKind::OmittedExpression,
                SyntaxKind::DotDotDotToken,
                SyntaxKind::ComputedPropertyName,
                SyntaxKind::StringLiteral,
                SyntaxKind::NumericLiteral,
            ] {
                assert!(kinds.contains(&kind), "{kind:?}");
            }
            assert_eq!(
                checker.binder.source(0).arena.node(original_name).pos,
                original_pos
            );
            assert_ne!(original_pos, u32::MAX);
            Ok(())
        },
    );
}

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
fn index_parameter_names_are_synthesized_from_declaration_text() {
    with_builder(
        r"type Indexed = { [\u0073lot: string]: number };",
        EmitNodeBuilderFlags::NONE,
        |checker, arena, target, context| {
            let rhs = first_alias_rhs(checker);
            let ty = checker
                .get_type_from_type_node(rhs)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            let infos = checker
                .get_index_infos_of_type(ty)
                .map_err(|abort| checker_abort_error(checker, context, abort))?;
            let node = index_info_to_index_signature_declaration_helper(
                checker, arena, target, &infos[0], context, None,
            )?;
            let NodeData::IndexSignature(data) = &arena.node(node).map_err(factory_error)?.data
            else {
                panic!("index signature")
            };
            let parameters = emitted_array(arena, target, data.parameters.expect("parameters"));
            let NodeData::Parameter(data) = &emitted_node(arena, target, parameters[0]).data else {
                panic!("parameter")
            };
            let name = emitted_node(arena, target, data.name.expect("name"));
            let NodeData::Identifier(data) = &name.data else {
                panic!("identifier")
            };
            assert_eq!(data.text, r"\u0073lot");
            assert_eq!((name.pos, name.end), (u32::MAX, u32::MAX));
            Ok(())
        },
    );
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
            // Copy-on-write (:52692+): entering a scope arms the
            // mustCreate flags but leaves the tables live; the next
            // write under an armed flag clones.
            assert!(context.must_create_type_parameter_symbol_list);
            assert_eq!(
                context.type_parameter_symbol_list.as_ref(),
                Some(&HashSet::from([SymbolId(11)]))
            );
            assert!(context.must_create_type_parameters_names_lookups);
            assert!(context.type_parameter_names.is_some());
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

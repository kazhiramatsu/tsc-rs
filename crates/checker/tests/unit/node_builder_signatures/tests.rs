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

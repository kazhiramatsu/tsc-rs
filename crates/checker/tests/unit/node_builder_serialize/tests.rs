use std::cell::RefCell;
use std::rc::Rc;

use tsc_emitter::{EmitFlags, EmitTrackerNode};
use tsc_types::CompilerOptions;

use crate::narrow::TypePredicateKind;
use crate::state::test_support::with_program_state;

use super::*;

#[derive(Clone, Default)]
struct RecordingTracker {
    events: Rc<RefCell<Vec<&'static str>>>,
}

impl EmitSymbolTracker for RecordingTracker {
    fn can_track_symbol(&self) -> bool {
        true
    }

    fn track_symbol(
        &mut self,
        _access: &mut dyn EmitTrackerAccess,
        _symbol: EmitTrackerSymbol,
        _symbol_flags: tsc_types::SymbolFlags,
        _enclosing_declaration: Option<EmitTrackerNode>,
        _meaning: EmitSymbolMeaning,
    ) -> Result<bool, EmitResolverError> {
        self.events.borrow_mut().push("track");
        Ok(false)
    }

    fn report_inference_fallback(
        &mut self,
        _access: &mut dyn EmitTrackerAccess,
        _node: EmitTrackerNode,
    ) -> Result<(), EmitResolverError> {
        self.events.borrow_mut().push("fallback");
        Ok(())
    }
}

fn root_statements(checker: &CheckerState<'_>) -> Vec<NodeId> {
    let root = checker.binder.source(0).root;
    match checker.data_of(root) {
        NodeData::SourceFile(data) => checker.nodes_of(data.statements),
        _ => Vec::new(),
    }
}

fn variable_declaration(checker: &CheckerState<'_>, name: &str) -> NodeId {
    for statement in root_statements(checker) {
        let NodeData::VariableStatement(statement) = checker.data_of(statement) else {
            continue;
        };
        let Some(list) = statement.declaration_list else {
            continue;
        };
        let NodeData::VariableDeclarationList(list) = checker.data_of(list) else {
            continue;
        };
        for declaration in checker.nodes_of(list.declarations) {
            let NodeData::VariableDeclaration(data) = checker.data_of(declaration) else {
                continue;
            };
            if data.name.and_then(|name| checker.identifier_text_of(name)) == Some(name) {
                return declaration;
            }
        }
    }
    panic!("variable declaration {name}")
}

fn function_declaration(checker: &CheckerState<'_>, name: &str) -> NodeId {
    root_statements(checker)
        .into_iter()
        .find(|&statement| {
            matches!(
                checker.data_of(statement),
                NodeData::FunctionDeclaration(data)
                    if data
                        .name
                        .and_then(|name_node| checker.identifier_text_of(name_node))
                        == Some(name)
            )
        })
        .unwrap_or_else(|| panic!("function declaration {name}"))
}

fn accessor_declaration(checker: &CheckerState<'_>, kind: SyntaxKind) -> NodeId {
    for statement in root_statements(checker) {
        let NodeData::ClassDeclaration(data) = checker.data_of(statement) else {
            continue;
        };
        if let Some(member) = checker
            .nodes_of(data.members)
            .into_iter()
            .find(|&member| checker.kind_of(member) == kind)
        {
            return member;
        }
    }
    panic!("accessor {kind:?}")
}

fn mounted_arena(checker: &CheckerState<'_>) -> (TransformArena, TransformSourceId) {
    let mut arena = TransformArena::new();
    let target = arena.add_source(
        checker.binder.source(0),
        Some(program_source_id(checker, 0)),
    );
    (arena, target)
}

fn kind(arena: &TransformArena, node: TransformNode) -> SyntaxKind {
    arena.node(node).expect("transform node").kind
}

#[test]
fn declaration_arms_reuse_annotations_consult_accessors_and_fallback_to_semantics() {
    let source = r#"
        declare function make(): { value: string };
        const annotated: { value: string } = make();
        const inferred = make();
        class C {
            get value(): string { return ""; }
            set value(value: string) {}
        }
    "#;
    with_program_state(
        &[("/main.ts", source)],
        &CompilerOptions::default(),
        |checker| {
            let root = checker.binder.source(0).root;
            let annotated = variable_declaration(checker, "annotated");
            let inferred = variable_declaration(checker, "inferred");
            let getter = accessor_declaration(checker, SyntaxKind::GetAccessor);
            let annotated_symbol = checker
                .get_symbol_of_declaration(annotated)
                .expect("annotated symbol");
            let inferred_symbol = checker
                .get_symbol_of_declaration(inferred)
                .expect("inferred symbol");
            let getter_symbol = checker
                .get_symbol_of_declaration(getter)
                .expect("getter symbol");
            let mut tracker = RecordingTracker::default();
            let events = Rc::clone(&tracker.events);
            let (mut arena, target) = mounted_arena(checker);
            let built = with_context(
                checker,
                &mut arena,
                target,
                Some(root),
                Some(EmitNodeBuilderFlags::NONE),
                None,
                Some(&mut tracker),
                None,
                None,
                |checker, arena, target, context| {
                    let annotated_type = checker
                        .get_type_of_symbol(annotated_symbol)
                        .map_err(|abort| checker_abort_error(checker, context, abort))?;
                    let inferred_type = checker
                        .get_type_of_symbol(inferred_symbol)
                        .map_err(|abort| checker_abort_error(checker, context, abort))?;
                    let getter_type = checker
                        .get_type_of_symbol(getter_symbol)
                        .map_err(|abort| checker_abort_error(checker, context, abort))?;
                    let annotated_node = serialize_type_for_declaration_in_context(
                        checker,
                        arena,
                        target,
                        context,
                        Some(annotated),
                        annotated_type,
                        Some(annotated_symbol),
                    )?
                    .expect("annotated type node");
                    let accessor_node = serialize_type_for_declaration_in_context(
                        checker,
                        arena,
                        target,
                        context,
                        Some(getter),
                        getter_type,
                        Some(getter_symbol),
                    )?
                    .expect("accessor type node");
                    events.borrow_mut().clear();
                    let inferred_node = serialize_type_for_declaration_in_context(
                        checker,
                        arena,
                        target,
                        context,
                        Some(inferred),
                        inferred_type,
                        Some(inferred_symbol),
                    )?
                    .expect("inferred type node");
                    Ok((annotated_node, inferred_node, accessor_node))
                },
                None,
            )
            .expect("serialization succeeds")
            .expect("context succeeds");

            assert_eq!(kind(&arena, built.0), SyntaxKind::TypeLiteral);
            assert_eq!(kind(&arena, built.1), SyntaxKind::TypeLiteral);
            assert_eq!(kind(&arena, built.2), SyntaxKind::StringKeyword);
            assert!(arena
                .metadata(built.0)
                .and_then(tsc_emitter::EmitMetadata::original)
                .is_some());
            assert_eq!(events.borrow().first().copied(), Some("fallback"));
        },
    );
}

#[test]
fn inferred_declaration_gate_declines_synthesized_and_widening_nodes() {
    assert!(should_use_syntactic_inferred_declaration(
        true, false, false
    ));
    assert!(!should_use_syntactic_inferred_declaration(
        true, true, false
    ));
    assert!(!should_use_syntactic_inferred_declaration(
        true, false, true
    ));
    assert!(!should_use_syntactic_inferred_declaration(
        false, false, false
    ));
}

#[test]
fn initialized_parameter_before_required_parameter_adds_undefined_union() {
    let options = CompilerOptions {
        strict_null_checks: Some(true),
        ..CompilerOptions::default()
    };
    with_program_state(
        &[("/main.ts", "function f(value = 1, required: string) {}")],
        &options,
        |checker| {
            let root = checker.binder.source(0).root;
            let function = function_declaration(checker, "f");
            let parameter = checker.parameters_of_function(function)[0];
            assert!(checker
                .emit_requires_adding_implicit_undefined(parameter, Some(root))
                .expect("implicit undefined query"));
            let symbol = checker
                .get_symbol_of_declaration(parameter)
                .expect("parameter symbol");
            let parameter_type = checker.get_type_of_symbol(symbol).expect("parameter type");
            let (mut arena, target) = mounted_arena(checker);
            let built = with_context(
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
                    serialize_type_for_declaration_in_context(
                        checker,
                        arena,
                        target,
                        context,
                        Some(parameter),
                        parameter_type,
                        Some(symbol),
                    )
                },
                None,
            )
            .expect("serialization succeeds")
            .flatten()
            .expect("type node");
            let NodeData::UnionType(data) = &arena.node(built).expect("union").data else {
                panic!("undefined composition must be a union")
            };
            let types = arena
                .source(built.source())
                .expect("source")
                .syntax()
                .arena
                .node_array(data.types.expect("union types"));
            assert!(types.nodes.iter().any(|&node| {
                arena
                    .node_ref(built.source(), node)
                    .is_some_and(|node| kind(&arena, node) == SyntaxKind::UndefinedKeyword)
            }));
        },
    );
}

#[test]
fn suppress_any_return_type_skips_node_and_restores_the_flag() {
    with_program_state(
        &[("/main.ts", "declare function f(): any;")],
        &CompilerOptions::default(),
        |checker| {
            let root = checker.binder.source(0).root;
            let declaration = function_declaration(checker, "f");
            let signature = checker
                .get_signature_from_declaration(declaration)
                .expect("signature");
            let (mut arena, target) = mounted_arena(checker);
            let (node, restored) = with_context(
                checker,
                &mut arena,
                target,
                Some(root),
                Some(EmitNodeBuilderFlags::SUPPRESS_ANY_RETURN_TYPE),
                None,
                None,
                None,
                None,
                |checker, arena, target, context| {
                    let node = serialize_return_type_for_signature_in_context(
                        checker, arena, target, context, signature,
                    )?;
                    Ok((
                        node,
                        context
                            .flags
                            .contains(EmitNodeBuilderFlags::SUPPRESS_ANY_RETURN_TYPE),
                    ))
                },
                None,
            )
            .expect("serialization succeeds")
            .expect("context succeeds");
            assert!(node.is_none());
            assert!(restored);
        },
    );
}

#[test]
fn front_doors_preserve_flags_and_build_real_node_shapes() {
    let source = "declare function f(): string; const annotated: { value: string } = { value: '' }; const n = 1;";
    with_program_state(
        &[("/main.ts", source)],
        &CompilerOptions::default(),
        |checker| {
            let root = checker.binder.source(0).root;
            let annotated = variable_declaration(checker, "annotated");
            let symbol = checker
                .get_symbol_of_declaration(annotated)
                .expect("symbol");
            let numeric = variable_declaration(checker, "n");
            let numeric_symbol = checker
                .get_symbol_of_declaration(numeric)
                .expect("numeric symbol");
            let function = function_declaration(checker, "f");
            let initializer = match checker.data_of(numeric) {
                NodeData::VariableDeclaration(data) => data.initializer.expect("initializer"),
                _ => unreachable!(),
            };
            let (mut arena, target) = mounted_arena(checker);
            let annotation = serialize_type_for_declaration(
                checker,
                &mut arena,
                target,
                annotated,
                symbol,
                Some(root),
                Some(EmitNodeBuilderFlags::NONE),
                None,
                None,
                None,
            )
            .expect("declaration front door")
            .expect("annotation node");
            assert_eq!(kind(&arena, annotation), SyntaxKind::TypeLiteral);
            assert!(arena
                .metadata(annotation)
                .is_some_and(|metadata| metadata.flags().contains(EmitFlags::SINGLE_LINE)));

            let inferred = serialize_type_for_declaration(
                checker,
                &mut arena,
                target,
                numeric,
                numeric_symbol,
                Some(root),
                Some(EmitNodeBuilderFlags::NONE),
                None,
                None,
                None,
            )
            .expect("inferred declaration front door")
            .expect("inferred declaration node");
            assert!(matches!(
                kind(&arena, inferred),
                SyntaxKind::LiteralType | SyntaxKind::NumberKeyword
            ));

            let return_type = serialize_return_type_for_signature(
                checker,
                &mut arena,
                target,
                function,
                Some(root),
                Some(EmitNodeBuilderFlags::NONE),
                None,
                None,
            )
            .expect("return-type front door")
            .expect("return-type node");
            assert_eq!(kind(&arena, return_type), SyntaxKind::StringKeyword);

            let number = type_to_type_node(
                checker,
                &mut arena,
                target,
                checker.tables.intrinsics.number,
                Some(root),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .expect("type front door")
            .expect("number node");
            assert_eq!(kind(&arena, number), SyntaxKind::NumberKeyword);

            let expression = serialize_type_for_expression(
                checker,
                &mut arena,
                target,
                initializer,
                Some(root),
                None,
                None,
                None,
            )
            .expect("expression front door")
            .expect("expression type");
            assert!(matches!(
                kind(&arena, expression),
                SyntaxKind::LiteralType | SyntaxKind::NumberKeyword
            ));

            let predicate = TypePredicate {
                kind: TypePredicateKind::Identifier,
                parameter_name: Some("value".to_owned()),
                parameter_index: 0,
                ty: Some(checker.tables.intrinsics.string),
            };
            let predicate = type_predicate_to_type_predicate_node(
                checker,
                &mut arena,
                target,
                &predicate,
                Some(root),
                None,
                None,
                None,
            )
            .expect("predicate front door")
            .expect("predicate node");
            assert_eq!(kind(&arena, predicate), SyntaxKind::TypePredicate);

            let index = IndexInfo {
                key_type: checker.tables.intrinsics.string,
                value_type: checker.tables.intrinsics.number,
                is_readonly: false,
                declaration: None,
                components: None,
                is_enum_number_index_info: false,
            };
            let index = index_info_to_index_signature_declaration(
                checker,
                &mut arena,
                target,
                &index,
                Some(root),
                None,
                None,
                None,
            )
            .expect("index front door")
            .expect("index node");
            assert_eq!(kind(&arena, index), SyntaxKind::IndexSignature);
        },
    );
}

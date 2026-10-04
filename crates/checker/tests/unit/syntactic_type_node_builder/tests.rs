use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use tsc_emitter::{
    EmitFunctionProperty, EmitNodeBuilderFlags, EmitSymbolAccessibility,
    EmitSymbolAccessibilityResult, EmitSymbolMeaning, EmitSymbolTracker, EmitTrackerAccess,
    EmitTrackerNode, EmitTrackerNodeDescription, EmitTrackerSymbol, EmitTrackerSymbolDescription,
    SourceFileId,
};

use crate::node_builder::{with_context, SyntacticScopeCleanup, SyntacticTrackedEntityName};
use crate::state::test_support::with_program_state;
use tsc_types::CompilerOptions;

use super::*;

#[derive(Clone)]
struct TestTracker {
    events: Rc<RefCell<Vec<String>>>,
}

impl EmitSymbolTracker for TestTracker {
    fn report_inference_fallback(
        &mut self,
        _access: &mut dyn EmitTrackerAccess,
        node: EmitTrackerNode,
    ) -> Result<(), EmitResolverError> {
        self.events.borrow_mut().push(format!("report:{}", node.0));
        Ok(())
    }
}

struct TestResolver {
    events: Rc<RefCell<Vec<String>>>,
    node_kinds: HashMap<TransformNode, SyntaxKind>,
    reuse_denied_kinds: HashSet<SyntaxKind>,
    track_error_kinds: HashSet<SyntaxKind>,
    existing_result: Option<SyntaxKind>,
}

impl TestResolver {
    fn new(events: Rc<RefCell<Vec<String>>>) -> Self {
        Self {
            events,
            node_kinds: HashMap::new(),
            reuse_denied_kinds: HashSet::new(),
            track_error_kinds: HashSet::new(),
            existing_result: Some(SyntaxKind::AnyKeyword),
        }
    }

    fn kind(arena: &TransformArena, node: TransformNode) -> Result<SyntaxKind, EmitResolverError> {
        arena
            .node(node)
            .map(|node| node.kind)
            .map_err(|error| EmitResolverError::Factory {
                method: EmitResolverMethod::CreateTypeOfDeclaration,
                error: Box::new(error),
            })
    }

    fn keyword(
        arena: &mut TransformArena,
        target: TransformSourceId,
        kind: Option<SyntaxKind>,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let Some(kind) = kind else {
            return Ok(None);
        };
        arena
            .factory()
            .create_token(target, kind, TransformFlags::CONTAINS_TYPE_SCRIPT)
            .map(Some)
            .map_err(|error| EmitResolverError::Factory {
                method: EmitResolverMethod::CreateTypeOfDeclaration,
                error: Box::new(error),
            })
    }
}

impl EmitTrackerAccess for TestResolver {
    fn is_child_of_bound_expando(
        &mut self,
        _node: EmitTrackerNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(false)
    }

    fn is_entity_in_type_node(
        &mut self,
        _node: tsc_emitter::EmitTrackerNode,
    ) -> Result<bool, tsc_emitter::EmitResolverError> {
        panic!("this mock does not classify entities in type nodes")
    }

    fn accessor_declarations(
        &mut self,
        _node: EmitTrackerNode,
    ) -> Result<tsc_emitter::EmitAccessorDeclarations, EmitResolverError> {
        panic!("this mock does not project accessor declarations")
    }

    fn parent_node(
        &mut self,
        _node: EmitTrackerNode,
    ) -> Result<Option<EmitTrackerNode>, EmitResolverError> {
        panic!("this mock does not project parent nodes")
    }

    fn is_symbol_accessible(
        &mut self,
        _symbol: EmitTrackerSymbol,
        _enclosing_declaration: Option<EmitTrackerNode>,
        _meaning: EmitSymbolMeaning,
        _should_compute_aliases: bool,
    ) -> Result<EmitSymbolAccessibilityResult, EmitResolverError> {
        Ok(EmitSymbolAccessibilityResult {
            accessibility: EmitSymbolAccessibility::Accessible,
            aliases_to_make_visible: None,
            error_symbol_name: None,
            error_module_name: None,
            error_node: None,
        })
    }

    fn is_expando_function_declaration(
        &mut self,
        _node: EmitTrackerNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(false)
    }

    fn get_properties_of_container_function(
        &mut self,
        _node: EmitTrackerNode,
    ) -> Result<Vec<EmitFunctionProperty>, EmitResolverError> {
        Ok(Vec::new())
    }

    fn requires_adding_implicit_undefined(
        &mut self,
        _parameter: EmitTrackerNode,
        _enclosing_declaration: Option<EmitTrackerNode>,
    ) -> Result<bool, EmitResolverError> {
        Ok(false)
    }

    fn describe_symbol(&mut self, _symbol: EmitTrackerSymbol) -> EmitTrackerSymbolDescription {
        EmitTrackerSymbolDescription::default()
    }

    fn describe_node(&mut self, _node: EmitTrackerNode) -> EmitTrackerNodeDescription {
        EmitTrackerNodeDescription::default()
    }
}

impl SyntacticBuilderResolver for TestResolver {
    fn has_late_bindable_name(
        &mut self,
        _arena: &mut tsc_emitter::TransformArena,
        _node: TransformNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(false)
    }

    fn should_remove_declaration(
        &mut self,
        _arena: &mut tsc_emitter::TransformArena,
        _context: &mut NodeBuilderContext<'_>,
        _node: TransformNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(false)
    }

    fn create_recovery_boundary(
        &mut self,
        _arena: &mut tsc_emitter::TransformArena,
        context: &mut NodeBuilderContext<'_>,
    ) -> Result<SyntacticRecoveryBoundary, EmitResolverError> {
        Ok(SyntacticRecoveryBoundary::new(context))
    }

    fn serialize_existing_type_node(
        &mut self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        _context: &mut NodeBuilderContext<'_>,
        type_node: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let kind = Self::kind(arena, type_node)?;
        self.events
            .borrow_mut()
            .push(format!("semantic-existing:{kind:?}"));
        Self::keyword(arena, target, self.existing_result)
    }

    fn serialize_type_name(
        &mut self,
        _arena: &mut TransformArena,
        _target: TransformSourceId,
        _context: &mut NodeBuilderContext<'_>,
        node: TransformNode,
        _is_type_of: bool,
        _type_arguments: Option<TransformNodeArray>,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        Ok(Some(node))
    }

    fn enter_new_scope(
        &mut self,
        _arena: &mut TransformArena,
        _target: TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        node: TransformNode,
    ) -> Result<SyntacticScopeCleanup, EmitResolverError> {
        let cleanup = SyntacticScopeCleanup::capture(context);
        context.enclosing_declaration = Some(node.node());
        Ok(cleanup)
    }

    fn mark_node_reuse(
        &mut self,
        arena: &mut TransformArena,
        context: &mut NodeBuilderContext<'_>,
        range: TransformNode,
        location: TransformNode,
    ) -> Result<TransformNode, EmitResolverError> {
        let synthesized = NodeFlags::from_bits(
            arena
                .node(range)
                .map_err(|error| EmitResolverError::Factory {
                    method: EmitResolverMethod::CreateTypeOfDeclaration,
                    error: Box::new(error),
                })?
                .flags,
        )
        .intersects(NodeFlags::SYNTHESIZED);
        let mut result = range;
        if !synthesized {
            result =
                arena
                    .factory()
                    .clone_node(range)
                    .map_err(|error| EmitResolverError::Factory {
                        method: EmitResolverMethod::CreateTypeOfDeclaration,
                        error: Box::new(error),
                    })?;
        }
        if result != location {
            arena
                .set_original_node(result, Some(location))
                .map_err(|error| EmitResolverError::Factory {
                    method: EmitResolverMethod::CreateTypeOfDeclaration,
                    error: Box::new(error),
                })?;
        }
        if context.enclosing_file.is_some() {
            arena
                .factory()
                .set_text_range(result, location)
                .map_err(|error| EmitResolverError::Factory {
                    method: EmitResolverMethod::CreateTypeOfDeclaration,
                    error: Box::new(error),
                })?;
        }
        Ok(result)
    }

    fn track_existing_entity_name(
        &mut self,
        arena: &mut TransformArena,
        _target: TransformSourceId,
        _context: &mut NodeBuilderContext<'_>,
        node: TransformNode,
    ) -> Result<SyntacticTrackedEntityName, EmitResolverError> {
        let kind = Self::kind(arena, node)?;
        self.events.borrow_mut().push(format!("track:{kind:?}"));
        Ok(SyntacticTrackedEntityName {
            node,
            introduces_error: self.track_error_kinds.contains(&kind),
        })
    }

    fn get_module_specifier_override(
        &mut self,
        _arena: &mut tsc_emitter::TransformArena,
        _context: &mut NodeBuilderContext<'_>,
        _parent: TransformNode,
        _literal: TransformNode,
    ) -> Result<Option<tsc_types::JsString>, EmitResolverError> {
        Ok(None)
    }

    fn can_reuse_type_node(
        &mut self,
        _arena: &mut tsc_emitter::TransformArena,
        _context: &mut NodeBuilderContext<'_>,
        type_node: TransformNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(!self
            .reuse_denied_kinds
            .contains(&self.kind_for_test(type_node).unwrap_or(SyntaxKind::Unknown)))
    }
}

impl TestResolver {
    // Filled after the test source is mounted; callbacks without an arena
    // parameter use this immutable parse-node projection.
    fn kind_for_test(&self, node: TransformNode) -> Option<SyntaxKind> {
        self.node_kinds.get(&node).copied()
    }
}

fn with_case(
    file_name: &str,
    source_text: &str,
    options: &CompilerOptions,
    resolver: &mut TestResolver,
    tracker: &mut TestTracker,
    run: impl FnOnce(
        &mut crate::state::CheckerState<'_>,
        &mut TransformArena,
        TransformSourceId,
        &mut NodeBuilderContext<'_>,
        &mut TestResolver,
    ) -> Result<(), EmitResolverError>,
) {
    with_program_state(&[(file_name, source_text)], options, |checker| {
        let root = checker.binder.source(0).root;
        let mut arena = TransformArena::new();
        let target = arena.add_source(checker.binder.source(0), Some(SourceFileId::from_raw(0)));
        for node in checker.binder.source(0).arena.node_ids() {
            if let Some(transform) = arena.node_ref(target, node) {
                resolver
                    .node_kinds
                    .insert(transform, checker.binder.source(0).arena.node(node).kind);
            }
        }
        let result = with_context(
            checker,
            &mut arena,
            target,
            Some(root),
            Some(EmitNodeBuilderFlags::NONE),
            None,
            Some(tracker),
            None,
            None,
            |checker, arena, target, context| run(checker, arena, target, context, resolver),
            None,
        )
        .expect("test context succeeds");
        assert_eq!(result, Some(()));
    });
}

fn find_transform_node(
    checker: &crate::state::CheckerState<'_>,
    arena: &TransformArena,
    target: TransformSourceId,
    kind: SyntaxKind,
    index: usize,
) -> TransformNode {
    let node = checker
        .binder
        .source(0)
        .arena
        .node_ids()
        .filter(|&node| checker.binder.source(0).arena.node(node).kind == kind)
        .nth(index)
        .expect("requested syntax node");
    arena.node_ref(target, node).expect("mounted syntax node")
}

#[test]
fn syntactic_annotation_reuse_round_trips_parse_provenance_and_length() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut resolver = TestResolver::new(Rc::clone(&events));
    let mut tracker = TestTracker {
        events: Rc::clone(&events),
    };
    let options = CompilerOptions::default();
    with_case(
        "/main.ts",
        "let value: Box<string>;\n",
        &options,
        &mut resolver,
        &mut tracker,
        |checker, arena, target, context, resolver| {
            let annotation =
                find_transform_node(checker, arena, target, SyntaxKind::TypeReference, 0);
            let original = arena
                .parse_tree_resolver_node(annotation)
                .map_err(|error| EmitResolverError::Factory {
                    method: EmitResolverMethod::CreateTypeOfDeclaration,
                    error: Box::new(error),
                })?;
            let record = arena
                .node(annotation)
                .map_err(|error| EmitResolverError::Factory {
                    method: EmitResolverMethod::CreateTypeOfDeclaration,
                    error: Box::new(error),
                })?;
            let expected_length = record.end - record.pos;
            let builder = SyntacticTypeNodeBuilder;
            let result = builder
                .try_reuse_existing_type_node(resolver, arena, target, context, annotation)?
                .expect("annotation reused");
            let round_trip = arena.parse_tree_resolver_node(result).map_err(|error| {
                EmitResolverError::Factory {
                    method: EmitResolverMethod::CreateTypeOfDeclaration,
                    error: Box::new(error),
                }
            })?;
            assert_eq!(round_trip, original);
            assert_eq!(context.approximate_length, expected_length);
            Ok(())
        },
    );
}

#[test]
fn syntactic_recovery_scope_contains_error_and_consults_semantic_serializer() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut resolver = TestResolver::new(Rc::clone(&events));
    resolver.reuse_denied_kinds.insert(SyntaxKind::ThisType);
    let mut tracker = TestTracker {
        events: Rc::clone(&events),
    };
    let options = CompilerOptions::default();
    with_case(
        "/main.ts",
        "let value: Box<this>;\n",
        &options,
        &mut resolver,
        &mut tracker,
        |checker, arena, target, context, resolver| {
            let annotation =
                find_transform_node(checker, arena, target, SyntaxKind::TypeReference, 0);
            let builder = SyntacticTypeNodeBuilder;
            assert!(builder
                .try_reuse_existing_type_node(resolver, arena, target, context, annotation)?
                .is_some());
            assert!(events
                .borrow()
                .iter()
                .any(|event| event == "semantic-existing:ThisType"));
            assert!(!context.recovery_boundary_had_error);
            assert_eq!(context.recovery_boundary_depth, 0);
            Ok(())
        },
    );
}

#[test]
fn syntactic_computed_name_error_marks_the_boundary_without_rewriting() {
    // tsgo marks an error for a computed property name whose entity name
    // introduces one and visits its children (nodecopy.go:751-759); strada's
    // rewriting from the evaluator and the checker is gone, so the enclosing
    // type literal falls back to its type.
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut resolver = TestResolver::new(Rc::clone(&events));
    resolver.track_error_kinds.insert(SyntaxKind::Identifier);
    let mut tracker = TestTracker {
        events: Rc::clone(&events),
    };
    let options = CompilerOptions::default();
    with_case(
        "/main.ts",
        "declare const key: unique symbol;\nlet value: { [key]: string };\n",
        &options,
        &mut resolver,
        &mut tracker,
        |checker, arena, target, context, resolver| {
            let literal = find_transform_node(checker, arena, target, SyntaxKind::TypeLiteral, 0);
            let builder = SyntacticTypeNodeBuilder;
            let result = builder
                .try_reuse_existing_type_node(resolver, arena, target, context, literal)?
                .expect("semantic fallback");
            assert_eq!(
                arena.node(result).expect("result node").kind,
                SyntaxKind::AnyKeyword
            );
            assert_eq!(
                *events.borrow(),
                ["track:Identifier", "semantic-existing:TypeLiteral"]
            );
            assert!(!context.recovery_boundary_had_error);
            assert_eq!(context.recovery_boundary_depth, 0);
            Ok(())
        },
    );
}

#[test]
fn syntactic_simple_visit_covers_keyof_typeof_and_indexed_access() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut resolver = TestResolver::new(Rc::clone(&events));
    let mut tracker = TestTracker {
        events: Rc::clone(&events),
    };
    let options = CompilerOptions::default();
    with_case(
        "/main.ts",
        "let value: keyof (typeof NS)[\"p\"];\n",
        &options,
        &mut resolver,
        &mut tracker,
        |checker, arena, target, context, resolver| {
            let annotation =
                find_transform_node(checker, arena, target, SyntaxKind::TypeOperator, 0);
            let builder = SyntacticTypeNodeBuilder;
            let result = builder
                .try_reuse_existing_type_node(resolver, arena, target, context, annotation)?
                .expect("simple type path reused");
            assert_eq!(
                arena.node(result).expect("result node").kind,
                SyntaxKind::TypeOperator
            );
            assert!(events
                .borrow()
                .iter()
                .any(|event| event == "track:Identifier"));
            assert!(!events
                .borrow()
                .iter()
                .any(|event| event.starts_with("semantic-existing")));
            Ok(())
        },
    );
}

#[test]
fn syntactic_jsdoc_type_literal_keeps_written_property_types() {
    // tsgo reparses a JSDoc type literal into a type literal whose
    // properties keep their written types (reparseJSDocTypeLiteral,
    // reparser.go:244-283): an optional `@property` stays `number` without
    // `| undefined`, and a name that is not an identifier is a string
    // literal.
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut resolver = TestResolver::new(Rc::clone(&events));
    let mut tracker = TestTracker {
        events: Rc::clone(&events),
    };
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        strict_null_checks: Some(true),
        ..CompilerOptions::default()
    };
    with_case(
        "/main.js",
        "/**\n * @typedef {Object} Box\n * @property {number} [value]\n * @property {string} data-name\n */\nconst value = {};\n",
        &options,
        &mut resolver,
        &mut tracker,
        |checker, arena, target, context, resolver| {
            let jsdoc =
                find_transform_node(checker, arena, target, SyntaxKind::JSDocTypeLiteral, 0);
            let builder = SyntacticTypeNodeBuilder;
            let result = builder
                .try_reuse_existing_type_node(resolver, arena, target, context, jsdoc)?
                .expect("JSDoc type literal reused");
            let NodeData::TypeLiteral(data) = &arena.node(result).expect("type literal").data
            else {
                panic!("expected type literal")
            };
            let members = arena
                .node_array_ref(result.source(), data.members.expect("members"))
                .expect("member array");
            let members = arena.node_array(members).expect("member records").nodes.to_vec();
            assert_eq!(members.len(), 2);
            let property = |index: usize| {
                let member = arena
                    .node_ref(result.source(), members[index])
                    .expect("property signature");
                let NodeData::PropertySignature(data) =
                    arena.node(member).expect("property signature record").data.clone()
                else {
                    panic!("expected property signature")
                };
                (member, data)
            };
            let (value, value_data) = property(0);
            assert!(value_data.question_token.is_some());
            let value_type = arena
                .node_ref(value.source(), value_data.r#type.expect("property type"))
                .expect("property type node");
            assert_eq!(
                arena
                    .node(value_type)
                    .expect("property type record")
                    .kind,
                SyntaxKind::NumberKeyword
            );
            let (name, name_data) = property(1);
            let name = arena
                .node_ref(name.source(), name_data.name.expect("property name"))
                .expect("property name node");
            let NodeData::StringLiteral(literal) = &arena.node(name).expect("name record").data
            else {
                panic!("expected a string literal name")
            };
            assert_eq!(literal.text, "data-name");
            Ok(())
        },
    );
}

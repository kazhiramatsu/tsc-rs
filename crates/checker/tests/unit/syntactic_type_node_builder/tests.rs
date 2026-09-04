use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use tsc_binder::SymbolId;
use tsc_emitter::{
    EmitFunctionProperty, EmitNodeBuilderFlags, EmitSymbolAccessibility,
    EmitSymbolAccessibilityResult, EmitSymbolMeaning, EmitSymbolTracker, EmitTrackerAccess,
    EmitTrackerNode, EmitTrackerNodeDescription, EmitTrackerSymbol, EmitTrackerSymbolDescription,
    SourceFileId,
};

use crate::evaluate::{evaluator_result, EvaluatorResult};
use crate::node_builder::{with_context, SyntacticScopeCleanup, SyntacticTrackedEntityName};
use crate::state::test_support::with_program_state;

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
    can_reuse_annotation: bool,
    reuse_denied_kinds: HashSet<SyntaxKind>,
    track_error_kinds: HashSet<SyntaxKind>,
    accessors: Option<SyntacticAccessorDeclarations>,
    declaration_result: Option<SyntaxKind>,
    expression_result: Option<SyntaxKind>,
    return_result: Option<SyntaxKind>,
    existing_result: Option<SyntaxKind>,
    jsdoc_override: Option<SyntaxKind>,
}

impl TestResolver {
    fn new(events: Rc<RefCell<Vec<String>>>) -> Self {
        Self {
            events,
            node_kinds: HashMap::new(),
            can_reuse_annotation: true,
            reuse_denied_kinds: HashSet::new(),
            track_error_kinds: HashSet::new(),
            accessors: None,
            declaration_result: Some(SyntaxKind::StringKeyword),
            expression_result: Some(SyntaxKind::NumberKeyword),
            return_result: Some(SyntaxKind::VoidKeyword),
            existing_result: Some(SyntaxKind::AnyKeyword),
            jsdoc_override: None,
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
    fn evaluate_entity_name_expression(
        &mut self,
        _arena: &mut tsc_emitter::TransformArena,
        _expression: TransformNode,
    ) -> Result<EvaluatorResult, EmitResolverError> {
        Ok(evaluator_result(None, false, false, false))
    }

    fn is_expando_function_declaration(
        &mut self,
        _arena: &mut tsc_emitter::TransformArena,
        _node: TransformNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(false)
    }

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

    fn is_definitely_reference_to_global_symbol_object(
        &mut self,
        _arena: &mut tsc_emitter::TransformArena,
        _node: TransformNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(false)
    }

    fn get_all_accessor_declarations(
        &mut self,
        _arena: &mut tsc_emitter::TransformArena,
        node: TransformNode,
    ) -> Result<SyntacticAccessorDeclarations, EmitResolverError> {
        Ok(self.accessors.unwrap_or(SyntacticAccessorDeclarations {
            first_accessor: node,
            second_accessor: None,
            get_accessor: (self.kind_for_test(node) == Some(SyntaxKind::GetAccessor))
                .then_some(node),
            set_accessor: (self.kind_for_test(node) == Some(SyntaxKind::SetAccessor))
                .then_some(node),
        }))
    }

    fn requires_adding_implicit_undefined(
        &mut self,
        _arena: &mut tsc_emitter::TransformArena,
        _declaration: TransformNode,
        _symbol: Option<SyntacticSymbol>,
        _enclosing_declaration: Option<NodeId>,
    ) -> Result<bool, EmitResolverError> {
        Ok(false)
    }

    fn is_optional_parameter(
        &mut self,
        _arena: &mut tsc_emitter::TransformArena,
        _parameter: TransformNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(false)
    }

    fn is_undefined_identifier_expression(
        &mut self,
        _arena: &mut tsc_emitter::TransformArena,
        _node: TransformNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(false)
    }

    fn is_entity_name_visible(
        &mut self,
        _arena: &mut tsc_emitter::TransformArena,
        _context: &mut NodeBuilderContext<'_>,
        _entity_name: TransformNode,
        _should_compute_aliases_to_make_visible: bool,
    ) -> Result<EmitSymbolAccessibilityResult, EmitResolverError> {
        Ok(EmitSymbolAccessibilityResult {
            accessibility: EmitSymbolAccessibility::Accessible,
            aliases_to_make_visible: None,
            error_symbol_name: None,
            error_module_name: None,
            error_node: None,
        })
    }

    fn serialize_existing_type_node(
        &mut self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        _context: &mut NodeBuilderContext<'_>,
        type_node: TransformNode,
        _add_undefined: bool,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let kind = Self::kind(arena, type_node)?;
        self.events
            .borrow_mut()
            .push(format!("semantic-existing:{kind:?}"));
        Self::keyword(arena, target, self.existing_result)
    }

    fn serialize_return_type_for_signature(
        &mut self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        _context: &mut NodeBuilderContext<'_>,
        _signature_declaration: TransformNode,
        _symbol: Option<SyntacticSymbol>,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        self.events.borrow_mut().push("infer-return".to_owned());
        Self::keyword(arena, target, self.return_result)
    }

    fn serialize_type_of_expression(
        &mut self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        _context: &mut NodeBuilderContext<'_>,
        _expression: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        self.events.borrow_mut().push("infer-expression".to_owned());
        Self::keyword(arena, target, self.expression_result)
    }

    fn serialize_type_of_declaration(
        &mut self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        _context: &mut NodeBuilderContext<'_>,
        _declaration: TransformNode,
        _symbol: Option<SyntacticSymbol>,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        self.events
            .borrow_mut()
            .push("infer-declaration".to_owned());
        Self::keyword(arena, target, self.declaration_result)
    }

    fn serialize_name_of_parameter(
        &mut self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        _context: &mut NodeBuilderContext<'_>,
        parameter: TransformNode,
    ) -> Result<TransformNode, EmitResolverError> {
        let name = match &arena
            .node(parameter)
            .map_err(|error| EmitResolverError::Factory {
                method: EmitResolverMethod::CreateTypeOfDeclaration,
                error: Box::new(error),
            })?
            .data
        {
            NodeData::Parameter(data) => data.name,
            _ => None,
        };
        if let Some(name) = name.and_then(|name| arena.node_ref(parameter.source(), name)) {
            return Ok(name);
        }
        arena
            .factory()
            .create_node(
                target,
                NodeData::Identifier(IdentifierData {
                    escaped_text: "arg".to_owned(),
                    text: "arg".to_owned(),
                }),
                TransformFlags::NONE,
            )
            .map_err(|error| EmitResolverError::Factory {
                method: EmitResolverMethod::CreateTypeOfDeclaration,
                error: Box::new(error),
            })
    }

    fn serialize_entity_name(
        &mut self,
        _arena: &mut TransformArena,
        _target: TransformSourceId,
        _context: &mut NodeBuilderContext<'_>,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        Ok(Some(node))
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

    fn get_js_doc_property_override(
        &mut self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        _context: &mut NodeBuilderContext<'_>,
        _js_doc_type_literal: TransformNode,
        _js_doc_property: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        self.events.borrow_mut().push("jsdoc-override".to_owned());
        Self::keyword(arena, target, self.jsdoc_override)
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

    fn track_computed_name(
        &mut self,
        _arena: &mut tsc_emitter::TransformArena,
        _context: &mut NodeBuilderContext<'_>,
        _access_expression: TransformNode,
    ) -> Result<(), EmitResolverError> {
        self.events.borrow_mut().push("track-computed".to_owned());
        Ok(())
    }

    fn get_module_specifier_override(
        &mut self,
        _arena: &mut tsc_emitter::TransformArena,
        _context: &mut NodeBuilderContext<'_>,
        _parent: TransformNode,
        _literal: TransformNode,
    ) -> Result<Option<String>, EmitResolverError> {
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

    fn can_reuse_type_node_annotation(
        &mut self,
        _arena: &mut tsc_emitter::TransformArena,
        _context: &mut NodeBuilderContext<'_>,
        _node: TransformNode,
        _existing: TransformNode,
        _symbol: Option<SyntacticSymbol>,
        _requires_adding_undefined: Option<bool>,
    ) -> Result<bool, EmitResolverError> {
        Ok(self.can_reuse_annotation)
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

fn find_identifier(
    checker: &crate::state::CheckerState<'_>,
    arena: &TransformArena,
    target: TransformSourceId,
    text: &str,
) -> TransformNode {
    let source = checker.binder.source(0);
    let node = source
        .arena
        .node_ids()
        .find(|&node| {
            matches!(
                &source.arena.node(node).data,
                NodeData::Identifier(data) if data.text == text
            )
        })
        .expect("requested identifier");
    arena.node_ref(target, node).expect("mounted identifier")
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
            let declaration =
                find_transform_node(checker, arena, target, SyntaxKind::VariableDeclaration, 0);
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
            let builder = SyntacticTypeNodeBuilder::new(&options);
            let result = builder
                .serialize_type_of_declaration(
                    resolver,
                    arena,
                    target,
                    context,
                    declaration,
                    Some(SyntacticSymbol {
                        id: SymbolId(0),
                        declaration_count: 1,
                        variable_declaration_count: 1,
                    }),
                )?
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
fn syntactic_expression_fallback_reports_before_resolver() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut resolver = TestResolver::new(Rc::clone(&events));
    let mut tracker = TestTracker {
        events: Rc::clone(&events),
    };
    let options = CompilerOptions::default();
    with_case(
        "/main.ts",
        "const value = unknownValue;\n",
        &options,
        &mut resolver,
        &mut tracker,
        |checker, arena, target, context, resolver| {
            let expression = find_identifier(checker, arena, target, "unknownValue");
            let builder = SyntacticTypeNodeBuilder::new(&options);
            let result = builder
                .serialize_type_of_expression(resolver, arena, target, context, expression)?
                .expect("semantic expression fallback");
            assert_eq!(
                arena.node(result).expect("result node").kind,
                SyntaxKind::NumberKeyword
            );
            let log = events.borrow();
            assert!(log[0].starts_with("report:"));
            assert_eq!(log[1], "infer-expression");
            Ok(())
        },
    );
}

#[test]
fn syntactic_declaration_fallback_reports_before_resolver() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut resolver = TestResolver::new(Rc::clone(&events));
    let mut tracker = TestTracker {
        events: Rc::clone(&events),
    };
    let options = CompilerOptions::default();
    with_case(
        "/main.ts",
        "const { value } = source;\n",
        &options,
        &mut resolver,
        &mut tracker,
        |checker, arena, target, context, resolver| {
            let declaration =
                find_transform_node(checker, arena, target, SyntaxKind::BindingElement, 0);
            let builder = SyntacticTypeNodeBuilder::new(&options);
            let result = builder
                .serialize_type_of_declaration(resolver, arena, target, context, declaration, None)?
                .expect("semantic declaration fallback");
            assert_eq!(
                arena.node(result).expect("result node").kind,
                SyntaxKind::StringKeyword
            );
            let log = events.borrow();
            assert!(log[0].starts_with("report:"));
            assert_eq!(log[1], "infer-declaration");
            Ok(())
        },
    );
}

#[test]
fn syntactic_return_fallback_reports_before_resolver() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut resolver = TestResolver::new(Rc::clone(&events));
    let mut tracker = TestTracker {
        events: Rc::clone(&events),
    };
    let options = CompilerOptions::default();
    with_case(
        "/main.ts",
        "function f() {}\n",
        &options,
        &mut resolver,
        &mut tracker,
        |checker, arena, target, context, resolver| {
            let signature =
                find_transform_node(checker, arena, target, SyntaxKind::FunctionDeclaration, 0);
            let builder = SyntacticTypeNodeBuilder::new(&options);
            let result = builder
                .serialize_return_type_for_signature(
                    resolver, arena, target, context, signature, None,
                )?
                .expect("semantic return fallback");
            assert_eq!(
                arena.node(result).expect("result node").kind,
                SyntaxKind::VoidKeyword
            );
            let log = events.borrow();
            assert!(log[0].starts_with("report:"));
            assert_eq!(log[1], "infer-return");
            Ok(())
        },
    );
}

#[test]
fn syntactic_accessor_fallback_reports_before_resolver() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut resolver = TestResolver::new(Rc::clone(&events));
    let mut tracker = TestTracker {
        events: Rc::clone(&events),
    };
    let options = CompilerOptions::default();
    with_case(
        "/main.ts",
        "class C { set value(v) {} }\n",
        &options,
        &mut resolver,
        &mut tracker,
        |checker, arena, target, context, resolver| {
            let accessor = find_transform_node(checker, arena, target, SyntaxKind::SetAccessor, 0);
            let builder = SyntacticTypeNodeBuilder::new(&options);
            let result = builder
                .serialize_type_of_accessor(resolver, arena, target, context, accessor, None)?
                .expect("semantic accessor fallback");
            assert_eq!(
                arena.node(result).expect("result node").kind,
                SyntaxKind::StringKeyword
            );
            let log = events.borrow();
            assert!(log[0].starts_with("report:"));
            assert_eq!(log[1], "infer-declaration");
            Ok(())
        },
    );
}

#[test]
fn syntactic_property_assignment_uses_report_fallback_false() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut resolver = TestResolver::new(Rc::clone(&events));
    let mut tracker = TestTracker {
        events: Rc::clone(&events),
    };
    let options = CompilerOptions::default();
    with_case(
        "/main.ts",
        "const value = { p: unknownValue };\n",
        &options,
        &mut resolver,
        &mut tracker,
        |checker, arena, target, context, resolver| {
            let property =
                find_transform_node(checker, arena, target, SyntaxKind::PropertyAssignment, 0);
            let builder = SyntacticTypeNodeBuilder::new(&options);
            assert!(builder
                .serialize_type_of_declaration(resolver, arena, target, context, property, None,)?
                .is_some());
            assert_eq!(events.borrow().as_slice(), ["infer-declaration"]);
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
            let builder = SyntacticTypeNodeBuilder::new(&options);
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
fn syntactic_no_inference_fallback_reports_then_skips_resolver() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut resolver = TestResolver::new(Rc::clone(&events));
    let mut tracker = TestTracker {
        events: Rc::clone(&events),
    };
    let options = CompilerOptions::default();
    with_case(
        "/main.ts",
        "const value = unknownValue; const tuple = [unknownElement] as const;\n",
        &options,
        &mut resolver,
        &mut tracker,
        |checker, arena, target, context, resolver| {
            let expression = find_identifier(checker, arena, target, "unknownValue");
            context.no_inference_fallback = Some(true);
            let builder = SyntacticTypeNodeBuilder::new(&options);
            let result = builder
                .serialize_type_of_expression(resolver, arena, target, context, expression)?
                .expect("gated Any result");
            assert_eq!(
                arena.node(result).expect("result node").kind,
                SyntaxKind::AnyKeyword
            );
            let log = events.borrow();
            assert_eq!(log.len(), 1);
            assert!(log[0].starts_with("report:"));
            drop(log);

            events.borrow_mut().clear();
            context.no_inference_fallback = Some(false);
            let assertion =
                find_transform_node(checker, arena, target, SyntaxKind::AsExpression, 0);
            assert!(builder
                .serialize_type_of_expression(resolver, arena, target, context, assertion,)?
                .is_some());
            assert_eq!(context.no_inference_fallback, Some(false));
            assert!(events
                .borrow()
                .iter()
                .any(|event| event == "infer-expression"));
            Ok(())
        },
    );
}

#[test]
fn syntactic_accessor_selects_other_accessor_annotation() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut resolver = TestResolver::new(Rc::clone(&events));
    let mut tracker = TestTracker {
        events: Rc::clone(&events),
    };
    let options = CompilerOptions::default();
    with_case(
        "/main.ts",
        "class C { get value() { return 1; } set value(v: number) {} }\n",
        &options,
        &mut resolver,
        &mut tracker,
        |checker, arena, target, context, resolver| {
            let getter = find_transform_node(checker, arena, target, SyntaxKind::GetAccessor, 0);
            let setter = find_transform_node(checker, arena, target, SyntaxKind::SetAccessor, 0);
            resolver.accessors = Some(SyntacticAccessorDeclarations {
                first_accessor: getter,
                second_accessor: Some(setter),
                get_accessor: Some(getter),
                set_accessor: Some(setter),
            });
            let builder = SyntacticTypeNodeBuilder::new(&options);
            let result = builder
                .serialize_type_of_accessor(resolver, arena, target, context, getter, None)?
                .expect("accessor type");
            assert_eq!(
                arena.node(result).expect("result node").kind,
                SyntaxKind::NumberKeyword
            );
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
            let builder = SyntacticTypeNodeBuilder::new(&options);
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
fn syntactic_jsdoc_type_literal_consults_property_override() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut resolver = TestResolver::new(Rc::clone(&events));
    resolver.jsdoc_override = Some(SyntaxKind::BooleanKeyword);
    let mut tracker = TestTracker {
        events: Rc::clone(&events),
    };
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    with_case(
        "/main.js",
        "/**\n * @typedef {Object} Box\n * @property {number} value\n */\nconst value = {};\n",
        &options,
        &mut resolver,
        &mut tracker,
        |checker, arena, target, context, resolver| {
            let jsdoc =
                find_transform_node(checker, arena, target, SyntaxKind::JSDocTypeLiteral, 0);
            let builder = SyntacticTypeNodeBuilder::new(&options);
            let result = builder
                .try_reuse_existing_type_node(resolver, arena, target, context, jsdoc)?
                .expect("JSDoc type literal reused");
            assert!(events
                .borrow()
                .iter()
                .any(|event| event == "jsdoc-override"));
            let NodeData::TypeLiteral(data) = &arena.node(result).expect("type literal").data
            else {
                panic!("expected type literal")
            };
            let member = arena
                .node_array_ref(result.source(), data.members.expect("members"))
                .expect("member array");
            let member_id = arena.node_array(member).expect("member records").nodes[0];
            let member = arena
                .node_ref(result.source(), member_id)
                .expect("property signature");
            let NodeData::PropertySignature(data) =
                &arena.node(member).expect("property signature record").data
            else {
                panic!("expected property signature")
            };
            let property_type = arena
                .node_ref(member.source(), data.r#type.expect("property type"))
                .expect("property type node");
            assert_eq!(
                arena
                    .node(property_type)
                    .expect("property type record")
                    .kind,
                SyntaxKind::BooleanKeyword
            );
            Ok(())
        },
    );
}

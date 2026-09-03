use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use tsc_diagnostics::gen as d;
use tsc_program::SourceFileId;
use tsc_syntax::{for_each_child, parse_source_file, NodeId, SourceFile, SyntaxKind};
use tsc_types::CompilerOptions;

use super::diagnostics::{comment_range, DiagnosticContext};
use super::state::{TransformState, VisitResult};
use super::tracker::DeclarationSymbolTracker;
use super::*;
use crate::{
    transform_nodes, EmitHost, EmitInternalNodeBuilderFlags, EmitNodeBuilderFlags,
    EmitResolverError, EmitResolverMethod, EmitResolverNode, EmitSource, EmitSymbolTracker,
    TransformArena, TransformSourceId, UnsupportedEmitFeature,
};

struct TestHost<'a> {
    options: &'a CompilerOptions,
    syntax: &'a SourceFile,
    ids: [SourceFileId; 1],
}

impl EmitHost for TestHost<'_> {
    fn compiler_options(&self) -> &CompilerOptions {
        self.options
    }

    fn current_directory(&self) -> &Path {
        Path::new("/")
    }

    fn common_source_directory(&self) -> &Path {
        Path::new("/")
    }

    fn config_file_path(&self) -> Option<&Path> {
        None
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }

    fn source_file_ids(&self) -> &[SourceFileId] {
        &self.ids
    }

    fn source_file(&self, id: SourceFileId) -> Option<EmitSource<'_>> {
        (id == self.ids[0]).then(|| {
            EmitSource::new(
                id,
                Path::new("/fixture.ts"),
                Path::new("/fixture.ts"),
                true,
                None,
                Some(self.syntax),
            )
        })
    }
}

struct NoPaths;

impl DeclarationPathResolver for NoPaths {
    fn declaration_file_path(&self, _source: SourceFileId) -> Option<PathBuf> {
        None
    }

    fn reference_target_path(&self, _source: SourceFileId) -> Option<PathBuf> {
        None
    }
}

#[derive(Clone)]
enum ProbeAction {
    Visit(NodeId),
    Ensure(Vec<(NodeId, bool)>),
    EnsureFailure(NodeId),
}

#[derive(Default)]
struct ProbeObservation {
    visit_is_empty: bool,
    enclosing: Option<NodeId>,
    diagnostic_context: Option<DiagnosticContext>,
    ensure_results: Vec<Option<(SyntaxKind, bool)>>,
    ensure_failed: bool,
    error_name_restored: bool,
    diagnostic_restored: bool,
    original_js_doc: Option<tsc_syntax::NodeArrayId>,
    output_js_doc: Option<tsc_syntax::NodeArrayId>,
    original_comment_range: Option<crate::CommentRange>,
    output_comment_range: Option<crate::CommentRange>,
    output_kind: Option<SyntaxKind>,
}

struct ProbeTransformer<'a> {
    declaration: DeclarationTransformer<'a>,
    action: ProbeAction,
    observation: Rc<RefCell<ProbeObservation>>,
}

impl Transformer for ProbeTransformer<'_> {
    fn name(&self) -> &'static str {
        "declaration-p1-probe"
    }

    fn transform_root(
        &mut self,
        cx: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        let source = match root {
            TransformRoot::SourceFile(source) => source,
            TransformRoot::Bundle(_) => {
                return Err(TransformError::Unsupported(
                    UnsupportedEmitFeature::BundleRoot,
                ));
            }
        };
        let root_node = cx.arena().root(source)?;
        let program_source = cx.arena().source(source)?.program_source();
        self.declaration.state = Some(TransformState::for_source(source, root_node));
        self.declaration
            .tracker
            .reset_for_file(program_source, source, false);

        match self.action.clone() {
            ProbeAction::Visit(node) => {
                let input = TransformNode::new(source, node);
                let result = self.declaration.visit_declaration_subtree(cx, input)?;
                let output = match &result {
                    VisitResult::Node(output) => Some(*output),
                    VisitResult::Nodes(outputs) => outputs.first().copied(),
                    VisitResult::None => None,
                };
                let mut observation = self.observation.borrow_mut();
                observation.visit_is_empty = match result {
                    VisitResult::None => true,
                    VisitResult::Nodes(nodes) => nodes.is_empty(),
                    VisitResult::Node(_) => false,
                };
                observation.enclosing = self
                    .declaration
                    .state()?
                    .enclosing_declaration
                    .map(TransformNode::node);
                observation.diagnostic_context = Some(self.declaration.tracker.diagnostic_context);
                observation.original_js_doc = cx.arena().node(input)?.js_doc;
                observation.original_comment_range = comment_range(cx.arena(), input)?;
                if let Some(output) = output {
                    observation.output_js_doc = cx.arena().node(output)?.js_doc;
                    observation.output_comment_range = cx
                        .arena()
                        .metadata(output)
                        .and_then(crate::EmitMetadata::comment_range);
                    observation.output_kind = Some(cx.arena().node(output)?.kind);
                }
            }
            ProbeAction::Ensure(ref nodes) => {
                let mut results = Vec::with_capacity(nodes.len());
                for &(node, ignore_private) in nodes {
                    let output = self.declaration.ensure_type(
                        cx,
                        TransformNode::new(source, node),
                        ignore_private,
                    )?;
                    results.push(match output {
                        Some(output) => Some((
                            cx.arena().node(output)?.kind,
                            cx.arena().parse_tree_node(output)?.is_none(),
                        )),
                        None => None,
                    });
                }
                self.observation.borrow_mut().ensure_results = results;
            }
            ProbeAction::EnsureFailure(node) => {
                self.declaration.tracker.error_name_node = Some(root_node);
                self.declaration.tracker.replace_diagnostic_context(
                    cx.arena(),
                    DiagnosticContext::DefaultExport(root_node),
                )?;
                let result =
                    self.declaration
                        .ensure_type(cx, TransformNode::new(source, node), false);
                let mut observation = self.observation.borrow_mut();
                observation.ensure_failed = result.is_err();
                observation.error_name_restored =
                    self.declaration.tracker.error_name_node == Some(root_node);
                observation.diagnostic_restored = self.declaration.tracker.diagnostic_context
                    == DiagnosticContext::DefaultExport(root_node);
                result?;
            }
        }
        Ok(TransformRoot::SourceFile(source))
    }
}

#[derive(Clone, Copy)]
struct FixtureResolver {
    first_declaration: bool,
    synthesized_declaration: Option<NodeId>,
    synthesized_return: Option<NodeId>,
    fail_declaration: Option<NodeId>,
}

impl FixtureResolver {
    fn factory_error(method: EmitResolverMethod, error: TransformError) -> EmitResolverError {
        EmitResolverError::Factory {
            method,
            error: Box::new(error),
        }
    }
}

impl EmitResolver for FixtureResolver {
    fn is_declaration_visible(&self, _node: EmitResolverNode) -> Result<bool, EmitResolverError> {
        Ok(true)
    }

    fn is_implementation_of_overload(
        &self,
        _node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(false)
    }

    fn is_first_declaration_of_symbol(
        &self,
        _node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(self.first_declaration)
    }

    fn is_literal_const_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(false)
    }

    fn requires_adding_implicit_undefined(
        &self,
        _parameter: EmitResolverNode,
        _enclosing_declaration: Option<EmitResolverNode>,
    ) -> Result<bool, EmitResolverError> {
        Ok(false)
    }

    fn create_type_of_declaration(
        &self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        declaration: EmitResolverNode,
        _enclosing_declaration: EmitResolverNode,
        _flags: EmitNodeBuilderFlags,
        _internal_flags: EmitInternalNodeBuilderFlags,
        _tracker: &mut dyn EmitSymbolTracker,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        if self.fail_declaration == Some(declaration.node()) {
            return Err(EmitResolverError::CheckerAborted {
                method: EmitResolverMethod::CreateTypeOfDeclaration,
                node: declaration,
                reason: "injected declaration-serialization failure",
            });
        }
        if self.synthesized_declaration != Some(declaration.node()) {
            return Ok(None);
        }
        arena
            .factory()
            .create_keyword_type_node(target, SyntaxKind::NumberKeyword)
            .map(Some)
            .map_err(|error| {
                Self::factory_error(EmitResolverMethod::CreateTypeOfDeclaration, error)
            })
    }

    fn create_return_type_of_signature_declaration(
        &self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        declaration: EmitResolverNode,
        _enclosing_declaration: EmitResolverNode,
        _flags: EmitNodeBuilderFlags,
        _internal_flags: EmitInternalNodeBuilderFlags,
        _tracker: &mut dyn EmitSymbolTracker,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        if self.synthesized_return != Some(declaration.node()) {
            return Ok(None);
        }
        arena
            .factory()
            .create_keyword_type_node(target, SyntaxKind::BooleanKeyword)
            .map(Some)
            .map_err(|error| {
                Self::factory_error(
                    EmitResolverMethod::CreateReturnTypeOfSignatureDeclaration,
                    error,
                )
            })
    }
}

fn nodes_of_kind(source: &SourceFile, kind: SyntaxKind) -> Vec<NodeId> {
    let mut pending = vec![source.root];
    let mut found = Vec::new();
    while let Some(node) = pending.pop() {
        let record = source.arena.node(node);
        if record.kind == kind {
            found.push(node);
        }
        for_each_child(&source.arena, record, |child| {
            pending.push(child);
            false
        });
    }
    found.sort_by_key(|node| source.arena.node(*node).pos);
    found
}

fn run_probe(
    parsed: &SourceFile,
    options: &CompilerOptions,
    resolver: &FixtureResolver,
    action: ProbeAction,
) -> (Result<(), TransformError>, Rc<RefCell<ProbeObservation>>) {
    let source_id = SourceFileId::from_raw(0);
    let host = TestHost {
        options,
        syntax: parsed,
        ids: [source_id],
    };
    let paths = NoPaths;
    let mut arena = TransformArena::new();
    let source = arena.add_source(parsed, Some(source_id));
    let observation = Rc::new(RefCell::new(ProbeObservation::default()));
    let transformer = ProbeTransformer {
        declaration: DeclarationTransformer::new(options, resolver, &host, &paths),
        action,
        observation: Rc::clone(&observation),
    };
    let result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![Box::new(transformer)],
        false,
    )
    .map(|_| ());
    (result, observation)
}

fn accessibility_result(
    accessibility: crate::EmitSymbolAccessibility,
    aliases: Option<Vec<EmitResolverNode>>,
    module: Option<&str>,
) -> crate::EmitSymbolAccessibilityResult {
    crate::EmitSymbolAccessibilityResult {
        accessibility,
        aliases_to_make_visible: aliases,
        error_symbol_name: Some("Hidden".to_owned()),
        error_module_name: module.map(str::to_owned),
        error_node: None,
    }
}

#[test]
fn state_reset_and_owned_frames_restore_before_error_propagation() {
    let parsed = parse_source_file(
        "fixture.ts",
        "let value: string;\n",
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let root = arena.root(source).expect("source root");
    let mut state = TransformState::for_source(source, root);

    assert!(state.needs_declare);
    assert!(!state.is_bundled_emit);
    assert!(!state.result_has_external_module_indicator);
    assert!(!state.needs_scope_fix_marker);
    assert!(!state.result_has_scope_marker);
    assert_eq!(state.enclosing_declaration, Some(root));
    assert!(state.late_statement_replacement.is_empty());
    assert_eq!(state.current_source_file, source);
    assert_eq!(state.references, Default::default());

    let error = state.with_enclosing_declaration(None, |_state| {
        Err::<(), _>(TransformError::Unsupported(
            UnsupportedEmitFeature::IsolatedDeclarations,
        ))
    });
    assert!(matches!(
        error,
        Err(TransformError::Unsupported(
            UnsupportedEmitFeature::IsolatedDeclarations
        ))
    ));
    assert_eq!(state.enclosing_declaration, Some(root));

    state
        .with_needs_declare(false, |state| {
            assert!(!state.needs_declare);
            state.with_scope_markers(true, true, |state| {
                assert!(state.needs_scope_fix_marker);
                assert!(state.result_has_scope_marker);
                Ok(())
            })
        })
        .expect("nested frames");
    assert!(state.needs_declare);
    assert!(!state.needs_scope_fix_marker);
    assert!(!state.result_has_scope_marker);
}

#[test]
fn subtree_private_method_direct_return_preserves_upstream_enclosing_leak() {
    let parsed = parse_source_file(
        "fixture.ts",
        "class C { private m(): void; private m() {} }\n",
        Default::default(),
        None,
    );
    let method = nodes_of_kind(&parsed, SyntaxKind::MethodDeclaration)[0];
    let options = CompilerOptions::default();
    let resolver = FixtureResolver {
        first_declaration: false,
        synthesized_declaration: None,
        synthesized_return: None,
        fail_declaration: None,
    };
    let (result, observation) = run_probe(&parsed, &options, &resolver, ProbeAction::Visit(method));
    result.expect("private method probe");
    let observation = observation.borrow();
    assert!(observation.visit_is_empty);
    assert_eq!(observation.enclosing, Some(method));
    assert_eq!(
        observation.diagnostic_context,
        Some(DiagnosticContext::None)
    );
}

#[test]
fn subtree_binding_pattern_direct_return_preserves_upstream_diagnostic_leak() {
    let parsed = parse_source_file(
        "fixture.ts",
        "const { value } = source;\n",
        Default::default(),
        None,
    );
    let variable = nodes_of_kind(&parsed, SyntaxKind::VariableDeclaration)[0];
    let options = CompilerOptions::default();
    let resolver = FixtureResolver {
        first_declaration: true,
        synthesized_declaration: None,
        synthesized_return: None,
        fail_declaration: None,
    };
    let (result, observation) =
        run_probe(&parsed, &options, &resolver, ProbeAction::Visit(variable));
    result.expect("binding-pattern probe");
    let observation = observation.borrow();
    assert!(!observation.visit_is_empty);
    assert!(matches!(
        observation.diagnostic_context,
        Some(DiagnosticContext::ForNode(node)) if node.node() == variable
    ));
}

#[test]
fn boundary_observer_records_rewritten_output_provenance_and_flags() {
    let parsed = parse_source_file(
        "fixture.ts",
        "class C { private method(): void {} }\n",
        Default::default(),
        None,
    );
    let method = nodes_of_kind(&parsed, SyntaxKind::MethodDeclaration)[0];
    let options = CompilerOptions::default();
    let resolver = FixtureResolver {
        first_declaration: true,
        synthesized_declaration: None,
        synthesized_return: None,
        fail_declaration: None,
    };
    let source_id = SourceFileId::from_raw(0);
    let host = TestHost {
        options: &options,
        syntax: &parsed,
        ids: [source_id],
    };
    let paths = NoPaths;
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(source_id));
    let events = Rc::new(RefCell::new(Vec::new()));
    let observed_events = Rc::clone(&events);
    let mut observer = move |event| observed_events.borrow_mut().push(event);
    let observation = Rc::new(RefCell::new(ProbeObservation::default()));
    let transformer = ProbeTransformer {
        declaration: DeclarationTransformer::new(&options, &resolver, &host, &paths)
            .with_boundary_observer(&mut observer),
        action: ProbeAction::Visit(method),
        observation,
    };

    transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![Box::new(transformer)],
        false,
    )
    .map(|_| ())
    .expect("private method rewrite probe");

    let events = events.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].input_ref, TransformNode::new(source, method));
    assert_ne!(events[0].output_ref, Some(events[0].input_ref));
    assert!(events[0].has_original);
    assert!(!events[0].transform_flags.is_empty());
}

#[test]
fn private_method_collapse_preserves_jsdoc_array_and_comment_range() {
    let parsed = parse_source_file(
        "fixture.ts",
        "class C {\n    /** method docs */\n    private method(): void {}\n}\n",
        Default::default(),
        None,
    );
    let method = nodes_of_kind(&parsed, SyntaxKind::MethodDeclaration)[0];
    let options = CompilerOptions::default();
    let resolver = FixtureResolver {
        first_declaration: true,
        synthesized_declaration: None,
        synthesized_return: None,
        fail_declaration: None,
    };

    let (result, observation) = run_probe(&parsed, &options, &resolver, ProbeAction::Visit(method));
    result.expect("private method JSDoc transfer");
    let observation = observation.borrow();
    assert_eq!(
        observation.output_kind,
        Some(SyntaxKind::PropertyDeclaration)
    );
    assert!(observation.original_js_doc.is_some());
    assert_eq!(observation.output_js_doc, observation.original_js_doc);
    assert!(observation.original_comment_range.is_some());
    assert_eq!(
        observation.output_comment_range,
        observation.original_comment_range
    );
}

#[test]
fn tracker_assigns_first_alias_vector_verbatim_then_merges_uniquely() {
    let parsed = parse_source_file(
        "fixture.ts",
        "let first; let second;\n",
        Default::default(),
        None,
    );
    let options = CompilerOptions::default();
    let host = TestHost {
        options: &options,
        syntax: &parsed,
        ids: [SourceFileId::from_raw(0)],
    };
    let declarations = nodes_of_kind(&parsed, SyntaxKind::VariableDeclaration);
    let first = EmitResolverNode::new(SourceFileId::from_raw(0), declarations[0]);
    let second = EmitResolverNode::new(SourceFileId::from_raw(0), declarations[1]);
    let mut arena = TransformArena::new();
    let transform_source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let mut tracker = DeclarationSymbolTracker::new(&options, &host);
    tracker.reset_for_file(Some(SourceFileId::from_raw(0)), transform_source, false);

    assert!(
        !tracker.handle_symbol_accessibility_error(accessibility_result(
            crate::EmitSymbolAccessibility::Accessible,
            Some(vec![first, first]),
            None,
        ))
    );
    let first_transform = TransformNode::new(transform_source, declarations[0]);
    let second_transform = TransformNode::new(transform_source, declarations[1]);
    assert_eq!(
        tracker.late_marked_statements,
        Some(vec![first_transform, first_transform])
    );

    assert!(
        !tracker.handle_symbol_accessibility_error(accessibility_result(
            crate::EmitSymbolAccessibility::Accessible,
            Some(vec![first, second, second]),
            None,
        ))
    );
    assert_eq!(
        tracker.late_marked_statements,
        Some(vec![first_transform, first_transform, second_transform])
    );
    tracker.reset_for_file(Some(SourceFileId::from_raw(0)), transform_source, false);
    assert_eq!(tracker.late_marked_statements, None);
    assert_eq!(tracker.diagnostic_context, DiagnosticContext::None);
    assert!(!tracker.suppress_new_diagnostic_contexts);
    assert!(tracker.error_fallback_stack.is_empty());
}

#[test]
fn diagnostic_context_selects_all_three_external_module_messages() {
    let parsed = parse_source_file(
        "fixture.ts",
        "export const publicValue = 1;\nexport class C { private constructor() {} }\n",
        Default::default(),
        None,
    );
    let variable = nodes_of_kind(&parsed, SyntaxKind::VariableDeclaration)[0];
    let constructor = nodes_of_kind(&parsed, SyntaxKind::Constructor)[0];
    let options = CompilerOptions::default();
    let source_id = SourceFileId::from_raw(0);
    let host = TestHost {
        options: &options,
        syntax: &parsed,
        ids: [source_id],
    };
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(source_id));
    let plan = DiagnosticContext::ForNode(TransformNode::new(source, variable))
        .plan(&arena)
        .expect("variable diagnostic plan");

    let cannot = plan
        .resolve(
            &host,
            &accessibility_result(
                crate::EmitSymbolAccessibility::CannotBeNamed,
                None,
                Some("external"),
            ),
        )
        .expect("cannot-be-named selection")
        .expect("diagnostic spec");
    assert_eq!(
        cannot.message.code,
        d::Exported_variable_0_has_or_is_using_name_1_from_external_module_2_but_cannot_be_named
            .code
    );

    let private_module = plan
        .resolve(
            &host,
            &accessibility_result(
                crate::EmitSymbolAccessibility::NotAccessible,
                None,
                Some("private"),
            ),
        )
        .expect("private-module selection")
        .expect("diagnostic spec");
    assert_eq!(
        private_module.message.code,
        d::Exported_variable_0_has_or_is_using_name_1_from_private_module_2.code
    );

    let private_name = plan
        .resolve(
            &host,
            &accessibility_result(crate::EmitSymbolAccessibility::NotAccessible, None, None),
        )
        .expect("private-name selection")
        .expect("diagnostic spec");
    assert_eq!(
        private_name.message.code,
        d::Exported_variable_0_has_or_is_using_private_name_1.code
    );

    let constructor_plan = DiagnosticContext::ForNode(TransformNode::new(source, constructor))
        .plan(&arena)
        .expect("constructor diagnostic plan");
    assert!(
        constructor_plan
            .resolve(
                &host,
                &accessibility_result(crate::EmitSymbolAccessibility::NotAccessible, None, None,),
            )
            .expect("constructor selection")
            .is_none(),
        "upstream's constructor message selector intentionally returns undefined"
    );
}

#[test]
fn ensure_type_selects_explicit_synthesized_fallback_and_private_arms() {
    let parsed = parse_source_file(
        "fixture.ts",
        concat!(
            "let explicit: string;\n",
            "let inferred;\n",
            "let fallback;\n",
            "declare function callable();\n",
            "class C { private hidden; }\n",
        ),
        Default::default(),
        None,
    );
    let variables = nodes_of_kind(&parsed, SyntaxKind::VariableDeclaration);
    let function = nodes_of_kind(&parsed, SyntaxKind::FunctionDeclaration)[0];
    let private_property = nodes_of_kind(&parsed, SyntaxKind::PropertyDeclaration)[0];
    let options = CompilerOptions::default();
    let resolver = FixtureResolver {
        first_declaration: true,
        synthesized_declaration: Some(variables[1]),
        synthesized_return: Some(function),
        fail_declaration: None,
    };
    let action = ProbeAction::Ensure(vec![
        (variables[0], false),
        (variables[1], false),
        (variables[2], false),
        (function, false),
        (private_property, false),
    ]);
    let (result, observation) = run_probe(&parsed, &options, &resolver, action);
    result.expect("ensureType arm probe");
    assert_eq!(
        observation.borrow().ensure_results,
        vec![
            Some((SyntaxKind::StringKeyword, false)),
            Some((SyntaxKind::NumberKeyword, true)),
            Some((SyntaxKind::AnyKeyword, true)),
            Some((SyntaxKind::BooleanKeyword, true)),
            None,
        ]
    );
}

#[test]
fn ensure_type_restores_error_name_and_diagnostic_context_on_resolver_error() {
    let parsed = parse_source_file("fixture.ts", "let inferred;\n", Default::default(), None);
    let variable = nodes_of_kind(&parsed, SyntaxKind::VariableDeclaration)[0];
    let options = CompilerOptions::default();
    let resolver = FixtureResolver {
        first_declaration: true,
        synthesized_declaration: None,
        synthesized_return: None,
        fail_declaration: Some(variable),
    };
    let (result, observation) = run_probe(
        &parsed,
        &options,
        &resolver,
        ProbeAction::EnsureFailure(variable),
    );
    assert!(matches!(result, Err(TransformError::Resolver(_))));
    let observation = observation.borrow();
    assert!(observation.ensure_failed);
    assert!(observation.error_name_restored);
    assert!(observation.diagnostic_restored);
}

use std::path::PathBuf;

use tsc_syntax::{parse_source_file, NodeData, SyntaxKind};

use super::{
    builtins::rewrite_relative_module_specifier,
    factory::{EmitHelperName, NodeFactory},
    EmitFlags, EmitOutcome, SourceMapObservation, TransformArena, TransformFlags, TransformNode,
    TransformSourceId,
};

#[test]
fn outcome_retains_optional_presence_and_independent_emitted_file_order() {
    let source_map = SourceMapObservation::new(
        vec![PathBuf::from("/project/input.ts")],
        "{\"version\":3}".into(),
    );
    let absent = EmitOutcome::new(Vec::new(), true, None, None, Default::default());
    let present = EmitOutcome::new(
        Vec::new(),
        false,
        Some(vec![
            PathBuf::from("/project/out.js"),
            PathBuf::from("/project/out.js.map"),
        ]),
        Some(vec![source_map]),
        Default::default(),
    );

    assert!(absent.emit_skipped());
    assert_eq!(absent.emitted_files(), None);
    assert_eq!(absent.source_maps(), None);
    assert_eq!(
        present.emitted_files(),
        Some(
            [
                PathBuf::from("/project/out.js"),
                PathBuf::from("/project/out.js.map"),
            ]
            .as_slice()
        )
    );
    let maps = present.source_maps().expect("present map observations");
    assert_eq!(
        maps[0].input_source_files(),
        [PathBuf::from("/project/input.ts")]
    );
    assert_eq!(maps[0].canonical_json(), "{\"version\":3}");
}

#[test]
fn relative_module_specifier_rewrite_matches_typescript_suffix_rules() {
    for (input, expected) in [
        ("./dep.ts", Some("./dep.js")),
        ("../dep.mts", Some("../dep.mjs")),
        ("./dep.cts", Some("./dep.cjs")),
        ("./dep.tsx", Some("./dep.js")),
    ] {
        assert_eq!(
            rewrite_relative_module_specifier(input).as_deref(),
            expected,
            "unexpected rewrite for {input}"
        );
    }

    for input in [
        "dep.ts",
        "./dep.js",
        "./dep.TS",
        "./dep.d.ts",
        "./dep.d.mts",
        "./dep.d.cts",
        "./dep.d.generated.ts",
    ] {
        assert_eq!(
            rewrite_relative_module_specifier(input),
            None,
            "specifier should remain unchanged: {input}"
        );
    }
}

#[test]
fn emit_helper_name_distinguishes_user_calls_and_survives_factory_updates() {
    let parsed = parse_source_file(
        "helper-name.ts",
        "__runInitializers();\n",
        Default::default(),
        None,
    );
    let NodeData::SourceFile(source_file) = &parsed.arena.node(parsed.root).data else {
        panic!("source file root");
    };
    let source_statements = parsed
        .arena
        .node_array(source_file.statements.expect("source statements"));
    let NodeData::ExpressionStatement(statement) =
        &parsed.arena.node(source_statements.nodes[0]).data
    else {
        panic!("parsed helper call statement");
    };
    let parsed_call_id = statement.expression.expect("parsed helper call");
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let parsed_call = arena
        .node_ref(source, parsed_call_id)
        .expect("mounted parsed helper call");

    let (helper, helper_call, user_call) = {
        let mut factory = NodeFactory::new(&mut arena);
        let helper = factory
            .create_unscoped_helper_identifier(source, EmitHelperName::RunInitializers)
            .expect("create typed helper identifier");
        let helper_call = create_test_call(&mut factory, source, helper);
        let user = create_test_identifier(&mut factory, source, "__runInitializers");
        let user_call = create_test_call(&mut factory, source, user);
        (helper, helper_call, user_call)
    };

    let helper_flags = arena
        .metadata(helper)
        .expect("typed helper metadata")
        .flags();
    assert!(helper_flags.contains(EmitFlags::HELPER_NAME));
    assert!(helper_flags.contains(EmitFlags::ADVISE_ON_EMIT_NODE));
    assert!(arena
        .is_call_to_emit_helper(helper_call, EmitHelperName::RunInitializers)
        .expect("classify typed helper call"));
    assert!(!arena
        .is_call_to_emit_helper(user_call, EmitHelperName::RunInitializers)
        .expect("classify same-spelling user call"));
    assert!(!arena
        .is_call_to_emit_helper(parsed_call, EmitHelperName::RunInitializers)
        .expect("classify parsed same-spelling user call"));

    let helper_data = arena.node(helper).expect("helper node").data.clone();
    let (cloned_call, updated_call) = {
        let mut factory = NodeFactory::new(&mut arena);
        let cloned = factory.clone_node(helper).expect("clone helper identifier");
        let updated = factory
            .update_node(helper, helper_data, TransformFlags::CONTAINS_ES_2015)
            .expect("update helper identifier");
        (
            create_test_call(&mut factory, source, cloned),
            create_test_call(&mut factory, source, updated),
        )
    };
    assert!(arena
        .is_call_to_emit_helper(cloned_call, EmitHelperName::RunInitializers)
        .expect("classify cloned helper call"));
    assert!(arena
        .is_call_to_emit_helper(updated_call, EmitHelperName::RunInitializers)
        .expect("classify updated helper call"));
}

#[test]
fn factory_private_expression_flags_distinguish_declarations_property_access_and_in() {
    let parsed = parse_source_file("private-flags.ts", "", Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);

    let (private_name, declaration, property_access, element_access, private_in) = {
        let mut factory = NodeFactory::new(&mut arena);
        let private_name = factory
            .create_node(
                source,
                NodeData::PrivateIdentifier(tsc_syntax::nodes::PrivateIdentifierData {
                    escaped_text: "#field".to_owned(),
                    text: "#field".to_owned(),
                }),
                TransformFlags::NONE,
            )
            .expect("create private name");
        let receiver = create_test_identifier(&mut factory, source, "receiver");
        let declaration = factory
            .create_node(
                source,
                NodeData::PropertyDeclaration(tsc_syntax::nodes::PropertyDeclarationData {
                    name: Some(private_name.node()),
                    modifiers: None,
                    question_token: None,
                    exclamation_token: None,
                    r#type: None,
                    initializer: None,
                }),
                TransformFlags::NONE,
            )
            .expect("create private declaration");
        let property_access = factory
            .create_node(
                source,
                NodeData::PropertyAccessExpression(
                    tsc_syntax::nodes::PropertyAccessExpressionData {
                        expression: Some(receiver.node()),
                        question_dot_token: None,
                        name: Some(private_name.node()),
                    },
                ),
                TransformFlags::NONE,
            )
            .expect("create private property access");
        let element_access = factory
            .create_node(
                source,
                NodeData::ElementAccessExpression(tsc_syntax::nodes::ElementAccessExpressionData {
                    expression: Some(receiver.node()),
                    question_dot_token: None,
                    argument_expression: Some(private_name.node()),
                }),
                TransformFlags::NONE,
            )
            .expect("create private element access");
        let in_token = factory
            .create_token(source, SyntaxKind::InKeyword, TransformFlags::NONE)
            .expect("create in token");
        let private_in = factory
            .create_node(
                source,
                NodeData::BinaryExpression(tsc_syntax::nodes::BinaryExpressionData {
                    left: Some(private_name.node()),
                    operator_token: Some(in_token.node()),
                    right: Some(receiver.node()),
                }),
                TransformFlags::NONE,
            )
            .expect("create private in expression");
        (
            private_name,
            declaration,
            property_access,
            element_access,
            private_in,
        )
    };

    let private_expression = TransformFlags::CONTAINS_PRIVATE_IDENTIFIER_IN_EXPRESSION;
    assert!(!arena
        .transform_flags(private_name)
        .contains(private_expression));
    assert!(!arena
        .transform_flags(declaration)
        .contains(private_expression));
    assert!(arena
        .transform_flags(property_access)
        .contains(private_expression));
    assert!(!arena
        .transform_flags(element_access)
        .contains(private_expression));
    assert!(arena
        .transform_flags(private_in)
        .contains(private_expression));
}

fn create_test_identifier(
    factory: &mut NodeFactory<'_>,
    source: TransformSourceId,
    text: &str,
) -> TransformNode {
    factory
        .create_node(
            source,
            NodeData::Identifier(tsc_syntax::nodes::IdentifierData {
                escaped_text: tsc_syntax::escape_leading_underscores(text),
                text: text.to_owned(),
            }),
            TransformFlags::NONE,
        )
        .expect("create test identifier")
}

fn create_test_call(
    factory: &mut NodeFactory<'_>,
    source: TransformSourceId,
    expression: TransformNode,
) -> TransformNode {
    let arguments = factory
        .create_node_array(source, Vec::new())
        .expect("create test arguments");
    factory
        .create_node(
            source,
            NodeData::CallExpression(tsc_syntax::nodes::CallExpressionData {
                expression: Some(expression.node()),
                question_dot_token: None,
                type_arguments: None,
                arguments: Some(arguments.array()),
            }),
            TransformFlags::NONE,
        )
        .expect("create test call")
}

// --- H2.5h-a / CS-2 comment-emission scope contracts ---
//
// The threaded scope replaces tsc's printer-closure triple
// (`containerPos`, `containerEnd`, `declarationListContainerEnd`,
// `_tsc.js:116957-116959`). These contracts pin the claim shape and the
// two guarded readers directly; the emitted-byte behavior is covered by
// the source-comment topology suite.

use super::comment_cursor::{CommentCursor, CommentEmissionScope};
use super::{CommentRange, SourceBytePosition, SourceByteRange, SourceRange};
use tsc_syntax::SourceFile;

struct ScopeFixture {
    parsed: SourceFile,
    source: TransformSourceId,
}

fn ranged_fixture(file_name: &str) -> ScopeFixture {
    let parsed = parse_source_file(
        file_name,
        "/* a */ value; other;\n",
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    ScopeFixture { parsed, source }
}

impl ScopeFixture {
    fn cursor(&self, value: u32) -> CommentCursor {
        CommentCursor::new(
            self.source,
            SourceBytePosition::new(value, self.parsed.positions()).expect("source position"),
        )
    }

    fn range(&self, start: u32, end: u32) -> CommentRange {
        CommentRange::new(
            self.source,
            SourceRange::Original(
                SourceByteRange::new(start, end, self.parsed.positions()).expect("source range"),
            ),
        )
    }
}

#[test]
fn empty_scope_retains_no_end_and_exposes_no_sides() {
    let fixture = ranged_fixture("scope.ts");
    let scope = CommentEmissionScope::empty();
    assert_eq!(scope.container_pos(), None);
    assert_eq!(scope.container_end(), None);
    assert!(!scope.retains_end(fixture.cursor(14)));
}

#[test]
fn per_side_claim_replaces_some_sides_and_inherits_none_sides() {
    let fixture = ranged_fixture("scope.ts");
    let outer = CommentEmissionScope::empty()
        .claim_sides(Some(fixture.cursor(0)), Some(fixture.cursor(21)));
    assert_eq!(outer.container_pos(), Some(fixture.cursor(0)));
    assert_eq!(outer.container_end(), Some(fixture.cursor(21)));
    assert!(outer.retains_end(fixture.cursor(21)));

    // One-sided claim: the unclaimed side stays with the enclosing scope.
    let leading_only = outer.claim_sides(Some(fixture.cursor(8)), None);
    assert_eq!(leading_only.container_pos(), Some(fixture.cursor(8)));
    assert_eq!(leading_only.container_end(), Some(fixture.cursor(21)));
    let trailing_only = outer.claim_sides(None, Some(fixture.cursor(14)));
    assert_eq!(trailing_only.container_pos(), Some(fixture.cursor(0)));
    assert!(trailing_only.retains_end(fixture.cursor(14)));
    assert!(!trailing_only.retains_end(fixture.cursor(21)));

    // A claim with neither side is pure inheritance.
    assert_eq!(outer.claim_sides(None, None), outer);
}

#[test]
fn range_views_reject_synthesized_and_zero_width_ranges() {
    let fixture = ranged_fixture("scope.ts");
    let synthesized = CommentRange::new(fixture.source, SourceRange::Synthesized);
    let zero_width = fixture.range(14, 14);
    for container in [synthesized, zero_width] {
        assert_eq!(CommentEmissionScope::container_pos_of(container), None);
        assert_eq!(CommentEmissionScope::container_end_of(container), None);
    }
    // An at-zero start with a positive end is a real claimable pair: the
    // upstream outer gate passes through the end side.
    let at_zero = fixture.range(0, 14);
    assert_eq!(
        CommentEmissionScope::container_pos_of(at_zero),
        Some(fixture.cursor(0)),
    );
    assert_eq!(
        CommentEmissionScope::container_end_of(at_zero),
        Some(fixture.cursor(14)),
    );
}

#[test]
fn guards_never_match_across_sources() {
    let first = parse_source_file(
        "scope.ts",
        "/* a */ value; other;\n",
        Default::default(),
        None,
    );
    let second = parse_source_file(
        "other.ts",
        "/* a */ value; other;\n",
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let first_source = arena.add_source(&first, None);
    let second_source = arena.add_source(&second, None);
    assert_ne!(first_source, second_source);
    let scope = CommentEmissionScope::empty().claim_sides(
        Some(CommentCursor::new(
            first_source,
            SourceBytePosition::new(8, first.positions()).expect("source position"),
        )),
        Some(CommentCursor::new(
            first_source,
            SourceBytePosition::new(14, first.positions()).expect("source position"),
        )),
    );
    let foreign_end = CommentCursor::new(
        second_source,
        SourceBytePosition::new(14, second.positions()).expect("source position"),
    );
    assert!(!scope.retains_end(foreign_end));
}

#[test]
fn claiming_preserves_the_declaration_list_end() {
    let fixture = ranged_fixture("scope.ts");
    let inherited = CommentEmissionScope::contract_scope(
        Some(fixture.cursor(0)),
        Some(fixture.cursor(21)),
        Some(fixture.cursor(14)),
    );
    let claimed = inherited.claim_sides(Some(fixture.cursor(8)), Some(fixture.cursor(13)));
    assert_eq!(claimed.container_end(), Some(fixture.cursor(13)));
    assert!(claimed.retains_end(fixture.cursor(13)));
    assert!(!claimed.retains_end(fixture.cursor(21)));
    // The declaration-list end survives every claim, exactly the non-list
    // shape of tsc's emitLeadingCommentsOfNode.
    assert!(claimed.retains_end(fixture.cursor(14)));
}

#[test]
fn declaration_list_end_guards_without_a_claimed_container() {
    let fixture = ranged_fixture("scope.ts");
    let scope = CommentEmissionScope::contract_scope(None, None, Some(fixture.cursor(14)));
    assert_eq!(scope.container_pos(), None);
    assert_eq!(scope.container_end(), None);
    assert!(scope.retains_end(fixture.cursor(14)));
    assert!(!scope.retains_end(fixture.cursor(13)));
}

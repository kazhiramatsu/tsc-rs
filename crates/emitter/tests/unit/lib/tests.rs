use std::path::PathBuf;

use tsc_syntax::{parse_source_file, NodeData};

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

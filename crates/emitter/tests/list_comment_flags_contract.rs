//! List and ordinary node comments retain distinct flag ownership.
//! The complete printer outputs are frozen from the pinned TypeScript printer.
use tsc_emitter::{
    create_printer, transform_nodes, EmitFlags, NewLineKind, PrintRequest, PrinterOptions,
    SourceFileTextMode, TransformArena, TransformError, TransformRoot, TransformationContext,
    Transformer,
};
use tsc_syntax::{parse_source_file, NodeData};

struct FlagTransformer {
    flags: EmitFlags,
    parent: bool,
    parent_and_body: bool,
    clone_name: bool,
}

impl Transformer for FlagTransformer {
    fn name(&self) -> &'static str {
        "list-comment-flags"
    }

    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        let TransformRoot::SourceFile(source) = root else {
            return Ok(root);
        };
        let arena = context.arena();
        let root_node = arena.root(source)?;
        let NodeData::SourceFile(file) = &arena.node(root_node)?.data else {
            unreachable!()
        };
        let statements = arena
            .node_array_ref(source, file.statements.unwrap())
            .unwrap();
        let statement = arena
            .node_ref(source, arena.node_array(statements)?.nodes[0])
            .unwrap();
        if let NodeData::FunctionDeclaration(data) = &arena.node(statement)?.data {
            let body = arena.node_ref(source, data.body.unwrap()).unwrap();
            if self.parent || self.parent_and_body {
                context
                    .arena_mut()?
                    .metadata_mut(statement)
                    .add_flags(self.flags);
            }
            if !self.parent {
                context
                    .arena_mut()?
                    .metadata_mut(body)
                    .add_flags(self.flags);
            }
            return Ok(root);
        }
        let parent = match &arena.node(statement)?.data {
            NodeData::ExpressionStatement(statement) => statement.expression.unwrap(),
            NodeData::VariableStatement(statement) => statement.declaration_list.unwrap(),
            _ => unreachable!(),
        };
        let parent = arena.node_ref(source, parent).unwrap();
        let array = match &arena.node(parent)?.data {
            NodeData::CallExpression(call) => call.arguments.unwrap(),
            NodeData::ArrayLiteralExpression(array) => array.elements.unwrap(),
            NodeData::VariableDeclarationList(list) => list.declarations.unwrap(),
            _ => unreachable!(),
        };
        let array_ref = arena.node_array_ref(source, array).unwrap();
        let declarations: Vec<_> = arena
            .node_array(array_ref)?
            .nodes
            .iter()
            .filter_map(|&id| {
                let node = arena.node_ref(source, id).unwrap();
                if let NodeData::VariableDeclaration(data) = &arena.node(node).unwrap().data {
                    Some((node, data.clone()))
                } else {
                    None
                }
            })
            .collect();
        let nodes = if self.parent {
            vec![parent]
        } else {
            arena
                .node_array(arena.node_array_ref(source, array).unwrap())?
                .nodes
                .iter()
                .map(|&node| {
                    let node = arena.node_ref(source, node).unwrap();
                    if let NodeData::VariableDeclaration(data) = &arena.node(node).unwrap().data {
                        arena.node_ref(source, data.name.unwrap()).unwrap()
                    } else {
                        node
                    }
                })
                .collect()
        };
        for (_, declaration) in &declarations {
            if let Some(annotation) = declaration.r#type {
                let name = context
                    .arena()
                    .node_ref(source, declaration.name.unwrap())
                    .unwrap();
                let annotation = context.arena().node_ref(source, annotation).unwrap();
                context
                    .arena_mut()?
                    .metadata_mut(name)
                    .set_type_node(annotation);
            }
        }
        if self.clone_name {
            let mut updated = Vec::new();
            for (original, declaration) in declarations {
                let node = |id| context.arena().node_ref(source, id).unwrap();
                let name = node(declaration.name.unwrap());
                let exclamation = declaration.exclamation_token.map(node);
                let annotation = declaration.r#type.map(node);
                let initializer = declaration.initializer.map(node);
                let clone = context.factory()?.clone_node(name)?;
                updated.push(context.factory()?.update_variable_declaration(
                    original,
                    clone,
                    exclamation,
                    annotation,
                    initializer,
                )?);
            }
            let list = context.factory()?.update_node_array(array_ref, updated)?;
            let list = context
                .factory()?
                .update_variable_declaration_list(parent, list)?;
            let modifiers =
                if let NodeData::VariableStatement(data) = &context.arena().node(statement)?.data {
                    data.modifiers
                        .and_then(|array| context.arena().node_array_ref(source, array))
                } else {
                    unreachable!()
                };
            let statement = context
                .factory()?
                .update_variable_statement(statement, modifiers, list)?;
            let statements = context
                .factory()?
                .update_node_array(statements, vec![statement])?;
            let syntax = context.arena().source(source)?.syntax();
            let (declaration, references, types, no_lib, libs) = (
                syntax.is_declaration_file,
                syntax.referenced_files.clone(),
                syntax.type_reference_directives.clone(),
                false,
                syntax.lib_reference_directives.clone(),
            );
            let root = context.factory()?.update_source_file(
                root_node,
                statements,
                declaration,
                references,
                types,
                no_lib,
                libs,
            )?;
            context.arena_mut()?.replace_root(source, root)?;
        }
        for node in nodes {
            context
                .arena_mut()?
                .metadata_mut(node)
                .add_flags(self.flags);
        }
        Ok(root)
    }
}

#[test]
fn list_comment_flags_match_typescript_printer() {
    let artifact: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/list-comment-flags.json")).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 172);
    let mut failures = Vec::new();
    for case in cases {
        let id = case["case_id"].as_str().unwrap();
        let mut exact = true;
        for repetition in 0..2 {
            let flags = match case["variant"].as_str().unwrap() {
                "None" | "CloneName" => EmitFlags::NONE,
                "NoLeadingComments" => EmitFlags::NO_LEADING_COMMENTS,
                "NoTrailingComments" => EmitFlags::NO_TRAILING_COMMENTS,
                "NoComments" => EmitFlags::NO_COMMENTS,
                "NoNestedComments" | "ParentNoNestedComments" | "ParentAndBodyNoNestedComments" => {
                    EmitFlags::NO_NESTED_COMMENTS
                }
                _ => unreachable!(),
            };
            let parsed = parse_source_file(
                "list-comments.ts",
                case["source"].as_str().unwrap(),
                Default::default(),
                None,
            );
            let mut arena = TransformArena::new();
            let source = arena.add_source(&parsed, None);
            let mut result = transform_nodes(
                arena,
                vec![TransformRoot::SourceFile(source)],
                vec![Box::new(FlagTransformer {
                    flags,
                    parent: case["variant"] == "ParentNoNestedComments",
                    parent_and_body: case["variant"] == "ParentAndBodyNoNestedComments",
                    clone_name: case["variant"] == "CloneName",
                })],
                false,
            )
            .unwrap();
            let output = create_printer(
                PrinterOptions::new(NewLineKind::LineFeed)
                    .with_remove_comments(case["remove_comments"].as_bool().unwrap())
                    .with_source_file_text_mode(SourceFileTextMode::Canonical),
            )
            .print(&mut result, PrintRequest::SourceFile(source), None)
            .unwrap();
            let expected = case["output"].as_str().unwrap();
            if output.text() != expected {
                exact = false;
                eprintln!("List comment flags FAIL {id} repetition {repetition}: actual {:?}, expected {expected:?}", output.text());
            }
        }
        if exact {
            eprintln!("List comment flags EXACT x2 {id}");
        } else {
            failures.push(id);
        }
    }
    assert!(
        failures.is_empty(),
        "list comment flag failures: {failures:?}"
    );
}

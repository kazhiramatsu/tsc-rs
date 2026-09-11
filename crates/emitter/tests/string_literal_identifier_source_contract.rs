//! Pinned TypeScript printer observations for string-literal text-source identity.
use tsc_emitter::TransformFlags;
use tsc_emitter::{
    create_printer, transform_nodes, EmitFlags, NewLineKind, PrintRequest, PrinterOptions,
    SourceFileTextMode, TransformArena, TransformError, TransformRoot, TransformationContext,
    Transformer,
};
use tsc_syntax::{
    nodes::{ExpressionStatementData, SourceFileData, StringLiteralData},
    parse_source_file, NodeData,
};

struct LiteralTransformer {
    origin: String,
    foreign_source: tsc_emitter::TransformSourceId,
    no_ascii_escaping: bool,
}

impl Transformer for LiteralTransformer {
    fn name(&self) -> &'static str {
        "string-literal-identifier-source"
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
        let end_of_file_token = file.end_of_file_token;
        let statements = arena
            .node_array_ref(source, file.statements.unwrap())
            .unwrap();
        let statement = arena
            .node_ref(source, arena.node_array(statements)?.nodes[0])
            .unwrap();
        let NodeData::ExpressionStatement(statement) = &arena.node(statement)?.data else {
            unreachable!()
        };
        let mut name = arena
            .node_ref(source, statement.expression.unwrap())
            .unwrap();
        if self.origin == "foreign" {
            let foreign_root = arena.root(self.foreign_source)?;
            let NodeData::SourceFile(file) = &arena.node(foreign_root)?.data else {
                unreachable!()
            };
            let statements = arena
                .node_array_ref(self.foreign_source, file.statements.unwrap())
                .unwrap();
            let statement = arena
                .node_ref(self.foreign_source, arena.node_array(statements)?.nodes[0])
                .unwrap();
            let NodeData::ExpressionStatement(statement) = &arena.node(statement)?.data else {
                unreachable!()
            };
            name = arena
                .node_ref(self.foreign_source, statement.expression.unwrap())
                .unwrap();
        }
        if self.origin == "clone" {
            name = context.factory()?.clone_node(name)?;
        } else if self.origin == "synthetic" {
            let data = context.arena().node(name)?.data.clone();
            name = context
                .factory()?
                .create_node(source, data, TransformFlags::NONE)?;
        }
        let data = context.arena().node(name)?.data.clone();
        let literal = match data {
            NodeData::Identifier(identifier) => context.factory()?.create_node(
                source,
                NodeData::StringLiteral(StringLiteralData {
                    text: identifier.text,
                    has_extended_unicode_escape: None,
                }),
                TransformFlags::NONE,
            )?,
            NodeData::StringLiteral(_) => context.factory()?.clone_node(name)?,
            _ => unreachable!(),
        };
        context
            .arena_mut()?
            .literal_properties_mut(literal)?
            .set_string_literal_text_source(name);
        let metadata = context.arena_mut()?.metadata_mut(literal);
        if self.no_ascii_escaping {
            metadata.add_flags(EmitFlags::NO_ASCII_ESCAPING);
        }
        let statement = context.factory()?.create_node(
            source,
            NodeData::ExpressionStatement(ExpressionStatementData {
                expression: Some(literal.node()),
            }),
            TransformFlags::NONE,
        )?;
        let statements = context
            .factory()?
            .create_node_array(source, vec![statement])?;
        let flags = context.arena().transform_flags(root_node);
        let updated = context.factory()?.update_node(
            root_node,
            NodeData::SourceFile(SourceFileData {
                statements: Some(statements.array()),
                end_of_file_token,
            }),
            flags,
        )?;
        context.arena_mut()?.replace_root(source, updated)?;
        Ok(root)
    }
}

#[test]
fn string_literal_identifier_source_matches_typescript_printer() {
    let artifact: serde_json::Value = serde_json::from_slice(include_bytes!(
        "fixtures/string-literal-identifier-source.json"
    ))
    .unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 72);
    let mut failures = Vec::new();
    for case in cases {
        let id = case["case_id"].as_str().unwrap();
        let mut exact = true;
        for repetition in 0..2 {
            let parsed = parse_source_file(
                "literal-source.ts",
                case["source"].as_str().unwrap(),
                Default::default(),
                None,
            );
            let mut arena = TransformArena::new();
            let source = arena.add_source(&parsed, None);
            let foreign = parse_source_file(
                "foreign.ts",
                case["source"].as_str().unwrap(),
                Default::default(),
                None,
            );
            let foreign_source = arena.add_source(&foreign, None);
            let mut result = transform_nodes(
                arena,
                vec![TransformRoot::SourceFile(source)],
                vec![Box::new(LiteralTransformer {
                    origin: case["origin"].as_str().unwrap().to_owned(),
                    foreign_source,
                    no_ascii_escaping: case["no_ascii_escaping"].as_bool().unwrap(),
                })],
                false,
            )
            .unwrap();
            let output = create_printer(
                PrinterOptions::new(NewLineKind::LineFeed)
                    .with_source_file_text_mode(SourceFileTextMode::Canonical),
            )
            .print(&mut result, PrintRequest::SourceFile(source), None)
            .unwrap();
            let expected = case["output"].as_str().unwrap();
            if output.text() != expected {
                exact = false;
                eprintln!("String literal identifier source FAIL {id} repetition {repetition}: actual {:?}, expected {expected:?}", output.text());
            }
        }
        if exact {
            eprintln!("String literal identifier source EXACT x2 {id}");
        } else {
            failures.push(id);
        }
    }
    assert!(
        failures.is_empty(),
        "string-literal identifier source failures: {failures:?}"
    );
}

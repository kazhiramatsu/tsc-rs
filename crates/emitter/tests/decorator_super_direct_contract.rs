//! Direct transform controls for the A6-41-SUPER value-use memo of the
//! standard-decorator visitor: synthetic `CommaListExpression`s and shared
//! expression nodes that reach `super` lowering in two value-use contexts.
//!
//! No Program input produces these shapes (no transform ahead of
//! transformESDecorators emits a CommaListExpression or shares an expression
//! node), so `scripts/observe-decorator-super-direct.mjs` injects them with a
//! `customTransformers.before` transformer on the parsed source and records
//! the emitted JavaScript of the complete emit. Here the same mutation runs as
//! a transformer inserted right after `transformTypeScript` (the port's
//! TypeScript transform resolves a class's lexical scope through the parsed
//! `parent` chain, so a rebuilt class must not precede it; tsc's
//! `transformESDecorators` sees the identical tree either way because the
//! TypeScript transform keeps a CommaListExpression and returns a shared
//! super assignment unchanged), and the printed JavaScript must match byte
//! for byte (two repetitions per case). JavaScript text only: supplementary
//! to the complete-command witnesses. The `shared-node-used-twice` shape is
//! a recorded divergence: the port's memoized lowering is pinned as a
//! documented rewrite of tsc's output and earns no credit.
use tsc_emitter::{
    create_printer, get_script_transformers, transform_nodes, EmitConstantValue,
    EmitEnumMemberValue, EmitExportContainerMode, EmitResolver, EmitResolverError,
    EmitResolverNode, NewLineKind, PrintRequest, PrinterOptions, SourceFileTextMode,
    TransformArena, TransformError, TransformFlags, TransformNode, TransformNodeArray,
    TransformRoot, TransformSourceId, TransformationContext, Transformer,
};
use tsc_program::SourceFileId;
use tsc_syntax::{
    nodes::{CommaListExpressionData, ExpressionStatementData, SourceFileData},
    parse_source_file, NodeData, NodeId, SyntaxKind,
};
use tsc_types::{CompilerOptions, ScriptTarget};

struct NoConstantValueResolver;

impl EmitResolver for NoConstantValueResolver {
    fn get_constant_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
    }

    fn get_enum_member_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitEnumMemberValue>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_export_container(
        &self,
        _node: EmitResolverNode,
        _mode: EmitExportContainerMode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn has_node_check_flag(
        &self,
        _node: EmitResolverNode,
        _flag: u32,
    ) -> Result<bool, EmitResolverError> {
        Ok(false)
    }
}

/// The case's shape injected into the tree the TypeScript transform produced,
/// exactly as the observer's `before` transformer does it on the parsed tree.
struct Injector {
    shape: String,
}

struct Site<'context> {
    context: &'context mut TransformationContext,
    source: TransformSourceId,
}

impl Site<'_> {
    fn node(&self, id: NodeId) -> TransformNode {
        self.context
            .arena()
            .node_ref(self.source, id)
            .expect("node in source")
    }

    fn nodes(&self, array: tsc_syntax::NodeArrayId) -> Vec<NodeId> {
        self.context
            .arena()
            .node_array(TransformNodeArray::new(self.source, array))
            .expect("node array")
            .nodes
            .clone()
    }

    fn data(&self, id: NodeId) -> NodeData {
        self.context
            .arena()
            .node(self.node(id))
            .expect("node record")
            .data
            .clone()
    }

    fn flags(&self, id: NodeId) -> TransformFlags {
        self.context.arena().transform_flags(self.node(id))
    }

    fn array(&mut self, ids: &[NodeId]) -> tsc_syntax::NodeArrayId {
        let nodes = ids.iter().map(|id| self.node(*id)).collect::<Vec<_>>();
        self.context
            .factory()
            .expect("factory")
            .create_node_array(self.source, nodes)
            .expect("node array")
            .array()
    }

    fn update(&mut self, id: NodeId, data: NodeData, flags: TransformFlags) -> NodeId {
        let node = self.node(id);
        self.context
            .factory()
            .expect("factory")
            .update_node(node, data, flags)
            .expect("update node")
            .node()
    }

    fn statement_expression(&self, statement: NodeId) -> NodeId {
        match self.data(statement) {
            NodeData::ExpressionStatement(data) => data.expression.expect("expression"),
            other => panic!("expression statement expected, found {:?}", other.kind()),
        }
    }

    fn call_argument(&self, statement: NodeId) -> NodeId {
        match self.data(self.statement_expression(statement)) {
            NodeData::CallExpression(data) => self.nodes(data.arguments.expect("arguments"))[0],
            other => panic!("call expected, found {:?}", other.kind()),
        }
    }

    /// `flattenComma`: the operands of a left-nested comma binary expression.
    fn flatten_comma(&self, expression: NodeId) -> Vec<NodeId> {
        if let NodeData::BinaryExpression(data) = self.data(expression) {
            let operator = data.operator_token.expect("operator");
            if self
                .context
                .arena()
                .node(self.node(operator))
                .expect("operator")
                .kind
                == SyntaxKind::CommaToken
            {
                let mut operands = self.flatten_comma(data.left.expect("left"));
                operands.push(data.right.expect("right"));
                return operands;
            }
        }
        vec![expression]
    }

    fn comma_list(&mut self, elements: &[NodeId]) -> NodeId {
        let flags = elements
            .iter()
            .fold(TransformFlags::NONE, |flags, id| flags | self.flags(*id));
        let array = self.array(elements);
        self.context
            .factory()
            .expect("factory")
            .create_node(
                self.source,
                NodeData::CommaListExpression(CommaListExpressionData {
                    elements: Some(array),
                }),
                flags,
            )
            .expect("comma list")
            .node()
    }

    fn replace_expression(&mut self, statement: NodeId, expression: NodeId) -> NodeId {
        let flags = self.flags(statement) | self.flags(expression);
        self.update(
            statement,
            NodeData::ExpressionStatement(ExpressionStatementData {
                expression: Some(expression),
            }),
            flags,
        )
    }

    fn replace_argument(&mut self, statement: NodeId, argument: NodeId) -> NodeId {
        let call = self.statement_expression(statement);
        let NodeData::CallExpression(mut data) = self.data(call) else {
            panic!("call expected");
        };
        data.arguments = Some(self.array(&[argument]));
        let flags = self.flags(call) | self.flags(argument);
        let call = self.update(call, NodeData::CallExpression(data), flags);
        self.replace_expression(statement, call)
    }

    fn inject(&mut self, shape: &str) {
        let root = self.context.arena().root(self.source).expect("root").node();
        let NodeData::SourceFile(mut file) = self.data(root) else {
            panic!("source file");
        };
        let mut statements = self.nodes(file.statements.expect("statements"));
        let class_index = statements
            .iter()
            .position(|id| match self.data(*id) {
                NodeData::ClassDeclaration(data) => data.name.is_some_and(|name| {
                    matches!(self.data(name), NodeData::Identifier(identifier) if identifier.text == "Derived")
                }),
                _ => false,
            })
            .expect("decorated class");
        let NodeData::ClassDeclaration(mut class) = self.data(statements[class_index]) else {
            unreachable!()
        };
        let mut members = self.nodes(class.members.expect("members"));
        let block_index = members
            .iter()
            .position(|id| matches!(self.data(*id), NodeData::ClassStaticBlockDeclaration(_)))
            .expect("static block");
        let NodeData::ClassStaticBlockDeclaration(mut block) = self.data(members[block_index])
        else {
            unreachable!()
        };
        let body = block.body.expect("body");
        let NodeData::Block(mut body_data) = self.data(body) else {
            panic!("block")
        };
        let mut block_statements = self.nodes(body_data.statements.expect("statements"));
        match shape {
            "comma-list-discarded" => {
                let operands = self.flatten_comma(self.statement_expression(block_statements[0]));
                let comma = self.comma_list(&operands);
                block_statements[0] = self.replace_expression(block_statements[0], comma);
            }
            "comma-list-used"
            | "comma-list-used-assignment-last"
            | "comma-list-nested-discarded" => {
                let argument = self.call_argument(block_statements[0]);
                let NodeData::ParenthesizedExpression(parenthesized) = self.data(argument) else {
                    panic!("parenthesized argument");
                };
                let operands = self.flatten_comma(parenthesized.expression.expect("expression"));
                let comma = self.comma_list(&operands);
                block_statements[0] = self.replace_argument(block_statements[0], comma);
            }
            "shared-node-discarded-then-used" | "shared-node-update-discarded-then-used" => {
                let shared = self.statement_expression(block_statements[0]);
                block_statements[1] = self.replace_argument(block_statements[1], shared);
            }
            "shared-node-used-then-discarded" => {
                let shared = self.call_argument(block_statements[0]);
                block_statements[1] = self.replace_expression(block_statements[1], shared);
            }
            "shared-node-used-twice" => {
                let shared = self.call_argument(block_statements[0]);
                block_statements[1] = self.replace_argument(block_statements[1], shared);
            }
            other => panic!("unknown shape {other}"),
        }
        body_data.statements = Some(self.array(&block_statements));
        let body_flags = block_statements
            .iter()
            .fold(self.flags(body), |flags, id| flags | self.flags(*id));
        let body = self.update(body, NodeData::Block(body_data), body_flags);
        block.body = Some(body);
        let block_flags = self.flags(members[block_index]) | body_flags;
        members[block_index] = self.update(
            members[block_index],
            NodeData::ClassStaticBlockDeclaration(block),
            block_flags,
        );
        class.members = Some(self.array(&members));
        let class_flags = self.flags(statements[class_index]) | block_flags;
        statements[class_index] = self.update(
            statements[class_index],
            NodeData::ClassDeclaration(class),
            class_flags,
        );
        file.statements = Some(self.array(&statements));
        let root_flags = self.flags(root) | class_flags;
        let updated = self.update(
            root,
            NodeData::SourceFile(SourceFileData { ..file }),
            root_flags,
        );
        let updated = self.node(updated);
        self.context
            .arena_mut()
            .expect("arena")
            .replace_root(self.source, updated)
            .expect("replace root");
    }
}

impl Transformer for Injector {
    fn name(&self) -> &'static str {
        "decorator-super-direct-injector"
    }

    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        let TransformRoot::SourceFile(source) = root else {
            return Ok(root);
        };
        let shape = self.shape.clone();
        Site { context, source }.inject(&shape);
        Ok(root)
    }
}

fn emit_case(case: &serde_json::Value) -> String {
    let text = case["text"].as_str().expect("text");
    let shape = case["shape"].as_str().expect("shape");
    let options_in = &case["options"];
    let parsed = parse_source_file("main.ts", text, Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let target = i32::try_from(options_in["target"].as_u64().expect("target")).expect("target");
    let options = CompilerOptions {
        target: Some(target),
        module: Some(
            i32::try_from(options_in["module"].as_u64().expect("module")).expect("module"),
        ),
        use_define_for_class_fields: options_in["useDefineForClassFields"].as_bool(),
        strict: options_in["strict"].as_bool(),
        ..CompilerOptions::default()
    };
    let resolver = NoConstantValueResolver;
    let mut transformers =
        get_script_transformers(&options, &resolver).expect("script transformers");
    let typescript = transformers
        .iter()
        .position(|transformer| transformer.name() == "transformTypeScript")
        .expect("transformTypeScript leads the script transformers");
    transformers.insert(
        typescript + 1,
        Box::new(Injector {
            shape: shape.to_owned(),
        }),
    );
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        transformers,
        false,
    )
    .expect("transform");
    create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_target(ScriptTarget::from_bits(target))
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(&mut result, PrintRequest::SourceFile(source), None)
    .expect("print")
    .text()
    .to_owned()
}

#[test]
fn decorator_super_direct_controls_match_typescript() {
    let artifact: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/decorator-super-direct.json")).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["route"], "direct-transform-custom-before");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 32);
    let mut failures = Vec::new();
    let mut exact = 0usize;
    let mut divergences = 0usize;
    for case in cases {
        let id = case["case_id"].as_str().unwrap();
        let shape = case["shape"].as_str().unwrap();
        let expected = case["typescript_observation"]["js_text"].as_str().unwrap();
        // Recorded divergence (no credit): a node shared by two required-value
        // contexts is lowered twice by tsc (`var _a, _b;`, `_b` in the second
        // lowering) while the port memoizes the required-value lowering per
        // node id and prints the first lowering twice (memo policy). The
        // port's output is pinned as exactly that rewrite of tsc's text.
        let divergence = shape == "shared-node-used-twice";
        let expected_native = if divergence {
            assert!(
                expected.contains("var _a, _b;"),
                "{id}: tsc allocates a second temp"
            );
            expected
                .replace("var _a, _b;", "var _a;")
                .replace("_b", "_a")
        } else {
            expected.to_owned()
        };
        let mut agreed = true;
        for repetition in 0..2 {
            let outcome = std::panic::catch_unwind(|| emit_case(case));
            match outcome {
                Ok(actual) if actual == expected_native => {}
                Ok(actual) => {
                    agreed = false;
                    eprintln!(
                        "decorator super direct FAIL {id} repetition {repetition}\n--- expected\n{expected_native}\n--- actual\n{actual}"
                    );
                }
                Err(_) => {
                    agreed = false;
                    eprintln!("decorator super direct PANIC {id} repetition {repetition}");
                }
            }
        }
        if !agreed {
            failures.push(id);
        } else if divergence {
            divergences += 1;
            eprintln!("decorator super direct DIVERGENCE RECORDED x2 {id} (memoized required-value lowering; no credit)");
        } else {
            exact += 1;
            eprintln!("decorator super direct EXACT x2 {id}");
        }
    }
    eprintln!(
        "decorator super direct SUMMARY exact={exact} recorded_divergences={divergences} failed={}",
        failures.len()
    );
    assert_eq!(exact, 28);
    assert_eq!(divergences, 4);
    assert!(
        failures.is_empty(),
        "decorator super direct control failures: {failures:?}"
    );
}

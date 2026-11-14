use super::*;

struct RuntimeAliasResolver {
    declarations: BTreeMap<NodeId, NodeId>,
}

impl RuntimeAliasResolver {
    fn new(parsed: &tsc_syntax::SourceFile) -> Self {
        let mut classes = BTreeMap::new();
        let mut names = BTreeSet::new();
        for id in parsed.arena.node_ids() {
            if let NodeData::ClassDeclaration(data) = parsed.arena.node(id).data() {
                if let Some(name) = data.name {
                    if let NodeData::Identifier(data) = parsed.arena.node(name).data() {
                        classes.insert(data.text().to_owned(), id);
                        names.insert(name);
                    }
                }
            }
        }
        Self {
            declarations: parsed
                .arena
                .node_ids()
                .filter_map(|id| {
                    if names.contains(&id) {
                        return None;
                    }
                    let NodeData::Identifier(data) = parsed.arena.node(id).data() else {
                        return None;
                    };
                    classes.get(data.text()).map(|class| (id, *class))
                })
                .collect(),
        }
    }
}

impl EmitResolver for RuntimeAliasResolver {
    fn get_constant_value(
        &self,
        _: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
    }
    fn get_referenced_export_container(
        &self,
        _: EmitResolverNode,
        _: EmitExportContainerMode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }
    fn get_referenced_import_declaration(
        &self,
        _: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }
    fn get_referenced_value_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(self
            .declarations
            .get(&node.node())
            .map(|class| EmitResolverNode::new(node.source(), *class)))
    }
    fn has_node_check_flag(&self, _: EmitResolverNode, _: u32) -> Result<bool, EmitResolverError> {
        // Alias decisions must use the transformed value references, even
        // without the old checker's constructor-reference flags.
        Ok(false)
    }
    fn is_referenced_alias_declaration(
        &self,
        _: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(true)
    }
    fn is_value_alias_declaration(&self, _: EmitResolverNode) -> Result<bool, EmitResolverError> {
        Ok(true)
    }
}

#[test]
fn class_aliases_follow_runtime_members_like_pinned_tsgo() {
    // Expected JavaScript is copied byte for byte from the pinned tsgo output.
    macro_rules! case {
        ($name:literal) => {
            (
                $name,
                include_str!(concat!("../../fixtures/tsgo-class-alias/", $name, ".ts")),
                include_str!(concat!("../../fixtures/tsgo-class-alias/", $name, ".js")),
            )
        };
    }
    let positions: serde_json::Value = serde_json::from_str(include_str!(
        "../../fixtures/tsgo-class-alias/name-positions.json"
    ))
    .unwrap();
    let mut cases = vec![
        case!("k-alias-type-only"),
        case!("q-type-only-type-argument"),
        case!("r-value-class-reference"),
    ];
    cases.extend(positions.as_array().unwrap().iter().map(|case| {
        (
            case["name"].as_str().unwrap(),
            case["source"].as_str().unwrap(),
            case["expected"].as_str().unwrap(),
        )
    }));
    for (name, text, expected) in cases {
        let parsed = parse_source_file(name, text, ParseOptions::default(), None);
        let resolver = RuntimeAliasResolver::new(&parsed);
        let options = CompilerOptions {
            target: Some(ScriptTarget::ES2024.bits()),
            module: Some(ModuleKind::ES_NEXT.bits()),
            experimental_decorators: true,
            use_define_for_class_fields: Some(false),
            ..Default::default()
        };
        let mut arena = TransformArena::new();
        let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
        let mut transformed = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(source)],
            vec![
                transform_type_script(&options, &resolver),
                transform_legacy_decorators(&options, &resolver),
                transform_class_fields(&options, &resolver),
            ],
            false,
        )
        .unwrap();
        let output = create_printer(
            PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2024),
        )
        .print(&mut transformed, PrintRequest::SourceFile(source), None)
        .unwrap();
        assert_eq!(output.text(), expected, "{name}");
    }
}

use super::*;
use tsc_syntax::SyntaxKind;

struct NamespaceLineResolver {
    references: BTreeMap<NodeId, NodeId>,
    jsx_scopes: BTreeMap<NodeId, NodeId>,
}

impl NamespaceLineResolver {
    fn new(parsed: &tsc_syntax::SourceFile) -> Self {
        let container = parsed.arena.node_ids().find(|id| {
            matches!(
                parsed.arena.node(*id).kind,
                SyntaxKind::ModuleDeclaration | SyntaxKind::EnumDeclaration
            )
        });
        let references = parsed
            .arena
            .node_ids()
            .filter_map(|id| {
                let NodeData::Identifier(data) = parsed.arena.node(id).data() else {
                    return None;
                };
                if matches!(data.text(), "a" | "A") {
                    container.map(|container| (id, container))
                } else {
                    None
                }
            })
            .collect();
        let mut jsx_scopes = BTreeMap::new();
        for id in parsed.arena.node_ids() {
            let mut parent = parsed.arena.node(id).parent;
            while let Some(ancestor) = parent {
                let record = parsed.arena.node(ancestor);
                if record.kind == SyntaxKind::ModuleDeclaration {
                    jsx_scopes.insert(id, ancestor);
                    break;
                }
                parent = record.parent;
            }
        }
        Self {
            references,
            jsx_scopes,
        }
    }
}

impl EmitResolver for NamespaceLineResolver {
    fn get_jsx_factory_import_declaration(
        &self,
        _location: EmitResolverNode,
        _name: &str,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn get_jsx_factory_export_container(
        &self,
        location: EmitResolverNode,
        name: &str,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        if !matches!(name, "React" | "h" | "frag") {
            return Ok(None);
        }
        Ok(self
            .jsx_scopes
            .get(&location.node())
            .map(|container| EmitResolverNode::new(location.source(), *container)))
    }

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

    fn get_referenced_value_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_export_container(
        &self,
        node: EmitResolverNode,
        _mode: EmitExportContainerMode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(self
            .references
            .get(&node.node())
            .map(|container| EmitResolverNode::new(node.source(), *container)))
    }

    fn get_referenced_import_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn is_instantiated_module(&self, _node: EmitResolverNode) -> Result<bool, EmitResolverError> {
        Ok(true)
    }

    fn is_referenced_alias_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(true)
    }

    fn is_value_alias_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(true)
    }
}

#[test]
fn substituted_expression_line_breaks_match_pinned_tsgo() {
    // Expected JavaScript is the pinned tsgo's output, byte for byte.
    macro_rules! case {
        ($name:literal) => {
            (
                $name,
                include_str!(concat!(
                    "../../fixtures/tsgo-substituted-lines/",
                    $name,
                    ".ts"
                )),
                include_str!(concat!(
                    "../../fixtures/tsgo-substituted-lines/",
                    $name,
                    ".js"
                )),
            )
        };
    }
    for (name, text, expected) in [
        case!("m-namespace-conditional"),
        case!("n-plain-conditional"),
        case!("u-namespace-binary"),
        case!("v-enum-conditional"),
    ] {
        let parsed = parse_source_file(name, text, ParseOptions::default(), None);
        let resolver = NamespaceLineResolver::new(&parsed);
        let options = CompilerOptions {
            target: Some(ScriptTarget::ES2024.bits()),
            module: Some(ModuleKind::ES_NEXT.bits()),
            ..Default::default()
        };
        let mut arena = TransformArena::new();
        let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
        let mut transformed = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(source)],
            vec![
                transform_type_script(&options, &resolver),
                transform_ecmascript_module(&options),
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

#[test]
fn namespace_jsx_factories_created_after_typescript_match_pinned_tsgo() {
    // Expected text is the pinned tsgo binary's output; the real-checker
    // compiler test also covers shadowed roots, imports, helpers and contexts.
    macro_rules! case {
        ($name:literal, $target:literal, $module:literal, $factory:expr, $fragment:expr) => {
            (
                $name,
                $target,
                $module,
                $factory,
                $fragment,
                include_str!(concat!(
                    "../../fixtures/tsgo-late-namespace-references/",
                    $name,
                    ".tsx"
                )),
                include_str!(concat!(
                    "../../fixtures/tsgo-late-namespace-references/",
                    $name,
                    ".js"
                )),
            )
        };
    }
    let cases: [(_, _, _, Option<&str>, Option<&str>, _, _); 6] = [
        case!("tsxReactEmit6", 2, 1, None, None),
        case!("fragment-namespace", 11, 99, None, None),
        case!("custom-factories", 11, 99, Some("h"), Some("frag")),
        case!("renamed-namespace", 11, 99, None, None),
        case!("later-helpers", 11, 99, None, None),
        case!("nested-namespace", 11, 99, None, None),
    ];
    for (name, target, module, factory, fragment, text, expected) in cases {
        let parsed = parse_source_file(
            name,
            text,
            ParseOptions {
                script_kind: ScriptKind::Tsx,
                ..Default::default()
            },
            None,
        );
        let resolver = NamespaceLineResolver::new(&parsed);
        let options = CompilerOptions {
            target: Some(target),
            module: Some(module),
            jsx: Some(2),
            jsx_factory: factory.map(Into::into),
            jsx_fragment_factory: fragment.map(Into::into),
            strict: Some(true),
            always_strict: Some(true),
            ..Default::default()
        };
        let mut arena = TransformArena::new();
        let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
        let host = ModuleFactoryHost::new(options.clone(), Some(module));
        let mut transformed = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(source)],
            super::super::get_script_transformers_for_source(
                &options,
                &resolver,
                &host,
                SourceFileId::from_raw(0),
            )
            .unwrap(),
            false,
        )
        .unwrap();
        let output = create_printer(
            PrinterOptions::new(NewLineKind::LineFeed).with_target(options.emit_script_target()),
        )
        .print(&mut transformed, PrintRequest::SourceFile(source), None)
        .unwrap();
        assert_eq!(output.text(), expected, "{name}");
    }
}

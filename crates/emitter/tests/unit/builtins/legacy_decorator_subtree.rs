use super::*;

#[test]
fn decorator_free_class_syntax_keeps_identity_and_ranges() {
    let parsed = parse_source_file(
        "plain.ts",
        concat!(
            "// before class\n",
            "class Plain { static [key()] = this; method() { return this; } }\n",
            "// before function\n",
            "function plain() { if (ready) run(); }\n",
        ),
        ParseOptions::default(),
        None,
    );
    let resolver = EnumBindingResolver::new(&parsed);
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2022.bits()),
        experimental_decorators: true,
        ..CompilerOptions::default()
    };
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    initialize_transform_flags(&mut arena, source).unwrap();
    let root = arena.root(source).unwrap();
    let flags = arena.transform_flags(root);
    let class = parsed
        .arena
        .node_ids()
        .find(|id| matches!(parsed.arena.node(*id).data(), NodeData::ClassDeclaration(_)))
        .unwrap();
    let class = arena.node_ref(source, class).unwrap();
    assert!(arena
        .transform_flags(class)
        .contains(TransformFlags::CONTAINS_TYPE_SCRIPT_CLASS_SYNTAX));
    assert!(!flags.contains(TransformFlags::CONTAINS_DECORATORS));
    let before = arena.source(source).unwrap().syntax().arena.nodes().len();
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![transform_legacy_decorators(&options, &resolver)],
        false,
    )
    .unwrap();
    assert_eq!(result.arena().root(source).unwrap(), root);
    assert_eq!(
        result
            .arena()
            .source(source)
            .unwrap()
            .syntax()
            .arena
            .nodes()
            .len(),
        before
    );
    for id in parsed.arena.node_ids() {
        let node = result.arena().node_ref(source, id).unwrap();
        let record = result.arena().node(node).unwrap();
        let original = parsed.arena.node(id);
        assert_eq!((record.pos, record.end), (original.pos, original.end));
        assert!(result.arena().metadata(node).is_none());
    }
    let output = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .unwrap()
        .text()
        .to_owned();
    assert!(output.contains("// before class\nclass Plain"), "{output}");
    assert!(
        output.contains("// before function\nfunction plain"),
        "{output}"
    );
}

#[test]
fn incomplete_synthetic_class_still_lowers_legacy_decorators() {
    let parsed = parse_source_file(
        "synthetic.ts",
        "@dec class C {}",
        ParseOptions::default(),
        None,
    );
    let resolver = EnumBindingResolver::new(&parsed);
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        experimental_decorators: true,
        ..CompilerOptions::default()
    };
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    initialize_transform_flags(&mut arena, source).unwrap();
    let root = arena.root(source).unwrap();
    let mut root_data = arena.node(root).unwrap().as_source_file().unwrap();
    let statements = arena
        .node_array_ref(source, root_data.statements.unwrap())
        .unwrap();
    let class = arena.node_array(statements).unwrap().nodes[0];
    let class = arena.node_ref(source, class).unwrap();
    let data = arena.node(class).unwrap().data();
    let flags = arena.transform_flags(root);
    let synthetic = arena
        .factory()
        .create_node(source, data, TransformFlags::NONE)
        .unwrap();
    arena.set_original_node(synthetic, Some(class)).unwrap();
    arena.factory().set_text_range(synthetic, class).unwrap();
    assert!(!arena.transform_flags_complete(synthetic));
    assert_eq!(arena.transform_flags(synthetic), TransformFlags::NONE);
    let statements = arena
        .factory()
        .create_node_array(source, vec![synthetic])
        .unwrap();
    root_data.statements = Some(statements.array());
    let root = arena
        .factory()
        .update_node(root, NodeData::SourceFile(root_data), flags)
        .unwrap();
    arena.replace_root(source, root).unwrap();
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![transform_legacy_decorators(&options, &resolver)],
        false,
    )
    .unwrap();
    let output = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .unwrap()
        .text()
        .to_owned();
    assert!(
        output.contains("C = __decorate([\n    dec\n], C);"),
        "{output}"
    );
    assert!(!output.contains("@dec"), "{output}");
}

struct ConstructorReferenceResolver {
    classes: BTreeSet<NodeId>,
    references: BTreeMap<NodeId, NodeId>,
}

impl EmitResolver for ConstructorReferenceResolver {
    fn get_constant_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_value_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(self
            .references
            .get(&node.node())
            .map(|id| EmitResolverNode::new(node.source(), *id)))
    }

    fn has_node_check_flag(
        &self,
        node: EmitResolverNode,
        flag: u32,
    ) -> Result<bool, EmitResolverError> {
        Ok(
            (flag == NodeCheckFlags::CONTAINS_CONSTRUCTOR_REFERENCE.bits() as u32
                && self.classes.contains(&node.node()))
                || (flag == NodeCheckFlags::CONSTRUCTOR_REFERENCE.bits() as u32
                    && self.references.contains_key(&node.node())),
        )
    }
}

#[test]
fn nested_decorator_free_bodies_keep_enclosing_class_aliases_and_name_order() {
    let parsed = parse_source_file(
        "aliases.ts",
        concat!(
            "function plainBefore() { return 1; }\n",
            "@dec class C {\n",
            " static self = C;\n",
            " method() {\n",
            "  function read() { return C; }\n",
            "  class Plain { value = C; }\n",
            "  return class { value = C; };\n",
            " }\n",
            " @dec [key()]() {}\n",
            "}\n",
            "@dec class D { static self = D; @dec [key()]() {} }\n",
            "// untouched sibling\n",
            "function plain() { if (ready) run(); }\n",
        ),
        ParseOptions::default(),
        None,
    );
    let mut declarations = BTreeMap::new();
    let mut declaration_names = BTreeSet::new();
    for id in parsed.arena.node_ids() {
        let NodeData::ClassDeclaration(data) = parsed.arena.node(id).data() else {
            continue;
        };
        let Some(name) = data.name else {
            continue;
        };
        let NodeData::Identifier(data) = parsed.arena.node(name).data() else {
            continue;
        };
        declarations.insert(data.text().to_owned(), id);
        declaration_names.insert(name);
    }
    let references = parsed
        .arena
        .node_ids()
        .filter_map(|id| {
            if declaration_names.contains(&id) {
                return None;
            }
            let NodeData::Identifier(data) = parsed.arena.node(id).data() else {
                return None;
            };
            declarations
                .get(data.text())
                .map(|declaration| (id, *declaration))
        })
        .collect();
    let resolver = ConstructorReferenceResolver {
        classes: declarations.values().copied().collect(),
        references,
    };
    for target in [ScriptTarget::ES2015, ScriptTarget::ES2022] {
        let options = CompilerOptions {
            target: Some(target.bits()),
            experimental_decorators: true,
            ..CompilerOptions::default()
        };
        let mut arena = TransformArena::new();
        let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
        initialize_transform_flags(&mut arena, source).unwrap();
        let plain = parsed.arena.node_ids().find(|id| {
            matches!(parsed.arena.node(*id).data(), NodeData::FunctionDeclaration(data)
                if data.name.is_some_and(|name| matches!(parsed.arena.node(name).data(), NodeData::Identifier(data) if data.text() == "plain")))
        }).unwrap();
        let mut result = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(source)],
            vec![transform_legacy_decorators(&options, &resolver)],
            false,
        )
        .unwrap();
        let root = result.arena().root(source).unwrap();
        let statements = result
            .arena()
            .node(root)
            .unwrap()
            .as_source_file()
            .unwrap()
            .statements
            .unwrap();
        let statements = result.arena().node_array_ref(source, statements).unwrap();
        assert_eq!(
            result.arena().node_array(statements).unwrap().nodes.last(),
            Some(&plain)
        );
        let output = create_printer(PrinterOptions::new(NewLineKind::LineFeed).with_target(target))
            .print(&mut result, PrintRequest::SourceFile(source), None)
            .unwrap()
            .text()
            .to_owned();
        assert!(output.contains("var C_1, D_1;"), "{target:?}: {output}");
        assert!(output.contains("return C_1;"), "{target:?}: {output}");
        assert_eq!(
            output.matches("value = C_1;").count(),
            2,
            "{target:?}: {output}"
        );
        assert!(
            output.contains("C = C_1 = __decorate("),
            "{target:?}: {output}"
        );
        assert!(
            output.contains("D = D_1 = __decorate("),
            "{target:?}: {output}"
        );
        assert!(output.contains("[_a = key()]"), "{target:?}: {output}");
        assert!(output.contains("[_b = key()]"), "{target:?}: {output}");
        assert!(
            output.contains("C.prototype, _a, null"),
            "{target:?}: {output}"
        );
        assert!(
            output.contains("D.prototype, _b, null"),
            "{target:?}: {output}"
        );
        assert!(
            output.contains("// untouched sibling\nfunction plain"),
            "{target:?}: {output}"
        );
    }
}

struct DetachedCommentResolver {
    declarations: BTreeMap<NodeId, NodeId>,
    classes: BTreeSet<NodeId>,
    type_imports: BTreeSet<NodeId>,
}

impl DetachedCommentResolver {
    fn new(parsed: &tsc_syntax::SourceFile) -> Self {
        let mut names = BTreeMap::new();
        let mut declaration_names = BTreeSet::new();
        let mut type_imports = BTreeSet::new();
        for id in parsed.arena.node_ids() {
            match parsed.arena.node(id).data() {
                NodeData::ClassDeclaration(data) => {
                    if let Some(name) = data.name {
                        if let NodeData::Identifier(data) = parsed.arena.node(name).data() {
                            names.insert(data.text().to_owned(), id);
                            declaration_names.insert(name);
                        }
                    }
                }
                NodeData::ImportSpecifier(data)
                    if data.name.is_some_and(|name| {
                        matches!(parsed.arena.node(name).data(), NodeData::Identifier(data) if data.text() == "I")
                    }) =>
                {
                    type_imports.insert(id);
                }
                _ => {}
            }
        }
        let declarations: BTreeMap<_, _> = parsed
            .arena
            .node_ids()
            .filter_map(|id| {
                if declaration_names.contains(&id) {
                    return None;
                }
                let NodeData::Identifier(data) = parsed.arena.node(id).data() else {
                    return None;
                };
                names.get(data.text()).map(|declaration| (id, *declaration))
            })
            .collect();
        Self {
            classes: declarations.values().copied().collect(),
            declarations,
            type_imports,
        }
    }
}

impl EmitResolver for DetachedCommentResolver {
    fn get_constant_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_value_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(self
            .declarations
            .get(&node.node())
            .map(|declaration| EmitResolverNode::new(node.source(), *declaration)))
    }

    fn get_referenced_export_container(
        &self,
        _node: EmitResolverNode,
        _mode: EmitExportContainerMode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_import_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn has_node_check_flag(
        &self,
        node: EmitResolverNode,
        flag: u32,
    ) -> Result<bool, EmitResolverError> {
        Ok(
            (flag == NodeCheckFlags::CONTAINS_CONSTRUCTOR_REFERENCE.bits() as u32
                && self.classes.contains(&node.node()))
                || (flag == NodeCheckFlags::CONSTRUCTOR_REFERENCE.bits() as u32
                    && self.declarations.contains_key(&node.node())),
        )
    }

    fn is_referenced_alias_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(!self.type_imports.contains(&node.node()))
    }

    fn is_value_alias_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(!self.type_imports.contains(&node.node()))
    }
}

#[test]
fn hoisted_decorator_comments_match_pinned_tsgo() {
    // Expected JavaScript is copied byte for byte from the pinned tsgo output.
    macro_rules! case {
        ($name:literal) => {
            (
                $name,
                include_str!(concat!(
                    "../../fixtures/tsgo-detached-comments/",
                    $name,
                    ".ts"
                )),
                include_str!(concat!(
                    "../../fixtures/tsgo-detached-comments/",
                    $name,
                    ".js"
                )),
            )
        };
    }
    for (name, text, expected) in [
        case!("a-helpers-kept-import"),
        case!("e-hoisted-var"),
        case!("g-hoisted-elided-import"),
        case!("h-hoisted-eof-comments"),
        case!("i-hoisted-no-header"),
        case!("j-hoisted-attached-header"),
        case!("o-hoisted-computed-key"),
    ] {
        let parsed = parse_source_file(name, text, ParseOptions::default(), None);
        let resolver = DetachedCommentResolver::new(&parsed);
        let options = CompilerOptions {
            target: Some(ScriptTarget::ES2024.bits()),
            module: Some(ModuleKind::ES_NEXT.bits()),
            experimental_decorators: true,
            use_define_for_class_fields: Some(false),
            ..CompilerOptions::default()
        };
        let mut arena = TransformArena::new();
        let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
        let mut result = transform_nodes(
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
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .unwrap();
        assert_eq!(output.text(), expected, "{name}");
    }
}

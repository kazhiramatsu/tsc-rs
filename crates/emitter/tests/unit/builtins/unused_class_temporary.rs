use super::*;
use tsc_syntax::SyntaxKind;

struct CurrentClassReferenceResolver {
    classes: BTreeSet<NodeId>,
    declarations: BTreeMap<NodeId, NodeId>,
    containing_members: BTreeSet<NodeId>,
}

impl CurrentClassReferenceResolver {
    fn new(parsed: &tsc_syntax::SourceFile) -> Self {
        let mut names = BTreeMap::new();
        let mut declaration_names = BTreeSet::new();
        for id in parsed.arena.node_ids() {
            if let NodeData::ClassDeclaration(data) = parsed.arena.node(id).data() {
                if let Some(name) = data.name {
                    if let NodeData::Identifier(data) = parsed.arena.node(name).data() {
                        names.insert(data.text().to_owned(), id);
                        declaration_names.insert(name);
                    }
                }
            }
        }
        let mut declarations = BTreeMap::new();
        let mut containing_members = BTreeSet::new();
        for id in parsed.arena.node_ids() {
            if declaration_names.contains(&id) {
                continue;
            }
            let NodeData::Identifier(data) = parsed.arena.node(id).data() else {
                continue;
            };
            let Some(class) = names.get(data.text()) else {
                continue;
            };
            declarations.insert(id, *class);
            let mut parent = parsed.arena.node(id).parent;
            while let Some(id) = parent {
                if matches!(
                    parsed.arena.node(id).kind,
                    SyntaxKind::PropertyDeclaration
                        | SyntaxKind::MethodDeclaration
                        | SyntaxKind::GetAccessor
                        | SyntaxKind::SetAccessor
                ) {
                    containing_members.insert(id);
                }
                parent = parsed.arena.node(id).parent;
            }
        }
        Self {
            classes: names.values().copied().collect(),
            declarations,
            containing_members,
        }
    }
}

impl EmitResolver for CurrentClassReferenceResolver {
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
                && (self.classes.contains(&node.node())
                    || self.containing_members.contains(&node.node())))
                || (flag == NodeCheckFlags::CONSTRUCTOR_REFERENCE.bits() as u32
                    && self.declarations.contains_key(&node.node())),
        )
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
fn class_temporaries_follow_surviving_references_like_pinned_tsgo() {
    // Expected JavaScript is the pinned tsgo's output, byte for byte.
    macro_rules! case {
        ($name:literal) => {
            (
                $name,
                include_str!(concat!(
                    "../../fixtures/tsgo-class-temporary/",
                    $name,
                    ".ts"
                )),
                include_str!(concat!(
                    "../../fixtures/tsgo-class-temporary/",
                    $name,
                    ".js"
                )),
            )
        };
    }
    for (name, text, expected) in [
        case!("l-unused-temp"),
        case!("s-decorated-private-method"),
        case!("t-retained-private-reference"),
        case!("u-private-type-only"),
    ] {
        let parsed = parse_source_file(name, text, ParseOptions::default(), None);
        let resolver = CurrentClassReferenceResolver::new(&parsed);
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

use std::collections::BTreeSet;

use tsc_syntax::parse_source_file;

use super::{
    allocate_ordinary_temp_name, AncestorBindingPolicy, GeneratedBindingScopes,
    OrdinaryTempNamePolicy, ParsedSourceIdentifierNames, TransformArena,
};

#[test]
fn traversal_temp_policy_uses_final_scope_cursor() {
    let mut scopes =
        GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::AllowShadow);

    assert_eq!(
        allocate_ordinary_temp_name(
            &mut scopes,
            "_d".into(),
            false,
            OrdinaryTempNamePolicy::FinalizerTraversal,
        ),
        "_a",
    );
    assert_eq!(scopes.allocate_temp(), "_b");
}

#[test]
fn authoritative_temp_policy_retains_available_planned_spelling() {
    let mut scopes =
        GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::AllowShadow);

    assert_eq!(
        allocate_ordinary_temp_name(
            &mut scopes,
            "_d".into(),
            false,
            OrdinaryTempNamePolicy::PlannedSpellingAuthoritative,
        ),
        "_d",
    );
    assert_eq!(scopes.allocate_temp(), "_a");
}

#[test]
fn authoritative_temp_policy_falls_back_on_collision() {
    let mut scopes = GeneratedBindingScopes::new(
        BTreeSet::from(["_d".to_owned()]),
        AncestorBindingPolicy::AllowShadow,
    );

    assert_eq!(
        allocate_ordinary_temp_name(
            &mut scopes,
            "_d".into(),
            true,
            OrdinaryTempNamePolicy::PlannedSpellingAuthoritative,
        ),
        "_a",
    );
}

#[test]
fn parsed_identifier_snapshot_retains_erased_file_level_collisions() {
    let parsed = parse_source_file(
        "file-level-collisions.ts",
        concat!(
            "type _default = number;\n",
            "interface _default_1 {}\n",
            "declare const _default_2: number;\n",
        ),
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let names =
        ParsedSourceIdentifierNames::collect(&arena, source).expect("parsed identifier snapshot");

    assert_eq!(names.optimistic_candidate("_default"), "_default_3");
    assert_eq!(
        names.optimistic_candidate("_default"),
        "_default_3",
        "candidate lookup is immutable for independent FileLevel IDs",
    );
}

#[test]
fn declaration_names_precede_uses_in_source_module_and_function_scopes() {
    use crate::{
        create_printer, transform_nodes, NewLineKind, PrintRequest, PrinterOptions,
        SourceFileTextMode, TransformNode, TransformRoot,
    };
    use crate::{TransformError, TransformNodeArray, TransformSourceId};
    use tsc_syntax::{NodeArrayId, NodeData, NodeDataChildVisitor, NodeId, SyntaxKind};
    struct ReplaceBindings<'a> {
        arena: &'a mut TransformArena,
        source: TransformSourceId,
        first: TransformNode,
        second: TransformNode,
    }
    impl NodeDataChildVisitor for ReplaceBindings<'_> {
        type Error = TransformError;
        fn node_kind(&self, id: NodeId) -> SyntaxKind {
            self.arena
                .node(TransformNode::new(self.source, id))
                .unwrap()
                .kind
        }
        fn visit_node(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
            let node = TransformNode::new(self.source, id);
            let mut data = self.arena.node(node)?.data.clone();
            if let NodeData::Identifier(data) = &data {
                if data.text == "firstBinding" {
                    return Ok(Some(self.first.node()));
                }
                if data.text == "secondBinding" {
                    return Ok(Some(self.second.node()));
                }
            }
            tsc_syntax::try_visit_each_child(&mut data, self)?;
            let flags = self.arena.transform_flags(node);
            Ok(Some(
                self.arena.factory().update_node(node, data, flags)?.node(),
            ))
        }
        fn visit_nodes(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, TransformError> {
            let array = TransformNodeArray::new(self.source, id);
            let ids = self.arena.node_array(array)?.nodes.clone();
            let mut nodes = Vec::new();
            for id in ids {
                let id = self.visit_node(id)?.unwrap();
                nodes.push(TransformNode::new(self.source, id));
            }
            Ok(Some(
                self.arena
                    .factory()
                    .update_node_array(array, nodes)?
                    .array(),
            ))
        }
        fn required_child_removed(
            &mut self,
            parent: SyntaxKind,
            field: &'static str,
        ) -> TransformError {
            TransformError::RequiredChildRemoved { parent, field }
        }
    }
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../fixtures/utf16-generated-declaration-order.json"
    ))
    .unwrap();
    assert_eq!(fixture["repetitions"], 2);
    for case in fixture["cases"].as_array().unwrap() {
        let parsed = parse_source_file(
            "main.ts",
            case["source"].as_str().unwrap(),
            Default::default(),
            None,
        );
        let mut arena = TransformArena::new();
        let source = arena.add_source(&parsed, None);
        // The TS witness creates fresh identifiers and shares each node
        // across its uses. Keep parsed identifiers immutable so a second
        // print cannot reserve the first print's generated spelling as a
        // source-language collision.
        let mut generated = Vec::new();
        for _ in 0..2 {
            let binding = arena.allocate_generated_binding_id();
            let node = arena
                .factory()
                .create_identifier(source, "templateObject")
                .unwrap();
            let metadata = arena.metadata_mut(node);
            metadata.set_generated_binding_id(binding);
            metadata.set_generated_binding_base("templateObject");
            generated.push(node);
        }
        let root = arena.root(source).unwrap();
        let rewritten = ReplaceBindings {
            arena: &mut arena,
            source,
            first: generated[0],
            second: generated[1],
        }
        .visit_node(root.node())
        .unwrap()
        .unwrap();
        arena
            .replace_root(source, TransformNode::new(source, rewritten))
            .unwrap();
        let mut result = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(source)],
            Vec::new(),
            false,
        )
        .unwrap();
        for _ in 0..2 {
            let printed = create_printer(
                PrinterOptions::new(NewLineKind::LineFeed)
                    .with_declaration_syntax(true)
                    .with_source_file_text_mode(SourceFileTextMode::Canonical),
            )
            .print(&mut result, PrintRequest::SourceFile(source), None)
            .unwrap();
            assert_eq!(
                printed.text(),
                case["expected"].as_str().unwrap(),
                "{}",
                case["id"]
            );
        }
    }
}

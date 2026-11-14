use std::collections::BTreeSet;

use tsc_emitter::{
    create_printer, get_script_transformers, transform_nodes, EmitConstantValue,
    EmitExportContainerMode, EmitResolver, EmitResolverError, EmitResolverNode, NewLineKind,
    PrintRequest, PrinterOptions, SourceFileId, TransformArena, TransformRoot,
};
use tsc_syntax::{parse_source_file, NodeData, NodeId};
use tsc_types::{CompilerOptions, ModuleKind, NodeCheckFlags, ScriptTarget};

struct ClassReferenceResolver {
    class: NodeId,
    references: BTreeSet<NodeId>,
}

impl EmitResolver for ClassReferenceResolver {
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
            .references
            .contains(&node.node())
            .then(|| EmitResolverNode::new(node.source(), self.class)))
    }

    fn has_node_check_flag(
        &self,
        node: EmitResolverNode,
        flag: u32,
    ) -> Result<bool, EmitResolverError> {
        Ok((node.node() == self.class
            && flag == NodeCheckFlags::CONTAINS_CONSTRUCTOR_REFERENCE.bits() as u32)
            || (self.references.contains(&node.node())
                && flag == NodeCheckFlags::CONSTRUCTOR_REFERENCE.bits() as u32))
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
fn eof_comment_after_generated_class_export_matches_pinned_tsgo() {
    let parsed = parse_source_file(
        "class-alias-min.ts",
        include_str!("fixtures/tsgo-detached-comments/class-alias-min.ts"),
        Default::default(),
        None,
    );
    let (class, name) = parsed
        .arena
        .node_ids()
        .find_map(|id| {
            let NodeData::ClassDeclaration(data) = parsed.arena.node(id).data() else {
                return None;
            };
            Some((id, data.name.unwrap()))
        })
        .unwrap();
    let resolver = ClassReferenceResolver {
        class,
        references: parsed.arena.node_ids().filter(|id| {
            *id != name && matches!(parsed.arena.node(*id).data(), NodeData::Identifier(data) if data.text() == "A")
        }).collect(),
    };
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2024.bits()),
        module: Some(ModuleKind::PRESERVE.bits()),
        experimental_decorators: true,
        use_define_for_class_fields: Some(false),
        ..Default::default()
    };
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let mut transformed = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &resolver).unwrap(),
        false,
    )
    .unwrap();
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2024),
    )
    .print(&mut transformed, PrintRequest::SourceFile(source), None)
    .unwrap();
    // Expected text is the pinned tsgo's output, byte for byte (note 28 fixture).
    assert_eq!(
        output.text(),
        include_str!("fixtures/tsgo-detached-comments/class-alias-min.js")
    );
}

// List and ordinary node comments retain distinct flag ownership.
// The complete printer outputs are frozen from the pinned TypeScript printer.
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
    body_range: Option<(bool, bool)>,
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
        if let NodeData::FunctionDeclaration(mut function) = arena.node(statement)?.data.clone() {
            let mut statement = statement;
            let mut body = arena.node_ref(source, function.body.unwrap()).unwrap();
            if let Some((keep_start, keep_end)) = self.body_range {
                let NodeData::Block(mut block) = arena.node(body)?.data.clone() else {
                    unreachable!()
                };
                let original = arena
                    .node_array_ref(source, block.statements.unwrap())
                    .unwrap();
                let original = arena.node_array(original)?;
                let (pos, end) = (original.pos, original.end);
                let children = original
                    .nodes
                    .iter()
                    .map(|&n| arena.node_ref(source, n).unwrap())
                    .collect();
                let mut root_children: Vec<_> = arena
                    .node_array(statements)?
                    .nodes
                    .iter()
                    .map(|&n| arena.node_ref(source, n).unwrap())
                    .collect();
                let mut file = file.clone();
                let body_flags = arena.transform_flags(body);
                let function_flags = arena.transform_flags(statement);
                let file_flags = arena.transform_flags(root_node);
                let array = context.factory()?.create_node_array(source, children)?;
                context.factory()?.set_node_array_text_range(
                    array,
                    if keep_start { pos } else { u32::MAX },
                    if keep_end { end } else { u32::MAX },
                )?;
                block.statements = Some(array.array());
                body = context
                    .factory()?
                    .update_node(body, NodeData::Block(block), body_flags)?;
                function.body = Some(body.node());
                statement = context.factory()?.update_node(
                    statement,
                    NodeData::FunctionDeclaration(function),
                    function_flags,
                )?;
                root_children[0] = statement;
                let array = context
                    .factory()?
                    .update_node_array(statements, root_children)?;
                file.statements = Some(array.array());
                let updated = context.factory()?.update_node(
                    root_node,
                    NodeData::SourceFile(file),
                    file_flags,
                )?;
                context.arena_mut()?.replace_root(source, updated)?;
            }
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


fn main() {
 let parsed=parse_source_file("list-comments.ts","function f() {\n// head\n\n// tail\n}\n",Default::default(),None);
 let mut arena=TransformArena::new();let source=arena.add_source(&parsed,None);
 let mut result=transform_nodes(arena,vec![TransformRoot::SourceFile(source)],vec![Box::new(FlagTransformer{flags:EmitFlags::NONE,parent:false,parent_and_body:false,clone_name:false,body_range:Some((false,false))})],false).unwrap();
 let printed=create_printer(PrinterOptions::new(NewLineKind::LineFeed).with_source_file_text_mode(SourceFileTextMode::Canonical)).print(&mut result,PrintRequest::SourceFile(source),None).unwrap();
 print!("{}",printed.text());
}

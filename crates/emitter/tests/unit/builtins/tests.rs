use std::collections::{BTreeMap, BTreeSet};

use tsc_program::SourceFileId;
use tsc_syntax::{
    for_each_child, parse_source_file, LanguageVariant, NodeData, NodeId, ParseOptions,
};
use tsc_types::{CompilerOptions, ModuleKind, NodeCheckFlags, ScriptTarget};

use super::{
    constructor_prologue,
    es2017::transform_es2017,
    es2021::{transform_es2016, transform_es2020},
    es_next::transform_es_next,
    get_script_transformers, initialize_transform_flags,
    jsx::transform_jsx,
    legacy_decorators::transform_legacy_decorators,
    standard_decorators::transform_standard_decorators,
    system::transform_system_module,
    transform_class_fields, transform_module, transform_type_script,
    CommonJsFileLevelGeneratedBindingExports,
};
use crate::{
    create_printer, transform_nodes, DisabledSourceMapRecorder, EmitConstantValue,
    EmitEnumMemberValue, EmitExportContainerMode, EmitFlags, EmitResolver, EmitResolverError,
    EmitResolverNode, JavaScriptNumber, NewLineKind, PrintRequest, PrinterOptions, TransformArena,
    TransformFlags, TransformRoot,
};

struct EnumBindingResolver {
    declarations_by_name: BTreeMap<NodeId, NodeId>,
    enum_member_values: BTreeMap<NodeId, EmitEnumMemberValue>,
    loop_scoped_private_names: BTreeSet<NodeId>,
}

struct ExportedVariableResolver {
    declaration_by_reference: BTreeMap<NodeId, NodeId>,
    direct_export_references: BTreeMap<NodeId, NodeId>,
}

/// Minimal binder projection for recovery tests whose syntactic export is
/// embedded below the SourceFile statement list. Production module-info
/// collection must stay shallow; the resolver, like tsc's checker, can still
/// identify a reference whose export container is the source file.
struct SourceExportContainerResolver {
    containers_by_reference: BTreeMap<NodeId, NodeId>,
}

/// Resolver projection for a named default declaration merged with a
/// namespace. The checker reports the SourceFile as the export container for
/// the namespace IIFE's parsed `Foo` references even though the declaration's
/// syntactic export name is `default`.
struct SourceNamedExportContainerResolver {
    containers_by_reference: BTreeMap<NodeId, NodeId>,
}

struct ImportEqualsCallResolver {
    declaration_by_reference: BTreeMap<NodeId, NodeId>,
}

struct DefaultImportCallResolver {
    declaration_by_reference: BTreeMap<NodeId, NodeId>,
}

struct ConstructorReferenceResolver {
    class: NodeId,
    private_method: NodeId,
    constructor_references: BTreeSet<NodeId>,
}

struct AmbientFunctionExportResolver {
    declaration_by_reference: BTreeMap<NodeId, NodeId>,
}

#[test]
fn parsed_private_expression_flags_exclude_declaration_names() {
    let parsed = parse_source_file(
        "private-expression-flags.ts",
        concat!(
            "declare const dec: any, receiver: any;\n",
            "class C {\n",
            "    static #field = 1;\n",
            "    @dec(receiver.#field) first() {}\n",
            "    @dec(#field in receiver) second() {}\n",
            "    @dec(class { #nested; }) third() {}\n",
            "    @dec(receiver[#field]) fourth() {}\n",
            "}\n",
        ),
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    initialize_transform_flags(&mut arena, source).expect("initialize private expression flags");

    let mut declaration = None;
    let mut property_access = None;
    let mut private_in = None;
    let mut decorators = Vec::new();
    let mut stack = vec![parsed.root];
    while let Some(id) = stack.pop() {
        let record = parsed.arena.node(id);
        match &record.data {
            NodeData::PropertyDeclaration(data)
                if data.name.is_some_and(|name| {
                    parsed.arena.node(name).kind == tsc_syntax::SyntaxKind::PrivateIdentifier
                }) =>
            {
                declaration = arena.node_ref(source, id);
            }
            NodeData::PropertyAccessExpression(data)
                if data.name.is_some_and(|name| {
                    parsed.arena.node(name).kind == tsc_syntax::SyntaxKind::PrivateIdentifier
                }) =>
            {
                property_access = arena.node_ref(source, id);
            }
            NodeData::BinaryExpression(data)
                if data.left.is_some_and(|left| {
                    parsed.arena.node(left).kind == tsc_syntax::SyntaxKind::PrivateIdentifier
                }) && data.operator_token.is_some_and(|operator| {
                    parsed.arena.node(operator).kind == tsc_syntax::SyntaxKind::InKeyword
                }) =>
            {
                private_in = arena.node_ref(source, id);
            }
            NodeData::Decorator(_) => decorators.push(
                arena
                    .node_ref(source, id)
                    .expect("mounted parsed decorator"),
            ),
            _ => {}
        }
        for_each_child(&parsed.arena, record, |child| {
            stack.push(child);
            false
        });
    }

    let private_expression = TransformFlags::CONTAINS_PRIVATE_IDENTIFIER_IN_EXPRESSION;
    assert!(
        !arena
            .transform_flags(declaration.expect("private declaration"))
            .contains(private_expression),
        "a private declaration name must not acquire the expression-only flag"
    );
    assert!(arena
        .transform_flags(property_access.expect("private property access"))
        .contains(private_expression));
    assert!(arena
        .transform_flags(private_in.expect("private in expression"))
        .contains(private_expression));
    assert_eq!(decorators.len(), 4);
    assert_eq!(
        decorators
            .into_iter()
            .filter(|decorator| arena
                .transform_flags(*decorator)
                .contains(private_expression))
            .count(),
        2,
        "only tsc's property-access and private-in producers should flag decorators"
    );
}

#[test]
fn constructor_prologue_stops_before_strings_and_noncontiguous_custom_statements() {
    // tsc-port: copyPrologue/copyStandardPrologue/copyCustomPrologue @6.0.3
    // tsc-hash: 555445a3fd02a4b53bbc05f05e48729ca0f7208892d66dbc7985f51f3e897a8e
    // tsc-span: _tsc.js:24827-24869
    let parsed = parse_source_file(
        "constructor-prologue.ts",
        concat!(
            "function owner() {\n",
            "    \"standard\";\n",
            "    custom();\n",
            "    \"late\";\n",
            "    late_custom();\n",
            "    body();\n",
            "}\n",
        ),
        Default::default(),
        None,
    );
    let NodeData::SourceFile(source_file) = &parsed.arena.node(parsed.root).data else {
        panic!("source file root");
    };
    let source_statements = parsed
        .arena
        .node_array(source_file.statements.expect("source statements"));
    let NodeData::FunctionDeclaration(function) =
        &parsed.arena.node(source_statements.nodes[0]).data
    else {
        panic!("function declaration");
    };
    let NodeData::Block(body) = &parsed
        .arena
        .node(function.body.expect("function body"))
        .data
    else {
        panic!("function body block");
    };
    let statement_ids = parsed
        .arena
        .node_array(body.statements.expect("body statements"))
        .nodes
        .clone();

    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let statements = statement_ids
        .into_iter()
        .map(|statement| arena.node_ref(source, statement).expect("body statement"))
        .collect::<Vec<_>>();
    arena
        .metadata_mut(statements[1])
        .add_flags(EmitFlags::CUSTOM_PROLOGUE);
    arena
        .metadata_mut(statements[3])
        .add_flags(EmitFlags::CUSTOM_PROLOGUE);

    let prologue = constructor_prologue(&arena, &statements).expect("constructor prologue");
    assert_eq!(prologue.standard_end(), 1);
    assert_eq!(prologue.custom_end(), 2);
    assert_eq!(prologue.body_start(), 2);
}

impl AmbientFunctionExportResolver {
    fn new(source: &tsc_syntax::SourceFile) -> Self {
        let mut declarations_by_name = BTreeMap::<String, NodeId>::new();
        let mut stack = vec![source.root];
        while let Some(node) = stack.pop() {
            if let NodeData::FunctionDeclaration(data) = &source.arena.node(node).data {
                if let Some(NodeData::Identifier(identifier)) =
                    data.name.map(|name| &source.arena.node(name).data)
                {
                    declarations_by_name.insert(identifier.text.clone(), node);
                }
            }
            for_each_child(&source.arena, source.arena.node(node), |child| {
                stack.push(child);
                false
            });
        }
        let mut declaration_by_reference = BTreeMap::new();
        let mut stack = vec![source.root];
        while let Some(node) = stack.pop() {
            if let NodeData::Identifier(identifier) = &source.arena.node(node).data {
                if let Some(declaration) = declarations_by_name.get(&identifier.text) {
                    declaration_by_reference.insert(node, *declaration);
                }
            }
            for_each_child(&source.arena, source.arena.node(node), |child| {
                stack.push(child);
                false
            });
        }
        Self {
            declaration_by_reference,
        }
    }
}

impl EmitResolver for AmbientFunctionExportResolver {
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
            .declaration_by_reference
            .get(&node.node())
            .copied()
            .map(|declaration| EmitResolverNode::new(node.source(), declaration)))
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

impl ConstructorReferenceResolver {
    fn new(source: &tsc_syntax::SourceFile) -> Self {
        let mut class = None;
        let mut private_method = None;
        let mut constructor_references = BTreeSet::new();
        let mut stack = vec![source.root];
        while let Some(id) = stack.pop() {
            let node = source.arena.node(id);
            match &node.data {
                NodeData::ClassDeclaration(data)
                    if data.name.is_some_and(|name| {
                        matches!(
                            &source.arena.node(name).data,
                            NodeData::Identifier(identifier) if identifier.text == "C"
                        )
                    }) =>
                {
                    class = Some(id);
                }
                NodeData::MethodDeclaration(data)
                    if data.name.is_some_and(|name| {
                        matches!(source.arena.node(name).data, NodeData::PrivateIdentifier(_))
                    }) =>
                {
                    private_method = Some(id);
                }
                NodeData::Identifier(identifier) if identifier.text == "C" => {
                    constructor_references.insert(id);
                }
                _ => {}
            }
            for_each_child(&source.arena, node, |child| {
                stack.push(child);
                false
            });
        }
        Self {
            class: class.expect("class C"),
            private_method: private_method.expect("private method"),
            constructor_references,
        }
    }
}

impl EmitResolver for ConstructorReferenceResolver {
    fn has_node_check_flag(
        &self,
        node: EmitResolverNode,
        flag: u32,
    ) -> Result<bool, EmitResolverError> {
        Ok((node.node() == self.private_method
            && flag == NodeCheckFlags::CONTAINS_CONSTRUCTOR_REFERENCE.bits() as u32)
            || (self.constructor_references.contains(&node.node())
                && flag == NodeCheckFlags::CONSTRUCTOR_REFERENCE.bits() as u32))
    }

    fn get_referenced_value_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(self
            .constructor_references
            .contains(&node.node())
            .then(|| EmitResolverNode::new(node.source(), self.class)))
    }
}

fn collect_binding_declarations(
    source: &tsc_syntax::SourceFile,
    name: NodeId,
    declaration: NodeId,
    declarations: &mut Vec<(String, NodeId)>,
) {
    match &source.arena.node(name).data {
        NodeData::Identifier(identifier) => {
            declarations.push((identifier.text.clone(), declaration));
        }
        NodeData::ObjectBindingPattern(pattern) => {
            for element in pattern
                .elements
                .map(|elements| source.arena.node_array(elements).nodes.as_slice())
                .unwrap_or_default()
            {
                if let NodeData::BindingElement(binding) = &source.arena.node(*element).data {
                    if let Some(name) = binding.name {
                        collect_binding_declarations(source, name, *element, declarations);
                    }
                }
            }
        }
        NodeData::ArrayBindingPattern(pattern) => {
            for element in pattern
                .elements
                .map(|elements| source.arena.node_array(elements).nodes.as_slice())
                .unwrap_or_default()
            {
                if let NodeData::BindingElement(binding) = &source.arena.node(*element).data {
                    if let Some(name) = binding.name {
                        collect_binding_declarations(source, name, *element, declarations);
                    }
                }
            }
        }
        _ => {}
    }
}

impl ExportedVariableResolver {
    fn new(source: &tsc_syntax::SourceFile) -> Self {
        let mut declarations_by_name = BTreeMap::new();
        let mut direct_exports = BTreeMap::new();
        let NodeData::SourceFile(root) = &source.arena.node(source.root).data else {
            panic!("source root");
        };
        for statement in root
            .statements
            .map(|statements| source.arena.node_array(statements).nodes.as_slice())
            .unwrap_or_default()
        {
            let NodeData::VariableStatement(variable) = &source.arena.node(*statement).data else {
                continue;
            };
            let direct = variable.modifiers.is_some_and(|modifiers| {
                source
                    .arena
                    .node_array(modifiers)
                    .nodes
                    .iter()
                    .any(|modifier| {
                        source.arena.node(*modifier).kind == tsc_syntax::SyntaxKind::ExportKeyword
                    })
            });
            let Some(list) = variable.declaration_list else {
                continue;
            };
            let NodeData::VariableDeclarationList(list) = &source.arena.node(list).data else {
                continue;
            };
            for declaration in list
                .declarations
                .map(|declarations| source.arena.node_array(declarations).nodes.as_slice())
                .unwrap_or_default()
            {
                let NodeData::VariableDeclaration(variable) = &source.arena.node(*declaration).data
                else {
                    continue;
                };
                let Some(name) = variable.name else {
                    continue;
                };
                let mut declarations = Vec::new();
                collect_binding_declarations(source, name, *declaration, &mut declarations);
                for (name, declaration) in declarations {
                    declarations_by_name.insert(name.clone(), declaration);
                    if direct {
                        direct_exports.insert(name, declaration);
                    }
                }
            }
        }
        let mut declaration_by_reference = BTreeMap::new();
        let mut direct_export_references = BTreeMap::new();
        let mut stack = vec![source.root];
        while let Some(node) = stack.pop() {
            if let NodeData::Identifier(identifier) = &source.arena.node(node).data {
                if let Some(declaration) = declarations_by_name.get(&identifier.text) {
                    declaration_by_reference.insert(node, *declaration);
                    if direct_exports.contains_key(&identifier.text) {
                        direct_export_references.insert(node, source.root);
                    }
                }
            }
            for_each_child(&source.arena, source.arena.node(node), |child| {
                stack.push(child);
                false
            });
        }
        Self {
            declaration_by_reference,
            direct_export_references,
        }
    }
}

impl EmitResolver for ExportedVariableResolver {
    fn get_constant_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_export_container(
        &self,
        node: EmitResolverNode,
        _mode: EmitExportContainerMode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(self
            .direct_export_references
            .get(&node.node())
            .copied()
            .map(|container| EmitResolverNode::new(node.source(), container)))
    }

    fn get_referenced_import_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_value_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(self
            .declaration_by_reference
            .get(&node.node())
            .copied()
            .map(|declaration| EmitResolverNode::new(node.source(), declaration)))
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

impl SourceExportContainerResolver {
    fn new(source: &tsc_syntax::SourceFile) -> Self {
        let mut exported_names = BTreeSet::new();
        let mut stack = vec![source.root];
        while let Some(node) = stack.pop() {
            if let NodeData::VariableStatement(variable) = &source.arena.node(node).data {
                let direct_export = variable.modifiers.is_some_and(|modifiers| {
                    source
                        .arena
                        .node_array(modifiers)
                        .nodes
                        .iter()
                        .any(|modifier| {
                            source.arena.node(*modifier).kind
                                == tsc_syntax::SyntaxKind::ExportKeyword
                        })
                });
                if direct_export {
                    if let Some(NodeData::VariableDeclarationList(list)) = variable
                        .declaration_list
                        .map(|list| &source.arena.node(list).data)
                    {
                        for declaration in list
                            .declarations
                            .map(|declarations| {
                                source.arena.node_array(declarations).nodes.as_slice()
                            })
                            .unwrap_or_default()
                        {
                            let NodeData::VariableDeclaration(variable) =
                                &source.arena.node(*declaration).data
                            else {
                                continue;
                            };
                            let Some(name) = variable.name else {
                                continue;
                            };
                            let mut declarations = Vec::new();
                            collect_binding_declarations(
                                source,
                                name,
                                *declaration,
                                &mut declarations,
                            );
                            exported_names.extend(declarations.into_iter().map(|(name, _)| name));
                        }
                    }
                }
            }
            for_each_child(&source.arena, source.arena.node(node), |child| {
                stack.push(child);
                false
            });
        }

        let mut containers_by_reference = BTreeMap::new();
        let mut stack = vec![source.root];
        while let Some(node) = stack.pop() {
            if matches!(
                &source.arena.node(node).data,
                NodeData::Identifier(identifier) if exported_names.contains(&identifier.text)
            ) {
                containers_by_reference.insert(node, source.root);
            }
            for_each_child(&source.arena, source.arena.node(node), |child| {
                stack.push(child);
                false
            });
        }
        Self {
            containers_by_reference,
        }
    }
}

impl EmitResolver for SourceExportContainerResolver {
    fn get_constant_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_export_container(
        &self,
        node: EmitResolverNode,
        _mode: EmitExportContainerMode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(self
            .containers_by_reference
            .get(&node.node())
            .copied()
            .map(|container| EmitResolverNode::new(node.source(), container)))
    }

    fn get_referenced_import_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_value_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
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

impl SourceNamedExportContainerResolver {
    fn new(source: &tsc_syntax::SourceFile, exported_name: &str) -> Self {
        let mut containers_by_reference = BTreeMap::new();
        let mut stack = vec![source.root];
        while let Some(node) = stack.pop() {
            if matches!(
                &source.arena.node(node).data,
                NodeData::Identifier(identifier) if identifier.text == exported_name
            ) {
                containers_by_reference.insert(node, source.root);
            }
            for_each_child(&source.arena, source.arena.node(node), |child| {
                stack.push(child);
                false
            });
        }
        Self {
            containers_by_reference,
        }
    }
}

impl EmitResolver for SourceNamedExportContainerResolver {
    fn get_constant_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_export_container(
        &self,
        node: EmitResolverNode,
        _mode: EmitExportContainerMode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(self
            .containers_by_reference
            .get(&node.node())
            .copied()
            .map(|container| EmitResolverNode::new(node.source(), container)))
    }

    fn get_referenced_import_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_value_declaration(
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

impl ImportEqualsCallResolver {
    fn new(source: &tsc_syntax::SourceFile) -> Self {
        let mut declaration = None;
        let mut local_name = None;
        let mut stack = vec![source.root];
        while let Some(node) = stack.pop() {
            if let NodeData::ImportEqualsDeclaration(data) = &source.arena.node(node).data {
                if let Some(name) = data.name {
                    if let NodeData::Identifier(identifier) = &source.arena.node(name).data {
                        declaration = Some(node);
                        local_name = Some(identifier.text.clone());
                    }
                }
            }
            for_each_child(&source.arena, source.arena.node(node), |child| {
                stack.push(child);
                false
            });
        }
        let declaration = declaration.expect("import-equals declaration");
        let local_name = local_name.expect("import-equals local name");
        let mut declaration_by_reference = BTreeMap::new();
        let mut stack = vec![source.root];
        while let Some(node) = stack.pop() {
            if matches!(
                &source.arena.node(node).data,
                NodeData::Identifier(identifier) if identifier.text == local_name
            ) {
                declaration_by_reference.insert(node, declaration);
            }
            for_each_child(&source.arena, source.arena.node(node), |child| {
                stack.push(child);
                false
            });
        }
        Self {
            declaration_by_reference,
        }
    }
}

impl EmitResolver for ImportEqualsCallResolver {
    fn get_constant_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
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
        node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(self
            .declaration_by_reference
            .get(&node.node())
            .copied()
            .map(|declaration| EmitResolverNode::new(node.source(), declaration)))
    }

    fn get_referenced_value_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
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

impl DefaultImportCallResolver {
    fn new(source: &tsc_syntax::SourceFile) -> Self {
        let mut declaration = None;
        let mut local_name = None;
        let mut stack = vec![source.root];
        while let Some(node) = stack.pop() {
            if let NodeData::ImportClause(data) = &source.arena.node(node).data {
                if let Some(name) = data.name {
                    if let NodeData::Identifier(identifier) = &source.arena.node(name).data {
                        declaration = Some(node);
                        local_name = Some(identifier.text.clone());
                    }
                }
            }
            for_each_child(&source.arena, source.arena.node(node), |child| {
                stack.push(child);
                false
            });
        }
        let declaration = declaration.expect("default-import clause");
        let local_name = local_name.expect("default-import local name");
        let mut declaration_by_reference = BTreeMap::new();
        let mut stack = vec![source.root];
        while let Some(node) = stack.pop() {
            if matches!(
                &source.arena.node(node).data,
                NodeData::Identifier(identifier) if identifier.text == local_name
            ) {
                declaration_by_reference.insert(node, declaration);
            }
            for_each_child(&source.arena, source.arena.node(node), |child| {
                stack.push(child);
                false
            });
        }
        Self {
            declaration_by_reference,
        }
    }
}

impl EmitResolver for DefaultImportCallResolver {
    fn get_constant_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_import_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(self
            .declaration_by_reference
            .get(&node.node())
            .copied()
            .map(|declaration| EmitResolverNode::new(node.source(), declaration)))
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

impl EnumBindingResolver {
    fn new(source: &tsc_syntax::SourceFile) -> Self {
        let mut declarations_by_name = BTreeMap::new();
        let mut stack = vec![source.root];
        while let Some(id) = stack.pop() {
            let node = source.arena.node(id);
            let name = match &node.data {
                NodeData::EnumDeclaration(data) => data.name,
                NodeData::ClassDeclaration(data) => data.name,
                _ => None,
            };
            if let Some(name) = name {
                declarations_by_name.insert(name, id);
            }
            for_each_child(&source.arena, node, |child| {
                stack.push(child);
                false
            });
        }
        Self {
            declarations_by_name,
            enum_member_values: BTreeMap::new(),
            loop_scoped_private_names: BTreeSet::new(),
        }
    }

    fn with_enum_member_number_value(mut self, member: NodeId, value: f64) -> Self {
        self.enum_member_values.insert(
            member,
            EmitEnumMemberValue::new(
                Some(EmitConstantValue::Number(JavaScriptNumber::from_f64(value))),
                false,
            ),
        );
        self
    }

    fn with_loop_scoped_private_names(mut self, source: &tsc_syntax::SourceFile) -> Self {
        let mut stack = vec![source.root];
        while let Some(id) = stack.pop() {
            let node = source.arena.node(id);
            if matches!(&node.data, NodeData::PrivateIdentifier(_)) {
                self.loop_scoped_private_names.insert(id);
            }
            for_each_child(&source.arena, node, |child| {
                stack.push(child);
                false
            });
        }
        self
    }
}

impl EmitResolver for EnumBindingResolver {
    fn get_constant_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
    }

    fn get_enum_member_value(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitEnumMemberValue>, EmitResolverError> {
        Ok(self.enum_member_values.get(&node.node()).cloned())
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

    fn get_referenced_value_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(self
            .declarations_by_name
            .get(&node.node())
            .copied()
            .map(|declaration| EmitResolverNode::new(node.source(), declaration)))
    }

    fn has_node_check_flag(
        &self,
        node: EmitResolverNode,
        flag: u32,
    ) -> Result<bool, EmitResolverError> {
        Ok(
            flag == NodeCheckFlags::BLOCK_SCOPED_BINDING_IN_LOOP.bits() as u32
                && self.loop_scoped_private_names.contains(&node.node()),
        )
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

struct NamespaceAliasResolver {
    containers_by_reference: BTreeMap<NodeId, NodeId>,
    parsed_accesses: BTreeSet<NodeId>,
}

struct LegacyScriptJsxResolver;

impl EmitResolver for LegacyScriptJsxResolver {
    fn get_constant_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
    }

    fn has_node_check_flag(
        &self,
        _node: EmitResolverNode,
        _flag: u32,
    ) -> Result<bool, EmitResolverError> {
        Ok(false)
    }

    fn is_external_or_common_js_module(
        &self,
        _node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(false)
    }

    fn get_referenced_import_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn get_jsx_factory_import_declaration(
        &self,
        _node: EmitResolverNode,
        _name: &str,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn get_jsx_factory_export_container(
        &self,
        _node: EmitResolverNode,
        _name: &str,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_export_container(
        &self,
        _node: EmitResolverNode,
        _mode: EmitExportContainerMode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_value_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }
}

impl NamespaceAliasResolver {
    fn new(source: &tsc_syntax::SourceFile) -> Self {
        let mut namespace = None;
        let mut references = Vec::new();
        let mut parsed_accesses = BTreeSet::new();
        let mut stack = vec![source.root];
        while let Some(id) = stack.pop() {
            let node = source.arena.node(id);
            if matches!(
                node.kind,
                tsc_syntax::SyntaxKind::PropertyAccessExpression
                    | tsc_syntax::SyntaxKind::ElementAccessExpression
            ) {
                parsed_accesses.insert(id);
            }
            match &node.data {
                NodeData::ModuleDeclaration(data)
                    if data.name.is_some_and(|name| {
                        matches!(&source.arena.node(name).data,
                            NodeData::Identifier(identifier) if identifier.text == "published")
                    }) =>
                {
                    namespace = Some(id);
                }
                NodeData::Identifier(identifier)
                    if identifier.text == "exports"
                        && node.parent.is_some_and(|parent| {
                            matches!(&source.arena.node(parent).data,
                                NodeData::NewExpression(data) if data.expression == Some(id))
                        }) =>
                {
                    references.push(id);
                }
                _ => {}
            }
            for_each_child(&source.arena, node, |child| {
                stack.push(child);
                false
            });
        }
        let namespace = namespace.expect("published namespace declaration");
        Self {
            containers_by_reference: references
                .into_iter()
                .map(|reference| (reference, namespace))
                .collect(),
            parsed_accesses,
        }
    }
}

impl EmitResolver for NamespaceAliasResolver {
    fn get_constant_value(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        assert!(
            self.parsed_accesses.contains(&node.node()),
            "emit substitution must not query a synthetic access: {node:?}"
        );
        Ok(None)
    }

    fn get_referenced_export_container(
        &self,
        node: EmitResolverNode,
        _mode: EmitExportContainerMode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(self
            .containers_by_reference
            .get(&node.node())
            .copied()
            .map(|container| EmitResolverNode::new(node.source(), container)))
    }

    fn get_referenced_import_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_value_declaration(
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
}

fn transform_and_print_module(source_text: &str, module: ModuleKind) -> String {
    transform_and_print_module_with_remove_comments(source_text, module, false)
}

fn transform_and_print_module_with_remove_comments(
    source_text: &str,
    module: ModuleKind,
    remove_comments: bool,
) -> String {
    let parsed = parse_source_file("module.ts", source_text, Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(module.bits()),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let resolver = LegacyScriptJsxResolver;
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![transform_module(&options, &resolver)],
        false,
    )
    .expect("module transform");
    create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_target(ScriptTarget::ES2015)
            .with_remove_comments(remove_comments),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print module transform")
    .text()
    .to_owned()
}

fn transform_and_print_typescript_module(
    parsed: &tsc_syntax::SourceFile,
    module: ModuleKind,
    resolver: &dyn EmitResolver,
) -> String {
    let mut arena = TransformArena::new();
    let source = arena.add_source(parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(module.bits()),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, resolver),
            transform_module(&options, resolver),
        ],
        false,
    )
    .expect("TypeScript module transform");
    create_printer(PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print TypeScript module transform")
        .text()
        .to_owned()
}

fn transform_and_print_preserved_tsx(source_text: &str) -> String {
    let parsed = parse_source_file(
        "preserved.tsx",
        source_text,
        ParseOptions {
            language_variant: LanguageVariant::Jsx,
            ..ParseOptions::default()
        },
        None,
    );
    let resolver = LegacyScriptJsxResolver;
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        jsx: Some(1),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("preserved TSX transform");
    create_printer(PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print preserved TSX transform")
        .text()
        .to_owned()
}

fn transform_and_print_es2015_class_fields(source_text: &str) -> String {
    let parsed = parse_source_file(
        "class-fields.ts",
        source_text,
        ParseOptions::default(),
        None,
    );
    let resolver = EnumBindingResolver::new(&parsed);
    transform_and_print_parsed_es2015_class_fields(&parsed, &resolver)
}

fn transform_and_print_parsed_es2015_class_fields(
    parsed: &tsc_syntax::SourceFile,
    resolver: &dyn EmitResolver,
) -> String {
    transform_and_print_parsed_es2015_class_fields_with_mode(parsed, resolver, false)
}

fn transform_and_print_parsed_es2015_class_fields_with_mode(
    parsed: &tsc_syntax::SourceFile,
    resolver: &dyn EmitResolver,
    use_define_for_class_fields: bool,
) -> String {
    let mut arena = TransformArena::new();
    let source = arena.add_source(parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        use_define_for_class_fields: Some(use_define_for_class_fields),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, resolver),
            transform_class_fields(&options, resolver),
            transform_es2020(&options),
        ],
        false,
    )
    .expect("ES2015 class-fields transform");
    create_printer(PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print ES2015 class-fields transform")
        .text()
        .to_owned()
}

fn transform_and_print_umd_module(source_text: &str) -> String {
    transform_and_print_module(source_text, ModuleKind::UMD)
}

#[test]
fn class_fields_do_not_synthesize_super_for_transparently_wrapped_null_heritage() {
    let output = transform_and_print_es2015_class_fields(concat!(
        "class Direct extends null { direct = 1; }\n",
        "class Wrapped extends (((null))) { wrapped = 2; }\n",
    ));

    assert!(!output.contains("super(...arguments);"), "{output}");
    assert!(
        output.contains(concat!(
            "class Direct extends null {\n",
            "    constructor() {\n",
            "        this.direct = 1;\n",
            "    }\n",
            "}\n",
        )),
        "{output}",
    );
    assert!(
        output.contains(concat!(
            "class Wrapped extends (((null))) {\n",
            "    constructor() {\n",
            "        this.wrapped = 2;\n",
            "    }\n",
            "}\n",
        )),
        "{output}",
    );
}

#[test]
fn class_fields_insert_initializers_after_a_parenthesized_super_statement() {
    let output = transform_and_print_es2015_class_fields(concat!(
        "class Base {}\n",
        "class Derived extends Base {\n",
        "    value = 1;\n",
        "    constructor() {\n",
        "        before();\n",
        "        (super());\n",
        "        after();\n",
        "    }\n",
        "}\n",
    ));

    assert!(
        output.contains(concat!(
            "        before();\n",
            "        (super());\n",
            "        this.value = 1;\n",
            "        after();\n",
        )),
        "{output}",
    );
}

#[test]
fn lowered_optional_chain_heritage_keeps_its_left_hand_side_boundary() {
    let output = transform_and_print_es2015_class_fields(concat!(
        "namespace A { export class B {} }\n",
        "class Derived extends A?.B {}\n",
    ));

    assert!(
        output.contains("class Derived extends (A === null || A === void 0 ? void 0 : A.B) {",),
        "{output}",
    );
}

fn emitted_line_with<'a>(output: &'a str, marker: &str) -> &'a str {
    output
        .lines()
        .find(|line| line.contains(marker))
        .unwrap_or_else(|| panic!("missing `{marker}` in output: {output}"))
}

fn receiver_before_static_f(line: &str) -> &str {
    let (prefix, _) = line
        .split_once(".f +")
        .unwrap_or_else(|| panic!("missing static f access: {line}"));
    prefix
        .rsplit(|character: char| {
            !(character.is_ascii_alphanumeric() || matches!(character, '_' | '$'))
        })
        .find(|part| !part.is_empty())
        .unwrap_or_else(|| panic!("missing static receiver: {line}"))
}

fn reflect_get_arguments(line: &str) -> &str {
    let (_, suffix) = line
        .split_once("Reflect.get(")
        .unwrap_or_else(|| panic!("missing Reflect.get: {line}"));
    suffix
        .split_once(") +")
        .map(|(arguments, _)| arguments)
        .unwrap_or_else(|| panic!("missing Reflect.get result boundary: {line}"))
}

#[test]
fn static_this_frame_is_inherited_by_arrows_but_not_functions_or_nested_classes() {
    let output = transform_and_print_es2015_class_fields(concat!(
        "class Outer {\n",
        "    static f = 0;\n",
        "    static arrow = () => this.f + 1;\n",
        "    static ordinary = function () { return this.f + 2; };\n",
        "    static nested = class Inner {\n",
        "        instance = this.f + 3;\n",
        "        static own = this.f + 4;\n",
        "    };\n",
        "}\n",
    ));

    let arrow = emitted_line_with(&output, ".f + 1");
    let ordinary = emitted_line_with(&output, ".f + 2");
    let instance = emitted_line_with(&output, ".f + 3");
    let nested_static = emitted_line_with(&output, ".f + 4");
    assert!(
        arrow.contains("=>") && !arrow.contains("this.f"),
        "{output}"
    );
    assert!(ordinary.contains("this.f + 2"), "{output}");
    assert!(instance.contains("this.instance = this.f + 3"), "{output}");
    assert!(!nested_static.contains("this.f"), "{output}");
    assert_ne!(
        receiver_before_static_f(arrow),
        receiver_before_static_f(nested_static),
        "nested static initializer reused the outer receiver: {output}",
    );
}

#[test]
fn static_super_frame_is_inherited_by_arrows_but_nested_class_owns_its_static_base() {
    let output = transform_and_print_es2015_class_fields(concat!(
        "class Base { static f = 0; }\n",
        "class Outer extends Base {\n",
        "    static arrow = () => super.f + 1;\n",
        "    static ordinary = function () { return super.f + 2; };\n",
        "    static nested = (() => {\n",
        "        class Inner extends Base {\n",
        "            instance = super.f + 3;\n",
        "            static own = super.f + 4;\n",
        "        }\n",
        "        return Inner;\n",
        "    })();\n",
        "}\n",
    ));

    let arrow = emitted_line_with(&output, "+ 1");
    let ordinary = emitted_line_with(&output, "+ 2");
    let instance = emitted_line_with(&output, "+ 3");
    let nested_static = emitted_line_with(&output, "+ 4");
    assert!(arrow.contains("=> Reflect.get("), "{output}");
    assert!(ordinary.contains("super.f + 2"), "{output}");
    assert!(instance.contains("this.instance = super.f + 3"), "{output}");
    assert!(nested_static.contains("Reflect.get("), "{output}");
    assert_ne!(
        reflect_get_arguments(arrow),
        reflect_get_arguments(nested_static),
        "nested static initializer reused the outer super frame: {output}",
    );
}

#[test]
fn static_super_targets_share_reflect_lowering_for_every_value_use() {
    let parsed = parse_source_file(
        "static-super-targets.ts",
        concat!(
            "class B { static a = 0; static f() {} }\n",
            "class C extends B {\n",
            "    static assign = super.a = 0;\n",
            "    static compound = super.a += 1;\n",
            "    static discarded = (() => { super.a = 2; })();\n",
            "    static destructuring = [super.a] = [3];\n",
            "    static prefix = ++super.a;\n",
            "    static elementPrefix = ++super[(\"a\")];\n",
            "    static postfix = super.a++;\n",
            "    static call = super.f();\n",
            "    static tag = super.f``;\n",
            "}\n",
        ),
        ParseOptions::default(),
        None,
    );
    let resolver = EnumBindingResolver::new(&parsed);

    for use_define in [false, true] {
        let output = transform_and_print_parsed_es2015_class_fields_with_mode(
            &parsed, &resolver, use_define,
        );
        assert_eq!(output.matches("Reflect.set(").count(), 7, "{output}");
        assert_eq!(output.matches("Reflect.get(").count(), 6, "{output}");
        assert_eq!(output.matches("set value(").count(), 1, "{output}");
        assert!(output.contains("=> { Reflect.set("), "{output}");
        assert!(output.contains(").value] = [3]"), "{output}");
        assert!(output.contains(".call("), "{output}");
        assert!(output.contains(".bind("), "{output}");
        assert!(!output.contains("super.a"), "{output}");
        assert!(!output.contains("super.f"), "{output}");
    }
}

#[test]
fn nested_class_computed_field_names_use_the_enclosing_static_evaluation_frame() {
    let parsed = parse_source_file(
        "nested-computed-static-this.ts",
        concat!(
            "class C {\n",
            "    static c = \"foo\";\n",
            "    static bar = class Inner {\n",
            "        static [this.c] = 123;\n",
            "        [this.c] = 123;\n",
            "    };\n",
            "}\n",
        ),
        ParseOptions::default(),
        None,
    );
    let resolver = EnumBindingResolver::new(&parsed);

    for use_define in [false, true] {
        let output = transform_and_print_parsed_es2015_class_fields_with_mode(
            &parsed, &resolver, use_define,
        );
        assert!(output.contains("_a = C;"), "{output}");
        assert!(output.contains("_b = class Inner"), "{output}");
        assert!(output.contains("_c = _a.c"), "{output}");
        assert!(output.contains("_d = _a.c"), "{output}");
        assert!(!output.contains("this.c"), "{output}");
        assert!(
            output.find("_b = class Inner").unwrap() < output.find("_c = _a.c").unwrap(),
            "the class identity must be reserved before computed-name caches: {output}",
        );
        if use_define {
            assert!(
                output.contains("Object.defineProperty(this, _d"),
                "{output}"
            );
            assert!(output.contains("Object.defineProperty(_b, _c"), "{output}");
        } else {
            assert!(output.contains("this[_d] = 123"), "{output}");
            assert!(output.contains("_b[_c] = 123"), "{output}");
        }
    }
}

#[test]
fn private_storage_captured_by_a_loop_class_expression_is_declared_in_the_loop_body() {
    let source_text = concat!(
        "const array = [];\n",
        "for (let i = 0; i < 2; ++i) {\n",
        "    array.push(class C {\n",
        "        #field = i;\n",
        "        #method() {}\n",
        "        get #accessor() { return i; }\n",
        "        set #accessor(value) {}\n",
        "    });\n",
        "}\n",
    );
    let parsed = parse_source_file("loop-private.ts", source_text, Default::default(), None);
    let resolver = EnumBindingResolver::new(&parsed).with_loop_scoped_private_names(&parsed);
    let output = transform_and_print_parsed_es2015_class_fields(&parsed, &resolver);

    let loop_start = output.find("for (").expect("loop output");
    let class_start = output[loop_start..]
        .find("array.push")
        .map(|offset| loop_start + offset)
        .expect("class expression in loop");
    assert!(!output[..loop_start].contains("_C_"), "{output}");
    assert!(
        output[loop_start..class_start].contains("let _C_"),
        "{output}"
    );
}

#[test]
fn private_storage_setup_precedes_a_computed_member_key() {
    let output = transform_and_print_es2015_class_fields(concat!(
        "let getX;\n",
        "class A {\n",
        "    #x = 100;\n",
        "    [(getX = (a) => a.#x, \"_\")]() {}\n",
        "}\n",
    ));

    assert!(
        output.contains("[(_A_x = new WeakMap(), getX = (a) =>"),
        "{output}"
    );
    assert!(!output.contains("\n_A_x = new WeakMap();"), "{output}");
}

#[test]
fn nested_private_storage_setup_precedes_its_private_computed_key_read() {
    let output = transform_and_print_es2015_class_fields(concat!(
        "class Foo {\n",
        "    #name;\n",
        "    read() {\n",
        "        const obj = this;\n",
        "        class Bar {\n",
        "            #y = 100;\n",
        "            [obj.#name]() { return this.#y; }\n",
        "        }\n",
        "        return Bar;\n",
        "    }\n",
        "}\n",
    ));

    assert!(
        output.contains("[(_Bar_y = new WeakMap(), __classPrivateFieldGet(obj"),
        "{output}"
    );
    assert!(
        !output.contains("\n        _Bar_y = new WeakMap();"),
        "{output}"
    );
}

#[test]
fn duplicate_private_getter_ordinal_precedes_the_generated_role_suffix() {
    let output = transform_and_print_es2015_class_fields(concat!(
        "class C {\n",
        "    get #value() { return 1; }\n",
        "    get #value() { return 2; }\n",
        "}\n",
    ));

    assert!(output.contains("_C_value_1_get"), "{output}");
    assert!(!output.contains("_C_value_get_1"), "{output}");
}

#[test]
fn invalid_private_enum_member_recovers_as_an_empty_expression_name() {
    let parsed = parse_source_file(
        "invalid-private-enum-member.ts",
        "enum E { #x }\n",
        Default::default(),
        None,
    );
    let mut stack = vec![parsed.root];
    let mut member = None;
    while let Some(node) = stack.pop() {
        if matches!(&parsed.arena.node(node).data, NodeData::EnumMember(_)) {
            member = Some(node);
        }
        for_each_child(&parsed.arena, parsed.arena.node(node), |child| {
            stack.push(child);
            false
        });
    }
    let resolver = EnumBindingResolver::new(&parsed)
        .with_enum_member_number_value(member.expect("private enum member"), 0.0);
    let output = transform_and_print_parsed_es2015_class_fields(&parsed, &resolver);

    assert!(output.contains("E[E[] = 0] = ;"), "{output}");
    assert!(!output.contains("E[E[\"\"]"), "{output}");
}

#[test]
fn private_assignment_target_skips_parentheses_and_erased_type_wrappers() {
    let output = transform_and_print_es2015_class_fields(concat!(
        "class Foo {\n",
        "    #value;\n",
        "    set1(value) { (this.#value as number) = value; }\n",
        "    set2(value) { (((this.#value as number))) = value; }\n",
        "    set3(value) { (this.#value) = value; }\n",
        "    set4(value) { (((this.#value))) = value; }\n",
        "}\n",
    ));

    assert!(!output.contains("var __classPrivateFieldGet"), "{output}");
    assert_eq!(
        output
            .matches("__classPrivateFieldSet(this, _Foo_value, value, \"f\")")
            .count(),
        4,
        "{output}",
    );
}

#[test]
fn common_js_family_preserves_recovery_empty_non_export_variable_statements() {
    let source = "var;\nlet;\nconst;\nexport {};\n";
    for (module, label) in [
        (ModuleKind::COMMON_JS, "CommonJS"),
        (ModuleKind::AMD, "AMD"),
        (ModuleKind::UMD, "UMD"),
    ] {
        let output = transform_and_print_module(source, module);
        let var = output
            .find("var ;")
            .unwrap_or_else(|| panic!("{label}: {output}"));
        let let_statement = output[var..]
            .find("let;")
            .map(|offset| var + offset)
            .unwrap_or_else(|| panic!("{label}: {output}"));
        let const_statement = output[let_statement..]
            .find("const ;")
            .map(|offset| let_statement + offset)
            .unwrap_or_else(|| panic!("{label}: {output}"));
        assert!(
            var < let_statement && let_statement < const_statement,
            "{label}: {output}"
        );
    }
}

#[test]
fn common_js_elides_an_embedded_uninitialized_direct_export_and_substitutes_its_read() {
    let source_text = concat!(
        "// https://github.com/microsoft/TypeScript/issues/59373\n\n",
        "if (true)\n",
        "export const cssExports: CssExports;\n",
        "export default cssExports;\n",
    );
    let parsed = parse_source_file("embedded-export.ts", source_text, Default::default(), None);
    let resolver = SourceExportContainerResolver::new(&parsed);
    let output = transform_and_print_typescript_module(&parsed, ModuleKind::COMMON_JS, &resolver);

    assert_eq!(output.matches("issues/59373").count(), 1, "{output}");
    let issue = output.find("issues/59373").expect("issue comment");
    let transformed_if = output
        .find("if (true) { }")
        .expect("single-line empty block");
    assert!(issue < transformed_if, "{output}");
    assert!(output.contains("if (true) { }"), "{output}");
    assert!(
        output.contains("exports.default = exports.cssExports;"),
        "{output}"
    );
    assert!(!output.contains("const cssExports"), "{output}");
    assert!(!output.contains("exports.cssExports = void 0"), "{output}");
}

#[test]
fn common_js_embedded_initialized_direct_export_owns_its_primary_publication() {
    let source_text = concat!(
        "if (true)\n",
        "export const value = 1;\n",
        "consume(value);\n",
        "export {};\n",
    );
    let parsed = parse_source_file(
        "embedded-initialized-export.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = SourceExportContainerResolver::new(&parsed);
    let output = transform_and_print_typescript_module(&parsed, ModuleKind::COMMON_JS, &resolver);

    assert!(output.contains("exports.value = 1;"), "{output}");
    assert!(output.contains("consume(exports.value);"), "{output}");
    assert!(!output.contains("const value"), "{output}");
    assert!(!output.contains("exports.value = void 0"), "{output}");
}

#[test]
fn common_js_erased_ambient_export_still_qualifies_same_file_reads() {
    let source_text = concat!("export declare let a: { __foo: 10 };\n", "a.___foo;\n",);
    let parsed = parse_source_file(
        "ambient-export-read.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = SourceExportContainerResolver::new(&parsed);
    let output = transform_and_print_typescript_module(&parsed, ModuleKind::COMMON_JS, &resolver);

    assert_eq!(
        output,
        concat!(
            "\"use strict\";\n",
            "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
            "exports.a.___foo;\n",
        ),
    );
}

#[test]
fn common_js_direct_variable_publishes_primary_before_collector_aliases() {
    let output = transform_and_print_module(
        concat!("export { value as alias };\n", "export const value = 1;\n",),
        ModuleKind::COMMON_JS,
    );

    assert!(
        output.contains("exports.value = exports.alias = void 0;"),
        "{output}"
    );
    let primary = output
        .find("exports.value = 1;")
        .unwrap_or_else(|| panic!("{output}"));
    let alias = output
        .find("exports.alias = exports.value;")
        .unwrap_or_else(|| panic!("{output}"));
    assert!(primary < alias, "{output}");
}

#[test]
fn dynamic_import_recovery_preserves_a_missing_argument_across_module_formats() {
    let source = concat!(
        "const missing = import();\n",
        "const present = import('./present');\n",
        "const extra = import('./first', './ignored');\n",
    );

    let common_js = transform_and_print_module(source, ModuleKind::COMMON_JS);
    assert!(
        common_js.contains("Promise.resolve().then(() => __importStar(require()))"),
        "{common_js}"
    );
    assert!(common_js.contains("require('./present')"), "{common_js}");
    assert!(common_js.contains("require('./first')"), "{common_js}");
    assert!(!common_js.contains("./ignored"), "{common_js}");

    let amd = transform_and_print_module(source, ModuleKind::AMD);
    assert!(amd.contains("require([,],"), "{amd}");
    assert!(amd.contains("require(['./present'],"), "{amd}");
    assert!(amd.contains("require(['./first'],"), "{amd}");
    assert!(!amd.contains("./ignored"), "{amd}");

    let umd = transform_and_print_module("const missing = import();\n", ModuleKind::UMD);
    assert!(umd.contains("= void 0"), "{umd}");
    assert!(umd.contains("require(_a)"), "{umd}");
    assert!(umd.contains("require([_a],"), "{umd}");
}

#[test]
fn umd_dynamic_import_preserves_literal_quote_provenance_in_its_amd_copy() {
    let output = transform_and_print_umd_module(concat!(
        "const single = import('./single');\n",
        "const double = import(\"./double\");\n",
    ));

    assert!(output.contains("require(['./single'],"), "{output}");
    assert!(output.contains("require([\"./double\"],"), "{output}");
}

#[test]
fn amd_nested_dynamic_import_reserves_executor_bindings_in_emit_order() {
    let output = transform_and_print_module(
        "async function load() { return import((await import(\"./foo\")).default); }\n",
        ModuleKind::AMD,
    );

    let outer = output
        .find("new Promise((resolve_1, reject_1)")
        .unwrap_or_else(|| panic!("{output}"));
    let nested = output
        .find("new Promise((resolve_2, reject_2)")
        .unwrap_or_else(|| panic!("{output}"));
    assert!(outer < nested, "{output}");
}

#[test]
fn umd_nested_dynamic_import_reserves_executor_bindings_in_emit_order() {
    let output = transform_and_print_umd_module(
        "async function load() { return import((await import(\"./foo\")).default); }\n",
    );

    let nested = output
        .find("new Promise((resolve_1, reject_1)")
        .unwrap_or_else(|| panic!("{output}"));
    let outer = output
        .find("new Promise((resolve_2, reject_2)")
        .unwrap_or_else(|| panic!("{output}"));
    assert!(nested < outer, "{output}");
}

#[test]
fn yield_parenthesizes_a_synthetic_umd_dynamic_import_comma_sequence() {
    let output = transform_and_print_umd_module(concat!(
        "export function* load(packageName) {\n",
        "    return yield import(packageName + '/package.json');\n",
        "}\n",
    ));

    assert!(
        output.contains("yield (_a = packageName + '/package.json', __syncRequire ?"),
        "{output}",
    );
}

#[test]
fn umd_dynamic_import_temp_is_owned_by_the_nearest_method_body() {
    let text = transform_and_print_umd_module(concat!(
        "class C {\n",
        "    path() { return './other'; }\n",
        "    dynamic() { return import(this.path()); }\n",
        "}\n",
    ));

    let class = text.find("class C").expect("emitted class");
    assert!(!text[..class].contains("var _a;"), "{text}");
    assert!(
        text.contains("dynamic() { var _a; return _a = this.path(),"),
        "{text}",
    );
}

#[test]
fn umd_dynamic_import_temp_is_owned_by_a_static_block() {
    let text =
        transform_and_print_umd_module("class C { static { consume(import(getPath())); } }\n");

    let class = text.find("class C").expect("emitted class");
    assert!(!text[..class].contains("var _a;"), "{text}");
    assert!(text.contains("static { var _a;"), "{text}");
    assert!(text.contains("require(_a)"), "{text}");
    assert!(text.contains("require([_a],"), "{text}");
}

#[test]
fn umd_dynamic_import_temp_converts_a_concise_arrow_to_an_owned_body() {
    let text = transform_and_print_umd_module("const load = () => import(getPath());\n");

    let arrow = text.find("const load").expect("emitted arrow");
    assert!(!text[..arrow].contains("var _a;"), "{text}");
    assert!(
        text.contains("const load = () => { var _a; return _a = getPath(),"),
        "{text}",
    );
}

#[test]
fn umd_dynamic_import_temp_moves_an_es2015_parameter_default_into_the_body() {
    let text = transform_and_print_umd_module(
        "function load(path = import(getPath())) { return path; }\n",
    );

    let function = text.find("function load").expect("emitted function");
    assert!(!text[..function].contains("var _a;"), "{text}");
    assert!(
        text.contains(
            "function load(path) { var _a; if (path === void 0) { path = (_a = getPath(),"
        ),
        "{text}",
    );
}

#[test]
fn umd_exported_function_inherits_the_module_parameter_temp_scope() {
    let text = transform_and_print_umd_module(
        "export function load(path = import(getPath())) { return path; }\n",
    );

    let function = text.find("function load").expect("emitted function");
    assert!(text[..function].contains("var _a;"), "{text}");
    assert!(
        text.contains("function load(path = (_a = getPath(),"),
        "{text}",
    );
    assert!(!text.contains("function load(path) { var _a;"), "{text}");
}

#[test]
fn namespace_exported_internal_alias_uses_container_storage() {
    let source_text = concat!(
        "export {};\n",
        "namespace local { import exports = m.c; }\n",
        "namespace published { export import exports = m.c; new exports(); }\n",
    );
    let parsed = parse_source_file("namespace-alias.ts", source_text, Default::default(), None);
    let resolver = NamespaceAliasResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::AMD.bits()),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("namespace alias transform");
    let text = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print namespace alias transform")
    .text()
    .to_owned();

    assert!(text.contains("var exports = m.c;"), "{text}");
    assert!(text.contains("published.exports = m.c;"), "{text}");
    assert!(text.contains("new published.exports();"), "{text}");
    assert_eq!(
        text.matches("var exports = m.c;").count(),
        1,
        "the exported alias must not allocate local storage:\n{text}",
    );
}

#[test]
fn namespace_erases_external_import_equals_even_when_the_alias_is_referenced() {
    let source_text = concat!(
        "export namespace published {\n",
        "    import foo = require(\"./dependency\");\n",
        "    var a = foo.x;\n",
        "}\n",
    );
    let parsed = parse_source_file(
        "namespace-external-import.ts",
        source_text,
        Default::default(),
        None,
    );
    // This resolver deliberately reports every alias as referenced. The
    // namespace-element visitor must still apply tsc's structural TS1147
    // recovery and erase the external import-equals declaration.
    let resolver = NamespaceAliasResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![transform_type_script(&options, &resolver)],
        false,
    )
    .expect("namespace TypeScript transform");
    let text = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print namespace TypeScript transform")
    .text()
    .to_owned();

    assert!(!text.contains("require(\"./dependency\")"), "{text}");
    assert!(text.contains("var a = foo.x;"), "{text}");
}

#[test]
fn module_export_substitution_uses_enum_binding_identity_across_namespaces() {
    let source_text = concat!(
        "export enum require { first, second }\n",
        "namespace local {\n",
        "    enum require { first, second }\n",
        "}\n",
        "namespace published {\n",
        "    export enum require { first, second }\n",
        "}\n",
    );
    let parsed = parse_source_file("enum-bindings.ts", source_text, Default::default(), None);
    let resolver = EnumBindingResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::AMD.bits()),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("AMD enum binding transform");
    let text = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print AMD enum binding transform")
    .text()
    .to_owned();

    assert_eq!(
        text.matches("exports.require = require = {}").count(),
        1,
        "only the source-file enum binding is a module export:\n{text}",
    );
    assert!(
        text.contains("})(require || (require = {}));"),
        "the nested unexported enum remains local:\n{text}",
    );
    assert!(
        text.contains("})(require = published.require || (published.require = {}));"),
        "the namespace export is owned by its namespace container:\n{text}",
    );
}

#[test]
fn common_js_marker_does_not_reown_exported_enum_jsdoc() {
    let source_text = "/**\n * comment\n */\nexport enum Color {\n    r, g, b\n}\n";
    let parsed = parse_source_file("enum-comment.ts", source_text, Default::default(), None);
    let resolver = EnumBindingResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("CommonJS enum transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print CommonJS enum transform")
    .text()
    .to_owned();

    let marker = output
        .find("Object.defineProperty(exports, \"__esModule\"")
        .expect("CommonJS marker");
    let export_initializer = output
        .find("exports.Color = void 0;")
        .expect("export initializer");
    let jsdoc = output.find("/**\n * comment\n */").expect("enum JSDoc");
    let declaration = output.find("var Color;").expect("enum declaration");
    assert!(marker < export_initializer && export_initializer < jsdoc && jsdoc < declaration);
    assert_eq!(
        output.matches("/**\n * comment\n */").count(),
        1,
        "{output}"
    );
}

#[test]
fn common_js_flattens_destructuring_assignments_to_all_exported_names() {
    let source_text = concat!(
        "export let exportedFoo: any;\n",
        "let nonexportedFoo: any;\n",
        "// sanity checks\n",
        "exportedFoo = null;\n",
        "nonexportedFoo = null;\n",
        "if (null as any) {\n",
        "    ({ exportedFoo, nonexportedFoo } = null as any);\n",
        "}\n",
        "else if (null as any) {\n",
        "    ({ foo: exportedFoo, bar: nonexportedFoo } = null as any);\n",
        "}\n",
        "else if (null as any) {\n",
        "    ({ foo: { bar: exportedFoo, baz: nonexportedFoo } } = null as any);\n",
        "}\n",
        "else if (null as any) {\n",
        "    ([exportedFoo, nonexportedFoo] = null as any);\n",
        "}\n",
        "else {\n",
        "    ([[exportedFoo, nonexportedFoo]] = null as any);\n",
        "}\n",
        "export { nonexportedFoo };\n",
        "export { exportedFoo as foo, nonexportedFoo as nfoo };\n",
    );
    let parsed = parse_source_file(
        "destructuring-exports.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = ExportedVariableResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("CommonJS exported destructuring transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print CommonJS exported destructuring")
    .text()
    .to_owned();

    assert_eq!(
        output,
        concat!(
            "\"use strict\";\n",
            "var _a, _b, _c, _d, _e;\n",
            "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
            "exports.nfoo = exports.foo = exports.nonexportedFoo = exports.exportedFoo = void 0;\n",
            "let nonexportedFoo;\n",
            "// sanity checks\n",
            "exports.foo = exports.exportedFoo = null;\n",
            "exports.nfoo = exports.nonexportedFoo = nonexportedFoo = null;\n",
            "if (null) {\n",
            "    (_a = null, exports.foo = exports.exportedFoo = _a.exportedFoo, exports.nfoo = exports.nonexportedFoo = nonexportedFoo = _a.nonexportedFoo);\n",
            "}\n",
            "else if (null) {\n",
            "    (_b = null, exports.foo = exports.exportedFoo = _b.foo, exports.nfoo = exports.nonexportedFoo = nonexportedFoo = _b.bar);\n",
            "}\n",
            "else if (null) {\n",
            "    (_c = null.foo, exports.foo = exports.exportedFoo = _c.bar, exports.nfoo = exports.nonexportedFoo = nonexportedFoo = _c.baz);\n",
            "}\n",
            "else if (null) {\n",
            "    (_d = null, exports.foo = exports.exportedFoo = _d[0], exports.nfoo = exports.nonexportedFoo = nonexportedFoo = _d[1]);\n",
            "}\n",
            "else {\n",
            "    (_e = null[0], exports.foo = exports.exportedFoo = _e[0], exports.nfoo = exports.nonexportedFoo = nonexportedFoo = _e[1]);\n",
            "}\n",
        )
    );
}

#[test]
fn common_js_flattens_exported_destructuring_variable_declarations() {
    let source_text = concat!(
        "export let { toString } = 1;\n",
        "{\n",
        "    let { toFixed } = 1;\n",
        "}\n",
    );
    let parsed = parse_source_file(
        "destructuring-variable-exports.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = ExportedVariableResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("CommonJS exported destructuring declaration transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print CommonJS exported destructuring declaration")
    .text()
    .to_owned();

    assert_eq!(
        output,
        concat!(
            "\"use strict\";\n",
            "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
            "exports.toString = void 0;\n",
            "exports.toString = 1..toString;\n",
            "{\n",
            "    let { toFixed } = 1;\n",
            "}\n",
        )
    );
}

#[test]
fn common_js_appends_explicit_exports_for_each_array_binding_leaf() {
    let source_text = concat!(
        "// issue: https://github.com/Microsoft/TypeScript/issues/10778\n",
        "const [a, , b] = [1, 2, 3];\n",
        "export { a, b };\n",
    );
    let parsed = parse_source_file(
        "export-array-binding-pattern.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = ExportedVariableResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("CommonJS explicit array-binding export transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print CommonJS explicit array-binding exports")
    .text()
    .to_owned();

    assert_eq!(
        output,
        concat!(
            "\"use strict\";\n",
            "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
            "exports.b = exports.a = void 0;\n",
            "// issue: https://github.com/Microsoft/TypeScript/issues/10778\n",
            "const [a, , b] = [1, 2, 3];\n",
            "exports.a = a;\n",
            "exports.b = b;\n",
        )
    );
}

#[test]
fn amd_unnamed_dependency_comment_is_emitted_only_outside_the_wrapper() {
    let output = transform_and_print_module(
        "///<amd-dependency path='bar'/>\n\nimport \"m2\";\n",
        ModuleKind::AMD,
    );

    assert_eq!(
        output.matches("amd-dependency path='bar'").count(),
        1,
        "{output}"
    );
    let define = output.find("define(").expect("AMD define wrapper");
    let dependency = output.find("\"bar\"").expect("AMD dependency path");
    assert!(define < dependency, "{output}");
    assert!(
        !output[define..].contains("amd-dependency path='bar'"),
        "{output}",
    );
}

#[test]
fn named_amd_module_keeps_named_dependency_semantics_without_reemitting_pragmas() {
    let output = transform_and_print_module(
        concat!(
            "///<amd-module name='named'/>\n",
            "///<amd-dependency path='bar' name='b'/>\n\n",
            "export const value = 1;\n",
        ),
        ModuleKind::AMD,
    );

    assert_eq!(
        output.matches("amd-module name='named'").count(),
        1,
        "{output}"
    );
    assert_eq!(
        output.matches("amd-dependency path='bar' name='b'").count(),
        1,
        "{output}",
    );
    assert!(output.contains("define(\"named\""), "{output}");
    assert!(
        output.contains("function (require, exports, b)"),
        "{output}"
    );
}

#[test]
fn umd_mixed_dependencies_keep_paired_aliases_and_one_outer_comment_prefix() {
    let output = transform_and_print_module(
        concat!(
            "///<amd-dependency path='bar' name='b'/>\n",
            "///<amd-dependency path='foo'/>\n",
            "///<amd-dependency path='goo' name='c'/>\n\n",
            "import \"m2\";\n",
        ),
        ModuleKind::UMD,
    );

    for pragma in [
        "amd-dependency path='bar' name='b'",
        "amd-dependency path='foo'",
        "amd-dependency path='goo' name='c'",
    ] {
        assert_eq!(output.matches(pragma).count(), 1, "{output}");
    }
    assert!(
        output.contains("function (require, exports, b, c)"),
        "{output}"
    );
    let bar = output.find("\"bar\"").expect("named dependency bar");
    let goo = output.find("\"goo\"").expect("named dependency goo");
    let foo = output.find("\"foo\"").expect("unnamed dependency foo");
    let m2 = output.find("\"m2\"").expect("external dependency m2");
    assert!(bar < goo && goo < foo && foo < m2, "{output}");
}

#[test]
fn umd_single_amd_dependency_pragmas_remain_owned_by_the_outer_source_prefix() {
    for (source, pragma, parameters) in [
        (
            "///<amd-dependency path='bar'/>\n\nimport m1 = require(\"m2\");\nm1.f();\n",
            "amd-dependency path='bar'",
            "function (require, exports)",
        ),
        (
            "///<amd-dependency path='bar' name='b'/>\n\nimport m1 = require(\"m2\");\nm1.f();\n",
            "amd-dependency path='bar' name='b'",
            "function (require, exports, b)",
        ),
    ] {
        let output = transform_and_print_module(source, ModuleKind::UMD);

        assert_eq!(output.matches(pragma).count(), 1, "{output}");
        let wrapper = output
            .find("(function (factory)")
            .expect("UMD outer wrapper");
        assert!(!output[wrapper..].contains(pragma), "{output}");
        assert!(output.contains(parameters), "{output}");
        let bar = output.find("\"bar\"").expect("pragma dependency");
        let m2 = output.find("\"m2\"").expect("external dependency");
        assert!(bar < m2, "{output}");
    }
}

#[test]
fn remove_comments_suppresses_relocated_prefix_without_losing_amd_dependency_semantics() {
    let output = transform_and_print_module_with_remove_comments(
        concat!(
            "///<amd-dependency path='bar' name='b'/>\n",
            "// ordinary detached comment\n\n",
            "export const value = 1;\n",
        ),
        ModuleKind::AMD,
        true,
    );

    assert!(!output.contains("amd-dependency"), "{output}");
    assert!(!output.contains("ordinary detached comment"), "{output}");
    assert!(output.contains("\"bar\""), "{output}");
    assert!(
        output.contains("function (require, exports, b)"),
        "{output}"
    );
}

#[test]
fn ordinary_detached_comment_uses_the_same_relocated_statement_list_contract() {
    let output = transform_and_print_module(
        "// detached control\n\nexport const value = 1;\n",
        ModuleKind::AMD,
    );

    assert_eq!(output.matches("// detached control").count(), 1, "{output}");
    let define = output.find("define(").expect("AMD define wrapper");
    assert!(
        !output[define..].contains("// detached control"),
        "{output}",
    );
}

#[test]
fn amd_import_re_exports_are_published_at_their_import_source_positions() {
    let output = transform_and_print_module(
        concat!(
            "import a = require(\"./dep\");\n",
            "a.value;\n",
            "import b, * as ns from \"./dep\";\n",
            "b; ns.value;\n",
            "import { value as named } from \"./dep\";\n",
            "named;\n",
            "export { a, b, ns, named };\n",
        ),
        ModuleKind::AMD,
    );

    let import_equals_export = output
        .find("exports.a = a;")
        .unwrap_or_else(|| panic!("{output}"));
    let import_equals_read = output
        .find("a.value;")
        .unwrap_or_else(|| panic!("{output}"));
    let namespace_alias = output
        .find("const ns = dep_1;")
        .unwrap_or_else(|| panic!("{output}"));
    let default_export = output
        .find("exports.b = dep_1.default;")
        .unwrap_or_else(|| panic!("{output}"));
    let namespace_export = output
        .find("exports.ns = ns;")
        .unwrap_or_else(|| panic!("{output}"));
    let named_export = output
        .find(concat!(
            "Object.defineProperty(exports, \"named\", ",
            "{ enumerable: true, get: function () { return dep_2.value; } });",
        ))
        .unwrap_or_else(|| panic!("{output}"));

    assert!(import_equals_export < import_equals_read, "{output}");
    assert!(import_equals_read < namespace_alias, "{output}");
    assert!(namespace_alias < default_export, "{output}");
    assert!(default_export < namespace_export, "{output}");
    assert!(namespace_export < named_export, "{output}");
}

#[test]
fn common_js_import_equals_appends_explicit_export_specifiers() {
    let output = transform_and_print_module(
        concat!(
            "import a = require(\"./dep\");\n",
            "a.value;\n",
            "export { a };\n",
        ),
        ModuleKind::COMMON_JS,
    );

    let declaration = output
        .find("const a = require(\"./dep\");")
        .unwrap_or_else(|| panic!("{output}"));
    let publication = output
        .find("exports.a = a;")
        .unwrap_or_else(|| panic!("{output}"));
    let read = output
        .find("a.value;")
        .unwrap_or_else(|| panic!("{output}"));
    assert!(declaration < publication && publication < read, "{output}");
}

#[test]
fn exported_import_equals_re_exports_from_the_export_object_owner() {
    let source = concat!(
        "export import a = require(\"./dep\");\n",
        "export { a as alias };\n",
    );

    let common_js = transform_and_print_module(source, ModuleKind::COMMON_JS);
    assert!(
        common_js.contains("exports.a = require(\"./dep\");"),
        "{common_js}",
    );
    assert!(
        common_js.contains("exports.alias = exports.a;"),
        "{common_js}",
    );
    assert!(!common_js.contains("exports.alias = a;"), "{common_js}");

    let amd = transform_and_print_module(source, ModuleKind::AMD);
    assert!(amd.contains("exports.a = a;"), "{amd}");
    assert!(amd.contains("exports.alias = exports.a;"), "{amd}");
    assert!(!amd.contains("exports.alias = a;"), "{amd}");
}

#[test]
fn named_default_function_namespace_merge_uses_the_source_export_owner() {
    let parsed = parse_source_file(
        "default-function-namespace.ts",
        concat!(
            "export default function Foo() {}\n",
            "namespace Foo { export var x; }\n",
        ),
        Default::default(),
        None,
    );
    let resolver = SourceNamedExportContainerResolver::new(&parsed, "Foo");
    let output = transform_and_print_typescript_module(&parsed, ModuleKind::COMMON_JS, &resolver);

    assert!(output.contains("exports.default = Foo;"), "{output}");
    assert!(
        output.contains(")(exports.Foo || (exports.Foo = {}));"),
        "{output}",
    );
    assert!(!output.contains(")(Foo || (Foo = {}));"), "{output}");
}

#[test]
fn amd_import_equals_call_keeps_its_local_parameter_receiver_free() {
    let source_text = concat!(
        "import fooFunc = require(\"dependency\");\n",
        "var n: number = fooFunc();\n",
    );
    let parsed = parse_source_file(
        "amd-import-equals-call.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = ImportEqualsCallResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::AMD.bits()),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("AMD import-equals call transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print AMD import-equals call")
    .text()
    .to_owned();

    assert!(output.contains("var n = fooFunc();"), "{output}");
    assert!(!output.contains("(0, fooFunc)()"), "{output}");
}

#[test]
fn common_js_file_level_generated_export_map_rejects_ordinary_generated_ids() {
    let parsed = parse_source_file(
        "using-file-level-defaults.ts",
        concat!(
            "declare function acquire(): Disposable;\n",
            "using resource = acquire();\n",
            "export default class {}\n",
            "export = resource;\n",
        ),
        Default::default(),
        None,
    );
    let resolver = EnumBindingResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2022.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_es_next(&options),
        ],
        false,
    )
    .expect("ESNext using-hoist transform");

    let arena = result.arena();
    let root = arena.root(source).expect("transformed source root");
    let syntax = arena.source(source).expect("transform source").syntax();
    let mut pending = vec![root];
    let mut identities = BTreeSet::new();
    let mut printable_texts = BTreeSet::new();
    let mut file_level_identifier = None;
    let mut non_file_generated_identifier = None;
    while let Some(node) = pending.pop() {
        let record = arena.node(node).expect("transformed node");
        if let Some(metadata) = arena.metadata(node) {
            if metadata.generated_binding_is_file_level_optimistic() {
                file_level_identifier.get_or_insert(node);
                identities.insert(
                    metadata
                        .generated_binding_id()
                        .expect("file-level name owns generated identity"),
                );
                let NodeData::Identifier(identifier) = &record.data else {
                    panic!("file-level generated binding must be an identifier");
                };
                printable_texts.insert(identifier.text.clone());
            } else if metadata.generated_binding_id().is_some() {
                non_file_generated_identifier.get_or_insert(node);
            }
        }
        for_each_child(&syntax.arena, record, |child| {
            if let Some(child) = arena.node_ref(source, child) {
                pending.push(child);
            }
            false
        });
    }

    assert_eq!(identities.len(), 2, "default export and export= identities");
    assert_eq!(
        printable_texts,
        BTreeSet::from(["_default".to_owned()]),
        "independent FileLevel names deliberately share printable text",
    );

    let mut exports = CommonJsFileLevelGeneratedBindingExports::default();
    let non_file_generated_identifier =
        non_file_generated_identifier.expect("using environment owns an ordinary generated ID");
    assert!(
        !exports.add_for_identifier(arena, non_file_generated_identifier, "bad"),
        "a GeneratedBindingId without FileLevelOptimistic metadata is rejected",
    );
    assert!(
        exports
            .get_for_identifier(arena, non_file_generated_identifier)
            .is_none(),
        "a non-file generated ID cannot enter the CommonJS export map",
    );

    let file_level_identifier = file_level_identifier.expect("default owns a FileLevel ID");
    assert!(exports.add_for_identifier(arena, file_level_identifier, "default"));
    assert_eq!(
        exports.get_for_identifier(arena, file_level_identifier),
        Some(&[Box::<str>::from("default")][..]),
    );
}

#[test]
fn common_js_export_equals_suppresses_appended_class_export() {
    let source_text = concat!(
        "export = exports;\n",
        "declare class exports {}\n",
        "export class Sub {}\n",
    );
    let parsed = parse_source_file(
        "export-equals-class.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = EnumBindingResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("CommonJS export-equals class transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print CommonJS export-equals class transform")
    .text()
    .to_owned();

    assert!(output.contains("exports.Sub = void 0;"), "{output}");
    assert!(output.contains("class Sub"), "{output}");
    assert!(!output.contains("exports.Sub = Sub;"), "{output}");
    assert!(output.contains("module.exports = exports;"), "{output}");
}

#[test]
fn common_js_duplicate_export_equals_keeps_the_first_assignment() {
    let output = transform_and_print_module(
        concat!(
            "var x = 10;\n",
            "var y = 20;\n",
            "var z = 30;\n",
            "export = x;\n",
            "export = y;\n",
            "export = z;\n",
        ),
        ModuleKind::COMMON_JS,
    );

    assert_eq!(output.matches("module.exports =").count(), 1, "{output}");
    assert!(output.contains("module.exports = x;"), "{output}");
    assert!(!output.contains("module.exports = y;"), "{output}");
    assert!(!output.contains("module.exports = z;"), "{output}");
}

#[test]
fn common_js_wraps_exported_legacy_decorator_assignment() {
    let source_text = "declare const dec: any;\n@dec\nexport class ClassA {}\n";
    let parsed = parse_source_file("decorated-export.ts", source_text, Default::default(), None);
    let resolver = EnumBindingResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        experimental_decorators: true,
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_legacy_decorators(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("CommonJS legacy-decorator export transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print CommonJS legacy-decorator export transform")
    .text()
    .to_owned();

    assert!(
        output.contains("exports.ClassA = ClassA = __decorate(["),
        "{output}",
    );
}

#[test]
fn common_js_materializes_anonymous_default_function_binding() {
    let source_text = "export default function () { return true; }\n";
    let parsed = parse_source_file(
        "anonymous-default.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = EnumBindingResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("CommonJS anonymous default function transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print CommonJS anonymous default function")
    .text()
    .to_owned();

    let publication = output
        .find("exports.default = default_1;")
        .unwrap_or_else(|| panic!("{output}"));
    let declaration = output
        .find("function default_1()")
        .unwrap_or_else(|| panic!("{output}"));
    assert!(publication < declaration, "{output}");
}

#[test]
fn export_default_reparenthesizes_a_class_exposed_by_type_erasure() {
    let source_text = "export default (class Foo {} as any);\n";
    let parsed = parse_source_file(
        "export-default-parenthesize.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = EnumBindingResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::ES_NEXT.bits()),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![transform_type_script(&options, &resolver)],
        false,
    )
    .expect("export-default class assertion transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print export-default class assertion")
    .text()
    .to_owned();

    assert_eq!(output, "export default (class Foo {\n});\n");
}

#[test]
fn typescript_transform_erases_preserved_jsx_type_arguments() {
    let output = transform_and_print_preserved_tsx(concat!(
        "const selfClosing = <Foo<unknown, string> />;\n",
        "const opening = <Foo<TypeProps>></Foo>;\n",
    ));

    assert_eq!(
        output,
        concat!(
            "const selfClosing = <Foo />;\n",
            "const opening = <Foo></Foo>;\n",
        )
    );
}

#[test]
fn typescript_transform_structurally_erases_jsx_recovery_type_arguments() {
    let output = transform_and_print_preserved_tsx(concat!(
        "const unknown = <Foo<?> />;\n",
        "const nullable = <Foo<string?>></Foo>;\n",
    ));

    assert_eq!(
        output,
        concat!(
            "const unknown = <Foo />;\n",
            "const nullable = <Foo></Foo>;\n",
        )
    );
}

#[test]
fn typescript_transform_preserves_jsdoc_recovery_type_arguments() {
    let source_text = concat!(
        "function foo<T>(x: T): T { return x; }\n",
        "const ValidFoo = foo<string>;\n",
        "const WhatFoo = foo<?>;\n",
        "const HuhFoo = foo<string?>;\n",
        "const NopeFoo = foo<?string>;\n",
        "const ComeOnFoo = foo<?string?>;\n",
        "type Erased = typeof foo<?>;\n",
    );
    let parsed = parse_source_file(
        "expression-with-jsdoc-type-arguments.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = EnumBindingResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::ES_NEXT.bits()),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![transform_type_script(&options, &resolver)],
        false,
    )
    .expect("JSDoc recovery type-argument transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print JSDoc recovery type arguments")
    .text()
    .to_owned();

    assert!(output.contains("const ValidFoo = foo;"), "{output}");
    for retained in [
        "const WhatFoo = foo<?>;",
        "const HuhFoo = foo<?string>;",
        "const NopeFoo = foo<?string>;",
        "const ComeOnFoo = foo<??string>;",
    ] {
        assert!(output.contains(retained), "missing {retained:?}:\n{output}");
    }
    assert!(!output.contains("type Erased"), "{output}");
}

#[test]
fn typescript_transform_materializes_missing_accessor_bodies() {
    let source_text = "var value = { get item() };\n";
    let parsed = parse_source_file(
        "accessors-without-bodies.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = EnumBindingResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::ES_NEXT.bits()),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![transform_type_script(&options, &resolver)],
        false,
    )
    .expect("missing accessor-body transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print accessors with materialized bodies")
    .text()
    .to_owned();

    assert_eq!(output, "var value = { get item() { } };\n");
}

#[test]
fn common_js_publishes_each_duplicate_anonymous_default_function() {
    let source_text = concat!(
        "export default interface A { a: string; }\n",
        "export default function () { return 1; }\n",
        "export default function () { return 2; }\n",
    );
    let parsed = parse_source_file(
        "duplicate-anonymous-default-functions.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = EnumBindingResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("duplicate anonymous default function transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print duplicate anonymous default functions")
    .text()
    .to_owned();

    let first_publication = output
        .find("exports.default = default_1;")
        .unwrap_or_else(|| panic!("{output}"));
    let second_publication = output
        .find("exports.default = default_2;")
        .unwrap_or_else(|| panic!("{output}"));
    let first_declaration = output
        .find("function default_1()")
        .unwrap_or_else(|| panic!("{output}"));
    let second_declaration = output
        .find("function default_2()")
        .unwrap_or_else(|| panic!("{output}"));
    assert!(
        first_publication < second_publication
            && second_publication < first_declaration
            && first_declaration < second_declaration,
        "{output}"
    );
}

#[test]
fn common_js_publishes_export_specifiers_for_erased_ambient_functions() {
    let source_text = concat!(
        "declare function foo(): any;\n",
        "declare function bar(): any;\n",
        "export { foo, bar as baz };\n",
    );
    let parsed = parse_source_file(
        "ambient-function-exports.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = AmbientFunctionExportResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("ambient function export transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print ambient function exports")
    .text()
    .to_owned();

    assert_eq!(
        output,
        concat!(
            "\"use strict\";\n",
            "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
            "exports.foo = foo;\n",
            "exports.baz = bar;\n",
        )
    );
}

#[test]
fn duplicate_default_re_export_preinitializes_a_hoisted_function_export() {
    let source_text = concat!(
        "export default function () {}\n",
        "export { default } from './hi';\n",
        "export { aa as default } from './hi';\n",
    );
    let parsed = parse_source_file(
        "export-default-duplicate.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = EnumBindingResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        es_module_interop: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("duplicate default export transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print duplicate default exports")
    .text()
    .to_owned();

    let preinitializer = output
        .find("exports.default = void 0;")
        .unwrap_or_else(|| panic!("{output}"));
    let publication = output
        .find("exports.default = default_1;")
        .unwrap_or_else(|| panic!("{output}"));
    let declaration = output
        .find("function default_1()")
        .unwrap_or_else(|| panic!("{output}"));
    assert!(
        preinitializer < publication && publication < declaration,
        "{output}"
    );
    assert_eq!(output.matches("exports.default = void 0;").count(), 1);
}

#[test]
fn common_js_default_class_skips_only_its_undefined_preinitializer() {
    let source_text = concat!(
        "export default class A { method() {} }\n",
        "export class B {}\n",
    );
    let parsed = parse_source_file("default-class.ts", source_text, Default::default(), None);
    let resolver = EnumBindingResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("CommonJS default-class transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print CommonJS default class")
    .text()
    .to_owned();

    assert!(!output.contains("exports.default = void 0;"), "{output}");
    assert!(output.contains("exports.B = void 0;"), "{output}");
    let declaration = output.find("class A").expect("default class declaration");
    let publication = output
        .find("exports.default = A;")
        .expect("default class publication");
    assert!(declaration < publication, "{output}");
}

#[test]
fn common_js_default_class_publishes_after_its_static_field_operations() {
    let source_text = concat!(
        "enum SomeEnum { one }\n",
        "export default class SomeClass {\n",
        "    public static E = SomeEnum;\n",
        "}\n",
    );
    let parsed = parse_source_file(
        "tsx-default-imports.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = EnumBindingResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        use_define_for_class_fields: Some(false),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_class_fields(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("CommonJS default-class static-field transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print CommonJS default-class static field")
    .text()
    .to_owned();

    let declaration = output.find("class SomeClass").expect("class declaration");
    let static_operation = output
        .find("SomeClass.E = SomeEnum;")
        .expect("class-owned static field operation");
    let publication = output
        .find("exports.default = SomeClass;")
        .expect("default export publication");
    assert!(
        declaration < static_operation && static_operation < publication,
        "{output}",
    );
}

#[test]
fn common_js_materializes_invalid_anonymous_exported_class_identity() {
    let source_text = "export class {\n}\n";
    let parsed = parse_source_file(
        "export-class-without-name.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = EnumBindingResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("CommonJS invalid anonymous exported class transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print CommonJS invalid anonymous exported class")
    .text()
    .to_owned();

    assert_eq!(
        output,
        concat!(
            "\"use strict\";\n",
            "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
            "class default_1 {\n",
            "}\n",
            "exports.default_1 = default_1;\n",
        )
    );
    assert!(!output.contains("exports.default_1 = void 0;"), "{output}");
}

#[test]
fn namespace_default_classes_share_generated_declaration_identity() {
    let source_text = concat!(
        "namespace ns_class { export default class {} }\n",
        "namespace ns_abstract_class { export default abstract class {} }\n",
    );
    let parsed = parse_source_file(
        "export-default-class-in-namespace.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = EnumBindingResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("namespace default-class transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print namespace default classes")
    .text()
    .to_owned();

    assert!(output.contains("class default_1 {"), "{output}");
    assert!(
        output.contains("ns_class.default_1 = default_1;"),
        "{output}"
    );
    assert!(output.contains("class default_2 {"), "{output}");
    assert!(
        output.contains("ns_abstract_class.default_2 = default_2;"),
        "{output}"
    );
}

#[test]
fn namespace_default_functions_keep_recovery_syntax_and_generated_export_identity() {
    let source_text = concat!(
        "namespace ns_function { export default function () {} }\n",
        "namespace ns_async_function { export default async function () {} }\n",
    );
    let parsed = parse_source_file(
        "export-default-function-in-namespace.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = EnumBindingResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_class_fields(&options, &resolver),
            transform_es2017(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("namespace default-function transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print namespace default functions")
    .text()
    .to_owned();

    assert!(output.contains("default function () { }"), "{output}");
    assert!(
        output.contains("ns_function.default_1 = default_1;"),
        "{output}"
    );
    assert!(
        output.contains("ns_async_function.default_2 = default_2;"),
        "{output}"
    );
}

#[test]
fn common_js_exported_updates_publish_new_values_and_preserve_postfix_results() {
    let source_text = concat!(
        "let bizz = 8;\n",
        "bizz++;\n",
        "bizz--;\n",
        "++bizz;\n",
        "let previous = bizz++;\n",
        "export { bizz };\n",
    );
    let parsed = parse_source_file("exported-updates.ts", source_text, Default::default(), None);
    let resolver = ExportedVariableResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("CommonJS exported-update transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print CommonJS exported updates")
    .text()
    .to_owned();

    assert_eq!(
        output,
        concat!(
            "\"use strict\";\n",
            "var _a;\n",
            "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
            "exports.bizz = void 0;\n",
            "let bizz = 8;\n",
            "exports.bizz = bizz;\n",
            "exports.bizz = (bizz++, bizz);\n",
            "exports.bizz = (bizz--, bizz);\n",
            "exports.bizz = ++bizz;\n",
            "let previous = (exports.bizz = (_a = bizz++, bizz), _a);\n",
        )
    );
}

#[test]
fn common_js_nested_export_update_temps_belong_to_the_module_scope() {
    let source_text = concat!(
        "let x = 1;\n",
        "export function foo(y: number) {\n",
        "    if (y <= x++) return y <= x++;\n",
        "    if (y <= x--) return y <= x--;\n",
        "    if (y <= ++x) return y <= ++x;\n",
        "    if (y <= --x) return y <= --x;\n",
        "    x++;\n",
        "    x--;\n",
        "    ++x;\n",
        "    --x;\n",
        "}\n",
        "export { x };\n",
    );
    let parsed = parse_source_file(
        "module-exports-unary-expression.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = ExportedVariableResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("CommonJS nested exported-update transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print CommonJS nested exported updates")
    .text()
    .to_owned();

    assert!(
        output.starts_with(concat!(
            "\"use strict\";\n",
            "var _a, _b, _c, _d;\n",
            "Object.defineProperty(exports, \"__esModule\"",
        )),
        "postfix result temporaries belong immediately after the module prologue:\n{output}",
    );
    let function = output
        .find("function foo(y) {")
        .map(|start| &output[start..])
        .expect("emitted exported function");
    assert!(
        !function.contains("var _"),
        "transformModule must not create a nested function hoist sink:\n{output}",
    );
    let temp_declarations = output
        .lines()
        .filter(|line| line.starts_with("var _"))
        .collect::<Vec<_>>();
    assert_eq!(
        temp_declarations,
        ["var _a, _b, _c, _d;"],
        "prefix updates and discarded postfix updates must not allocate result temporaries:\n{output}",
    );
}

#[test]
fn common_js_publishes_the_standard_decorator_synthetic_named_export() {
    let source_text = concat!(
        "declare var dec: any;\n",
        "export class C {\n",
        "    @dec x: any;\n",
        "    constructor(@dec x: any) {}\n",
        "}\n",
    );
    let parsed = parse_source_file(
        "parameter-decorators-emit.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = EnumBindingResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_standard_decorators(&options),
            transform_class_fields(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("standard decorator CommonJS transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print standard decorator CommonJS transform")
    .text()
    .to_owned();

    assert!(
        output.contains("if (_metadata) Object.defineProperty"),
        "{output}"
    );
    assert_eq!(output.matches("exports.C = C;").count(), 1, "{output}");
    assert!(output.ends_with("exports.C = C;\n"), "{output}");
}

#[test]
fn standard_decorator_class_references_preserve_parsed_and_generated_identity() {
    let parsed = parse_source_file(
        "standard-decorator-class-references.ts",
        concat!("@cls export class E {}\n", "consume(@cls class {});\n",),
        Default::default(),
        None,
    );
    let NodeData::SourceFile(parsed_source) = &parsed.arena.node(parsed.root).data else {
        panic!("parsed source-file root");
    };
    let mut parsed_name = None;
    let mut parsed_expression = None;
    let mut pending = parsed
        .arena
        .node_array(parsed_source.statements.expect("parsed statements"))
        .nodes
        .clone();
    while let Some(node) = pending.pop() {
        let record = parsed.arena.node(node);
        match &record.data {
            NodeData::ClassDeclaration(data) => parsed_name = data.name,
            NodeData::ClassExpression(_) => parsed_expression = Some(node),
            _ => {}
        }
        for_each_child(&parsed.arena, record, |child| {
            pending.push(child);
            false
        });
    }
    let parsed_name = parsed_name.expect("parsed class-declaration name");
    let parsed_name_record = parsed.arena.node(parsed_name);
    let parsed_name_range = (parsed_name_record.pos, parsed_name_record.end);
    let parsed_expression = parsed_expression.expect("parsed anonymous class expression");

    let resolver = LegacyScriptJsxResolver;
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2022.bits()),
        module: Some(ModuleKind::ES_NEXT.bits()),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_es_next(&options),
            transform_standard_decorators(&options),
            transform_class_fields(&options, &resolver),
        ],
        false,
    )
    .expect("standard-decorator class-reference transform");

    let arena = result.arena();
    let root = arena.root(source).expect("transformed source root");
    let syntax = arena.source(source).expect("transform source").syntax();
    let required_parsed_flags =
        EmitFlags::LOCAL_NAME | EmitFlags::NO_COMMENTS | EmitFlags::NO_SOURCE_MAP;
    let mut parsed_references = 0usize;
    let mut generated_references = 0usize;
    let mut generated_identities = BTreeSet::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        let record = arena.node(node).expect("transformed node");
        if let NodeData::Identifier(identifier) = &record.data {
            let metadata = arena.metadata(node);
            if identifier.text == "E"
                && metadata.is_some_and(|metadata| metadata.flags().contains(required_parsed_flags))
            {
                parsed_references += 1;
                assert_eq!(arena.get_original_node(node).node(), parsed_name);
                assert_eq!((record.pos, record.end), parsed_name_range);
            }
            if metadata.is_some_and(|metadata| metadata.generated_binding_base() == Some("class")) {
                generated_references += 1;
                let metadata = metadata.expect("generated class-reference metadata");
                generated_identities.insert(
                    metadata
                        .generated_binding_id()
                        .expect("generated class reference owns a binding identity"),
                );
                assert_eq!(arena.get_original_node(node).node(), parsed_expression);
                assert_eq!((record.pos, record.end), (u32::MAX, u32::MAX));
            }
        }
        for_each_child(&syntax.arena, record, |child| {
            if let Some(child) = arena.node_ref(source, child) {
                pending.push(child);
            }
            false
        });
    }

    assert_eq!(
        parsed_references, 3,
        "IIFE declaration, decoration assignment, and return assignment",
    );
    assert_eq!(
        generated_references, 3,
        "anonymous IIFE references must be separately materialized",
    );
    assert_eq!(
        generated_identities.len(),
        1,
        "all anonymous IIFE references must share one TargetBinding",
    );
}

#[test]
fn using_anonymous_default_decorator_handoff_keeps_owner_and_binding_domains() {
    for (case, class_body, expected_default_projections) in [
        ("class decorator", "{}", 3usize),
        ("class and member decorators", "{ @dec m() {} }", 5usize),
    ] {
        let source_text = format!(
            concat!(
                "declare function acquire(): Disposable;\n",
                "declare const cls: any;\n",
                "declare const dec: any;\n",
                "using resource = acquire();\n",
                "@cls export default class {class_body}\n",
            ),
            class_body = class_body,
        );
        let parsed = parse_source_file(
            "using-standard-default-handoff.ts",
            &source_text,
            Default::default(),
            None,
        );
        let NodeData::SourceFile(parsed_source) = &parsed.arena.node(parsed.root).data else {
            panic!("parsed source-file root");
        };
        let parsed_class = parsed
            .arena
            .node_array(parsed_source.statements.expect("parsed statements"))
            .nodes
            .iter()
            .copied()
            .find(|statement| {
                parsed.arena.node(*statement).kind == tsc_syntax::SyntaxKind::ClassDeclaration
            })
            .expect("parsed anonymous default class");

        let resolver = LegacyScriptJsxResolver;
        let mut arena = TransformArena::new();
        let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
        let options = CompilerOptions {
            target: Some(ScriptTarget::ES2022.bits()),
            module: Some(ModuleKind::ES_NEXT.bits()),
            use_define_for_class_fields: Some(true),
            always_strict: Some(false),
            ..CompilerOptions::default()
        };
        let result = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(source)],
            vec![
                transform_type_script(&options, &resolver),
                transform_es_next(&options),
                transform_standard_decorators(&options),
                transform_class_fields(&options, &resolver),
            ],
            false,
        )
        .unwrap_or_else(|error| panic!("{case} transform failed: {error}"));

        let arena = result.arena();
        let root = arena.root(source).expect("transformed source root");
        let syntax = arena.source(source).expect("transform source").syntax();
        let mut pending = vec![root];
        let mut default_ids = BTreeSet::new();
        let mut file_level_ids = BTreeSet::new();
        let mut default_projections = 0usize;
        let mut default_identifier = None;
        let mut file_level_identifier = None;
        while let Some(node) = pending.pop() {
            let record = arena.node(node).expect("transformed node");
            if let NodeData::Identifier(identifier) = &record.data {
                if let Some(metadata) = arena.metadata(node) {
                    if metadata.generated_binding_base() == Some("default") {
                        default_identifier.get_or_insert(node);
                        default_projections += 1;
                        default_ids.insert(
                            metadata
                                .generated_binding_id()
                                .expect("ordinary default projection owns an ID"),
                        );
                        assert_eq!(identifier.text, "default_1", "{case}");
                        assert_eq!((record.pos, record.end), (u32::MAX, u32::MAX), "{case}");
                        assert_eq!(arena.get_original_node(node).node(), parsed_class, "{case}");
                        assert!(
                            !metadata.flags().intersects(
                                EmitFlags::LOCAL_NAME
                                    | EmitFlags::NO_COMMENTS
                                    | EmitFlags::NO_SOURCE_MAP
                            ),
                            "generated projections must bypass parsed-name flags ({case})",
                        );
                        assert!(
                            !metadata.generated_binding_is_file_level_optimistic(),
                            "ordinary default_N must not enter the FileLevel domain ({case})",
                        );
                    }
                    if metadata.generated_binding_is_file_level_optimistic() {
                        file_level_identifier.get_or_insert(node);
                        assert_eq!(identifier.text, "_default", "{case}");
                        assert_eq!(
                            metadata.generated_binding_preferred_base(),
                            Some("_default"),
                            "{case}",
                        );
                        assert!(
                            metadata.generated_binding_reserved_in_nested_scopes(),
                            "FileLevel _default remains reserved below the source scope ({case})",
                        );
                        file_level_ids.insert(
                            metadata
                                .generated_binding_id()
                                .expect("FileLevel projection owns an ID"),
                        );
                    }
                }
            }
            for_each_child(&syntax.arena, record, |child| {
                if let Some(child) = arena.node_ref(source, child) {
                    pending.push(child);
                }
                false
            });
        }

        assert_eq!(default_projections, expected_default_projections, "{case}");
        assert_eq!(
            default_ids.len(),
            1,
            "one ordinary default_N identity ({case})"
        );
        assert_eq!(
            file_level_ids.len(),
            1,
            "one FileLevel _default identity ({case})"
        );
        assert!(
            default_ids.is_disjoint(&file_level_ids),
            "ordinary default_N and FileLevel _default must remain separate domains ({case})",
        );
        let mut exports = CommonJsFileLevelGeneratedBindingExports::default();
        let file_level_identifier =
            file_level_identifier.expect("FileLevel representative identifier");
        let default_identifier = default_identifier.expect("ordinary default representative");
        assert!(
            exports.add_for_identifier(arena, file_level_identifier, "default"),
            "FileLevel _default enters the CommonJS identity map ({case})",
        );
        assert!(
            !exports.add_for_identifier(arena, default_identifier, "bad"),
            "ordinary default_N must be rejected by the CommonJS identity map ({case})",
        );
        assert!(
            exports
                .get_for_identifier(arena, default_identifier)
                .is_none(),
            "ordinary default_N cannot resolve through the FileLevel map ({case})",
        );
    }
}

#[test]
fn legacy_decorator_and_class_field_temps_share_final_binding_order() {
    let source_text = concat!(
        "declare const dec: any;\n",
        "declare function key(): string;\n",
        "class C { @dec [key()]: any; [key()]: any = 1; }\n",
    );
    let parsed = parse_source_file(
        "decorated-computed.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = EnumBindingResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        experimental_decorators: true,
        use_define_for_class_fields: Some(false),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_legacy_decorators(&options, &resolver),
            transform_class_fields(&options, &resolver),
            transform_es2016(&options),
        ],
        false,
    )
    .expect("composed legacy-decorator and class-field transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print composed generated bindings")
    .text()
    .to_owned();

    let class_field_binding = output
        .find("var _a;")
        .unwrap_or_else(|| panic!("class-field binding:\n{output}"));
    let decorator_binding = output
        .find("var _b;")
        .unwrap_or_else(|| panic!("decorator binding:\n{output}"));
    assert!(class_field_binding < decorator_binding, "{output}");
    assert!(output.contains("this[_a] = 1;"), "{output}");
    assert!(output.contains("_b = key(), _a = key();"), "{output}");
    assert!(output.contains("C.prototype, _b, void 0"), "{output}");
}

#[test]
fn decorated_computed_names_are_shared_for_identifiers_and_class_expressions() {
    let source_text = concat!(
        "declare const dec: any;\n",
        "declare const propertyName: string;\n",
        "declare function key(): string;\n",
        "class C { @dec [propertyName]: any = 1; }\n",
        "void class D { @dec [key()]: any = 1; };\n",
    );
    let parsed = parse_source_file(
        "decorated-computed-ownership.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = EnumBindingResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        experimental_decorators: true,
        use_define_for_class_fields: Some(false),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_legacy_decorators(&options, &resolver),
            transform_class_fields(&options, &resolver),
            transform_es2016(&options),
        ],
        false,
    )
    .expect("decorated computed-name ownership transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print decorated computed-name ownership")
    .text()
    .to_owned();

    assert!(output.contains("var _a;\nvar _b, _c;"), "{output}");
    assert!(output.contains("this[_b] = 1;"), "{output}");
    assert!(output.contains("_b = propertyName;"), "{output}");
    assert!(output.contains("C.prototype, _b, void 0"), "{output}");
    assert!(output.contains("this[_c] = 1;"), "{output}");
    assert!(output.contains("_c = key(),"), "{output}");
}

#[test]
fn legacy_decorator_recovers_default_class_without_export() {
    let source_text = concat!(
        "declare function decorator(constructor: any): any;\n",
        "@decorator default class {}\n",
    );
    let parsed = parse_source_file(
        "default-without-export.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = EnumBindingResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        experimental_decorators: true,
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_legacy_decorators(&options, &resolver),
        ],
        false,
    )
    .expect("legacy-decorator default recovery transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print legacy-decorator default recovery")
    .text()
    .to_owned();

    assert!(output.contains("let default_1 = class {"), "{output}");
    assert!(
        output.contains("default_1 = __decorate([\n    decorator\n], default_1);"),
        "{output}",
    );
    assert!(!output.contains("export default"), "{output}");
}

#[test]
fn classic_jsx_preserves_single_line_whitespace_text_children() {
    let source_text = concat!(
        "var p = 0;\n",
        "var whitespace1 = <div>      </div>;\n",
        "var whitespace2 = <div>  {p}    </div>;\n",
        "var whitespace3 = <div>\n",
        "    {p}\n",
        "</div>;\n",
        "var nested = <Foo> <Bar> q </Bar> <Bar />   s <Bar /><Bar /></Foo>;\n",
    );
    let parsed = parse_source_file(
        "classic-jsx-whitespace.tsx",
        source_text,
        ParseOptions {
            language_variant: LanguageVariant::Jsx,
            ..ParseOptions::default()
        },
        None,
    );
    let resolver = LegacyScriptJsxResolver;
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        jsx: Some(2),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_jsx(&options, &resolver),
        ],
        false,
    )
    .expect("classic JSX whitespace transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print classic JSX whitespace transform")
    .text()
    .to_owned();

    assert_eq!(
        output,
        concat!(
            "var p = 0;\n",
            "var whitespace1 = React.createElement(\"div\", null, \"      \");\n",
            "var whitespace2 = React.createElement(\"div\", null,\n",
            "    \"  \",\n",
            "    p,\n",
            "    \"    \");\n",
            "var whitespace3 = React.createElement(\"div\", null, p);\n",
            "var nested = React.createElement(Foo, null,\n",
            "    \" \",\n",
            "    React.createElement(Bar, null, \" q \"),\n",
            "    \" \",\n",
            "    React.createElement(Bar, null),\n",
            "    \"   s \",\n",
            "    React.createElement(Bar, null),\n",
            "    React.createElement(Bar, null));\n",
        )
    );
}

#[test]
fn common_js_substitutes_orphan_automatic_jsx_import_references() {
    let source_text = "const value = <div>{null/* preserved */}</div>;\n";
    let parsed = parse_source_file(
        "legacy-script.tsx",
        source_text,
        ParseOptions {
            language_variant: LanguageVariant::Jsx,
            ..ParseOptions::default()
        },
        None,
    );
    let resolver = LegacyScriptJsxResolver;
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        jsx: Some(4),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_jsx(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("legacy-script automatic JSX transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print legacy-script automatic JSX transform")
    .text()
    .to_owned();

    assert!(
        output.contains("(0, _a.jsx)(\"div\", { children: null /* preserved */ })"),
        "{output}",
    );
    assert!(!output.contains("react/jsx-runtime"), "{output}");
}

#[test]
fn system_module_preserves_generated_automatic_jsx_local_reference() {
    let source_text = "export {}; const value = <div>{null/* preserved */}</div>;\n";
    let parsed = parse_source_file(
        "system-jsx.tsx",
        source_text,
        ParseOptions {
            language_variant: LanguageVariant::Jsx,
            ..ParseOptions::default()
        },
        None,
    );
    let resolver = LegacyScriptJsxResolver;
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::SYSTEM.bits()),
        jsx: Some(4),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_jsx(&options, &resolver),
            transform_system_module(&options, &resolver),
        ],
        false,
    )
    .expect("System automatic JSX transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print System automatic JSX transform")
    .text()
    .to_owned();

    assert!(output.contains("_jsx(\"div\""), "{output}");
    assert!(!output.contains("(0, jsx_runtime_1.jsx)"), "{output}");
}

#[test]
fn system_default_import_call_keeps_the_substituted_direct_callee() {
    let source_text = concat!(
        "import repeat from \"repeat\";\n",
        "const value: string = repeat(\"text\", 2);\n",
    );
    let parsed = parse_source_file(
        "system-default-import-call.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = DefaultImportCallResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::SYSTEM.bits()),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_system_module(&options, &resolver),
        ],
        false,
    )
    .expect("System default-import call transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print System default-import call")
    .text()
    .to_owned();

    assert!(output.contains("repeat_1.default(\"text\", 2)"), "{output}");
    assert!(!output.contains("(0, repeat_1.default)"), "{output}");
}

#[test]
fn system_module_reuses_single_destructuring_initializer_without_a_temp() {
    let source_text = concat!(
        "export let { toString } = 1;\n",
        "{\n",
        "    let { toFixed } = 1;\n",
        "}\n",
    );
    let parsed = parse_source_file(
        "system-destructuring-variable.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = ExportedVariableResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::SYSTEM.bits()),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_system_module(&options, &resolver),
        ],
        false,
    )
    .expect("System exported destructuring declaration transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print System exported destructuring declaration")
    .text()
    .to_owned();

    assert_eq!(
        output,
        concat!(
            "System.register([], function (exports_1, context_1) {\n",
            "    \"use strict\";\n",
            "    var toString;\n",
            "    var __moduleName = context_1 && context_1.id;\n",
            "    return {\n",
            "        setters: [],\n",
            "        execute: function () {\n",
            "            exports_1(\"toString\", toString = 1..toString);\n",
            "            {\n",
            "                let { toFixed } = 1;\n",
            "            }\n",
            "        }\n",
            "    };\n",
            "});\n",
        )
    );
}

#[test]
fn system_module_hoists_uninitialized_export_from_source_owned_if_statement() {
    let source_text = concat!(
        "// https://github.com/microsoft/TypeScript/issues/59373\n\n",
        "if (true)\n",
        "export const cssExports: CssExports;\n",
        "export default cssExports;\n",
    );
    let parsed = parse_source_file(
        "system-export-in-if.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = ExportedVariableResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::SYSTEM.bits()),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let transformers = get_script_transformers(&options, &resolver)
        .expect("construct the complete System transform pipeline");
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        transformers,
        false,
    )
    .expect("System embedded uninitialized export transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print System embedded uninitialized export")
    .text()
    .to_owned();

    assert_eq!(
        output,
        concat!(
            "// https://github.com/microsoft/TypeScript/issues/59373\n",
            "System.register([], function (exports_1, context_1) {\n",
            "    \"use strict\";\n",
            "    var cssExports;\n",
            "    var __moduleName = context_1 && context_1.id;\n",
            "    return {\n",
            "        setters: [],\n",
            "        execute: function () {\n",
            "            if (true) { }\n",
            "            exports_1(\"default\", cssExports);\n",
            "        }\n",
            "    };\n",
            "});\n",
        )
    );
}

#[test]
fn common_js_named_re_exports_use_live_bindings_and_source_comments() {
    let source_text = concat!(
        "/* retained */\n",
        "// retained line\n",
        "export { subject } from \"./0\";\n",
    );
    let parsed = parse_source_file("named-re-export.ts", source_text, Default::default(), None);
    let resolver = EnumBindingResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("CommonJS named re-export transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print CommonJS named re-export")
    .text()
    .to_owned();

    assert!(output.contains("exports.subject = void 0;"), "{output}");
    assert!(output.contains("var _0_1 = require(\"./0\");"), "{output}",);
    assert!(
        output.contains(concat!(
            "Object.defineProperty(exports, \"subject\", ",
            "{ enumerable: true, get: function () { return _0_1.subject; } });",
        )),
        "{output}",
    );
    assert_eq!(output.matches("/* retained */").count(), 1, "{output}");
    assert_eq!(output.matches("// retained line").count(), 1, "{output}");
}

#[test]
fn common_js_unused_export_star_identity_does_not_consume_a_module_name() {
    let source_text = concat!(
        "import * as fs from \"./fs\";\n",
        "fs;\n",
        "export * from \"./fs\";\n",
        "export { x } from \"./fs\";\n",
        "export { x as y } from \"./fs\";\n",
    );
    let parsed = parse_source_file("module-name.ts", source_text, Default::default(), None);
    let resolver = EnumBindingResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        es_module_interop: Some(true),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("CommonJS export-star module-name transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print CommonJS export-star module names")
    .text()
    .to_owned();

    assert!(
        output.contains("__exportStar(require(\"./fs\"), exports);"),
        "{output}"
    );
    assert!(output.contains("var fs_1 = require(\"./fs\");"), "{output}");
    assert!(output.contains("var fs_2 = require(\"./fs\");"), "{output}");
    assert!(!output.contains("fs_3"), "{output}");
}

#[test]
fn common_js_trailing_module_specifier_uses_tsc_generated_name() {
    let source_text = "import { register } from \"./\";\n";
    let parsed = parse_source_file("module-name.ts", source_text, Default::default(), None);
    let resolver = EnumBindingResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("CommonJS trailing module-specifier transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print CommonJS trailing module-specifier name")
    .text()
    .to_owned();

    assert!(output.contains("const _1 = require(\"./\");"), "{output}");
    assert!(!output.contains("module_1"), "{output}");
}

#[test]
fn common_js_generated_require_leaves_import_tail_comments_with_the_statement() {
    let source_text = concat!(
        "import { first } from \"./first\"; // first tail\n",
        "import { second } from \"second\"; // second tail\n",
        "first; second;\n",
    );
    let parsed = parse_source_file(
        "import-tail-comments.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = EnumBindingResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("CommonJS import-tail comment transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print CommonJS import-tail comments")
    .text()
    .to_owned();

    assert!(
        output.contains("require(\"./first\"); // first tail"),
        "{output}",
    );
    assert!(
        output.contains("require(\"second\"); // second tail"),
        "{output}",
    );
    assert_eq!(output.matches("// first tail").count(), 1, "{output}");
    assert_eq!(output.matches("// second tail").count(), 1, "{output}");
}

#[test]
fn common_js_direct_export_does_not_reexport_a_conflicting_import_binding() {
    let source_text = concat!(
        "import * as pick from \"./pick\";\n",
        "export const pick = () => pick();\n",
    );
    let parsed = parse_source_file(
        "conflicting-import.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = EnumBindingResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("CommonJS conflicting import transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print CommonJS conflicting import")
    .text()
    .to_owned();

    assert_eq!(
        output.matches("exports.pick = pick;").count(),
        1,
        "{output}"
    );
    let import = output.find("require(\"./pick\")").expect("runtime import");
    let declaration = output
        .rfind("const pick = () =>")
        .expect("local declaration");
    let publication = output
        .rfind("exports.pick = pick;")
        .expect("local publication");
    assert!(
        import < declaration && declaration < publication,
        "{output}"
    );
}

#[test]
fn private_method_constructor_references_use_the_class_alias() {
    let source_text = concat!(
        "class C {\n",
        "    #field = 1;\n",
        "    #method() { return new C().#field; }\n",
        "}\n",
    );
    let parsed = parse_source_file(
        "private-constructor-reference.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = ConstructorReferenceResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        use_define_for_class_fields: Some(false),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_class_fields(&options, &resolver),
            transform_es2016(&options),
        ],
        false,
    )
    .expect("private constructor-reference transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print private constructor-reference transform")
    .text()
    .to_owned();

    assert!(
        output.contains("var _C_instances, _a, _C_field, _C_method;"),
        "{output}",
    );
    assert!(
        output.contains(concat!(
            "_a = C, _C_field = new WeakMap(), _C_instances = new WeakSet(), ",
            "_C_method = function _C_method() { return __classPrivateFieldGet(new _a(), ",
            "_C_field, \"f\"); };",
        )),
        "moved private methods must reference the stable class alias:\n{output}",
    );
}

struct MergedNamespaceResolver {
    source_file: NodeId,
    primary_declaration: NodeId,
    declarations: Vec<NodeId>,
    references: BTreeSet<NodeId>,
}

impl MergedNamespaceResolver {
    fn new(source: &tsc_syntax::SourceFile) -> Self {
        let mut class_declaration = None;
        let mut namespace_declaration = None;
        let mut references = BTreeSet::new();
        let mut stack = vec![source.root];
        while let Some(node) = stack.pop() {
            let record = source.arena.node(node);
            match &record.data {
                NodeData::ClassDeclaration(data)
                    if data.name.is_some_and(|name| {
                        matches!(
                            &source.arena.node(name).data,
                            NodeData::Identifier(identifier) if identifier.text == "Observable"
                        )
                    }) =>
                {
                    class_declaration = Some(node);
                }
                NodeData::ModuleDeclaration(data)
                    if data.name.is_some_and(|name| {
                        matches!(
                            &source.arena.node(name).data,
                            NodeData::Identifier(identifier) if identifier.text == "Observable"
                        )
                    }) =>
                {
                    namespace_declaration = Some(node);
                }
                NodeData::Identifier(identifier) if identifier.text == "Observable" => {
                    references.insert(node);
                }
                _ => {}
            }
            for_each_child(&source.arena, record, |child| {
                stack.push(child);
                false
            });
        }
        let class_declaration = class_declaration.expect("merged class declaration");
        let namespace_declaration = namespace_declaration.expect("merged namespace declaration");
        Self {
            source_file: source.root,
            primary_declaration: class_declaration,
            declarations: vec![class_declaration, namespace_declaration],
            references,
        }
    }
}

impl EmitResolver for MergedNamespaceResolver {
    fn get_constant_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_export_container(
        &self,
        node: EmitResolverNode,
        mode: EmitExportContainerMode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(
            (mode.prefixes_locals() && self.references.contains(&node.node()))
                .then(|| EmitResolverNode::new(node.source(), self.source_file)),
        )
    }

    fn get_referenced_import_declaration(
        &self,
        _node: EmitResolverNode,
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
            .then(|| EmitResolverNode::new(node.source(), self.primary_declaration)))
    }

    fn get_referenced_value_declarations(
        &self,
        node: EmitResolverNode,
    ) -> Result<Vec<EmitResolverNode>, EmitResolverError> {
        Ok(if self.references.contains(&node.node()) {
            self.declarations
                .iter()
                .copied()
                .map(|declaration| EmitResolverNode::new(node.source(), declaration))
                .collect()
        } else {
            Vec::new()
        })
    }

    fn is_instantiated_module(&self, _node: EmitResolverNode) -> Result<bool, EmitResolverError> {
        Ok(true)
    }
}

#[test]
fn common_js_merged_exported_namespace_publishes_its_initializer() {
    let source_text = concat!(
        "export declare class Observable<T> {}\n",
        "export namespace Observable {\n",
        "    let someValue: number;\n",
        "}\n",
    );
    let parsed = parse_source_file("merged-namespace.ts", source_text, Default::default(), None);
    let resolver = MergedNamespaceResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_type_script(&options, &resolver),
            transform_module(&options, &resolver),
        ],
        false,
    )
    .expect("CommonJS merged namespace transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print CommonJS merged namespace")
    .text()
    .to_owned();

    assert!(output.contains("exports.Observable = void 0;"), "{output}");
    assert!(
        output.contains("})(Observable || (exports.Observable = Observable = {}));",),
        "{output}",
    );
}

/// The B-5 registered joint pass: the ES5 pipeline pushes
/// `transformES2015` then `transformGenerators` between the es2016 entry
/// and the module transformer — the upstream registration order
/// (`_tsc.js:115942-115945`, owner-graph `upstream_registration`).
#[test]
fn es5_pipeline_registers_the_joint_es2015_generators_pass_in_upstream_order() {
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES5.bits()),
        module: Some(ModuleKind::SYSTEM.bits()),
        ..CompilerOptions::default()
    };
    struct PipelineShapeResolver;
    impl crate::EmitResolver for PipelineShapeResolver {
        fn get_constant_value(
            &self,
            _node: crate::EmitResolverNode,
        ) -> Result<Option<crate::EmitConstantValue>, crate::EmitResolverError> {
            Ok(None)
        }

        fn get_enum_member_value(
            &self,
            _node: crate::EmitResolverNode,
        ) -> Result<Option<crate::EmitEnumMemberValue>, crate::EmitResolverError> {
            Ok(None)
        }
    }
    let resolver = PipelineShapeResolver;
    let transformers = get_script_transformers(&options, &resolver)
        .expect("construct the complete ES5 transform pipeline");
    let names = transformers
        .iter()
        .map(|transformer| transformer.name())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            "transformTypeScript",
            "transformESNext",
            "transformESDecorators",
            "transformClassFields",
            "transformES2021",
            "transformES2020",
            "transformES2019",
            "transformES2018",
            "transformES2017",
            "transformES2016",
            "transformES2015",
            "transformGenerators",
            "transformSystemModule",
        ],
    );
}

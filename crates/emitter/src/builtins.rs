use std::collections::{BTreeMap, BTreeSet};

use tsc_syntax::{
    for_each_child, for_each_child_array, try_visit_each_child, Node, NodeArrayId, NodeData,
    NodeDataChildVisitor, NodeId, SyntaxKind,
};
use tsc_types::{CompilerOptions, NodeFlags, ScriptTarget};

use crate::{
    EmitHint, EmitResolver, EmitResolverNode, H2ActivityCanary, TransformArena, TransformError,
    TransformFlags, TransformNode, TransformNodeArray, TransformRoot, TransformSourceId,
    TransformationContext, Transformer, UnsupportedTransformFeature,
};

const MODULE_PRESERVE: i32 = 200;

/// tsc-port: getScriptTransformers @6.0.3
/// tsc-hash: 69bdc65a0c428ad5819419fabd0ecd483bb661350434c5ad0ea0bdec15096fd0
/// tsc-span: _tsc.js:115903-115949
pub fn get_script_transformers<'resolver>(
    options: &CompilerOptions,
    resolver: &'resolver dyn EmitResolver,
) -> Result<Vec<Box<dyn Transformer + 'resolver>>, TransformError> {
    let mut activity = H2ActivityCanary::h1_profile();
    get_script_transformers_with_activity(options, resolver, &mut activity)
}

pub(crate) fn get_script_transformers_with_activity<'resolver>(
    options: &CompilerOptions,
    resolver: &'resolver dyn EmitResolver,
    activity: &mut H2ActivityCanary,
) -> Result<Vec<Box<dyn Transformer + 'resolver>>, TransformError> {
    if options.emit_script_target() != ScriptTarget::ES_NEXT {
        return Err(TransformError::UnsupportedCompilerOption {
            option: "target",
            detail: "the H1 bootstrap transformer list requires ESNext",
        });
    }
    if options.emit_module_kind() != MODULE_PRESERVE {
        return Err(TransformError::UnsupportedCompilerOption {
            option: "module",
            detail: "the H1 bootstrap transformer list requires Preserve",
        });
    }
    if !options.use_define_for_class_fields_effective() {
        return Err(TransformError::UnsupportedCompilerOption {
            option: "useDefineForClassFields",
            detail: "the H1 bootstrap profile requires absent or true",
        });
    }
    if options.experimental_decorators {
        return Err(TransformError::UnsupportedCompilerOption {
            option: "experimentalDecorators",
            detail: "legacy decorator transformation is outside the H1 bootstrap profile",
        });
    }

    activity.construct_script_transformer_list();
    activity.construct_transform_typescript();
    let transform_typescript = transform_type_script(resolver);
    activity.construct_transform_class_fields();
    let transform_class_fields = transform_class_fields(options);
    activity.construct_transform_ecmascript_module();
    let transform_ecmascript_module = transform_ecmascript_module(options);
    Ok(vec![
        transform_typescript,
        transform_class_fields,
        transform_ecmascript_module,
    ])
}

/// tsc-port: transformTypeScript @6.0.3
/// tsc-hash: 08e4305bdbb440c9e05fb551bd3a1988f4edf67ccc0119b2642f7a3dd2258e79
/// tsc-span: _tsc.js:94036-95849
pub fn transform_type_script<'resolver>(
    resolver: &'resolver dyn EmitResolver,
) -> Box<dyn Transformer + 'resolver> {
    Box::new(TypeScriptTransformer { resolver })
}

/// tsc-port: transformClassFields @6.0.3
/// tsc-hash: 65cacc85f81402ff4468cf65c7636dbd5a0ce9eb6c3248f060aa5193c3af8304
/// tsc-span: _tsc.js:95852-98038
pub fn transform_class_fields(options: &CompilerOptions) -> Box<dyn Transformer> {
    Box::new(ClassFieldsTransformer {
        target: options.emit_script_target(),
        use_define_for_class_fields: options.use_define_for_class_fields_effective(),
    })
}

/// tsc-port: transformECMAScriptModule @6.0.3
/// tsc-hash: a4106ecc07d7c7b1d1caa38cb6ef962b9c244316d94f1f6acca0e3d497b28d22
/// tsc-span: _tsc.js:113369-113727
pub fn transform_ecmascript_module(options: &CompilerOptions) -> Box<dyn Transformer> {
    Box::new(EcmaScriptModuleTransformer {
        module_kind: options.emit_module_kind(),
        rewrite_relative_import_extensions: options
            .rewrite_relative_import_extensions
            .unwrap_or(false),
    })
}

struct TypeScriptTransformer<'resolver> {
    resolver: &'resolver dyn EmitResolver,
}

impl Transformer for TypeScriptTransformer<'_> {
    fn name(&self) -> &'static str {
        "transformTypeScript"
    }

    fn initialize(&mut self, context: &mut TransformationContext) -> Result<(), TransformError> {
        context.enable_substitution(SyntaxKind::PropertyAccessExpression)?;
        context.enable_substitution(SyntaxKind::ElementAccessExpression)?;
        Ok(())
    }

    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        let TransformRoot::SourceFile(source) = root else {
            return Err(TransformError::Unsupported(
                crate::UnsupportedEmitFeature::BundleRoot,
            ));
        };
        if context.arena().source(source)?.syntax().is_declaration_file {
            return Ok(TransformRoot::SourceFile(source));
        }
        preflight_source(context.arena(), source)?;
        initialize_transform_flags(context.arena_mut()?, source)?;
        let root_node = context.arena().root(source)?;
        let mut visitor = TypeScriptVisitor::new(context, source, self.resolver);
        let transformed =
            visitor
                .visit(root_node.node())?
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::SourceFile,
                    field: "root",
                })?;
        let transformed = visitor
            .context
            .arena()
            .node_ref(source, transformed)
            .expect("transform visitor only returns nodes from its source");
        visitor
            .context
            .arena_mut()?
            .replace_root(source, transformed)?;
        Ok(TransformRoot::SourceFile(source))
    }
}

struct ClassFieldsTransformer {
    target: ScriptTarget,
    use_define_for_class_fields: bool,
}

impl Transformer for ClassFieldsTransformer {
    fn name(&self) -> &'static str {
        "transformClassFields"
    }

    fn initialize(&mut self, _context: &mut TransformationContext) -> Result<(), TransformError> {
        if self.target != ScriptTarget::ES_NEXT || !self.use_define_for_class_fields {
            return Err(TransformError::UnsupportedCompilerOption {
                option: "class-field transform",
                detail: "downlevel class-field branches are inactive in the H1 bootstrap profile",
            });
        }
        Ok(())
    }
}

struct EcmaScriptModuleTransformer {
    module_kind: i32,
    rewrite_relative_import_extensions: bool,
}

impl Transformer for EcmaScriptModuleTransformer {
    fn name(&self) -> &'static str {
        "transformECMAScriptModule"
    }

    fn initialize(&mut self, context: &mut TransformationContext) -> Result<(), TransformError> {
        if self.module_kind != MODULE_PRESERVE || self.rewrite_relative_import_extensions {
            return Err(TransformError::UnsupportedCompilerOption {
                option: "module transform",
                detail: "only Preserve without relative-extension rewriting is active",
            });
        }
        context.enable_emit_notification(SyntaxKind::SourceFile)?;
        context.enable_substitution(SyntaxKind::Identifier)?;
        Ok(())
    }

    fn substitute_node(
        &mut self,
        _context: &TransformationContext,
        _hint: EmitHint,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        Ok(node)
    }
}

struct TypeScriptVisitor<'context, 'resolver> {
    context: &'context mut TransformationContext,
    source: TransformSourceId,
    resolver: &'resolver dyn EmitResolver,
    nodes: BTreeMap<NodeId, Option<NodeId>>,
    arrays: BTreeMap<NodeArrayId, Option<NodeArrayId>>,
}

impl<'context, 'resolver> TypeScriptVisitor<'context, 'resolver> {
    fn new(
        context: &'context mut TransformationContext,
        source: TransformSourceId,
        resolver: &'resolver dyn EmitResolver,
    ) -> Self {
        Self {
            context,
            source,
            resolver,
            nodes: BTreeMap::new(),
            arrays: BTreeMap::new(),
        }
    }

    fn visit(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        if let Some(mapped) = self.nodes.get(&id) {
            return Ok(*mapped);
        }
        let original = self
            .context
            .arena()
            .node_ref(self.source, id)
            .ok_or_else(|| TransformError::UnknownNode(self.node(id)))?;
        let record = self.context.arena().node(original)?.clone();
        let kind = record.kind;

        let transformed = if is_type_node(kind)
            || is_typescript_modifier(kind)
            || matches!(
                kind,
                SyntaxKind::InterfaceDeclaration
                    | SyntaxKind::TypeAliasDeclaration
                    | SyntaxKind::IndexSignature
                    | SyntaxKind::NamespaceExportDeclaration
                    | SyntaxKind::PropertySignature
                    | SyntaxKind::MethodSignature
                    | SyntaxKind::CallSignature
                    | SyntaxKind::ConstructSignature
            ) {
            None
        } else {
            match record.data {
                NodeData::Token => Some(id),
                NodeData::AsExpression(data) => {
                    self.visit_partially_emitted(original, data.expression)?
                }
                NodeData::SatisfiesExpression(data) => {
                    self.visit_partially_emitted(original, data.expression)?
                }
                NodeData::TypeAssertionExpression(data) => {
                    self.visit_partially_emitted(original, data.expression)?
                }
                NodeData::NonNullExpression(data) => {
                    self.visit_partially_emitted(original, data.expression)?
                }
                NodeData::FunctionDeclaration(mut data) => {
                    if data.body.is_none()
                        || self.has_modifier(data.modifiers, SyntaxKind::DeclareKeyword)?
                    {
                        None
                    } else {
                        data.type_parameters = None;
                        data.r#type = None;
                        Some(self.update_generic(original, NodeData::FunctionDeclaration(data))?)
                    }
                }
                NodeData::FunctionExpression(mut data) => {
                    data.type_parameters = None;
                    data.r#type = None;
                    Some(self.update_generic(original, NodeData::FunctionExpression(data))?)
                }
                NodeData::ArrowFunction(mut data) => {
                    data.type_parameters = None;
                    data.r#type = None;
                    Some(self.update_generic(original, NodeData::ArrowFunction(data))?)
                }
                NodeData::Parameter(mut data) => {
                    if data.name.is_some_and(|name| {
                        self.context
                            .arena()
                            .node_ref(self.source, name)
                            .and_then(|name| self.context.arena().node(name).ok())
                            .is_some_and(|name| name.kind == SyntaxKind::ThisKeyword)
                    }) {
                        None
                    } else {
                        data.question_token = None;
                        data.r#type = None;
                        Some(self.update_generic(original, NodeData::Parameter(data))?)
                    }
                }
                NodeData::VariableStatement(data) => {
                    if self.has_modifier(data.modifiers, SyntaxKind::DeclareKeyword)? {
                        None
                    } else {
                        Some(self.update_generic(original, NodeData::VariableStatement(data))?)
                    }
                }
                NodeData::VariableDeclaration(mut data) => {
                    data.exclamation_token = None;
                    data.r#type = None;
                    Some(self.update_generic(original, NodeData::VariableDeclaration(data))?)
                }
                NodeData::ClassDeclaration(mut data) => {
                    if self.has_modifier(data.modifiers, SyntaxKind::DeclareKeyword)? {
                        None
                    } else {
                        data.type_parameters = None;
                        Some(self.update_generic(original, NodeData::ClassDeclaration(data))?)
                    }
                }
                NodeData::ClassExpression(mut data) => {
                    data.type_parameters = None;
                    Some(self.update_generic(original, NodeData::ClassExpression(data))?)
                }
                NodeData::PropertyDeclaration(mut data) => {
                    if self.has_modifier(data.modifiers, SyntaxKind::DeclareKeyword)?
                        || self.has_modifier(data.modifiers, SyntaxKind::AbstractKeyword)?
                    {
                        None
                    } else {
                        data.question_token = None;
                        data.exclamation_token = None;
                        data.r#type = None;
                        Some(self.update_generic(original, NodeData::PropertyDeclaration(data))?)
                    }
                }
                NodeData::Constructor(mut data) => {
                    if data.body.is_none() {
                        None
                    } else {
                        data.type_parameters = None;
                        data.r#type = None;
                        data.modifiers = None;
                        Some(self.update_generic(original, NodeData::Constructor(data))?)
                    }
                }
                NodeData::MethodDeclaration(mut data) => {
                    if data.body.is_none() {
                        None
                    } else {
                        data.question_token = None;
                        data.exclamation_token = None;
                        data.type_parameters = None;
                        data.r#type = None;
                        Some(self.update_generic(original, NodeData::MethodDeclaration(data))?)
                    }
                }
                NodeData::GetAccessor(mut data) => {
                    if data.body.is_none() {
                        None
                    } else {
                        data.type_parameters = None;
                        data.r#type = None;
                        Some(self.update_generic(original, NodeData::GetAccessor(data))?)
                    }
                }
                NodeData::SetAccessor(mut data) => {
                    if data.body.is_none() {
                        None
                    } else {
                        data.type_parameters = None;
                        data.r#type = None;
                        Some(self.update_generic(original, NodeData::SetAccessor(data))?)
                    }
                }
                NodeData::CallExpression(mut data) => {
                    data.type_arguments = None;
                    Some(self.update_generic(original, NodeData::CallExpression(data))?)
                }
                NodeData::NewExpression(mut data) => {
                    data.type_arguments = None;
                    Some(self.update_generic(original, NodeData::NewExpression(data))?)
                }
                NodeData::TaggedTemplateExpression(mut data) => {
                    data.type_arguments = None;
                    Some(self.update_generic(original, NodeData::TaggedTemplateExpression(data))?)
                }
                NodeData::ExpressionWithTypeArguments(mut data) => {
                    data.type_arguments = None;
                    Some(
                        self.update_generic(original, NodeData::ExpressionWithTypeArguments(data))?,
                    )
                }
                NodeData::HeritageClause(data) if data.token == SyntaxKind::ImplementsKeyword => {
                    None
                }
                NodeData::ImportDeclaration(data) => {
                    self.visit_import_declaration(original, data)?
                }
                NodeData::ExportDeclaration(data) => {
                    self.visit_export_declaration(original, data)?
                }
                NodeData::ExportAssignment(data) => {
                    if self
                        .resolver
                        .is_value_alias_declaration(self.resolver_node(original)?)?
                    {
                        Some(self.update_generic(original, NodeData::ExportAssignment(data))?)
                    } else {
                        None
                    }
                }
                data => Some(self.update_generic(original, data)?),
            }
        };
        self.nodes.insert(id, transformed);
        Ok(transformed)
    }

    fn update_generic(
        &mut self,
        original: TransformNode,
        mut data: NodeData,
    ) -> Result<NodeId, TransformError> {
        try_visit_each_child(&mut data, self)?;
        let flags = flags_after_update(self.context.arena(), original, &data)?;
        let updated = self.context.factory()?.update_node(original, data, flags)?;
        Ok(updated.node())
    }

    fn visit_partially_emitted(
        &mut self,
        original: TransformNode,
        expression: Option<NodeId>,
    ) -> Result<Option<NodeId>, TransformError> {
        let expression = match expression {
            Some(id) => self.visit(id)?,
            None => None,
        };
        let data = NodeData::PartiallyEmittedExpression(
            tsc_syntax::nodes::PartiallyEmittedExpressionData { expression },
        );
        let flags = self.context.arena().transform_flags(original);
        let created = self
            .context
            .factory()?
            .create_node(self.source, data, flags)?;
        self.context.factory()?.set_text_range(created, original)?;
        self.context
            .arena_mut()?
            .set_original_node(created, Some(original))?;
        Ok(Some(created.node()))
    }

    fn visit_import_declaration(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ImportDeclarationData,
    ) -> Result<Option<NodeId>, TransformError> {
        let Some(clause_id) = data.import_clause else {
            return Ok(Some(
                self.update_generic(original, NodeData::ImportDeclaration(data))?,
            ));
        };
        let clause_node = self.node(clause_id);
        let clause_data = match &self.context.arena().node(clause_node)?.data {
            NodeData::ImportClause(data) => data.clone(),
            _ => {
                return Ok(Some(
                    self.update_generic(original, NodeData::ImportDeclaration(data))?,
                ));
            }
        };
        if clause_data.is_type_only {
            return Ok(None);
        }
        let clause = self.visit_import_clause(clause_node, clause_data)?;
        let Some(clause) = clause else {
            return Ok(None);
        };
        data.import_clause = Some(clause.node());
        data.modifiers = None;
        Ok(Some(self.update_generic(
            original,
            NodeData::ImportDeclaration(data),
        )?))
    }

    fn visit_import_clause(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ImportClauseData,
    ) -> Result<Option<TransformNode>, TransformError> {
        if data.name.is_some()
            && !self
                .resolver
                .is_referenced_alias_declaration(self.resolver_node(original)?)?
        {
            data.name = None;
        }
        if let Some(bindings) = data.named_bindings {
            data.named_bindings = self.visit_import_bindings(bindings)?;
        }
        if data.name.is_none() && data.named_bindings.is_none() {
            return Ok(None);
        }
        let node = self.update_generic(original, NodeData::ImportClause(data))?;
        Ok(Some(self.node(node)))
    }

    fn visit_import_bindings(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        let node = self.node(id);
        match self.context.arena().node(node)?.data.clone() {
            NodeData::NamespaceImport(_data) => {
                if self
                    .resolver
                    .is_referenced_alias_declaration(self.resolver_node(node)?)?
                {
                    Ok(Some(id))
                } else {
                    Ok(None)
                }
            }
            NodeData::NamedImports(mut data) => {
                let Some(elements) = data.elements else {
                    return Ok(None);
                };
                let original_array = self.array(elements);
                let ids = self
                    .context
                    .arena()
                    .node_array(original_array)?
                    .nodes
                    .clone();
                let mut retained = Vec::new();
                for specifier in ids {
                    let specifier_node = self.node(specifier);
                    let is_type_only = match &self.context.arena().node(specifier_node)?.data {
                        NodeData::ImportSpecifier(data) => data.is_type_only,
                        _ => false,
                    };
                    if !is_type_only
                        && self
                            .resolver
                            .is_referenced_alias_declaration(self.resolver_node(specifier_node)?)?
                    {
                        retained.push(specifier_node);
                    }
                }
                if retained.is_empty() {
                    return Ok(None);
                }
                let updated = self
                    .context
                    .factory()?
                    .update_node_array(original_array, retained)?;
                data.elements = Some(updated.array());
                Ok(Some(
                    self.update_generic(node, NodeData::NamedImports(data))?,
                ))
            }
            _ => self.visit(id),
        }
    }

    fn visit_export_declaration(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ExportDeclarationData,
    ) -> Result<Option<NodeId>, TransformError> {
        if data.is_type_only {
            return Ok(None);
        }
        let Some(clause) = data.export_clause else {
            return Ok(Some(
                self.update_generic(original, NodeData::ExportDeclaration(data))?,
            ));
        };
        let clause_node = self.node(clause);
        let NodeData::NamedExports(mut named) =
            self.context.arena().node(clause_node)?.data.clone()
        else {
            return Ok(Some(
                self.update_generic(original, NodeData::ExportDeclaration(data))?,
            ));
        };
        let Some(elements) = named.elements else {
            return Ok(None);
        };
        let original_array = self.array(elements);
        let ids = self
            .context
            .arena()
            .node_array(original_array)?
            .nodes
            .clone();
        let mut retained = Vec::new();
        for specifier in ids {
            let specifier_node = self.node(specifier);
            let is_type_only = match &self.context.arena().node(specifier_node)?.data {
                NodeData::ExportSpecifier(data) => data.is_type_only,
                _ => false,
            };
            if !is_type_only
                && self
                    .resolver
                    .is_value_alias_declaration(self.resolver_node(specifier_node)?)?
            {
                retained.push(specifier_node);
            }
        }
        if retained.is_empty() {
            return Ok(None);
        }
        named.elements = Some(
            self.context
                .factory()?
                .update_node_array(original_array, retained)?
                .array(),
        );
        data.export_clause = Some(self.update_generic(clause_node, NodeData::NamedExports(named))?);
        data.modifiers = None;
        Ok(Some(self.update_generic(
            original,
            NodeData::ExportDeclaration(data),
        )?))
    }

    fn resolver_node(&self, node: TransformNode) -> Result<EmitResolverNode, TransformError> {
        let original = self.context.arena().get_original_node(node);
        let source = self
            .context
            .arena()
            .source(original.source())?
            .program_source()
            .ok_or(TransformError::MissingProgramSource(original))?;
        Ok(EmitResolverNode::new(source, original.node()))
    }

    fn has_modifier(
        &self,
        modifiers: Option<NodeArrayId>,
        expected: SyntaxKind,
    ) -> Result<bool, TransformError> {
        let Some(modifiers) = modifiers else {
            return Ok(false);
        };
        let array = self.array(modifiers);
        Ok(self
            .context
            .arena()
            .node_array(array)?
            .nodes
            .iter()
            .any(|id| {
                self.context
                    .arena()
                    .node_ref(self.source, *id)
                    .and_then(|node| self.context.arena().node(node).ok())
                    .is_some_and(|node| node.kind == expected)
            }))
    }

    const fn node(&self, id: NodeId) -> TransformNode {
        TransformNode::new(self.source, id)
    }

    const fn array(&self, id: NodeArrayId) -> TransformNodeArray {
        TransformNodeArray::new(self.source, id)
    }
}

impl NodeDataChildVisitor for TypeScriptVisitor<'_, '_> {
    type Error = TransformError;

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.context
            .arena()
            .node(self.node(id))
            .expect("generated child belongs to the current transform source")
            .kind
    }

    fn visit_node(&mut self, id: NodeId) -> Result<Option<NodeId>, Self::Error> {
        self.visit(id)
    }

    fn visit_nodes(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, Self::Error> {
        if let Some(mapped) = self.arrays.get(&id) {
            return Ok(*mapped);
        }
        let original = self.array(id);
        let nodes = self.context.arena().node_array(original)?.nodes.clone();
        let mut visited = Vec::with_capacity(nodes.len());
        for node in nodes {
            if let Some(node) = self.visit(node)? {
                visited.push(self.node(node));
            }
        }
        let updated = self
            .context
            .factory()?
            .update_node_array(original, visited)?;
        let mapped = Some(updated.array());
        self.arrays.insert(id, mapped);
        Ok(mapped)
    }

    fn required_child_removed(&mut self, parent: SyntaxKind, field: &'static str) -> Self::Error {
        TransformError::RequiredChildRemoved { parent, field }
    }
}

fn preflight_source(
    arena: &TransformArena,
    source: TransformSourceId,
) -> Result<(), TransformError> {
    let syntax = arena.source(source)?.syntax();
    let mut stack = vec![syntax.root];
    while let Some(id) = stack.pop() {
        let node = syntax.arena.node(id);
        let feature = match node.kind {
            SyntaxKind::Decorator => Some(UnsupportedTransformFeature::Decorators),
            SyntaxKind::ImportEqualsDeclaration => Some(UnsupportedTransformFeature::ImportEquals),
            SyntaxKind::EnumDeclaration => Some(UnsupportedTransformFeature::RuntimeEnums),
            SyntaxKind::ModuleDeclaration => Some(UnsupportedTransformFeature::RuntimeNamespaces),
            kind if is_jsx_kind(kind) => Some(UnsupportedTransformFeature::Jsx),
            SyntaxKind::ExportAssignment
                if matches!(
                    &node.data,
                    NodeData::ExportAssignment(data) if data.is_export_equals == Some(true)
                ) =>
            {
                Some(UnsupportedTransformFeature::ExportEquals)
            }
            SyntaxKind::Parameter if parameter_has_property_modifier(syntax, node) => {
                Some(UnsupportedTransformFeature::ParameterProperties)
            }
            _ => None,
        };
        if let Some(feature) = feature {
            return Err(TransformError::UnsupportedSyntax {
                feature,
                node: arena
                    .node_ref(source, id)
                    .expect("source walk yields a transform-arena node"),
            });
        }
        for_each_child(&syntax.arena, node, |child| {
            stack.push(child);
            false
        });
    }
    Ok(())
}

fn parameter_has_property_modifier(source: &tsc_syntax::SourceFile, node: &Node) -> bool {
    let NodeData::Parameter(data) = &node.data else {
        return false;
    };
    let Some(modifiers) = data.modifiers else {
        return false;
    };
    source
        .arena
        .node_array(modifiers)
        .nodes
        .iter()
        .any(|modifier| {
            matches!(
                source.arena.node(*modifier).kind,
                SyntaxKind::PublicKeyword
                    | SyntaxKind::PrivateKeyword
                    | SyntaxKind::ProtectedKeyword
                    | SyntaxKind::ReadonlyKeyword
                    | SyntaxKind::OverrideKeyword
            )
        })
}

const fn is_jsx_kind(kind: SyntaxKind) -> bool {
    kind as u16 >= SyntaxKind::JsxElement as u16
        && kind as u16 <= SyntaxKind::JsxNamespacedName as u16
}

const fn is_type_node(kind: SyntaxKind) -> bool {
    kind as u16 >= SyntaxKind::FirstTypeNode as u16
        && kind as u16 <= SyntaxKind::LastTypeNode as u16
}

const fn is_typescript_modifier(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PublicKeyword
            | SyntaxKind::PrivateKeyword
            | SyntaxKind::ProtectedKeyword
            | SyntaxKind::AbstractKeyword
            | SyntaxKind::OverrideKeyword
            | SyntaxKind::DeclareKeyword
            | SyntaxKind::ReadonlyKeyword
    )
}

fn flags_after_update(
    arena: &TransformArena,
    original: TransformNode,
    data: &NodeData,
) -> Result<TransformFlags, TransformError> {
    let old = arena.transform_flags(original);
    let record = arena.node(original)?;
    let mut probe = record.clone();
    probe.data = data.clone();
    let mut flags = old & !TransformFlags::CONTAINS_TYPE_SCRIPT;
    if local_contains_typescript(&probe) {
        flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
    }
    let source = arena.source(original.source())?.syntax();
    for_each_child(&source.arena, &probe, |child| {
        if let Some(child) = arena.node_ref(original.source(), child) {
            if let Ok(child_flags) = arena.propagate_child_flags(child) {
                flags |= child_flags & TransformFlags::CONTAINS_TYPE_SCRIPT;
            }
        }
        false
    });
    Ok(flags)
}

fn initialize_transform_flags(
    arena: &mut TransformArena,
    source: TransformSourceId,
) -> Result<(), TransformError> {
    let root = arena.root(source)?.node();
    let mut visiting = BTreeSet::new();
    let mut complete = BTreeSet::new();
    compute_transform_flags(arena, source, root, &mut visiting, &mut complete)?;
    Ok(())
}

fn compute_transform_flags(
    arena: &mut TransformArena,
    source: TransformSourceId,
    id: NodeId,
    visiting: &mut BTreeSet<NodeId>,
    complete: &mut BTreeSet<NodeId>,
) -> Result<TransformFlags, TransformError> {
    if complete.contains(&id) {
        return Ok(arena.transform_flags(
            arena
                .node_ref(source, id)
                .expect("completed transform node remains in its arena"),
        ));
    }
    if !visiting.insert(id) {
        return Ok(TransformFlags::NONE);
    }
    let node = arena
        .node_ref(source, id)
        .ok_or_else(|| TransformError::UnknownNode(TransformNode::new(source, id)))?;
    let record = arena.node(node)?.clone();
    let syntax = arena.source(source)?.syntax();
    let mut children = Vec::new();
    for_each_child(&syntax.arena, &record, |child| {
        children.push(child);
        false
    });
    let mut arrays = Vec::new();
    for_each_child_array(&record, |array| {
        arrays.push(array);
        false
    });

    for child in &children {
        compute_transform_flags(arena, source, *child, visiting, complete)?;
    }
    for array in arrays {
        let array_ref = arena
            .node_array_ref(source, array)
            .expect("generated child array belongs to its source");
        let ids = arena.node_array(array_ref)?.nodes.clone();
        let mut flags = TransformFlags::NONE;
        for child in ids {
            let child_flags = compute_transform_flags(arena, source, child, visiting, complete)?;
            let child = arena
                .node_ref(source, child)
                .expect("generated array child belongs to its source");
            let kind = arena.node(child)?.kind;
            flags |= child_flags & !TransformFlags::subtree_exclusions(kind);
        }
        arena.set_array_transform_flags(array_ref, flags);
    }

    let mut flags = local_transform_flags(&record);
    for child in children {
        let child = arena
            .node_ref(source, child)
            .expect("generated child belongs to its source");
        flags |= arena.propagate_child_flags(child)?;
    }
    arena.set_transform_flags(node, flags);
    visiting.remove(&id);
    complete.insert(id);
    Ok(flags)
}

fn local_contains_typescript(node: &Node) -> bool {
    local_transform_flags(node).contains(TransformFlags::CONTAINS_TYPE_SCRIPT)
}

fn local_transform_flags(node: &Node) -> TransformFlags {
    let kind = node.kind;
    let mut flags = TransformFlags::NONE;
    if is_type_node(kind)
        || is_typescript_modifier(kind)
        || matches!(
            kind,
            SyntaxKind::InterfaceDeclaration
                | SyntaxKind::TypeAliasDeclaration
                | SyntaxKind::IndexSignature
                | SyntaxKind::NamespaceExportDeclaration
                | SyntaxKind::PropertySignature
                | SyntaxKind::MethodSignature
                | SyntaxKind::CallSignature
                | SyntaxKind::ConstructSignature
                | SyntaxKind::ImportEqualsDeclaration
        )
    {
        flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
    }
    match &node.data {
        NodeData::Token => match kind {
            SyntaxKind::ThisKeyword => flags |= TransformFlags::CONTAINS_LEXICAL_THIS,
            SyntaxKind::SuperKeyword => {
                flags |= TransformFlags::CONTAINS_ES_2015;
                flags |= TransformFlags::CONTAINS_LEXICAL_SUPER;
            }
            _ => {}
        },
        NodeData::Parameter(data) => {
            if data.question_token.is_some() || data.r#type.is_some() {
                flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
            }
            if data.dot_dot_dot_token.is_some() || data.initializer.is_some() {
                flags |= TransformFlags::CONTAINS_ES_2015;
            }
        }
        NodeData::PropertyDeclaration(data) => {
            flags |= TransformFlags::CONTAINS_CLASS_FIELDS;
            if data.question_token.is_some()
                || data.exclamation_token.is_some()
                || data.r#type.is_some()
            {
                flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
            }
        }
        NodeData::MethodDeclaration(data) => {
            flags |= TransformFlags::CONTAINS_ES_2015;
            if data.question_token.is_some()
                || data.type_parameters.is_some()
                || data.r#type.is_some()
            {
                flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
            }
        }
        NodeData::Constructor(data) => {
            flags |= TransformFlags::CONTAINS_ES_2015;
            if data.type_parameters.is_some() || data.r#type.is_some() {
                flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
            }
        }
        NodeData::FunctionDeclaration(data) => {
            flags |= TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION;
            if data.type_parameters.is_some() || data.r#type.is_some() {
                flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
            }
        }
        NodeData::FunctionExpression(data) => {
            flags |= TransformFlags::CONTAINS_ES_2015;
            if data.type_parameters.is_some() || data.r#type.is_some() {
                flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
            }
        }
        NodeData::ArrowFunction(data) => {
            flags |= TransformFlags::CONTAINS_ES_2015;
            if data.type_parameters.is_some() || data.r#type.is_some() {
                flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
            }
        }
        NodeData::ClassDeclaration(data) => {
            flags |= TransformFlags::CONTAINS_ES_2015;
            if data.type_parameters.is_some() {
                flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
            }
        }
        NodeData::ClassExpression(data) => {
            flags |= TransformFlags::CONTAINS_ES_2015;
            if data.type_parameters.is_some() {
                flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
            }
        }
        NodeData::VariableDeclaration(data) => {
            if data.exclamation_token.is_some() || data.r#type.is_some() {
                flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
            }
        }
        NodeData::VariableDeclarationList(_) => {
            flags |= TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION;
            let node_flags = NodeFlags::from_bits(node.flags);
            if node_flags.intersects(NodeFlags::BLOCK_SCOPED) {
                flags |= TransformFlags::CONTAINS_ES_2015;
                flags |= TransformFlags::CONTAINS_BLOCK_SCOPED_BINDING;
            }
        }
        NodeData::ReturnStatement(_) => {
            flags |= TransformFlags::CONTAINS_ES_2018;
            flags |= TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION;
        }
        NodeData::AsExpression(_)
        | NodeData::SatisfiesExpression(_)
        | NodeData::TypeAssertionExpression(_)
        | NodeData::NonNullExpression(_) => {
            flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
        }
        NodeData::CallExpression(data) => {
            if data.type_arguments.is_some() {
                flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
            }
        }
        NodeData::NewExpression(data) => {
            flags |= TransformFlags::CONTAINS_ES_2020;
            if data.type_arguments.is_some() {
                flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
            }
        }
        NodeData::TaggedTemplateExpression(data) => {
            if data.type_arguments.is_some() {
                flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
            }
        }
        NodeData::ExpressionWithTypeArguments(data) => {
            if data.type_arguments.is_some() {
                flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
            }
        }
        NodeData::PartiallyEmittedExpression(_) => {
            flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
        }
        _ => {}
    }
    flags
}

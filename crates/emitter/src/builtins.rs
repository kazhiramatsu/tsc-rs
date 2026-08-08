use std::collections::{BTreeMap, BTreeSet};

use tsc_syntax::{
    for_each_child, for_each_child_array, try_visit_each_child, Node, NodeArrayId, NodeData,
    NodeDataChildVisitor, NodeId, SyntaxKind,
};
use tsc_types::{CompilerOptions, NodeFlags, ScriptTarget};

use crate::{
    EmitHint, EmitHost, EmitResolver, EmitResolverNode, H2ActivityCanary, H2RuntimeSlice,
    TransformArena, TransformError, TransformFlags, TransformNode, TransformNodeArray,
    TransformRoot, TransformSourceId, TransformationContext, Transformer,
    UnsupportedTransformFeature,
};

const MODULE_COMMON_JS: i32 = 1;
const MODULE_ES_NEXT: i32 = 99;
const MODULE_PRESERVE: i32 = 200;

/// tsc-port: getScriptTransformers @6.0.3
/// tsc-hash: 69bdc65a0c428ad5819419fabd0ecd483bb661350434c5ad0ea0bdec15096fd0
/// tsc-span: _tsc.js:115903-115949
pub fn get_script_transformers<'resolver>(
    options: &CompilerOptions,
    resolver: &'resolver dyn EmitResolver,
) -> Result<Vec<Box<dyn Transformer + 'resolver>>, TransformError> {
    let mut activity = H2ActivityCanary::h2_1a_profile();
    get_script_transformers_with_optional_host(options, resolver, None, &mut activity)
}

pub(crate) fn get_script_transformers_with_activity<'transformers>(
    options: &CompilerOptions,
    resolver: &'transformers dyn EmitResolver,
    host: &'transformers dyn EmitHost,
    activity: &mut H2ActivityCanary,
) -> Result<Vec<Box<dyn Transformer + 'transformers>>, TransformError> {
    get_script_transformers_with_optional_host(options, resolver, Some(host), activity)
}

fn get_script_transformers_with_optional_host<'transformers>(
    options: &CompilerOptions,
    resolver: &'transformers dyn EmitResolver,
    host: Option<&'transformers dyn EmitHost>,
    activity: &mut H2ActivityCanary,
) -> Result<Vec<Box<dyn Transformer + 'transformers>>, TransformError> {
    if options.emit_script_target() != ScriptTarget::ES_NEXT {
        return Err(TransformError::UnsupportedCompilerOption {
            option: "target",
            detail: "the H1 bootstrap transformer list requires ESNext",
        });
    }
    if !matches!(
        options.emit_module_kind(),
        MODULE_PRESERVE | MODULE_ES_NEXT | MODULE_COMMON_JS
    ) {
        return Err(TransformError::UnsupportedCompilerOption {
            option: "module",
            detail: "the current transformer list requires Preserve, ESNext, or the fail-closed CommonJS control",
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
    let transform_typescript = transform_type_script(options, resolver);
    activity.construct_transform_class_fields();
    let transform_class_fields = transform_class_fields(options);
    let module_transformer = if options.emit_module_kind() == MODULE_PRESERVE {
        activity.construct_transform_ecmascript_module();
        transform_ecmascript_module(options)
    } else {
        let host = host.ok_or(TransformError::EmitHostRequiredForImpliedModuleFormat)?;
        activity.observe_runtime_slice(H2RuntimeSlice::H2_1a);
        transform_implied_node_format_dependent_module(options, host, activity)
    };
    Ok(vec![
        transform_typescript,
        transform_class_fields,
        module_transformer,
    ])
}

/// tsc-port: transformTypeScript @6.0.3
/// tsc-hash: 08e4305bdbb440c9e05fb551bd3a1988f4edf67ccc0119b2642f7a3dd2258e79
/// tsc-span: _tsc.js:94036-95849
pub fn transform_type_script<'resolver>(
    options: &CompilerOptions,
    resolver: &'resolver dyn EmitResolver,
) -> Box<dyn Transformer + 'resolver> {
    Box::new(TypeScriptTransformer {
        resolver,
        always_strict: options.always_strict_effective(),
        module_kind: options.emit_module_kind(),
    })
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

/// tsc-port: transformImpliedNodeFormatDependentModule @6.0.3
/// tsc-hash: 1fa1716e96e65c34c4d0972d80814f9551d401359de602394255a5b312ebbe55
/// tsc-span: _tsc.js:113730-113793
///
/// TypeScript constructs both module factories against one context, captures
/// each factory's hooks, and dispatches the transform and hooks per source.
/// Rust represents the same ownership with a composite transformer. The CJS
/// child is intentionally a fail-closed constructor until H2.1b ports
/// `transformModule`; selecting it returns before an emit artifact exists.
fn transform_implied_node_format_dependent_module<'host>(
    options: &CompilerOptions,
    host: &'host dyn EmitHost,
    activity: &mut H2ActivityCanary,
) -> Box<dyn Transformer + 'host> {
    activity.construct_transform_ecmascript_module();
    let esm = transform_ecmascript_module(options);
    let cjs: Box<dyn Transformer> = Box::new(DeferredCommonJsModuleTransformer);
    Box::new(ImpliedNodeFormatDependentModuleTransformer { host, esm, cjs })
}

struct TypeScriptTransformer<'resolver> {
    resolver: &'resolver dyn EmitResolver,
    always_strict: bool,
    module_kind: i32,
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
        let ensure_use_strict = {
            let syntax = context.arena().source(source)?.syntax();
            // Preserve is the frozen H1 transformer profile. H2.1a expands
            // only omitted/ESNext module selection, so its newly reachable
            // strict-prologue behavior must not reinterpret H1 outputs.
            self.module_kind != MODULE_PRESERVE
                && self.always_strict
                && !(syntax.external_module_indicator.is_some() && self.module_kind >= 5)
                && !syntax.file_name.to_ascii_lowercase().ends_with(".json")
        };
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
        let mut transformed = visitor
            .context
            .arena()
            .node_ref(source, transformed)
            .expect("transform visitor only returns nodes from its source");
        if ensure_use_strict {
            transformed = ensure_use_strict_prologue(visitor.context, source, transformed)?;
        }
        visitor
            .context
            .arena_mut()?
            .replace_root(source, transformed)?;
        Ok(TransformRoot::SourceFile(source))
    }
}

/// tsc-port: visitLexicalEnvironment @6.0.3
/// tsc-hash: d322dd3121930fa88830125937b2ff9559dc274e2eedaf345db513dd630e2516
/// tsc-span: _tsc.js:91162-91168
/// tsc-port: createUseStrictPrologue @6.0.3
/// tsc-hash: cb6ce70c3ac5a20c6b97a3d8c59d48eb4c805f4eb6f8f4fa179f0b34cdba4554
/// tsc-span: _tsc.js:24834-24836
/// tsc-port: ensureUseStrict @6.0.3
/// tsc-hash: e55c61fa8bf9b8ddf9d078e77180f82a1c3c65b54c279947119daa994b93d436
/// tsc-span: _tsc.js:24871-24877
/// tsc-port: visitSourceFile @6.0.3
/// tsc-hash: 75c734f6cc8bd6eeda6922ea05aef32c5457225afb81235efa081e9196d27cc3
/// tsc-span: _tsc.js:94390-94403
fn ensure_use_strict_prologue(
    context: &mut TransformationContext,
    source: TransformSourceId,
    root: TransformNode,
) -> Result<TransformNode, TransformError> {
    let (mut data, original_statements) = match context.arena().node(root)?.data.clone() {
        NodeData::SourceFile(data) => {
            let statements = data.statements;
            (data, statements)
        }
        _ => {
            return Err(TransformError::RootKindExpected {
                actual: context.arena().node(root)?.kind,
            });
        }
    };

    if source_file_has_use_strict_prologue(context.arena(), source, original_statements)? {
        return Ok(root);
    }

    let literal = context.factory()?.create_node(
        source,
        NodeData::StringLiteral(tsc_syntax::nodes::StringLiteralData {
            text: "use strict".to_owned(),
            has_extended_unicode_escape: None,
        }),
        TransformFlags::NONE,
    )?;
    let statement = context.factory()?.create_node(
        source,
        NodeData::ExpressionStatement(tsc_syntax::nodes::ExpressionStatementData {
            expression: Some(literal.node()),
        }),
        TransformFlags::NONE,
    )?;

    let mut statements = vec![statement];
    if let Some(original_statements) = original_statements {
        let original = context
            .arena()
            .node_array_ref(source, original_statements)
            .ok_or(TransformError::UnknownNodeArray(TransformNodeArray::new(
                source,
                original_statements,
            )))?;
        statements.extend(
            context
                .arena()
                .node_array(original)?
                .nodes
                .iter()
                .filter_map(|id| context.arena().node_ref(source, *id)),
        );
        data.statements = Some(
            context
                .factory()?
                .update_node_array(original, statements)?
                .array(),
        );
    } else {
        data.statements = Some(
            context
                .factory()?
                .create_node_array(source, statements)?
                .array(),
        );
    }

    let flags = context.arena().transform_flags(root);
    context
        .factory()?
        .update_node(root, NodeData::SourceFile(data), flags)
}

fn source_file_has_use_strict_prologue(
    arena: &TransformArena,
    source: TransformSourceId,
    statements: Option<NodeArrayId>,
) -> Result<bool, TransformError> {
    let Some(statements) = statements.and_then(|id| arena.node_array_ref(source, id)) else {
        return Ok(false);
    };
    for id in &arena.node_array(statements)?.nodes {
        let statement = arena
            .node_ref(source, *id)
            .ok_or_else(|| TransformError::UnknownNode(TransformNode::new(source, *id)))?;
        let NodeData::ExpressionStatement(statement_data) = &arena.node(statement)?.data else {
            break;
        };
        let Some(expression) = statement_data
            .expression
            .and_then(|id| arena.node_ref(source, id))
        else {
            break;
        };
        let NodeData::StringLiteral(literal) = &arena.node(expression)?.data else {
            break;
        };
        if literal.text == "use strict" {
            return Ok(true);
        }
    }
    Ok(false)
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
        if self.rewrite_relative_import_extensions {
            return Err(TransformError::UnsupportedCompilerOption {
                option: "module transform",
                detail: "relative-extension rewriting is outside the active module transform",
            });
        }
        context.enable_emit_notification(SyntaxKind::SourceFile)?;
        context.enable_substitution(SyntaxKind::Identifier)?;
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
        let was_external = context
            .arena()
            .source(source)?
            .syntax()
            .external_module_indicator
            .is_some();
        if self.module_kind != MODULE_PRESERVE {
            let current_root = context.arena().root(source)?;
            if context
                .arena()
                .metadata(current_root)
                .and_then(crate::EmitMetadata::original)
                .is_none()
            {
                let cloned = context.factory()?.clone_node(current_root)?;
                context.arena_mut()?.replace_root(source, cloned)?;
            }
        }
        if !was_external || self.module_kind == MODULE_PRESERVE {
            return Ok(TransformRoot::SourceFile(source));
        }

        let root_node = context.arena().root(source)?;
        let (mut source_data, statement_array) = match context.arena().node(root_node)?.data.clone()
        {
            NodeData::SourceFile(data) => {
                let statements = data.statements;
                (data, statements)
            }
            _ => {
                return Err(TransformError::RootKindExpected {
                    actual: context.arena().node(root_node)?.kind,
                });
            }
        };
        if transformed_source_has_external_module_indicator(
            context.arena(),
            source,
            statement_array,
        )? {
            return Ok(TransformRoot::SourceFile(source));
        }

        let elements = context.factory()?.create_node_array(source, Vec::new())?;
        let clause = context.factory()?.create_node(
            source,
            NodeData::NamedExports(tsc_syntax::nodes::NamedExportsData {
                elements: Some(elements.array()),
            }),
            TransformFlags::NONE,
        )?;
        let export = context.factory()?.create_node(
            source,
            NodeData::ExportDeclaration(tsc_syntax::nodes::ExportDeclarationData {
                modifiers: None,
                is_type_only: false,
                export_clause: Some(clause.node()),
                module_specifier: None,
                attributes: None,
            }),
            TransformFlags::NONE,
        )?;
        let mut statements = if let Some(original) =
            statement_array.and_then(|id| context.arena().node_array_ref(source, id))
        {
            context
                .arena()
                .node_array(original)?
                .nodes
                .iter()
                .filter_map(|id| context.arena().node_ref(source, *id))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        statements.push(export);
        source_data.statements = Some(
            if let Some(original) =
                statement_array.and_then(|id| context.arena().node_array_ref(source, id))
            {
                context
                    .factory()?
                    .update_node_array(original, statements)?
                    .array()
            } else {
                context
                    .factory()?
                    .create_node_array(source, statements)?
                    .array()
            },
        );
        let flags = context.arena().transform_flags(root_node);
        let updated =
            context
                .factory()?
                .update_node(root_node, NodeData::SourceFile(source_data), flags)?;
        context.arena_mut()?.replace_root(source, updated)?;
        Ok(TransformRoot::SourceFile(source))
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

fn transformed_source_has_external_module_indicator(
    arena: &TransformArena,
    source: TransformSourceId,
    statements: Option<NodeArrayId>,
) -> Result<bool, TransformError> {
    let Some(statements) = statements.and_then(|id| arena.node_array_ref(source, id)) else {
        return Ok(false);
    };
    for statement in &arena.node_array(statements)?.nodes {
        let statement = arena
            .node_ref(source, *statement)
            .ok_or_else(|| TransformError::UnknownNode(TransformNode::new(source, *statement)))?;
        if matches!(
            arena.node(statement)?.kind,
            SyntaxKind::ImportDeclaration
                | SyntaxKind::ExportDeclaration
                | SyntaxKind::ExportAssignment
        ) || transformed_statement_has_export_modifier(arena, statement)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn transformed_statement_has_export_modifier(
    arena: &TransformArena,
    statement: TransformNode,
) -> Result<bool, TransformError> {
    let modifiers = match &arena.node(statement)?.data {
        NodeData::FunctionDeclaration(data) => data.modifiers,
        NodeData::ClassDeclaration(data) => data.modifiers,
        NodeData::VariableStatement(data) => data.modifiers,
        _ => None,
    };
    let Some(modifiers) = modifiers.and_then(|id| arena.node_array_ref(statement.source(), id))
    else {
        return Ok(false);
    };
    Ok(arena.node_array(modifiers)?.nodes.iter().any(|modifier| {
        arena
            .node_ref(statement.source(), *modifier)
            .and_then(|modifier| arena.node(modifier).ok())
            .is_some_and(|modifier| modifier.kind == SyntaxKind::ExportKeyword)
    }))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ImpliedModuleBranch {
    Esm,
    CommonJs,
}

struct ImpliedNodeFormatDependentModuleTransformer<'host> {
    host: &'host dyn EmitHost,
    esm: Box<dyn Transformer>,
    cjs: Box<dyn Transformer>,
}

impl ImpliedNodeFormatDependentModuleTransformer<'_> {
    fn branch_for_source(
        &self,
        context: &TransformationContext,
        source: TransformSourceId,
    ) -> Result<ImpliedModuleBranch, TransformError> {
        let program_source = context
            .arena()
            .source(source)?
            .program_source()
            .ok_or(TransformError::MissingProgramSourceForModuleFormat(source))?;
        let format = self
            .host
            .get_emit_module_format_of_file(program_source)
            .ok_or(TransformError::MissingProgramSourceForModuleFormat(source))?;
        Ok(if format >= 5 {
            ImpliedModuleBranch::Esm
        } else {
            ImpliedModuleBranch::CommonJs
        })
    }

    fn branch_mut(&mut self, branch: ImpliedModuleBranch) -> &mut dyn Transformer {
        match branch {
            ImpliedModuleBranch::Esm => self.esm.as_mut(),
            ImpliedModuleBranch::CommonJs => self.cjs.as_mut(),
        }
    }
}

impl Transformer for ImpliedNodeFormatDependentModuleTransformer<'_> {
    fn name(&self) -> &'static str {
        "transformImpliedNodeFormatDependentModule"
    }

    fn initialize(&mut self, context: &mut TransformationContext) -> Result<(), TransformError> {
        // Construction order is observable in upstream hook composition.
        self.esm.initialize(context)?;
        self.cjs.initialize(context)?;
        context.enable_substitution(SyntaxKind::SourceFile)?;
        context.enable_emit_notification(SyntaxKind::SourceFile)?;
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
        let branch = self.branch_for_source(context, source)?;
        self.branch_mut(branch)
            .transform_root(context, TransformRoot::SourceFile(source))
    }

    fn substitute_node(
        &mut self,
        context: &TransformationContext,
        hint: EmitHint,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if context.arena().node(node)?.kind == SyntaxKind::SourceFile {
            return Ok(node);
        }
        let branch = self.branch_for_source(context, node.source())?;
        self.branch_mut(branch).substitute_node(context, hint, node)
    }

    fn before_emit_node(
        &mut self,
        context: &TransformationContext,
        hint: EmitHint,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        let branch = self.branch_for_source(context, node.source())?;
        self.branch_mut(branch)
            .before_emit_node(context, hint, node)
    }

    fn after_emit_node(
        &mut self,
        context: &TransformationContext,
        hint: EmitHint,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        let branch = self.branch_for_source(context, node.source())?;
        self.branch_mut(branch).after_emit_node(context, hint, node)
    }

    fn dispose(&mut self) {
        self.cjs.dispose();
        self.esm.dispose();
    }
}

struct DeferredCommonJsModuleTransformer;

impl Transformer for DeferredCommonJsModuleTransformer {
    fn name(&self) -> &'static str {
        "transformModule"
    }

    fn transform_root(
        &mut self,
        _context: &mut TransformationContext,
        _root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        Err(TransformError::DeferredModuleFormat {
            format: MODULE_COMMON_JS,
            owner_slice: "H2.1b",
        })
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
                NodeData::Block(data) => {
                    let empty = data.statements.is_none_or(|statements| {
                        self.context
                            .arena()
                            .node_array_ref(self.source, statements)
                            .and_then(|array| self.context.arena().node_array(array).ok())
                            .is_none_or(|array| array.nodes.is_empty())
                    });
                    let updated = self.update_generic(original, NodeData::Block(data))?;
                    // Empty function bodies have an observable canonical printer
                    // form, including whether the original body was multi-line.
                    if empty && updated == id {
                        Some(self.context.factory()?.clone_node(original)?.node())
                    } else {
                        Some(updated)
                    }
                }
                NodeData::Parameter(mut data) => {
                    if data.name.is_some_and(|name| {
                        self.context
                            .arena()
                            .node_ref(self.source, name)
                            .and_then(|name| self.context.arena().node(name).ok())
                            .is_some_and(|name| {
                                name.kind == SyntaxKind::ThisKeyword
                                    || matches!(
                                        &name.data,
                                        NodeData::Identifier(data) if data.text == "this"
                                    )
                            })
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
    if !syntax.parse_diagnostics.is_empty() {
        return Err(TransformError::ParseDiagnosticsDeferred {
            count: syntax.parse_diagnostics.len(),
            owner_slice: "H2.9",
        });
    }
    if has_advanced_comment_placement(syntax.text()) {
        return Err(TransformError::AdvancedCommentPlacementDeferred {
            owner_slice: "H2.8a",
        });
    }
    const MAX_TRANSFORM_DEPTH: usize = 256;
    let mut stack = vec![(syntax.root, 1usize)];
    while let Some((id, depth)) = stack.pop() {
        if depth > MAX_TRANSFORM_DEPTH {
            return Err(TransformError::AstDepthDeferred {
                limit: MAX_TRANSFORM_DEPTH,
                owner_slice: "H2.9",
            });
        }
        let node = syntax.arena.node(id);
        if matches!(
            &node.data,
            NodeData::ImportDeclaration(data) if data.attributes.is_some()
        ) || matches!(
            &node.data,
            NodeData::ExportDeclaration(data) if data.attributes.is_some()
        ) {
            return Err(TransformError::ImportAttributesDeferred {
                owner_slice: "H2.1e",
            });
        }
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
            stack.push((child, depth + 1));
            false
        });
    }
    Ok(())
}

fn has_advanced_comment_placement(text: &str) -> bool {
    has_comment_after_ellipsis(text)
        || has_comment_between_private_name_and_in(text)
        || has_commented_optional_chain_type_assertion(text)
}

fn has_comment_after_ellipsis(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut cursor = 0usize;
    while cursor + 2 < bytes.len() {
        if &bytes[cursor..cursor + 3] != b"..." {
            cursor += 1;
            continue;
        }
        let mut next = cursor + 3;
        while next < bytes.len() && bytes[next].is_ascii_whitespace() {
            next += 1;
        }
        if bytes.get(next..next + 2) == Some(b"/*") || bytes.get(next..next + 2) == Some(b"//") {
            return true;
        }
        cursor += 3;
    }
    false
}

fn has_comment_between_private_name_and_in(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if bytes[cursor] != b'#' {
            cursor += 1;
            continue;
        }
        let mut next = cursor + 1;
        let name_start = next;
        while bytes
            .get(next)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
        {
            next += 1;
        }
        if next == name_start {
            cursor += 1;
            continue;
        }
        while bytes
            .get(next)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
        {
            next += 1;
        }
        if bytes.get(next..next + 2) != Some(b"/*") {
            cursor = next;
            continue;
        }
        loop {
            while bytes.get(next).is_some_and(u8::is_ascii_whitespace) {
                next += 1;
            }
            if bytes.get(next..next + 2) != Some(b"/*") {
                break;
            }
            next += 2;
            while next + 1 < bytes.len() && bytes.get(next..next + 2) != Some(b"*/") {
                next += 1;
            }
            next = (next + 2).min(bytes.len());
        }
        if bytes.get(next..next + 2) == Some(b"in")
            && bytes
                .get(next + 2)
                .is_none_or(|byte| !byte.is_ascii_alphanumeric() && !matches!(byte, b'_' | b'$'))
        {
            return true;
        }
        cursor = next.max(cursor + 1);
    }
    false
}

fn has_commented_optional_chain_type_assertion(text: &str) -> bool {
    text.lines().any(|line| {
        line.contains("/*")
            && line.contains("?.")
            && (line.contains(" as ")
                || line
                    .find('<')
                    .zip(line.find('>'))
                    .is_some_and(|(left, right)| left < right))
    })
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

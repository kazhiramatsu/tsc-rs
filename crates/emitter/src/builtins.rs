use std::collections::{BTreeMap, BTreeSet};

use tsc_program::SourceFileId;
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
const MODULE_AMD: i32 = 2;
const MODULE_UMD: i32 = 3;
const MODULE_ES_NEXT: i32 = 99;
const MODULE_PRESERVE: i32 = 200;

const CREATE_BINDING_HELPER_TEXT: &str = r#"var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));"#;
const SET_MODULE_DEFAULT_HELPER_TEXT: &str = r#"var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});"#;
const IMPORT_STAR_HELPER_TEXT: &str = r#"var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();"#;
const IMPORT_DEFAULT_HELPER_TEXT: &str = r#"var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};"#;

/// tsc-port: getScriptTransformers @6.0.3
/// tsc-hash: 69bdc65a0c428ad5819419fabd0ecd483bb661350434c5ad0ea0bdec15096fd0
/// tsc-span: _tsc.js:115903-115949
pub fn get_script_transformers<'resolver>(
    options: &CompilerOptions,
    resolver: &'resolver dyn EmitResolver,
) -> Result<Vec<Box<dyn Transformer + 'resolver>>, TransformError> {
    let mut activity = H2ActivityCanary::h2_1c_profile();
    get_script_transformers_with_optional_host(options, resolver, None, &mut activity)
}

pub(crate) fn get_script_transformers_with_activity<'transformers>(
    options: &CompilerOptions,
    resolver: &'transformers dyn EmitResolver,
    host: &'transformers dyn EmitHost,
    source: SourceFileId,
    activity: &mut H2ActivityCanary,
) -> Result<Vec<Box<dyn Transformer + 'transformers>>, TransformError> {
    get_script_transformers_with_optional_host(options, resolver, Some((host, source)), activity)
}

fn get_script_transformers_with_optional_host<'transformers>(
    options: &CompilerOptions,
    resolver: &'transformers dyn EmitResolver,
    host: Option<(&'transformers dyn EmitHost, SourceFileId)>,
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
        MODULE_PRESERVE | MODULE_ES_NEXT | MODULE_COMMON_JS | MODULE_AMD | MODULE_UMD
    ) {
        return Err(TransformError::UnsupportedCompilerOption {
            option: "module",
            detail: "the current transformer list requires Preserve, ESNext, CommonJS, AMD, or UMD",
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
        let (host, source) = host.ok_or(TransformError::EmitHostRequiredForImpliedModuleFormat)?;
        activity.observe_runtime_slice(H2RuntimeSlice::H2_1a);
        let emit_format = host.get_emit_module_format_of_file(source);
        if emit_format.is_some_and(|format| format < 5) {
            activity.observe_runtime_slice(H2RuntimeSlice::H2_1b);
        }
        if emit_format.is_some_and(|format| matches!(format, MODULE_AMD | MODULE_UMD)) {
            activity.observe_runtime_slice(H2RuntimeSlice::H2_1c);
        }
        transform_implied_node_format_dependent_module(options, resolver, host, activity)
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
/// Rust represents the same ownership with a composite transformer. H2.1b
/// supplies the CommonJS child while later implied formats retain their
/// independently owned fail-closed boundaries.
fn transform_implied_node_format_dependent_module<'dependencies>(
    options: &CompilerOptions,
    resolver: &'dependencies dyn EmitResolver,
    host: &'dependencies dyn EmitHost,
    activity: &mut H2ActivityCanary,
) -> Box<dyn Transformer + 'dependencies> {
    activity.construct_transform_ecmascript_module();
    let esm = transform_ecmascript_module(options);
    let cjs = transform_module(options, resolver);
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
    esm: Box<dyn Transformer + 'host>,
    cjs: Box<dyn Transformer + 'host>,
}

impl<'host> ImpliedNodeFormatDependentModuleTransformer<'host> {
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

    fn branch_mut(&mut self, branch: ImpliedModuleBranch) -> &mut (dyn Transformer + 'host) {
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

/// tsc-port: transformModule @6.0.3
/// tsc-hash: 3d54d8672774bc47f161ad1b4747b2d39a9a04f3da0a7cdab4c8b5ea125ca3eb
/// tsc-span: _tsc.js:110090-112041
fn transform_module<'resolver>(
    options: &CompilerOptions,
    resolver: &'resolver dyn EmitResolver,
) -> Box<dyn Transformer + 'resolver> {
    Box::new(CommonJsModuleTransformer {
        resolver,
        module_kind: options.emit_module_kind(),
        always_strict: options.always_strict_effective(),
        es_module_interop: options.es_module_interop_effective(),
    })
}

struct CommonJsModuleTransformer<'resolver> {
    resolver: &'resolver dyn EmitResolver,
    module_kind: i32,
    always_strict: bool,
    es_module_interop: bool,
}

impl Transformer for CommonJsModuleTransformer<'_> {
    fn name(&self) -> &'static str {
        "transformModule"
    }

    fn initialize(&mut self, context: &mut TransformationContext) -> Result<(), TransformError> {
        for kind in [
            SyntaxKind::CallExpression,
            SyntaxKind::TaggedTemplateExpression,
            SyntaxKind::Identifier,
            SyntaxKind::BinaryExpression,
            SyntaxKind::ShorthandPropertyAssignment,
        ] {
            context.enable_substitution(kind)?;
        }
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
        let is_external = context
            .arena()
            .source(source)?
            .syntax()
            .external_module_indicator
            .is_some();
        let current_root = context.arena().root(source)?;
        let has_dynamic_import = source_contains_dynamic_import(context.arena(), current_root)?;
        if !is_external && !has_dynamic_import {
            return Ok(TransformRoot::SourceFile(source));
        }
        if matches!(self.module_kind, MODULE_AMD | MODULE_UMD) || self.always_strict || is_external
        {
            let strict_root = context.arena().root(source)?;
            let strict_root = ensure_use_strict_prologue(context, source, strict_root)?;
            context.arena_mut()?.replace_root(source, strict_root)?;
        }

        let current_root = context.arena().root(source)?;
        let info = CommonJsModuleInfo::collect(context.arena(), source, current_root)?;
        let referenced_declarations = collect_referenced_value_declarations(
            context.arena(),
            source,
            current_root,
            self.resolver,
            &info.exported_variable_declarations,
        )?;
        let mut visitor = CommonJsVisitor::new(
            context,
            source,
            self.resolver,
            CommonJsVisitorOptions {
                module_kind: self.module_kind,
                es_module_interop: self.es_module_interop,
                has_dynamic_import,
            },
            info,
            referenced_declarations,
        );
        let mut updated = visitor.transform_source_file(current_root)?;
        if matches!(self.module_kind, MODULE_AMD | MODULE_UMD) {
            updated = visitor.wrap_asynchronous_module(updated)?;
        }
        visitor.context.arena_mut()?.replace_root(source, updated)?;
        Ok(TransformRoot::SourceFile(source))
    }

    fn substitute_node(
        &mut self,
        _context: &TransformationContext,
        _hint: EmitHint,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        // This Rust ownership adaptation performs substitutions while the AST
        // is mutable. The hook remains installed because the implied-format
        // composite must preserve transformModule's upstream hook surface.
        Ok(node)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ImportHelperKind {
    None,
    Star,
    Default,
}

#[derive(Clone, Debug)]
struct ImportPlan {
    generated_name: Box<str>,
    module_specifier: Box<str>,
    has_import_clause: bool,
    helper: ImportHelperKind,
}

#[derive(Clone, Debug)]
struct ImportBinding {
    generated_name: Box<str>,
    property: Option<Box<str>>,
}

#[derive(Debug)]
struct CommonJsModuleInfo {
    is_external: bool,
    imports: BTreeMap<NodeId, ImportPlan>,
    external_imports: Vec<NodeId>,
    import_bindings: BTreeMap<NodeId, ImportBinding>,
    exports_by_local: BTreeMap<Box<str>, Vec<Box<str>>>,
    exported_names: Vec<Box<str>>,
    hoisted_function_exports: Vec<(Box<str>, Box<str>)>,
    exported_variable_declarations: BTreeSet<NodeId>,
}

impl CommonJsModuleInfo {
    fn collect(
        arena: &TransformArena,
        source: TransformSourceId,
        root: TransformNode,
    ) -> Result<Self, TransformError> {
        let is_external = arena
            .source(source)?
            .syntax()
            .external_module_indicator
            .is_some();
        let statements = source_file_statement_nodes(arena, source, root)?;
        let mut info = Self {
            is_external,
            imports: BTreeMap::new(),
            external_imports: Vec::new(),
            import_bindings: BTreeMap::new(),
            exports_by_local: BTreeMap::new(),
            exported_names: Vec::new(),
            hoisted_function_exports: Vec::new(),
            exported_variable_declarations: BTreeSet::new(),
        };
        let mut generated_names = BTreeMap::<String, usize>::new();

        for statement in &statements {
            let record = arena.node(*statement)?;
            match &record.data {
                NodeData::ImportDeclaration(data) => {
                    let Some(module_specifier) = data
                        .module_specifier
                        .and_then(|id| arena.node_ref(source, id))
                    else {
                        continue;
                    };
                    let module_text = string_literal_text(arena, module_specifier)?;
                    let base = generated_module_name(module_text);
                    let ordinal = generated_names.entry(base.clone()).or_insert(0);
                    *ordinal += 1;
                    let mut generated_name = format!("{base}_{}", *ordinal).into_boxed_str();
                    let mut helper = ImportHelperKind::None;
                    if let Some(clause) =
                        data.import_clause.and_then(|id| arena.node_ref(source, id))
                    {
                        if let NodeData::ImportClause(clause_data) = &arena.node(clause)?.data {
                            let has_default = clause_data.name.is_some();
                            let has_namespace = clause_data
                                .named_bindings
                                .and_then(|id| arena.node_ref(source, id))
                                .is_some_and(|node| {
                                    arena
                                        .node(node)
                                        .is_ok_and(|node| node.kind == SyntaxKind::NamespaceImport)
                                });
                            helper = if has_namespace
                                || has_default && clause_data.named_bindings.is_some()
                            {
                                ImportHelperKind::Star
                            } else if has_default {
                                ImportHelperKind::Default
                            } else {
                                ImportHelperKind::None
                            };
                            if let Some(_name) = clause_data.name {
                                info.import_bindings.insert(
                                    arena.get_original_node(clause).node(),
                                    ImportBinding {
                                        generated_name: generated_name.clone(),
                                        property: Some("default".into()),
                                    },
                                );
                            }
                            if let Some(bindings) = clause_data
                                .named_bindings
                                .and_then(|id| arena.node_ref(source, id))
                            {
                                match &arena.node(bindings)?.data {
                                    NodeData::NamespaceImport(namespace) => {
                                        if let Some(namespace_name) = namespace
                                            .name
                                            .and_then(|id| arena.node_ref(source, id))
                                            .and_then(|name| {
                                                identifier_or_literal_text(arena, name).ok()
                                            })
                                        {
                                            generated_name = namespace_name.into_boxed_str();
                                            if clause_data.name.is_some() {
                                                info.import_bindings.insert(
                                                    arena.get_original_node(clause).node(),
                                                    ImportBinding {
                                                        generated_name: generated_name.clone(),
                                                        property: Some("default".into()),
                                                    },
                                                );
                                            }
                                        }
                                        info.import_bindings.insert(
                                            arena.get_original_node(bindings).node(),
                                            ImportBinding {
                                                generated_name: generated_name.clone(),
                                                property: None,
                                            },
                                        );
                                    }
                                    NodeData::NamedImports(named) => {
                                        for specifier in
                                            node_array_nodes(arena, source, named.elements)?
                                        {
                                            let NodeData::ImportSpecifier(specifier_data) =
                                                &arena.node(specifier)?.data
                                            else {
                                                continue;
                                            };
                                            let property = specifier_data
                                                .property_name
                                                .or(specifier_data.name)
                                                .and_then(|id| arena.node_ref(source, id))
                                                .and_then(|name| {
                                                    identifier_or_literal_text(arena, name).ok()
                                                })
                                                .unwrap_or_default();
                                            info.import_bindings.insert(
                                                arena.get_original_node(specifier).node(),
                                                ImportBinding {
                                                    generated_name: generated_name.clone(),
                                                    property: Some(property.into_boxed_str()),
                                                },
                                            );
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    info.imports.insert(
                        arena.get_original_node(*statement).node(),
                        ImportPlan {
                            generated_name,
                            module_specifier: module_text.to_owned().into_boxed_str(),
                            has_import_clause: data.import_clause.is_some(),
                            helper,
                        },
                    );
                    info.external_imports
                        .push(arena.get_original_node(*statement).node());
                }
                NodeData::FunctionDeclaration(data)
                    if has_modifier(arena, source, data.modifiers, SyntaxKind::ExportKeyword)? =>
                {
                    if let Some(local) = data
                        .name
                        .and_then(|id| arena.node_ref(source, id))
                        .and_then(|name| identifier_or_literal_text(arena, name).ok())
                    {
                        let export = if has_modifier(
                            arena,
                            source,
                            data.modifiers,
                            SyntaxKind::DefaultKeyword,
                        )? {
                            "default".to_owned()
                        } else {
                            local.clone()
                        };
                        info.add_export(&local, &export);
                        info.hoisted_function_exports
                            .push((export.into_boxed_str(), local.into_boxed_str()));
                    }
                }
                NodeData::ClassDeclaration(data)
                    if has_modifier(arena, source, data.modifiers, SyntaxKind::ExportKeyword)? =>
                {
                    if let Some(local) = data
                        .name
                        .and_then(|id| arena.node_ref(source, id))
                        .and_then(|name| identifier_or_literal_text(arena, name).ok())
                    {
                        let export = if has_modifier(
                            arena,
                            source,
                            data.modifiers,
                            SyntaxKind::DefaultKeyword,
                        )? {
                            "default".to_owned()
                        } else {
                            local.clone()
                        };
                        info.add_export(&local, &export);
                    }
                }
                NodeData::VariableStatement(data)
                    if has_modifier(arena, source, data.modifiers, SyntaxKind::ExportKeyword)? =>
                {
                    for declaration in variable_declarations(arena, source, data.declaration_list)?
                    {
                        if let NodeData::VariableDeclaration(variable) =
                            &arena.node(declaration)?.data
                        {
                            if let Some(local) = variable
                                .name
                                .and_then(|id| arena.node_ref(source, id))
                                .and_then(|name| identifier_or_literal_text(arena, name).ok())
                            {
                                info.add_export(&local, &local);
                                info.exported_variable_declarations
                                    .insert(arena.get_original_node(declaration).node());
                            }
                        }
                    }
                }
                NodeData::ExportDeclaration(data) if data.module_specifier.is_none() => {
                    let Some(clause) = data.export_clause.and_then(|id| arena.node_ref(source, id))
                    else {
                        continue;
                    };
                    let NodeData::NamedExports(named) = &arena.node(clause)?.data else {
                        continue;
                    };
                    for specifier in node_array_nodes(arena, source, named.elements)? {
                        let NodeData::ExportSpecifier(specifier) = &arena.node(specifier)?.data
                        else {
                            continue;
                        };
                        let Some(local) = specifier
                            .property_name
                            .or(specifier.name)
                            .and_then(|id| arena.node_ref(source, id))
                            .and_then(|name| identifier_or_literal_text(arena, name).ok())
                        else {
                            continue;
                        };
                        let Some(export) = specifier
                            .name
                            .and_then(|id| arena.node_ref(source, id))
                            .and_then(|name| identifier_or_literal_text(arena, name).ok())
                        else {
                            continue;
                        };
                        info.add_export(&local, &export);
                    }
                }
                _ => {}
            }
        }

        for statement in &statements {
            collect_exported_variable_declarations(arena, source, *statement, &mut info)?;
        }
        Ok(info)
    }

    fn add_export(&mut self, local: &str, export: &str) {
        let exports = self.exports_by_local.entry(local.into()).or_default();
        if !exports.iter().any(|existing| existing.as_ref() == export) {
            exports.push(export.into());
        }
        if !self
            .exported_names
            .iter()
            .any(|existing| existing.as_ref() == export)
        {
            self.exported_names.push(export.into());
        }
    }
}

fn source_file_statement_nodes(
    arena: &TransformArena,
    source: TransformSourceId,
    root: TransformNode,
) -> Result<Vec<TransformNode>, TransformError> {
    let NodeData::SourceFile(data) = &arena.node(root)?.data else {
        return Err(TransformError::RootKindExpected {
            actual: arena.node(root)?.kind,
        });
    };
    node_array_nodes(arena, source, data.statements)
}

fn node_array_nodes(
    arena: &TransformArena,
    source: TransformSourceId,
    array: Option<NodeArrayId>,
) -> Result<Vec<TransformNode>, TransformError> {
    let Some(array) = array.and_then(|id| arena.node_array_ref(source, id)) else {
        return Ok(Vec::new());
    };
    arena
        .node_array(array)?
        .nodes
        .iter()
        .map(|id| {
            arena
                .node_ref(source, *id)
                .ok_or_else(|| TransformError::UnknownNode(TransformNode::new(source, *id)))
        })
        .collect()
}

fn source_contains_dynamic_import(
    arena: &TransformArena,
    root: TransformNode,
) -> Result<bool, TransformError> {
    let mut stack = vec![root.node()];
    while let Some(id) = stack.pop() {
        let node = arena
            .node_ref(root.source(), id)
            .ok_or_else(|| TransformError::UnknownNode(TransformNode::new(root.source(), id)))?;
        let record = arena.node(node)?;
        if let NodeData::CallExpression(data) = &record.data {
            if data
                .expression
                .and_then(|id| arena.node_ref(root.source(), id))
                .is_some_and(|expression| {
                    arena
                        .node(expression)
                        .is_ok_and(|expression| expression.kind == SyntaxKind::ImportKeyword)
                })
            {
                return Ok(true);
            }
        }
        for_each_child(
            &arena.source(root.source())?.syntax().arena,
            record,
            |child| {
                stack.push(child);
                false
            },
        );
    }
    Ok(false)
}

fn string_literal_text(
    arena: &TransformArena,
    node: TransformNode,
) -> Result<&str, TransformError> {
    match &arena.node(node)?.data {
        NodeData::StringLiteral(data) => Ok(&data.text),
        _ => Err(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::ImportDeclaration,
            field: "string module_specifier",
        }),
    }
}

fn identifier_or_literal_text(
    arena: &TransformArena,
    node: TransformNode,
) -> Result<String, TransformError> {
    match &arena.node(node)?.data {
        NodeData::Identifier(data) => Ok(data.text.clone()),
        NodeData::StringLiteral(data) => Ok(data.text.clone()),
        _ => Err(TransformError::RequiredChildRemoved {
            parent: arena.node(node)?.kind,
            field: "identifier or literal text",
        }),
    }
}

fn generated_module_name(module_specifier: &str) -> String {
    let segment = module_specifier
        .rsplit('/')
        .next()
        .unwrap_or(module_specifier)
        .split('.')
        .next()
        .unwrap_or("module");
    let mut generated = segment
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '$') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if generated.is_empty() || generated.as_bytes()[0].is_ascii_digit() {
        generated.insert_str(0, "module");
    }
    generated
}

fn has_modifier(
    arena: &TransformArena,
    source: TransformSourceId,
    modifiers: Option<NodeArrayId>,
    expected: SyntaxKind,
) -> Result<bool, TransformError> {
    Ok(node_array_nodes(arena, source, modifiers)?
        .iter()
        .any(|modifier| {
            arena
                .node(*modifier)
                .is_ok_and(|node| node.kind == expected)
        }))
}

fn variable_declarations(
    arena: &TransformArena,
    source: TransformSourceId,
    declaration_list: Option<NodeId>,
) -> Result<Vec<TransformNode>, TransformError> {
    let Some(list) = declaration_list.and_then(|id| arena.node_ref(source, id)) else {
        return Ok(Vec::new());
    };
    let NodeData::VariableDeclarationList(data) = &arena.node(list)?.data else {
        return Ok(Vec::new());
    };
    variable_declarations_from_array(arena, source, data.declarations)
}

fn variable_declarations_from_array(
    arena: &TransformArena,
    source: TransformSourceId,
    declarations: Option<NodeArrayId>,
) -> Result<Vec<TransformNode>, TransformError> {
    node_array_nodes(arena, source, declarations)
}

fn collect_exported_variable_declarations(
    arena: &TransformArena,
    source: TransformSourceId,
    statement: TransformNode,
    info: &mut CommonJsModuleInfo,
) -> Result<(), TransformError> {
    let mut stack = vec![statement.node()];
    while let Some(id) = stack.pop() {
        let node = arena
            .node_ref(source, id)
            .ok_or_else(|| TransformError::UnknownNode(TransformNode::new(source, id)))?;
        let record = arena.node(node)?;
        if matches!(
            record.kind,
            SyntaxKind::FunctionDeclaration
                | SyntaxKind::FunctionExpression
                | SyntaxKind::ArrowFunction
                | SyntaxKind::ClassDeclaration
                | SyntaxKind::ClassExpression
        ) && node != statement
        {
            continue;
        }
        if let NodeData::VariableDeclaration(data) = &record.data {
            if let Some(local) = data
                .name
                .and_then(|id| arena.node_ref(source, id))
                .and_then(|name| identifier_or_literal_text(arena, name).ok())
            {
                if info.exports_by_local.contains_key(local.as_str()) {
                    info.exported_variable_declarations
                        .insert(arena.get_original_node(node).node());
                }
            }
        }
        for_each_child(&arena.source(source)?.syntax().arena, record, |child| {
            stack.push(child);
            false
        });
    }
    Ok(())
}

fn collect_referenced_value_declarations(
    arena: &TransformArena,
    source: TransformSourceId,
    root: TransformNode,
    resolver: &dyn EmitResolver,
    candidates: &BTreeSet<NodeId>,
) -> Result<BTreeSet<NodeId>, TransformError> {
    if candidates.is_empty() {
        return Ok(BTreeSet::new());
    }
    let program_source = arena
        .source(source)?
        .program_source()
        .ok_or(TransformError::MissingProgramSource(root))?;
    let mut referenced = BTreeSet::new();
    let mut stack = vec![root.node()];
    while let Some(id) = stack.pop() {
        let node = arena
            .node_ref(source, id)
            .ok_or_else(|| TransformError::UnknownNode(TransformNode::new(source, id)))?;
        let record = arena.node(node)?;
        if record.kind == SyntaxKind::Identifier {
            let original = arena.get_original_node(node);
            if !is_declaration_name_node(arena, original)? {
                if let Some(declaration) = resolver.get_referenced_value_declaration(
                    EmitResolverNode::new(program_source, original.node()),
                )? {
                    if candidates.contains(&declaration.node()) {
                        referenced.insert(declaration.node());
                    }
                }
            }
        }
        for_each_child(&arena.source(source)?.syntax().arena, record, |child| {
            stack.push(child);
            false
        });
    }
    Ok(referenced)
}

fn is_declaration_name_node(
    arena: &TransformArena,
    node: TransformNode,
) -> Result<bool, TransformError> {
    let Some(parent) = arena
        .node(node)?
        .parent
        .and_then(|id| arena.node_ref(node.source(), id))
    else {
        return Ok(false);
    };
    Ok(match &arena.node(parent)?.data {
        NodeData::VariableDeclaration(data) => data.name == Some(node.node()),
        NodeData::FunctionDeclaration(data) => data.name == Some(node.node()),
        NodeData::ClassDeclaration(data) => data.name == Some(node.node()),
        NodeData::Parameter(data) => data.name == Some(node.node()),
        NodeData::BindingElement(data) => data.name == Some(node.node()),
        NodeData::ImportClause(data) => data.name == Some(node.node()),
        NodeData::ImportSpecifier(data) => data.name == Some(node.node()),
        NodeData::NamespaceImport(data) => data.name == Some(node.node()),
        _ => false,
    })
}

fn is_prologue_statement(
    arena: &TransformArena,
    statement: TransformNode,
) -> Result<bool, TransformError> {
    let NodeData::ExpressionStatement(data) = &arena.node(statement)?.data else {
        return Ok(false);
    };
    Ok(data
        .expression
        .and_then(|id| arena.node_ref(statement.source(), id))
        .is_some_and(|expression| {
            arena
                .node(expression)
                .is_ok_and(|expression| matches!(expression.data, NodeData::StringLiteral(_)))
        }))
}

fn variable_has_initializer(
    arena: &TransformArena,
    declaration: TransformNode,
) -> Result<bool, TransformError> {
    Ok(matches!(
        &arena.node(declaration)?.data,
        NodeData::VariableDeclaration(data) if data.initializer.is_some()
    ))
}

fn variable_list_has_exported_declaration(
    arena: &TransformArena,
    source: TransformSourceId,
    list: TransformNode,
    exports: &BTreeMap<Box<str>, Vec<Box<str>>>,
) -> Result<bool, TransformError> {
    let NodeData::VariableDeclarationList(data) = &arena.node(list)?.data else {
        return Ok(false);
    };
    for declaration in variable_declarations_from_array(arena, source, data.declarations)? {
        if let NodeData::VariableDeclaration(data) = &arena.node(declaration)?.data {
            if data
                .name
                .and_then(|id| arena.node_ref(source, id))
                .and_then(|name| identifier_or_literal_text(arena, name).ok())
                .is_some_and(|name| exports.contains_key(name.as_str()))
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn is_identifier_export_name(name: &str) -> bool {
    let mut characters = name.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    (first == '_' || first == '$' || first.is_ascii_alphabetic())
        && characters.all(|character| {
            character == '_' || character == '$' || character.is_ascii_alphanumeric()
        })
}

#[derive(Clone, Copy)]
struct CommonJsVisitorOptions {
    module_kind: i32,
    es_module_interop: bool,
    has_dynamic_import: bool,
}

struct AsynchronousDependencies {
    aliased: Vec<String>,
    unaliased: Vec<String>,
    parameters: Vec<String>,
}

struct CommonJsVisitor<'context, 'resolver> {
    context: &'context mut TransformationContext,
    source: TransformSourceId,
    resolver: &'resolver dyn EmitResolver,
    module_kind: i32,
    es_module_interop: bool,
    has_dynamic_import: bool,
    info: CommonJsModuleInfo,
    referenced_declarations: BTreeSet<NodeId>,
    nodes: BTreeMap<NodeId, NodeId>,
    arrays: BTreeMap<NodeArrayId, NodeArrayId>,
    dynamic_import_ordinal: usize,
}

impl<'context, 'resolver> CommonJsVisitor<'context, 'resolver> {
    fn new(
        context: &'context mut TransformationContext,
        source: TransformSourceId,
        resolver: &'resolver dyn EmitResolver,
        options: CommonJsVisitorOptions,
        info: CommonJsModuleInfo,
        referenced_declarations: BTreeSet<NodeId>,
    ) -> Self {
        Self {
            context,
            source,
            resolver,
            module_kind: options.module_kind,
            es_module_interop: options.es_module_interop,
            has_dynamic_import: options.has_dynamic_import,
            info,
            referenced_declarations,
            nodes: BTreeMap::new(),
            arrays: BTreeMap::new(),
            dynamic_import_ordinal: 0,
        }
    }

    fn transform_source_file(
        &mut self,
        root: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let (mut source_data, original_array) = match self.context.arena().node(root)?.data.clone()
        {
            NodeData::SourceFile(data) => {
                let statements = data.statements;
                (data, statements)
            }
            _ => {
                return Err(TransformError::RootKindExpected {
                    actual: self.context.arena().node(root)?.kind,
                })
            }
        };
        let input = node_array_nodes(self.context.arena(), self.source, original_array)?;
        let mut output = Vec::new();
        let mut offset = 0usize;
        while offset < input.len() && is_prologue_statement(self.context.arena(), input[offset])? {
            output.push(self.visit(input[offset].node())?);
            offset += 1;
        }

        if self.module_kind == MODULE_UMD && self.has_dynamic_import {
            output.push(self.create_sync_require_declaration()?);
        }
        if self.info.is_external {
            output.push(self.create_es_module_marker()?);
        }
        let hoisted_function_exports = self.info.hoisted_function_exports.clone();
        let function_export_names = hoisted_function_exports
            .iter()
            .map(|(export, _)| export.clone())
            .collect::<BTreeSet<_>>();
        let preinitialized = self
            .info
            .exported_names
            .iter()
            .filter(|name| !function_export_names.contains(*name))
            .cloned()
            .collect::<Vec<_>>();
        for chunk in preinitialized.chunks(50) {
            let mut expression = self.create_void_zero()?;
            for name in chunk {
                let target = self.create_export_access(name)?;
                expression = self.create_assignment(target, expression)?;
            }
            output.push(self.create_expression_statement(expression)?);
        }
        for (export, local) in hoisted_function_exports {
            let target = self.create_export_access(&export)?;
            let value = self.create_identifier(&local)?;
            let assignment = self.create_assignment(target, value)?;
            output.push(self.create_expression_statement(assignment)?);
        }

        if self.module_kind == MODULE_AMD {
            output.extend(self.create_amd_import_initializers()?);
        }

        for statement in input.into_iter().skip(offset) {
            output.extend(self.visit_top_level_statement(statement)?);
        }
        let statements = if let Some(original_array) =
            original_array.and_then(|array| self.context.arena().node_array_ref(self.source, array))
        {
            self.context
                .factory()?
                .update_node_array(original_array, output)?
        } else {
            self.context
                .factory()?
                .create_node_array(self.source, output)?
        };
        source_data.statements = Some(statements.array());
        let flags = self.context.arena().transform_flags(root);
        self.context
            .factory()?
            .update_node(root, NodeData::SourceFile(source_data), flags)
    }

    fn create_amd_import_initializers(&mut self) -> Result<Vec<TransformNode>, TransformError> {
        let plans = self
            .info
            .external_imports
            .iter()
            .filter_map(|key| self.info.imports.get(key).cloned())
            .collect::<Vec<_>>();
        let mut statements = Vec::new();
        for plan in plans {
            if !plan.has_import_clause || !self.es_module_interop {
                continue;
            }
            let helper_name = match plan.helper {
                ImportHelperKind::Star => "__importStar",
                ImportHelperKind::Default => "__importDefault",
                ImportHelperKind::None => continue,
            };
            match plan.helper {
                ImportHelperKind::Star => self.request_import_star_helper()?,
                ImportHelperKind::Default => self.request_import_default_helper()?,
                ImportHelperKind::None => {}
            }
            let target = self.create_identifier(&plan.generated_name)?;
            let helper = self.create_identifier(helper_name)?;
            let argument = self.create_identifier(&plan.generated_name)?;
            let value = self.create_call(helper, vec![argument])?;
            let assignment = self.create_assignment(target, value)?;
            statements.push(self.create_expression_statement(assignment)?);
        }
        Ok(statements)
    }

    fn create_sync_require_declaration(&mut self) -> Result<TransformNode, TransformError> {
        let module = self.create_identifier("module")?;
        let module_type = self.create_typeof(module)?;
        let object = self.create_string_literal("object")?;
        let module_is_object =
            self.create_binary(module_type, SyntaxKind::EqualsEqualsEqualsToken, object)?;

        let module = self.create_identifier("module")?;
        let module_exports = self.create_property_access(module, "exports")?;
        let exports_type = self.create_typeof(module_exports)?;
        let object = self.create_string_literal("object")?;
        let exports_is_object =
            self.create_binary(exports_type, SyntaxKind::EqualsEqualsEqualsToken, object)?;
        let condition = self.create_binary(
            module_is_object,
            SyntaxKind::AmpersandAmpersandToken,
            exports_is_object,
        )?;
        let declaration = self.create_variable_declaration("__syncRequire", condition)?;
        self.create_variable_statement(vec![declaration], NodeFlags::NONE)
    }

    fn asynchronous_dependencies(&self) -> Result<AsynchronousDependencies, TransformError> {
        let source = self.context.arena().source(self.source)?.syntax();
        let amd_dependencies = source.amd_dependencies.clone();
        let import_plans = self
            .info
            .external_imports
            .iter()
            .filter_map(|key| self.info.imports.get(key).cloned())
            .collect::<Vec<_>>();
        let mut aliased = Vec::new();
        let mut unaliased = Vec::new();
        let mut parameters = Vec::new();
        for dependency in amd_dependencies {
            let path = dependency.path;
            if let Some(name) = dependency.name {
                aliased.push(path);
                parameters.push(name);
            } else {
                unaliased.push(path);
            }
        }
        for plan in import_plans {
            if self.module_kind == MODULE_AMD && plan.has_import_clause {
                aliased.push(plan.module_specifier.to_string());
                parameters.push(plan.generated_name.to_string());
            } else {
                unaliased.push(plan.module_specifier.to_string());
            }
        }
        Ok(AsynchronousDependencies {
            aliased,
            unaliased,
            parameters,
        })
    }

    fn wrap_asynchronous_module(
        &mut self,
        root: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let (mut source_data, original_array) = match self.context.arena().node(root)?.data.clone()
        {
            NodeData::SourceFile(data) => {
                let statements = data.statements;
                (data, statements)
            }
            _ => {
                return Err(TransformError::RootKindExpected {
                    actual: self.context.arena().node(root)?.kind,
                })
            }
        };
        let body_statements =
            node_array_nodes(self.context.arena(), self.source, source_data.statements)?;
        let body = self.create_block_from_array(body_statements, original_array, true)?;
        let asynchronous_dependencies = self.asynchronous_dependencies()?;

        let mut body_parameters = vec![
            self.create_parameter("require")?,
            self.create_parameter("exports")?,
        ];
        for parameter in asynchronous_dependencies.parameters {
            body_parameters.push(self.create_parameter(&parameter)?);
        }
        let body_function = self.create_function_expression(body_parameters, body)?;
        let mut dependency_elements = vec![
            self.create_string_literal("require")?,
            self.create_string_literal("exports")?,
        ];
        for dependency in asynchronous_dependencies
            .aliased
            .into_iter()
            .chain(asynchronous_dependencies.unaliased)
        {
            dependency_elements.push(self.create_string_literal(&dependency)?);
        }
        let dependencies = self.create_array_literal(dependency_elements)?;
        let module_name = self
            .context
            .arena()
            .source(self.source)?
            .syntax()
            .module_name
            .clone();

        let wrapper = if self.module_kind == MODULE_AMD {
            let define = self.create_identifier("define")?;
            let mut arguments = Vec::new();
            if let Some(module_name) = module_name {
                arguments.push(self.create_string_literal(&module_name)?);
            }
            arguments.push(dependencies);
            arguments.push(body_function);
            let call = self.create_call(define, arguments)?;
            self.create_expression_statement(call)?
        } else {
            self.create_umd_wrapper(module_name.as_deref(), dependencies, body_function)?
        };

        let statements = if let Some(original_array) =
            original_array.and_then(|array| self.context.arena().node_array_ref(self.source, array))
        {
            self.context
                .factory()?
                .update_node_array(original_array, vec![wrapper])?
        } else {
            self.context
                .factory()?
                .create_node_array(self.source, vec![wrapper])?
        };
        source_data.statements = Some(statements.array());
        let flags = self.context.arena().transform_flags(root);
        self.context
            .factory()?
            .update_node(root, NodeData::SourceFile(source_data), flags)
    }

    fn create_umd_wrapper(
        &mut self,
        module_name: Option<&str>,
        dependencies: TransformNode,
        body_function: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let module = self.create_identifier("module")?;
        let module_type = self.create_typeof(module)?;
        let object = self.create_string_literal("object")?;
        let module_is_object =
            self.create_binary(module_type, SyntaxKind::EqualsEqualsEqualsToken, object)?;
        let module = self.create_identifier("module")?;
        let module_exports = self.create_property_access(module, "exports")?;
        let exports_type = self.create_typeof(module_exports)?;
        let object = self.create_string_literal("object")?;
        let exports_is_object =
            self.create_binary(exports_type, SyntaxKind::EqualsEqualsEqualsToken, object)?;
        let common_js_condition = self.create_binary(
            module_is_object,
            SyntaxKind::AmpersandAmpersandToken,
            exports_is_object,
        )?;

        let factory = self.create_identifier("factory")?;
        let require = self.create_identifier("require")?;
        let exports = self.create_identifier("exports")?;
        let factory_call = self.create_call(factory, vec![require, exports])?;
        let value_declaration = self.create_variable_declaration("v", factory_call)?;
        let value_statement =
            self.create_variable_statement(vec![value_declaration], NodeFlags::NONE)?;
        let value = self.create_identifier("v")?;
        let undefined = self.create_identifier("undefined")?;
        let value_present =
            self.create_binary(value, SyntaxKind::ExclamationEqualsEqualsToken, undefined)?;
        let module = self.create_identifier("module")?;
        let module_exports = self.create_property_access(module, "exports")?;
        let value = self.create_identifier("v")?;
        let assignment = self.create_assignment(module_exports, value)?;
        let assignment = self.create_expression_statement(assignment)?;
        self.context
            .arena_mut()?
            .metadata_mut(assignment)
            .add_flags(crate::EmitFlags::SINGLE_LINE);
        let value_if = self.create_if_statement(value_present, assignment, None)?;
        self.context
            .arena_mut()?
            .metadata_mut(value_if)
            .add_flags(crate::EmitFlags::SINGLE_LINE);
        let common_js_block = self.create_block(vec![value_statement, value_if], true)?;

        let define = self.create_identifier("define")?;
        let define_type = self.create_typeof(define)?;
        let function = self.create_string_literal("function")?;
        let define_is_function =
            self.create_binary(define_type, SyntaxKind::EqualsEqualsEqualsToken, function)?;
        let define = self.create_identifier("define")?;
        let define_amd = self.create_property_access(define, "amd")?;
        let amd_condition = self.create_binary(
            define_is_function,
            SyntaxKind::AmpersandAmpersandToken,
            define_amd,
        )?;
        let define = self.create_identifier("define")?;
        let mut define_arguments = Vec::new();
        if let Some(module_name) = module_name {
            define_arguments.push(self.create_string_literal(module_name)?);
        }
        define_arguments.push(dependencies);
        define_arguments.push(self.create_identifier("factory")?);
        let define_call = self.create_call(define, define_arguments)?;
        let define_statement = self.create_expression_statement(define_call)?;
        let amd_block = self.create_block(vec![define_statement], true)?;
        let amd_if = self.create_if_statement(amd_condition, amd_block, None)?;
        let outer_if =
            self.create_if_statement(common_js_condition, common_js_block, Some(amd_if))?;
        let header_body = self.create_block(vec![outer_if], true)?;
        let factory_parameter = self.create_parameter("factory")?;
        let header = self.create_function_expression(vec![factory_parameter], header_body)?;
        let header = self.create_parenthesized(header)?;
        let call = self.create_call(header, vec![body_function])?;
        self.create_expression_statement(call)
    }

    fn visit_top_level_statement(
        &mut self,
        statement: TransformNode,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let record = self.context.arena().node(statement)?.clone();
        match record.data {
            NodeData::ImportDeclaration(data) => self.transform_import(statement, data),
            NodeData::ExportDeclaration(data) => self.transform_export_declaration(statement, data),
            NodeData::ExportAssignment(data) => {
                if data.is_export_equals == Some(true) {
                    return Err(TransformError::DeferredModuleFormat {
                        format: self.module_kind,
                        owner_slice: "H2.2d",
                    });
                }
                let expression = data
                    .expression
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ExportAssignment,
                        field: "expression",
                    })?;
                let value = self.visit(expression)?;
                let target = self.create_export_access("default")?;
                let assignment = self.create_assignment(target, value)?;
                let emitted = self.create_expression_statement(assignment)?;
                self.set_original_and_range(emitted, statement)?;
                Ok(vec![emitted])
            }
            NodeData::FunctionDeclaration(mut data) => {
                data.modifiers = self.remove_export_modifiers(data.modifiers)?;
                let function =
                    self.update_generic(statement, NodeData::FunctionDeclaration(data))?;
                Ok(vec![function])
            }
            NodeData::ClassDeclaration(mut data) => {
                let name = data
                    .name
                    .and_then(|id| self.context.arena().node_ref(self.source, id))
                    .and_then(|name| identifier_or_literal_text(self.context.arena(), name).ok());
                let exported = name
                    .as_deref()
                    .and_then(|name| self.info.exports_by_local.get(name).cloned())
                    .unwrap_or_default();
                data.modifiers = self.remove_export_modifiers(data.modifiers)?;
                let class = self.update_generic(statement, NodeData::ClassDeclaration(data))?;
                let mut statements = vec![class];
                if let Some(name) = name {
                    for export in exported {
                        let target = self.create_export_access(&export)?;
                        let value = self.create_identifier(&name)?;
                        let assignment = self.create_assignment(target, value)?;
                        statements.push(self.create_expression_statement(assignment)?);
                    }
                }
                Ok(statements)
            }
            NodeData::VariableStatement(data) => self.transform_variable_statement(statement, data),
            NodeData::Block(data) => Ok(vec![self.transform_block(statement, data)?]),
            NodeData::IfStatement(data) => self.transform_if_statement(statement, data),
            NodeData::SwitchStatement(data) => self.transform_switch_statement(statement, data),
            NodeData::WhileStatement(data) => self.transform_while_statement(statement, data),
            NodeData::DoStatement(data) => self.transform_do_statement(statement, data),
            NodeData::ForStatement(data) => self.transform_for_statement(statement, data),
            NodeData::ForInStatement(data) => self.transform_for_in_statement(statement, data),
            NodeData::ForOfStatement(data) => self.transform_for_of_statement(statement, data),
            NodeData::TryStatement(data) => self.transform_try_statement(statement, data),
            NodeData::LabeledStatement(data) => self.transform_labeled_statement(statement, data),
            NodeData::WithStatement(data) => self.transform_with_statement(statement, data),
            _ => Ok(vec![self.visit(statement.node())?]),
        }
    }

    fn transform_import(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::ImportDeclarationData,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let key = self.context.arena().get_original_node(original).node();
        let plan =
            self.info
                .imports
                .get(&key)
                .cloned()
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ImportDeclaration,
                    field: "module plan",
                })?;
        let module_specifier = data
            .module_specifier
            .and_then(|id| self.context.arena().node_ref(self.source, id))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ImportDeclaration,
                field: "module_specifier",
            })?;
        if self.module_kind == MODULE_AMD {
            return Ok(Vec::new());
        }
        let require = self.create_require_call(module_specifier)?;
        if data.import_clause.is_none() {
            let statement = self.create_expression_statement(require)?;
            self.set_original_and_range(statement, original)?;
            return Ok(vec![statement]);
        }
        let initializer = match plan.helper {
            ImportHelperKind::None => require,
            ImportHelperKind::Star if self.es_module_interop => {
                self.request_import_star_helper()?;
                let helper = self.create_identifier("__importStar")?;
                self.create_call(helper, vec![require])?
            }
            ImportHelperKind::Default if self.es_module_interop => {
                self.request_import_default_helper()?;
                let helper = self.create_identifier("__importDefault")?;
                self.create_call(helper, vec![require])?
            }
            ImportHelperKind::Star | ImportHelperKind::Default => require,
        };
        let declaration = self.create_variable_declaration(&plan.generated_name, initializer)?;
        let statement = self.create_variable_statement(vec![declaration], NodeFlags::CONST)?;
        self.set_original_and_range(statement, original)?;
        Ok(vec![statement])
    }

    fn transform_export_declaration(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::ExportDeclarationData,
    ) -> Result<Vec<TransformNode>, TransformError> {
        if data.module_specifier.is_none() {
            return Ok(Vec::new());
        }
        let module_specifier = data
            .module_specifier
            .and_then(|id| self.context.arena().node_ref(self.source, id))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ExportDeclaration,
                field: "module_specifier",
            })?;
        let require = self.create_require_call(module_specifier)?;
        if data.export_clause.is_none() {
            let helper = self.create_identifier("__exportStar")?;
            let exports = self.create_identifier("exports")?;
            let call = self.create_call(helper, vec![require, exports])?;
            let statement = self.create_expression_statement(call)?;
            self.set_original_and_range(statement, original)?;
            return Ok(vec![statement]);
        }
        Err(TransformError::DeferredModuleFormat {
            format: self.module_kind,
            owner_slice: "H2.2d",
        })
    }

    fn transform_variable_statement(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::VariableStatementData,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let direct_export = has_modifier(
            self.context.arena(),
            self.source,
            data.modifiers,
            SyntaxKind::ExportKeyword,
        )?;
        data.modifiers = self.remove_export_modifiers(data.modifiers)?;
        let declarations =
            variable_declarations(self.context.arena(), self.source, data.declaration_list)?;
        let mut retained = Vec::new();
        let mut trailing = Vec::new();
        for declaration in declarations {
            let NodeData::VariableDeclaration(mut variable) =
                self.context.arena().node(declaration)?.data.clone()
            else {
                retained.push(self.visit(declaration.node())?);
                continue;
            };
            let local = variable
                .name
                .and_then(|id| self.context.arena().node_ref(self.source, id))
                .and_then(|name| identifier_or_literal_text(self.context.arena(), name).ok());
            let exports = local
                .as_deref()
                .and_then(|name| self.info.exports_by_local.get(name).cloned())
                .unwrap_or_default();
            let original_declaration = self.context.arena().get_original_node(declaration).node();
            let unreferenced_direct = direct_export
                && !self.referenced_declarations.contains(&original_declaration)
                && variable.initializer.is_some()
                && exports.len() == 1;
            if unreferenced_direct {
                let initializer = self.visit(variable.initializer.expect("checked initializer"))?;
                let target = self.create_export_access(&exports[0])?;
                let assignment = self.create_assignment(target, initializer)?;
                let statement = self.create_expression_statement(assignment)?;
                self.set_original_and_range(statement, original)?;
                trailing.push(statement);
                continue;
            }
            if let Some(initializer) = variable.initializer {
                let mut initializer = self.visit(initializer)?;
                if direct_export {
                    for export in &exports {
                        let target = self.create_export_access(export)?;
                        initializer = self.create_assignment(target, initializer)?;
                    }
                }
                variable.initializer = Some(initializer.node());
            }
            let updated =
                self.update_generic(declaration, NodeData::VariableDeclaration(variable))?;
            retained.push(updated);
            if !direct_export && variable_has_initializer(self.context.arena(), updated)? {
                if let Some(local) = &local {
                    for export in exports {
                        let target = self.create_export_access(&export)?;
                        let value = self.create_identifier(local)?;
                        let assignment = self.create_assignment(target, value)?;
                        trailing.push(self.create_expression_statement(assignment)?);
                    }
                }
            }
        }
        let mut result = Vec::new();
        if !retained.is_empty() {
            let list = data
                .declaration_list
                .and_then(|id| self.context.arena().node_ref(self.source, id))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::VariableStatement,
                    field: "declaration_list",
                })?;
            let NodeData::VariableDeclarationList(mut list_data) =
                self.context.arena().node(list)?.data.clone()
            else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::VariableStatement,
                    field: "declaration_list",
                });
            };
            let array = if let Some(original_array) = list_data
                .declarations
                .and_then(|id| self.context.arena().node_array_ref(self.source, id))
            {
                self.context
                    .factory()?
                    .update_node_array(original_array, retained)?
            } else {
                self.context
                    .factory()?
                    .create_node_array(self.source, retained)?
            };
            list_data.declarations = Some(array.array());
            let flags = self.context.arena().transform_flags(list);
            let list = self.context.factory()?.update_node(
                list,
                NodeData::VariableDeclarationList(list_data),
                flags,
            )?;
            data.declaration_list = Some(list.node());
            let flags = self.context.arena().transform_flags(original);
            result.push(self.context.factory()?.update_node(
                original,
                NodeData::VariableStatement(data),
                flags,
            )?);
        }
        result.extend(trailing);
        Ok(result)
    }

    fn transform_block(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::BlockData,
    ) -> Result<TransformNode, TransformError> {
        let input = node_array_nodes(self.context.arena(), self.source, data.statements)?;
        let mut output = Vec::new();
        for statement in input {
            output.extend(self.visit_top_level_statement(statement)?);
        }
        data.statements = Some(
            if let Some(array) = data
                .statements
                .and_then(|id| self.context.arena().node_array_ref(self.source, id))
            {
                self.context
                    .factory()?
                    .update_node_array(array, output)?
                    .array()
            } else {
                self.context
                    .factory()?
                    .create_node_array(self.source, output)?
                    .array()
            },
        );
        let flags = self.context.arena().transform_flags(original);
        self.context
            .factory()?
            .update_node(original, NodeData::Block(data), flags)
    }

    fn transform_embedded_statement(
        &mut self,
        statement: Option<NodeId>,
        parent: SyntaxKind,
    ) -> Result<TransformNode, TransformError> {
        let statement = statement
            .and_then(|id| self.context.arena().node_ref(self.source, id))
            .ok_or(TransformError::RequiredChildRemoved {
                parent,
                field: "statement",
            })?;
        if let NodeData::Block(data) = self.context.arena().node(statement)?.data.clone() {
            return self.transform_block(statement, data);
        }
        let mut statements = self.visit_top_level_statement(statement)?;
        if statements.len() == 1 {
            return Ok(statements.remove(0));
        }
        self.create_block(statements, true)
    }

    fn transform_if_statement(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::IfStatementData,
    ) -> Result<Vec<TransformNode>, TransformError> {
        data.expression = data
            .expression
            .map(|id| self.visit(id).map(TransformNode::node))
            .transpose()?;
        data.then_statement = Some(
            self.transform_embedded_statement(data.then_statement, SyntaxKind::IfStatement)?
                .node(),
        );
        data.else_statement = data
            .else_statement
            .map(|id| {
                self.transform_embedded_statement(Some(id), SyntaxKind::IfStatement)
                    .map(TransformNode::node)
            })
            .transpose()?;
        let flags = self.context.arena().transform_flags(original);
        Ok(vec![self.context.factory()?.update_node(
            original,
            NodeData::IfStatement(data),
            flags,
        )?])
    }

    fn transform_switch_statement(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::SwitchStatementData,
    ) -> Result<Vec<TransformNode>, TransformError> {
        data.expression = data
            .expression
            .map(|id| self.visit(id).map(TransformNode::node))
            .transpose()?;
        if let Some(case_block) = data
            .case_block
            .and_then(|id| self.context.arena().node_ref(self.source, id))
        {
            let NodeData::CaseBlock(mut block_data) =
                self.context.arena().node(case_block)?.data.clone()
            else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::SwitchStatement,
                    field: "case_block",
                });
            };
            let clauses = node_array_nodes(self.context.arena(), self.source, block_data.clauses)?;
            let mut updated_clauses = Vec::new();
            for clause in clauses {
                let record = self.context.arena().node(clause)?.clone();
                let updated = match record.data {
                    NodeData::CaseClause(mut clause_data) => {
                        clause_data.expression = clause_data
                            .expression
                            .map(|id| self.visit(id).map(TransformNode::node))
                            .transpose()?;
                        clause_data.statements = Some(
                            self.transform_statement_array(clause_data.statements)?
                                .array(),
                        );
                        let flags = self.context.arena().transform_flags(clause);
                        self.context.factory()?.update_node(
                            clause,
                            NodeData::CaseClause(clause_data),
                            flags,
                        )?
                    }
                    NodeData::DefaultClause(mut clause_data) => {
                        clause_data.statements = Some(
                            self.transform_statement_array(clause_data.statements)?
                                .array(),
                        );
                        let flags = self.context.arena().transform_flags(clause);
                        self.context.factory()?.update_node(
                            clause,
                            NodeData::DefaultClause(clause_data),
                            flags,
                        )?
                    }
                    _ => self.visit(clause.node())?,
                };
                updated_clauses.push(updated);
            }
            block_data.clauses = Some(
                if let Some(array) = block_data
                    .clauses
                    .and_then(|id| self.context.arena().node_array_ref(self.source, id))
                {
                    self.context
                        .factory()?
                        .update_node_array(array, updated_clauses)?
                        .array()
                } else {
                    self.context
                        .factory()?
                        .create_node_array(self.source, updated_clauses)?
                        .array()
                },
            );
            let flags = self.context.arena().transform_flags(case_block);
            data.case_block = Some(
                self.context
                    .factory()?
                    .update_node(case_block, NodeData::CaseBlock(block_data), flags)?
                    .node(),
            );
        }
        let flags = self.context.arena().transform_flags(original);
        Ok(vec![self.context.factory()?.update_node(
            original,
            NodeData::SwitchStatement(data),
            flags,
        )?])
    }

    fn transform_statement_array(
        &mut self,
        statements: Option<NodeArrayId>,
    ) -> Result<TransformNodeArray, TransformError> {
        let input = node_array_nodes(self.context.arena(), self.source, statements)?;
        let mut output = Vec::new();
        for statement in input {
            output.extend(self.visit_top_level_statement(statement)?);
        }
        if let Some(array) =
            statements.and_then(|id| self.context.arena().node_array_ref(self.source, id))
        {
            self.context.factory()?.update_node_array(array, output)
        } else {
            self.context
                .factory()?
                .create_node_array(self.source, output)
        }
    }

    fn transform_while_statement(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::WhileStatementData,
    ) -> Result<Vec<TransformNode>, TransformError> {
        data.expression = data
            .expression
            .map(|id| self.visit(id).map(TransformNode::node))
            .transpose()?;
        data.statement = Some(
            self.transform_embedded_statement(data.statement, SyntaxKind::WhileStatement)?
                .node(),
        );
        let flags = self.context.arena().transform_flags(original);
        Ok(vec![self.context.factory()?.update_node(
            original,
            NodeData::WhileStatement(data),
            flags,
        )?])
    }

    fn transform_do_statement(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::DoStatementData,
    ) -> Result<Vec<TransformNode>, TransformError> {
        data.statement = Some(
            self.transform_embedded_statement(data.statement, SyntaxKind::DoStatement)?
                .node(),
        );
        data.expression = data
            .expression
            .map(|id| self.visit(id).map(TransformNode::node))
            .transpose()?;
        let flags = self.context.arena().transform_flags(original);
        Ok(vec![self.context.factory()?.update_node(
            original,
            NodeData::DoStatement(data),
            flags,
        )?])
    }

    fn transform_for_statement(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ForStatementData,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let mut prefix = Vec::new();
        if let Some(initializer) = data
            .initializer
            .and_then(|id| self.context.arena().node_ref(self.source, id))
        {
            if self.context.arena().node(initializer)?.kind == SyntaxKind::VariableDeclarationList
                && variable_list_has_exported_declaration(
                    self.context.arena(),
                    self.source,
                    initializer,
                    &self.info.exports_by_local,
                )?
            {
                let statement = self.context.factory()?.create_node(
                    self.source,
                    NodeData::VariableStatement(tsc_syntax::nodes::VariableStatementData {
                        modifiers: None,
                        declaration_list: Some(initializer.node()),
                    }),
                    TransformFlags::NONE,
                )?;
                prefix.extend(self.transform_variable_statement(
                    statement,
                    tsc_syntax::nodes::VariableStatementData {
                        modifiers: None,
                        declaration_list: Some(initializer.node()),
                    },
                )?);
                data.initializer = None;
            } else {
                data.initializer = Some(self.visit(initializer.node())?.node());
            }
        }
        data.condition = data
            .condition
            .map(|id| self.visit(id).map(TransformNode::node))
            .transpose()?;
        data.incrementor = data
            .incrementor
            .map(|id| self.visit(id).map(TransformNode::node))
            .transpose()?;
        data.statement = Some(
            self.transform_embedded_statement(data.statement, SyntaxKind::ForStatement)?
                .node(),
        );
        let flags = self.context.arena().transform_flags(original);
        prefix.push(self.context.factory()?.update_node(
            original,
            NodeData::ForStatement(data),
            flags,
        )?);
        Ok(prefix)
    }

    fn transform_for_in_statement(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ForInStatementData,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let assignments = self.export_assignments_for_initializer(data.initializer)?;
        data.initializer = data
            .initializer
            .map(|id| self.visit(id).map(TransformNode::node))
            .transpose()?;
        data.expression = data
            .expression
            .map(|id| self.visit(id).map(TransformNode::node))
            .transpose()?;
        data.statement = Some(
            self.transform_loop_body(data.statement, SyntaxKind::ForInStatement, assignments)?
                .node(),
        );
        let flags = self.context.arena().transform_flags(original);
        Ok(vec![self.context.factory()?.update_node(
            original,
            NodeData::ForInStatement(data),
            flags,
        )?])
    }

    fn transform_for_of_statement(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ForOfStatementData,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let assignments = self.export_assignments_for_initializer(data.initializer)?;
        data.initializer = data
            .initializer
            .map(|id| self.visit(id).map(TransformNode::node))
            .transpose()?;
        data.expression = data
            .expression
            .map(|id| self.visit(id).map(TransformNode::node))
            .transpose()?;
        data.statement = Some(
            self.transform_loop_body(data.statement, SyntaxKind::ForOfStatement, assignments)?
                .node(),
        );
        let flags = self.context.arena().transform_flags(original);
        Ok(vec![self.context.factory()?.update_node(
            original,
            NodeData::ForOfStatement(data),
            flags,
        )?])
    }

    fn export_assignments_for_initializer(
        &mut self,
        initializer: Option<NodeId>,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let Some(initializer) =
            initializer.and_then(|id| self.context.arena().node_ref(self.source, id))
        else {
            return Ok(Vec::new());
        };
        if self.context.arena().node(initializer)?.kind != SyntaxKind::VariableDeclarationList {
            return Ok(Vec::new());
        }
        let declarations = match &self.context.arena().node(initializer)?.data {
            NodeData::VariableDeclarationList(data) => variable_declarations_from_array(
                self.context.arena(),
                self.source,
                data.declarations,
            )?,
            _ => Vec::new(),
        };
        let mut assignments = Vec::new();
        for declaration in declarations {
            let NodeData::VariableDeclaration(data) = &self.context.arena().node(declaration)?.data
            else {
                continue;
            };
            let Some(local) = data
                .name
                .and_then(|id| self.context.arena().node_ref(self.source, id))
                .and_then(|name| identifier_or_literal_text(self.context.arena(), name).ok())
            else {
                continue;
            };
            let exports = self
                .info
                .exports_by_local
                .get(local.as_str())
                .cloned()
                .unwrap_or_default();
            for export in exports {
                let target = self.create_export_access(&export)?;
                let value = self.create_identifier(&local)?;
                let assignment = self.create_assignment(target, value)?;
                assignments.push(self.create_expression_statement(assignment)?);
            }
        }
        Ok(assignments)
    }

    fn transform_loop_body(
        &mut self,
        statement: Option<NodeId>,
        parent: SyntaxKind,
        mut prefix: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let body = self.transform_embedded_statement(statement, parent)?;
        if prefix.is_empty() {
            return Ok(body);
        }
        if let NodeData::Block(mut data) = self.context.arena().node(body)?.data.clone() {
            prefix.extend(node_array_nodes(
                self.context.arena(),
                self.source,
                data.statements,
            )?);
            data.statements = Some(
                self.context
                    .factory()?
                    .create_node_array(self.source, prefix)?
                    .array(),
            );
            let flags = self.context.arena().transform_flags(body);
            return self
                .context
                .factory()?
                .update_node(body, NodeData::Block(data), flags);
        }
        prefix.push(body);
        self.create_block(prefix, true)
    }

    fn transform_try_statement(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::TryStatementData,
    ) -> Result<Vec<TransformNode>, TransformError> {
        if let Some(block) = data
            .try_block
            .and_then(|id| self.context.arena().node_ref(self.source, id))
        {
            let NodeData::Block(block_data) = self.context.arena().node(block)?.data.clone() else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::TryStatement,
                    field: "try_block",
                });
            };
            data.try_block = Some(self.transform_block(block, block_data)?.node());
        }
        if let Some(catch) = data
            .catch_clause
            .and_then(|id| self.context.arena().node_ref(self.source, id))
        {
            let NodeData::CatchClause(mut catch_data) =
                self.context.arena().node(catch)?.data.clone()
            else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::TryStatement,
                    field: "catch_clause",
                });
            };
            catch_data.variable_declaration = catch_data
                .variable_declaration
                .map(|id| self.visit(id).map(TransformNode::node))
                .transpose()?;
            if let Some(block) = catch_data
                .block
                .and_then(|id| self.context.arena().node_ref(self.source, id))
            {
                let NodeData::Block(block_data) = self.context.arena().node(block)?.data.clone()
                else {
                    return Err(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::CatchClause,
                        field: "block",
                    });
                };
                catch_data.block = Some(self.transform_block(block, block_data)?.node());
            }
            let flags = self.context.arena().transform_flags(catch);
            data.catch_clause = Some(
                self.context
                    .factory()?
                    .update_node(catch, NodeData::CatchClause(catch_data), flags)?
                    .node(),
            );
        }
        if let Some(block) = data
            .finally_block
            .and_then(|id| self.context.arena().node_ref(self.source, id))
        {
            let NodeData::Block(block_data) = self.context.arena().node(block)?.data.clone() else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::TryStatement,
                    field: "finally_block",
                });
            };
            data.finally_block = Some(self.transform_block(block, block_data)?.node());
        }
        let flags = self.context.arena().transform_flags(original);
        Ok(vec![self.context.factory()?.update_node(
            original,
            NodeData::TryStatement(data),
            flags,
        )?])
    }

    fn transform_labeled_statement(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::LabeledStatementData,
    ) -> Result<Vec<TransformNode>, TransformError> {
        data.statement = Some(
            self.transform_embedded_statement(data.statement, SyntaxKind::LabeledStatement)?
                .node(),
        );
        let flags = self.context.arena().transform_flags(original);
        Ok(vec![self.context.factory()?.update_node(
            original,
            NodeData::LabeledStatement(data),
            flags,
        )?])
    }

    fn transform_with_statement(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::WithStatementData,
    ) -> Result<Vec<TransformNode>, TransformError> {
        data.expression = data
            .expression
            .map(|id| self.visit(id).map(TransformNode::node))
            .transpose()?;
        data.statement = Some(
            self.transform_embedded_statement(data.statement, SyntaxKind::WithStatement)?
                .node(),
        );
        let flags = self.context.arena().transform_flags(original);
        Ok(vec![self.context.factory()?.update_node(
            original,
            NodeData::WithStatement(data),
            flags,
        )?])
    }

    fn visit(&mut self, id: NodeId) -> Result<TransformNode, TransformError> {
        if let Some(mapped) = self.nodes.get(&id) {
            return Ok(self.node(*mapped));
        }
        let original = self
            .context
            .arena()
            .node_ref(self.source, id)
            .ok_or_else(|| TransformError::UnknownNode(self.node(id)))?;
        let record = self.context.arena().node(original)?.clone();
        let transformed = match record.data {
            NodeData::Token => original,
            NodeData::Identifier(_) => self.substitute_import_identifier(original)?,
            NodeData::CallExpression(data) => self.visit_call_expression(original, data)?,
            data => self.update_generic(original, data)?,
        };
        self.nodes.insert(id, transformed.node());
        Ok(transformed)
    }

    fn visit_call_expression(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::CallExpressionData,
    ) -> Result<TransformNode, TransformError> {
        let is_dynamic_import = data
            .expression
            .and_then(|id| self.context.arena().node_ref(self.source, id))
            .is_some_and(|expression| {
                self.context
                    .arena()
                    .node(expression)
                    .is_ok_and(|node| node.kind == SyntaxKind::ImportKeyword)
            });
        if is_dynamic_import {
            let arguments = node_array_nodes(self.context.arena(), self.source, data.arguments)?;
            let Some(argument) = arguments.first().copied() else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::CallExpression,
                    field: "arguments[0]",
                });
            };
            let argument = self.visit(argument.node())?;
            if self.module_kind == MODULE_AMD {
                let transformed = self.create_amd_dynamic_import(argument)?;
                self.set_original_and_range(transformed, original)?;
                return Ok(transformed);
            }
            if self.module_kind == MODULE_UMD {
                let transformed = self.create_umd_dynamic_import(argument)?;
                self.set_original_and_range(transformed, original)?;
                return Ok(transformed);
            }
            let require = self.create_require_call(argument)?;
            let loaded = if self.es_module_interop {
                self.request_import_star_helper()?;
                let helper = self.create_identifier("__importStar")?;
                self.create_call(helper, vec![require])?
            } else {
                require
            };
            let body = loaded;
            let parameters = self
                .context
                .factory()?
                .create_node_array(self.source, Vec::new())?;
            let arrow_token = self.context.factory()?.create_token(
                self.source,
                SyntaxKind::EqualsGreaterThanToken,
                TransformFlags::NONE,
            )?;
            let arrow = self.context.factory()?.create_node(
                self.source,
                NodeData::ArrowFunction(tsc_syntax::nodes::ArrowFunctionData {
                    type_parameters: None,
                    parameters: Some(parameters.array()),
                    r#type: None,
                    body: Some(body.node()),
                    modifiers: None,
                    equals_greater_than_token: Some(arrow_token.node()),
                }),
                TransformFlags::NONE,
            )?;
            let promise = self.create_identifier("Promise")?;
            let resolve = self.create_property_access(promise, "resolve")?;
            let resolved = self.create_call(resolve, Vec::new())?;
            let then = self.create_property_access(resolved, "then")?;
            let transformed = self.create_call(then, vec![arrow])?;
            self.set_original_and_range(transformed, original)?;
            return Ok(transformed);
        }

        let imported_callee = data
            .expression
            .and_then(|id| self.context.arena().node_ref(self.source, id))
            .map(|callee| self.import_binding_for_reference(callee))
            .transpose()?
            .flatten()
            .is_some();
        let mut node_data = NodeData::CallExpression(data);
        try_visit_each_child(&mut node_data, self)?;
        let NodeData::CallExpression(mut data) = node_data else {
            unreachable!("call expression visitor preserves kind")
        };
        if imported_callee {
            let callee = data
                .expression
                .and_then(|id| self.context.arena().node_ref(self.source, id))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::CallExpression,
                    field: "expression",
                })?;
            let zero = self.create_numeric_literal("0")?;
            let comma = self.context.factory()?.create_token(
                self.source,
                SyntaxKind::CommaToken,
                TransformFlags::NONE,
            )?;
            let indirect = self.context.factory()?.create_node(
                self.source,
                NodeData::BinaryExpression(tsc_syntax::nodes::BinaryExpressionData {
                    left: Some(zero.node()),
                    operator_token: Some(comma.node()),
                    right: Some(callee.node()),
                }),
                TransformFlags::NONE,
            )?;
            let parenthesized = self.context.factory()?.create_node(
                self.source,
                NodeData::ParenthesizedExpression(tsc_syntax::nodes::ParenthesizedExpressionData {
                    expression: Some(indirect.node()),
                }),
                TransformFlags::NONE,
            )?;
            data.expression = Some(parenthesized.node());
        }
        self.update_generic_without_visit(original, NodeData::CallExpression(data))
    }

    fn create_common_js_dynamic_import_value(
        &mut self,
        argument: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let require = self.create_require_call(argument)?;
        let loaded = if self.es_module_interop {
            self.request_import_star_helper()?;
            let helper = self.create_identifier("__importStar")?;
            self.create_call(helper, vec![require])?
        } else {
            require
        };
        let arrow = self.create_arrow_function(Vec::new(), loaded)?;
        let promise = self.create_identifier("Promise")?;
        let resolve = self.create_property_access(promise, "resolve")?;
        let resolved = self.create_call(resolve, Vec::new())?;
        let then = self.create_property_access(resolved, "then")?;
        self.create_call(then, vec![arrow])
    }

    fn create_amd_dynamic_import(
        &mut self,
        argument: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.dynamic_import_ordinal += 1;
        let resolve_name = format!("resolve_{}", self.dynamic_import_ordinal);
        let reject_name = format!("reject_{}", self.dynamic_import_ordinal);
        let resolve_parameter = self.create_parameter(&resolve_name)?;
        let reject_parameter = self.create_parameter(&reject_name)?;
        let dependency = self.create_array_literal(vec![argument])?;
        let require = self.create_identifier("require")?;
        let resolve = self.create_identifier(&resolve_name)?;
        let reject = self.create_identifier(&reject_name)?;
        let require_call = self.create_call(require, vec![dependency, resolve, reject])?;
        let require_statement = self.create_expression_statement(require_call)?;
        let body = self.create_block(vec![require_statement], false)?;
        let executor =
            self.create_arrow_function(vec![resolve_parameter, reject_parameter], body)?;
        let promise = self.create_identifier("Promise")?;
        let loaded = self.create_new(promise, vec![executor])?;
        if self.es_module_interop {
            self.request_import_star_helper()?;
            let then = self.create_property_access(loaded, "then")?;
            let helper = self.create_identifier("__importStar")?;
            self.create_call(then, vec![helper])
        } else {
            Ok(loaded)
        }
    }

    fn create_umd_dynamic_import(
        &mut self,
        argument: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let asynchronous_argument = self.context.factory()?.clone_node(argument)?;
        let common_js = self.create_common_js_dynamic_import_value(argument)?;
        let amd = self.create_amd_dynamic_import(asynchronous_argument)?;
        let condition = self.create_identifier("__syncRequire")?;
        self.create_conditional(condition, common_js, amd)
    }

    fn substitute_import_identifier(
        &mut self,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let Some(binding) = self.import_binding_for_reference(original)? else {
            return Ok(original);
        };
        let target = self.create_identifier(&binding.generated_name)?;
        let transformed = if let Some(property) = binding.property {
            self.create_property_access(target, &property)?
        } else {
            target
        };
        self.set_original_and_range(transformed, original)?;
        Ok(transformed)
    }

    fn import_binding_for_reference(
        &self,
        node: TransformNode,
    ) -> Result<Option<ImportBinding>, TransformError> {
        let original = self.context.arena().get_original_node(node);
        if original == node
            && NodeFlags::from_bits(self.context.arena().node(node)?.flags)
                .contains(NodeFlags::SYNTHESIZED)
        {
            return Ok(None);
        }
        let resolver_node = self.resolver_node(node)?;
        let declaration = self
            .resolver
            .get_referenced_import_declaration(resolver_node)?;
        Ok(declaration
            .and_then(|declaration| self.info.import_bindings.get(&declaration.node()).cloned()))
    }

    fn update_generic(
        &mut self,
        original: TransformNode,
        mut data: NodeData,
    ) -> Result<TransformNode, TransformError> {
        try_visit_each_child(&mut data, self)?;
        self.update_generic_without_visit(original, data)
    }

    fn update_generic_without_visit(
        &mut self,
        original: TransformNode,
        data: NodeData,
    ) -> Result<TransformNode, TransformError> {
        let flags = flags_after_update(self.context.arena(), original, &data)?;
        self.context.factory()?.update_node(original, data, flags)
    }

    fn remove_export_modifiers(
        &mut self,
        modifiers: Option<NodeArrayId>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let Some(modifiers) =
            modifiers.and_then(|id| self.context.arena().node_array_ref(self.source, id))
        else {
            return Ok(None);
        };
        let retained = self
            .context
            .arena()
            .node_array(modifiers)?
            .nodes
            .iter()
            .filter_map(|id| self.context.arena().node_ref(self.source, *id))
            .filter(|modifier| {
                self.context.arena().node(*modifier).is_ok_and(|node| {
                    !matches!(
                        node.kind,
                        SyntaxKind::ExportKeyword | SyntaxKind::DefaultKeyword
                    )
                })
            })
            .collect::<Vec<_>>();
        if retained.is_empty() {
            Ok(None)
        } else {
            Ok(Some(
                self.context
                    .factory()?
                    .update_node_array(modifiers, retained)?
                    .array(),
            ))
        }
    }

    fn create_identifier(&mut self, text: &str) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::Identifier(tsc_syntax::nodes::IdentifierData {
                escaped_text: text.to_owned(),
                text: text.to_owned(),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_numeric_literal(&mut self, text: &str) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::NumericLiteral(tsc_syntax::nodes::NumericLiteralData {
                text: text.to_owned(),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_string_literal(&mut self, text: &str) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::StringLiteral(tsc_syntax::nodes::StringLiteralData {
                text: text.to_owned(),
                has_extended_unicode_escape: None,
            }),
            TransformFlags::NONE,
        )
    }

    fn create_array_literal(
        &mut self,
        elements: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let elements = self
            .context
            .factory()?
            .create_node_array(self.source, elements)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ArrayLiteralExpression(tsc_syntax::nodes::ArrayLiteralExpressionData {
                elements: Some(elements.array()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_parameter(&mut self, name: &str) -> Result<TransformNode, TransformError> {
        let name = self.create_identifier(name)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::Parameter(tsc_syntax::nodes::ParameterData {
                name: Some(name.node()),
                modifiers: None,
                dot_dot_dot_token: None,
                question_token: None,
                r#type: None,
                initializer: None,
            }),
            TransformFlags::NONE,
        )
    }

    fn create_function_expression(
        &mut self,
        parameters: Vec<TransformNode>,
        body: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let parameters = self
            .context
            .factory()?
            .create_node_array(self.source, parameters)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::FunctionExpression(tsc_syntax::nodes::FunctionExpressionData {
                name: None,
                type_parameters: None,
                parameters: Some(parameters.array()),
                r#type: None,
                asterisk_token: None,
                body: Some(body.node()),
                modifiers: None,
            }),
            TransformFlags::NONE,
        )
    }

    fn create_arrow_function(
        &mut self,
        parameters: Vec<TransformNode>,
        body: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let parameters = self
            .context
            .factory()?
            .create_node_array(self.source, parameters)?;
        let arrow = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::EqualsGreaterThanToken,
            TransformFlags::NONE,
        )?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ArrowFunction(tsc_syntax::nodes::ArrowFunctionData {
                type_parameters: None,
                parameters: Some(parameters.array()),
                r#type: None,
                body: Some(body.node()),
                modifiers: None,
                equals_greater_than_token: Some(arrow.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_new(
        &mut self,
        expression: TransformNode,
        arguments: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let arguments = self
            .context
            .factory()?
            .create_node_array(self.source, arguments)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::NewExpression(tsc_syntax::nodes::NewExpressionData {
                expression: Some(expression.node()),
                type_arguments: None,
                arguments: Some(arguments.array()),
                question_dot_token: None,
            }),
            TransformFlags::NONE,
        )
    }

    fn create_typeof(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::TypeOfExpression(tsc_syntax::nodes::TypeOfExpressionData {
                expression: Some(expression.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_parenthesized(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::ParenthesizedExpression(tsc_syntax::nodes::ParenthesizedExpressionData {
                expression: Some(expression.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_binary(
        &mut self,
        left: TransformNode,
        operator: SyntaxKind,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let operator =
            self.context
                .factory()?
                .create_token(self.source, operator, TransformFlags::NONE)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::BinaryExpression(tsc_syntax::nodes::BinaryExpressionData {
                left: Some(left.node()),
                operator_token: Some(operator.node()),
                right: Some(right.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_conditional(
        &mut self,
        condition: TransformNode,
        when_true: TransformNode,
        when_false: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let question = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::QuestionToken,
            TransformFlags::NONE,
        )?;
        let colon = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::ColonToken,
            TransformFlags::NONE,
        )?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ConditionalExpression(tsc_syntax::nodes::ConditionalExpressionData {
                condition: Some(condition.node()),
                question_token: Some(question.node()),
                when_true: Some(when_true.node()),
                colon_token: Some(colon.node()),
                when_false: Some(when_false.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_if_statement(
        &mut self,
        expression: TransformNode,
        then_statement: TransformNode,
        else_statement: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::IfStatement(tsc_syntax::nodes::IfStatementData {
                expression: Some(expression.node()),
                then_statement: Some(then_statement.node()),
                else_statement: else_statement.map(TransformNode::node),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_property_access(
        &mut self,
        expression: TransformNode,
        name: &str,
    ) -> Result<TransformNode, TransformError> {
        let name = self.create_identifier(name)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::PropertyAccessExpression(tsc_syntax::nodes::PropertyAccessExpressionData {
                name: Some(name.node()),
                expression: Some(expression.node()),
                question_dot_token: None,
            }),
            TransformFlags::NONE,
        )
    }

    fn create_call(
        &mut self,
        expression: TransformNode,
        arguments: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let arguments = self
            .context
            .factory()?
            .create_node_array(self.source, arguments)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::CallExpression(tsc_syntax::nodes::CallExpressionData {
                expression: Some(expression.node()),
                question_dot_token: None,
                type_arguments: None,
                arguments: Some(arguments.array()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_require_call(
        &mut self,
        module_specifier: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let require = self.create_identifier("require")?;
        self.create_call(require, vec![module_specifier])
    }

    fn create_assignment(
        &mut self,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let equals = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::EqualsToken,
            TransformFlags::NONE,
        )?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::BinaryExpression(tsc_syntax::nodes::BinaryExpressionData {
                left: Some(left.node()),
                operator_token: Some(equals.node()),
                right: Some(right.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_expression_statement(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::ExpressionStatement(tsc_syntax::nodes::ExpressionStatementData {
                expression: Some(expression.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_export_access(&mut self, name: &str) -> Result<TransformNode, TransformError> {
        let exports = self.create_identifier("exports")?;
        if is_identifier_export_name(name) {
            self.create_property_access(exports, name)
        } else {
            let name = self.create_string_literal(name)?;
            self.context.factory()?.create_node(
                self.source,
                NodeData::ElementAccessExpression(tsc_syntax::nodes::ElementAccessExpressionData {
                    expression: Some(exports.node()),
                    question_dot_token: None,
                    argument_expression: Some(name.node()),
                }),
                TransformFlags::NONE,
            )
        }
    }

    fn create_void_zero(&mut self) -> Result<TransformNode, TransformError> {
        let zero = self.create_numeric_literal("0")?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::VoidExpression(tsc_syntax::nodes::VoidExpressionData {
                expression: Some(zero.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_es_module_marker(&mut self) -> Result<TransformNode, TransformError> {
        let object = self.create_identifier("Object")?;
        let define_property = self.create_property_access(object, "defineProperty")?;
        let exports = self.create_identifier("exports")?;
        let marker = self.create_string_literal("__esModule")?;
        let value_name = self.create_identifier("value")?;
        let value = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::TrueKeyword,
            TransformFlags::NONE,
        )?;
        let property = self.context.factory()?.create_node(
            self.source,
            NodeData::PropertyAssignment(tsc_syntax::nodes::PropertyAssignmentData {
                name: Some(value_name.node()),
                initializer: Some(value.node()),
                modifiers: None,
                question_token: None,
                exclamation_token: None,
            }),
            TransformFlags::NONE,
        )?;
        let properties = self
            .context
            .factory()?
            .create_node_array(self.source, vec![property])?;
        let descriptor = self.context.factory()?.create_node(
            self.source,
            NodeData::ObjectLiteralExpression(tsc_syntax::nodes::ObjectLiteralExpressionData {
                properties: Some(properties.array()),
            }),
            TransformFlags::NONE,
        )?;
        let call = self.create_call(define_property, vec![exports, marker, descriptor])?;
        let statement = self.create_expression_statement(call)?;
        self.context
            .arena_mut()?
            .metadata_mut(statement)
            .add_flags(crate::EmitFlags::CUSTOM_PROLOGUE);
        Ok(statement)
    }

    fn create_variable_declaration(
        &mut self,
        name: &str,
        initializer: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let name = self.create_identifier(name)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::VariableDeclaration(tsc_syntax::nodes::VariableDeclarationData {
                name: Some(name.node()),
                exclamation_token: None,
                r#type: None,
                initializer: Some(initializer.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_variable_statement(
        &mut self,
        declarations: Vec<TransformNode>,
        flags: NodeFlags,
    ) -> Result<TransformNode, TransformError> {
        let declarations = self
            .context
            .factory()?
            .create_node_array(self.source, declarations)?;
        let list = self.context.factory()?.create_node(
            self.source,
            NodeData::VariableDeclarationList(tsc_syntax::nodes::VariableDeclarationListData {
                declarations: Some(declarations.array()),
            }),
            TransformFlags::NONE,
        )?;
        self.context.factory()?.set_node_flags(list, flags)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::VariableStatement(tsc_syntax::nodes::VariableStatementData {
                modifiers: None,
                declaration_list: Some(list.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_block(
        &mut self,
        statements: Vec<TransformNode>,
        multi_line: bool,
    ) -> Result<TransformNode, TransformError> {
        let statements = self
            .context
            .factory()?
            .create_node_array(self.source, statements)?;
        let block = self.context.factory()?.create_node(
            self.source,
            NodeData::Block(tsc_syntax::nodes::BlockData {
                statements: Some(statements.array()),
            }),
            TransformFlags::NONE,
        )?;
        self.context.factory()?.set_multi_line(block, multi_line)
    }

    fn create_block_from_array(
        &mut self,
        statements: Vec<TransformNode>,
        original_array: Option<NodeArrayId>,
        multi_line: bool,
    ) -> Result<TransformNode, TransformError> {
        let statements = if let Some(original) =
            original_array.and_then(|array| self.context.arena().node_array_ref(self.source, array))
        {
            self.context
                .factory()?
                .update_node_array(original, statements)?
        } else {
            self.context
                .factory()?
                .create_node_array(self.source, statements)?
        };
        let block = self.context.factory()?.create_node(
            self.source,
            NodeData::Block(tsc_syntax::nodes::BlockData {
                statements: Some(statements.array()),
            }),
            TransformFlags::NONE,
        )?;
        self.context.factory()?.set_multi_line(block, multi_line)
    }

    fn request_import_star_helper(&mut self) -> Result<(), TransformError> {
        let create_binding = crate::EmitHelper::with_text(
            "typescript:commonjscreatebinding",
            false,
            CREATE_BINDING_HELPER_TEXT,
            1,
            Vec::new(),
        );
        let set_default = crate::EmitHelper::with_text(
            "typescript:commonjscreatevalue",
            false,
            SET_MODULE_DEFAULT_HELPER_TEXT,
            1,
            Vec::new(),
        );
        self.context
            .request_emit_helper(crate::EmitHelper::with_text(
                "typescript:commonjsimportstar",
                false,
                IMPORT_STAR_HELPER_TEXT,
                2,
                vec![create_binding, set_default],
            ))
    }

    fn request_import_default_helper(&mut self) -> Result<(), TransformError> {
        self.context
            .request_emit_helper(crate::EmitHelper::with_text(
                "typescript:commonjsimportdefault",
                false,
                IMPORT_DEFAULT_HELPER_TEXT,
                1,
                Vec::new(),
            ))
    }

    fn set_original_and_range(
        &mut self,
        node: TransformNode,
        original: TransformNode,
    ) -> Result<(), TransformError> {
        self.context.factory()?.set_text_range(node, original)?;
        self.context
            .arena_mut()?
            .set_original_node(node, Some(original))
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

    const fn node(&self, id: NodeId) -> TransformNode {
        TransformNode::new(self.source, id)
    }

    const fn array(&self, id: NodeArrayId) -> TransformNodeArray {
        TransformNodeArray::new(self.source, id)
    }
}

impl NodeDataChildVisitor for CommonJsVisitor<'_, '_> {
    type Error = TransformError;

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.context
            .arena()
            .node(self.node(id))
            .expect("CommonJS child belongs to its transform source")
            .kind
    }

    fn visit_node(&mut self, id: NodeId) -> Result<Option<NodeId>, Self::Error> {
        self.visit(id).map(|node| Some(node.node()))
    }

    fn visit_nodes(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, Self::Error> {
        if let Some(mapped) = self.arrays.get(&id) {
            return Ok(Some(*mapped));
        }
        let original = self.array(id);
        let nodes = self.context.arena().node_array(original)?.nodes.clone();
        let mut visited = Vec::with_capacity(nodes.len());
        for node in nodes {
            visited.push(self.visit(node)?);
        }
        let updated = self
            .context
            .factory()?
            .update_node_array(original, visited)?;
        self.arrays.insert(id, updated.array());
        Ok(Some(updated.array()))
    }

    fn required_child_removed(&mut self, parent: SyntaxKind, field: &'static str) -> Self::Error {
        TransformError::RequiredChildRemoved { parent, field }
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

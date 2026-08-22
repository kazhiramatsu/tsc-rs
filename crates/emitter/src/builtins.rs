use std::collections::{BTreeMap, BTreeSet};

use tsc_program::SourceFileId;
use tsc_syntax::{
    for_each_child, for_each_child_array, identifier_to_keyword_kind, skip_trivia,
    try_visit_each_child, Node, NodeArrayId, NodeData, NodeDataChildVisitor, NodeId, SyntaxKind,
};
use tsc_types::{CompilerOptions, NodeFlags, ScriptTarget};

use crate::transform::GeneratedBindingId;
use crate::{
    factory::{private_identifier_expression_flags, EmitHelperName},
    EmitConstantValue, EmitExportContainerMode, EmitFlags, EmitHint, EmitHost, EmitResolver,
    EmitResolverError, EmitResolverMethod, EmitResolverNode, H2ActivityCanary, H2RuntimeSlice,
    InternalEmitFlags, LexicalEnvironment, LexicalEnvironmentFlags, SourceMapRange, SourceRange,
    SyntheticComment, SyntheticCommentKind, TransformArena, TransformError, TransformFlags,
    TransformNode, TransformNodeArray, TransformRoot, TransformSourceId, TransformationContext,
    Transformer, UnsupportedTransformFeature,
};

const MODULE_NONE: i32 = 0;
const MODULE_COMMON_JS: i32 = 1;
const MODULE_AMD: i32 = 2;
const MODULE_UMD: i32 = 3;
const MODULE_SYSTEM: i32 = 4;
const MODULE_ES2015: i32 = 5;
const MODULE_ES2020: i32 = 6;
const MODULE_ES2022: i32 = 7;
const MODULE_ES_NEXT: i32 = 99;
const MODULE_NODE16: i32 = 100;
const MODULE_NODE18: i32 = 101;
const MODULE_NODE20: i32 = 102;
const MODULE_NODE_NEXT: i32 = 199;
const MODULE_PRESERVE: i32 = 200;

mod class_fields;
mod es2015;
mod es2017;
mod es2018;
mod es2021;
mod es_next;
mod flatten_destructuring;
mod generated_bindings;
mod generators;
mod helpers;
mod jsx;
mod legacy_decorators;
mod standard_decorators;
mod system;
mod target_bindings;

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
const EXPORT_STAR_HELPER_TEXT: &str = r#"var __exportStar = (this && this.__exportStar) || function(m, exports) {
    for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports, p)) __createBinding(exports, m, p);
};"#;
const REWRITE_RELATIVE_IMPORT_EXTENSIONS_HELPER_TEXT: &str = r#"var __rewriteRelativeImportExtension = (this && this.__rewriteRelativeImportExtension) || function (path, preserveJsx) {
    if (typeof path === "string" && /^\.\.?\//.test(path)) {
        return path.replace(/\.(tsx)$|((?:\.d)?)((?:\.[^./]+?)?)\.([cm]?)ts$/i, function (m, tsx, d, ext, cm) {
            return tsx ? preserveJsx ? ".jsx" : ".js" : d && (!ext || !cm) ? m : (d + ext + "." + cm.toLowerCase() + "js");
        });
    }
    return path;
};"#;

/// tsc-port: getScriptTransformers @6.0.3
/// tsc-hash: 69bdc65a0c428ad5819419fabd0ecd483bb661350434c5ad0ea0bdec15096fd0
/// tsc-span: _tsc.js:115903-115949
pub fn get_script_transformers<'resolver>(
    options: &CompilerOptions,
    resolver: &'resolver dyn EmitResolver,
) -> Result<Vec<Box<dyn Transformer + 'resolver>>, TransformError> {
    let mut activity = H2ActivityCanary::h2_5g_profile();
    get_script_transformers_with_optional_host(options, resolver, None, &mut activity)
}

/// Select the script transforms for one Program-owned source.
///
/// Module transforms whose effective format can depend on source metadata
/// require this entry point rather than reconstructing that format from the
/// compiler options alone.
pub fn get_script_transformers_for_source<'transformers>(
    options: &CompilerOptions,
    resolver: &'transformers dyn EmitResolver,
    host: &'transformers dyn EmitHost,
    source: SourceFileId,
) -> Result<Vec<Box<dyn Transformer + 'transformers>>, TransformError> {
    let mut activity = H2ActivityCanary::h2_5g_profile();
    get_script_transformers_with_optional_host(
        options,
        resolver,
        Some((host, source)),
        &mut activity,
    )
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
    let target = options.emit_script_target();
    if target < ScriptTarget::ES2015 || target > ScriptTarget::ES_NEXT {
        return Err(TransformError::UnsupportedCompilerOption {
            option: "target",
            detail: "H2.5g admits ES2015 through ESNext; older targets belong to later target-ladder slices",
        });
    }
    if !matches!(
        options.emit_module_kind(),
        MODULE_NONE
            | MODULE_PRESERVE
            | MODULE_ES_NEXT
            | MODULE_COMMON_JS
            | MODULE_AMD
            | MODULE_UMD
            | MODULE_SYSTEM
            | MODULE_ES2015
            | MODULE_ES2020
            | MODULE_ES2022
            | MODULE_NODE16
            | MODULE_NODE18
            | MODULE_NODE20
            | MODULE_NODE_NEXT
    ) {
        return Err(TransformError::UnsupportedCompilerOption {
            option: "module",
            detail: "the current transformer list requires None, Preserve, ES2015, ES2020, ES2022, ESNext, CommonJS, AMD, UMD, System, Node16, Node18, Node20, or NodeNext",
        });
    }
    if let Some((host, source)) = host {
        let source_record = host.source_file(source);
        let source_name = source_record
            .map(|record| record.path().to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let owns_node_format = matches!(
            options.emit_module_kind(),
            MODULE_NODE16 | MODULE_NODE18 | MODULE_NODE20 | MODULE_NODE_NEXT
        ) || options.rewrite_relative_import_extensions == Some(true)
            || source_name.ends_with(".mts")
            || source_name.ends_with(".cts")
            || source_record
                .and_then(crate::EmitSource::syntax)
                .is_some_and(source_contains_import_attributes);
        if owns_node_format {
            activity.observe_runtime_slice(H2RuntimeSlice::H2_1e);
        }
        if source_record
            .and_then(crate::EmitSource::syntax)
            .is_some_and(source_contains_runtime_enum)
        {
            activity.observe_runtime_slice(H2RuntimeSlice::H2_2a);
        }
        if source_record
            .and_then(crate::EmitSource::syntax)
            .is_some_and(source_contains_runtime_namespace)
        {
            activity.observe_runtime_slice(H2RuntimeSlice::H2_2b);
        }
        if source_record
            .and_then(crate::EmitSource::syntax)
            .is_some_and(source_contains_parameter_property)
        {
            activity.observe_runtime_slice(H2RuntimeSlice::H2_2c);
        }
        if source_record
            .and_then(crate::EmitSource::syntax)
            .is_some_and(source_contains_import_or_export_equals)
        {
            activity.observe_runtime_slice(H2RuntimeSlice::H2_2d);
        }
        if options.experimental_decorators
            && source_record
                .and_then(crate::EmitSource::syntax)
                .is_some_and(source_contains_decorator)
        {
            activity.observe_runtime_slice(H2RuntimeSlice::H2_4a);
        }
        if !options.use_define_for_class_fields_effective()
            || (!options.experimental_decorators
                && source_record
                    .and_then(crate::EmitSource::syntax)
                    .is_some_and(source_contains_decorator))
        {
            activity.observe_runtime_slice(H2RuntimeSlice::H2_4b);
        }
        if target < ScriptTarget::ES_NEXT {
            activity.observe_runtime_slice(H2RuntimeSlice::H2_5a);
        }
        if target < ScriptTarget::ES2021 {
            activity.observe_runtime_slice(H2RuntimeSlice::H2_5b);
        }
        if target < ScriptTarget::ES2020 {
            activity.observe_runtime_slice(H2RuntimeSlice::H2_5c);
        }
        if target < ScriptTarget::ES2019 {
            activity.observe_runtime_slice(H2RuntimeSlice::H2_5d);
        }
        if target < ScriptTarget::ES2018 {
            activity.observe_runtime_slice(H2RuntimeSlice::H2_5e);
        }
        if target < ScriptTarget::ES2017 {
            activity.observe_runtime_slice(H2RuntimeSlice::H2_5f);
        }
        if target < ScriptTarget::ES2016 {
            activity.observe_runtime_slice(H2RuntimeSlice::H2_5g);
        }
    }

    activity.construct_script_transformer_list();
    activity.construct_transform_typescript();
    let transform_typescript = transform_type_script(options, resolver);
    let transform_legacy_decorators = options
        .experimental_decorators
        .then(|| legacy_decorators::transform_legacy_decorators(options, resolver));
    let transform_jsx =
        matches!(options.jsx, Some(2 | 4 | 5)).then(|| jsx::transform_jsx(options, resolver));
    let transform_es_next =
        (target < ScriptTarget::ES_NEXT).then(|| es_next::transform_es_next(options));
    let transform_standard_decorators = (!options.experimental_decorators
        && (target < ScriptTarget::ES_NEXT || !options.use_define_for_class_fields_effective()))
    .then(|| standard_decorators::transform_standard_decorators(options));
    activity.construct_transform_class_fields();
    let transform_class_fields = transform_class_fields(options, resolver);
    let transform_es2021 =
        (target < ScriptTarget::ES2021).then(|| es2021::transform_es2021(options));
    let transform_es2020 =
        (target < ScriptTarget::ES2020).then(|| es2021::transform_es2020(options));
    let transform_es2019 =
        (target < ScriptTarget::ES2019).then(|| es2021::transform_es2019(options));
    let transform_es2018 =
        (target < ScriptTarget::ES2018).then(|| es2018::transform_es2018(options));
    let transform_es2017 =
        (target < ScriptTarget::ES2017).then(|| es2017::transform_es2017(options, resolver));
    let transform_es2016 =
        (target < ScriptTarget::ES2016).then(|| es2021::transform_es2016(options));
    let module_transformer = if options.emit_module_kind() == MODULE_PRESERVE {
        activity.construct_transform_ecmascript_module();
        transform_ecmascript_module(options)
    } else if options.emit_module_kind() == MODULE_SYSTEM {
        activity.observe_runtime_slice(H2RuntimeSlice::H2_1d);
        system::transform_system_module(options, resolver)
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
    let mut transformers = vec![transform_typescript];
    if let Some(transform_legacy_decorators) = transform_legacy_decorators {
        transformers.push(transform_legacy_decorators);
    }
    if let Some(transform_jsx) = transform_jsx {
        transformers.push(transform_jsx);
    }
    if let Some(transform_es_next) = transform_es_next {
        transformers.push(transform_es_next);
    }
    if let Some(transform_standard_decorators) = transform_standard_decorators {
        transformers.push(transform_standard_decorators);
    }
    transformers.push(transform_class_fields);
    if let Some(transform_es2021) = transform_es2021 {
        transformers.push(transform_es2021);
    }
    if let Some(transform_es2020) = transform_es2020 {
        transformers.push(transform_es2020);
    }
    if let Some(transform_es2019) = transform_es2019 {
        transformers.push(transform_es2019);
    }
    if let Some(transform_es2018) = transform_es2018 {
        transformers.push(transform_es2018);
    }
    if let Some(transform_es2017) = transform_es2017 {
        transformers.push(transform_es2017);
    }
    if let Some(transform_es2016) = transform_es2016 {
        transformers.push(transform_es2016);
    }
    transformers.push(module_transformer);
    Ok(transformers)
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
        legacy_decorators: options.experimental_decorators,
        always_strict: options.always_strict_effective(),
        downlevels_es2018: options.emit_script_target() < ScriptTarget::ES2018,
        downlevel_iteration: options.downlevel_iteration == Some(true),
        module_kind: options.emit_module_kind(),
        preserve_const_enums: options.should_preserve_const_enums(),
        isolated_modules: options.isolated_modules == Some(true)
            || options.verbatim_module_syntax == Some(true),
        remove_comments: options.remove_comments == Some(true),
        allow_jsx: matches!(options.jsx, None | Some(1..=5)),
        allow_legacy_decorators: true,
        // transformTypeScript always projects parameter properties into
        // synthetic class members. transformClassFields then owns their
        // ordering relative to private brands and ordinary field
        // initializers, including assignment-mode output where it replaces
        // the earlier constructor statements.
        project_parameter_properties_for_class_fields: true,
        namespace_container_names: BTreeMap::new(),
        enum_container_names: BTreeMap::new(),
        active_namespace_emit_depth: 0,
        active_enum_emit_depth: 0,
    })
}

/// tsc-port: transformClassFields @6.0.3
/// tsc-hash: 65cacc85f81402ff4468cf65c7636dbd5a0ce9eb6c3248f060aa5193c3af8304
/// tsc-span: _tsc.js:95852-98038
pub fn transform_class_fields<'resolver>(
    options: &CompilerOptions,
    resolver: &'resolver dyn EmitResolver,
) -> Box<dyn Transformer + 'resolver> {
    class_fields::transform_class_fields(options, resolver)
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
    /// Selects the transformTypeScript -> transformLegacyDecorators handoff.
    /// This is semantic mode, unlike `allow_legacy_decorators`, which only
    /// records that decorator syntax is supported by the complete pipeline.
    legacy_decorators: bool,
    always_strict: bool,
    downlevels_es2018: bool,
    downlevel_iteration: bool,
    module_kind: i32,
    preserve_const_enums: bool,
    isolated_modules: bool,
    remove_comments: bool,
    allow_jsx: bool,
    allow_legacy_decorators: bool,
    project_parameter_properties_for_class_fields: bool,
    namespace_container_names: BTreeMap<TransformNode, Box<str>>,
    enum_container_names: BTreeMap<TransformNode, Box<str>>,
    active_namespace_emit_depth: usize,
    active_enum_emit_depth: usize,
}

impl Transformer for TypeScriptTransformer<'_> {
    fn name(&self) -> &'static str {
        "transformTypeScript"
    }

    fn initialize(&mut self, context: &mut TransformationContext) -> Result<(), TransformError> {
        for kind in [
            SyntaxKind::Identifier,
            SyntaxKind::ShorthandPropertyAssignment,
            SyntaxKind::PropertyAccessExpression,
            SyntaxKind::ElementAccessExpression,
        ] {
            context.enable_substitution(kind)?;
        }
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
            // tsc's TS 6 alwaysStrict default also applies to Preserve-mode
            // scripts. Files already classified as external ESM retain their
            // module strictness without a synthetic directive.
            self.always_strict
                && !(syntax.external_module_indicator.is_some() && self.module_kind >= 5)
                && !syntax.file_name.to_ascii_lowercase().ends_with(".json")
        };
        preflight_source(
            context.arena(),
            source,
            self.module_kind == MODULE_SYSTEM,
            self.allow_jsx,
            self.allow_legacy_decorators,
            self.downlevels_es2018,
        )?;
        initialize_transform_flags(context.arena_mut()?, source)?;
        let root_node = context.arena().root(source)?;
        let mut visitor = TypeScriptVisitor::new(
            context,
            source,
            self.resolver,
            self.legacy_decorators,
            self.preserve_const_enums,
            self.project_parameter_properties_for_class_fields,
            self.downlevel_iteration,
        );
        let transformed = visitor.visit_typescript(root_node.node())?.ok_or(
            TransformError::RequiredChildRemoved {
                parent: SyntaxKind::SourceFile,
                field: "root",
            },
        )?;
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
        let namespace_container_names = std::mem::take(&mut visitor.namespace_container_names);
        self.namespace_container_names.extend(
            namespace_container_names
                .into_iter()
                .map(|(declaration, name)| (TransformNode::new(source, declaration), name)),
        );
        let enum_container_names = std::mem::take(&mut visitor.enum_container_names);
        self.enum_container_names.extend(
            enum_container_names
                .into_iter()
                .map(|(declaration, name)| (TransformNode::new(source, declaration), name)),
        );
        Ok(TransformRoot::SourceFile(source))
    }

    fn substitute_node(
        &mut self,
        context: &mut TransformationContext,
        hint: EmitHint,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if matches!(
            context.arena().node(node)?.kind,
            SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
        ) {
            if let Some(substitute) = self.try_substitute_constant_value(context, node)? {
                return Ok(substitute);
            }
        }
        if self.active_namespace_emit_depth == 0 && self.active_enum_emit_depth == 0 {
            return Ok(node);
        }
        match context.arena().node(node)?.data.clone() {
            NodeData::Identifier(_) if hint == EmitHint::Expression => self
                .try_substitute_exported_name(context, node)
                .map(|substitute| substitute.unwrap_or(node)),
            NodeData::ShorthandPropertyAssignment(data)
                if self.active_namespace_emit_depth != 0 =>
            {
                self.substitute_namespace_shorthand(context, node, data)
            }
            _ => Ok(node),
        }
    }

    fn before_emit_node(
        &mut self,
        context: &TransformationContext,
        _hint: EmitHint,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        let original = context.arena().get_original_node(node);
        if self.namespace_container_names.contains_key(&original) {
            self.active_namespace_emit_depth += 1;
        }
        if self.enum_container_names.contains_key(&original) {
            self.active_enum_emit_depth += 1;
        }
        Ok(())
    }

    fn after_emit_node(
        &mut self,
        context: &TransformationContext,
        _hint: EmitHint,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        let original = context.arena().get_original_node(node);
        if self.namespace_container_names.contains_key(&original) {
            self.active_namespace_emit_depth = self
                .active_namespace_emit_depth
                .checked_sub(1)
                .expect("namespace emit notifications are balanced");
        }
        if self.enum_container_names.contains_key(&original) {
            self.active_enum_emit_depth = self
                .active_enum_emit_depth
                .checked_sub(1)
                .expect("enum emit notifications are balanced");
        }
        Ok(())
    }

    fn dispose(&mut self) {
        self.namespace_container_names.clear();
        self.enum_container_names.clear();
        self.active_namespace_emit_depth = 0;
        self.active_enum_emit_depth = 0;
    }
}

impl TypeScriptTransformer<'_> {
    /// tsc-port: substituteConstantValue @6.0.3
    /// tsc-hash: bf287a3da8a7c335cc85c24c792272896e41ad2f81fd23a6dbb1098c9c450011
    /// tsc-span: _tsc.js:95827-95839
    ///
    /// Const-enum folding is an emit substitution, not a TypeScript-tree
    /// rewrite. Keeping it here lets the ordinary visitor retain its
    /// transform-flag gate while every access expression remains eligible at
    /// print time.
    fn try_substitute_constant_value(
        &self,
        context: &mut TransformationContext,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        if self.isolated_modules {
            return Ok(None);
        }
        let original = context.arena().get_original_node(node);
        if !matches!(
            context.arena().node(original)?.kind,
            SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
        ) {
            return Ok(None);
        }
        let Some(resolver_node) = context.arena().parse_tree_resolver_node(node)? else {
            return Ok(None);
        };
        let Some(value) = self.resolver.get_constant_value(resolver_node)? else {
            return Ok(None);
        };
        // `setConstantValue` belongs to the access node, not its replacement.
        // The parent property-access printer consults this metadata after the
        // child has been substituted to decide whether an integer needs a
        // second dot before the following property name.
        context
            .arena_mut()?
            .metadata_mut(node)
            .set_constant_value(value.clone());

        let trailing_comment = if self.remove_comments {
            None
        } else {
            let source = context.arena().source(original.source())?.syntax();
            let record = context.arena().node(original)?;
            let text = if record.end == u32::MAX || record.pos > record.end {
                String::new()
            } else {
                let start = skip_trivia(source.text(), record.pos as usize);
                safe_multi_line_comment(
                    source
                        .text()
                        .get(start..record.end as usize)
                        .unwrap_or_default(),
                )
            };
            Some(text)
        };

        let source = node.source();
        let mut javascript_string = None;
        let substitute = {
            let mut factory = context.substitution_factory()?;
            let substitute = match &value {
                EmitConstantValue::String(value) => {
                    javascript_string = Some(value.clone());
                    factory.create_node(
                        source,
                        NodeData::StringLiteral(tsc_syntax::nodes::StringLiteralData {
                            text: String::from_utf16_lossy(value.code_units()),
                            has_extended_unicode_escape: None,
                        }),
                        TransformFlags::NONE,
                    )?
                }
                EmitConstantValue::Number(value) => {
                    let value = value.as_f64();
                    if value.is_nan() {
                        factory.create_node(
                            source,
                            NodeData::Identifier(tsc_syntax::nodes::IdentifierData {
                                escaped_text: "NaN".to_owned(),
                                text: "NaN".to_owned(),
                            }),
                            TransformFlags::NONE,
                        )?
                    } else if value.is_infinite() {
                        let infinity = factory.create_node(
                            source,
                            NodeData::Identifier(tsc_syntax::nodes::IdentifierData {
                                escaped_text: "Infinity".to_owned(),
                                text: "Infinity".to_owned(),
                            }),
                            TransformFlags::NONE,
                        )?;
                        if value.is_sign_negative() {
                            factory.create_node(
                                source,
                                NodeData::PrefixUnaryExpression(
                                    tsc_syntax::nodes::PrefixUnaryExpressionData {
                                        operator: SyntaxKind::MinusToken,
                                        operand: Some(infinity.node()),
                                    },
                                ),
                                TransformFlags::NONE,
                            )?
                        } else {
                            infinity
                        }
                    } else {
                        let magnitude = if value < 0.0 { -value } else { value };
                        let literal = factory.create_node(
                            source,
                            NodeData::NumericLiteral(tsc_syntax::nodes::NumericLiteralData {
                                text: tsc_types::js_number_to_string(magnitude),
                            }),
                            TransformFlags::NONE,
                        )?;
                        if value < 0.0 {
                            factory.create_node(
                                source,
                                NodeData::PrefixUnaryExpression(
                                    tsc_syntax::nodes::PrefixUnaryExpressionData {
                                        operator: SyntaxKind::MinusToken,
                                        operand: Some(literal.node()),
                                    },
                                ),
                                TransformFlags::NONE,
                            )?
                        } else {
                            literal
                        }
                    }
                }
                EmitConstantValue::Boolean(value) => factory.create_token(
                    source,
                    if *value {
                        SyntaxKind::TrueKeyword
                    } else {
                        SyntaxKind::FalseKeyword
                    },
                    TransformFlags::NONE,
                )?,
            };
            factory.set_text_range(substitute, node)?;
            substitute
        };
        context
            .arena_mut()?
            .set_original_node(substitute, Some(original))?;
        if let Some(value) = javascript_string {
            context
                .arena_mut()?
                .metadata_mut(substitute)
                .set_javascript_string_value(value);
        }
        if let Some(text) = trailing_comment {
            context
                .arena_mut()?
                .metadata_mut(substitute)
                .add_trailing_comment(SyntheticComment::new(
                    SyntheticCommentKind::MultiLine,
                    format!(" {text} "),
                    false,
                    false,
                ));
        }
        Ok(Some(substitute))
    }

    fn try_substitute_exported_name(
        &self,
        context: &mut TransformationContext,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        if context
            .arena()
            .metadata(node)
            .is_some_and(|metadata| metadata.flags().contains(EmitFlags::LOCAL_NAME))
        {
            return Ok(None);
        }
        let container = if let Some(container) = context
            .arena()
            .metadata(node)
            .and_then(|metadata| metadata.referenced_export_container())
        {
            container
        } else {
            let Some(resolver_node) = context.arena().parse_tree_resolver_node(node)? else {
                return Ok(None);
            };
            let Some(container) = self.resolver.get_referenced_export_container(
                resolver_node,
                EmitExportContainerMode::Reference,
            )?
            else {
                return Ok(None);
            };
            TransformNode::new(node.source(), container.node())
        };
        let Some(container_name) = (self.active_namespace_emit_depth != 0)
            .then(|| self.namespace_container_names.get(&container))
            .flatten()
            .or_else(|| {
                (self.active_enum_emit_depth != 0)
                    .then(|| self.enum_container_names.get(&container))
                    .flatten()
            })
            .cloned()
        else {
            return Ok(None);
        };
        let NodeData::Identifier(identifier) = context.arena().node(node)?.data.clone() else {
            return Ok(None);
        };
        let source = node.source();
        let (container, name, access) = {
            let mut factory = context.substitution_factory()?;
            let container = factory.create_node(
                source,
                NodeData::Identifier(tsc_syntax::nodes::IdentifierData {
                    escaped_text: container_name.to_string(),
                    text: container_name.to_string(),
                }),
                TransformFlags::NONE,
            )?;
            let name = factory.create_node(
                source,
                NodeData::Identifier(identifier),
                TransformFlags::NONE,
            )?;
            let access = factory.create_node(
                source,
                NodeData::PropertyAccessExpression(
                    tsc_syntax::nodes::PropertyAccessExpressionData {
                        name: Some(name.node()),
                        expression: Some(container.node()),
                        question_dot_token: None,
                    },
                ),
                TransformFlags::NONE,
            )?;
            factory.set_text_range(access, node)?;
            (container, name, access)
        };
        context.arena_mut()?.set_original_node(access, Some(node))?;
        for generated in [container, name] {
            context
                .arena_mut()?
                .metadata_mut(generated)
                .add_flags(EmitFlags::NO_SUBSTITUTION);
        }
        Ok(Some(access))
    }

    fn substitute_namespace_shorthand(
        &self,
        context: &mut TransformationContext,
        original: TransformNode,
        data: tsc_syntax::nodes::ShorthandPropertyAssignmentData,
    ) -> Result<TransformNode, TransformError> {
        let Some(name) = data
            .name
            .and_then(|name| context.arena().node_ref(original.source(), name))
        else {
            return Ok(original);
        };
        let Some(exported_name) = self.try_substitute_exported_name(context, name)? else {
            return Ok(original);
        };
        let initializer = if let Some(default_value) = data.object_assignment_initializer {
            let default_value = context
                .arena()
                .node_ref(original.source(), default_value)
                .ok_or_else(|| {
                    TransformError::UnknownNode(TransformNode::new(
                        original.source(),
                        default_value,
                    ))
                })?;
            let mut factory = context.substitution_factory()?;
            let operator = factory.create_token(
                original.source(),
                SyntaxKind::EqualsToken,
                TransformFlags::NONE,
            )?;
            factory.create_node(
                original.source(),
                NodeData::BinaryExpression(tsc_syntax::nodes::BinaryExpressionData {
                    left: Some(exported_name.node()),
                    operator_token: Some(operator.node()),
                    right: Some(default_value.node()),
                }),
                TransformFlags::NONE,
            )?
        } else {
            exported_name
        };
        let property = {
            let mut factory = context.substitution_factory()?;
            let property = factory.create_node(
                original.source(),
                NodeData::PropertyAssignment(tsc_syntax::nodes::PropertyAssignmentData {
                    name: Some(name.node()),
                    initializer: Some(initializer.node()),
                    modifiers: None,
                    question_token: None,
                    exclamation_token: None,
                }),
                TransformFlags::NONE,
            )?;
            factory.set_text_range(property, original)?;
            property
        };
        context
            .arena_mut()?
            .set_original_node(property, Some(original))?;
        Ok(property)
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

struct EcmaScriptModuleTransformer {
    module_kind: i32,
    rewrite_relative_import_extensions: bool,
}

impl Transformer for EcmaScriptModuleTransformer {
    fn name(&self) -> &'static str {
        "transformECMAScriptModule"
    }

    fn initialize(&mut self, context: &mut TransformationContext) -> Result<(), TransformError> {
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
        if context.arena().source(source)?.syntax().is_declaration_file {
            return Ok(TransformRoot::SourceFile(source));
        }
        let was_external = context
            .arena()
            .source(source)?
            .syntax()
            .external_module_indicator
            .is_some();
        if was_external && self.rewrite_relative_import_extensions {
            let current_root = context.arena().root(source)?;
            let mut visitor = RelativeModuleSpecifierVisitor::new(context, source);
            let rewritten = visitor.visit(current_root.node())?;
            visitor
                .context
                .arena_mut()?
                .replace_root(source, rewritten)?;
        }
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
        if was_external {
            let current_root = context.arena().root(source)?;
            let mut visitor = EcmaScriptModuleEqualsVisitor::new(context, source, self.module_kind);
            let rewritten = visitor.transform_source_file(current_root)?;
            visitor
                .context
                .arena_mut()?
                .replace_root(source, rewritten)?;
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
        _context: &mut TransformationContext,
        _hint: EmitHint,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        Ok(node)
    }
}

struct EcmaScriptModuleEqualsVisitor<'context> {
    context: &'context mut TransformationContext,
    source: TransformSourceId,
    module_kind: i32,
    used_names: BTreeSet<String>,
    create_require_name: Option<String>,
    require_name: Option<String>,
}

impl<'context> EcmaScriptModuleEqualsVisitor<'context> {
    fn new(
        context: &'context mut TransformationContext,
        source: TransformSourceId,
        module_kind: i32,
    ) -> Self {
        let used_names = system::collect_identifier_texts(context.arena(), source);
        Self {
            context,
            source,
            module_kind,
            used_names,
            create_require_name: None,
            require_name: None,
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
                });
            }
        };
        let input = node_array_nodes(self.context.arena(), self.source, original_array)?;
        let mut output = Vec::with_capacity(input.len() + 2);
        for statement in input {
            match self.context.arena().node(statement)?.data.clone() {
                NodeData::ImportEqualsDeclaration(data) => {
                    output.extend(self.transform_import_equals(statement, data)?);
                }
                NodeData::ExportAssignment(data) if data.is_export_equals == Some(true) => {
                    if self.module_kind == MODULE_PRESERVE {
                        output.push(self.transform_preserve_export_equals(statement, data)?);
                    }
                }
                _ => output.push(statement),
            }
        }
        if self.create_require_name.is_some() {
            let helpers = self.create_require_helpers()?;
            let offset = output
                .iter()
                .take_while(|statement| {
                    is_prologue_statement(self.context.arena(), **statement).unwrap_or(false)
                })
                .count();
            output.splice(offset..offset, helpers);
        }
        let statements = if let Some(original) =
            original_array.and_then(|array| self.context.arena().node_array_ref(self.source, array))
        {
            self.context
                .factory()?
                .update_node_array(original, output)?
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

    /// tsc-port: visitImportEqualsDeclaration/appendExportsOfImportEqualsDeclaration @6.0.3
    /// tsc-hash: f9985c8750f1c4ce6ded0360679e5de13d68aea797aa62d2df69f05797defcf9
    /// tsc-span: _tsc.js:113569-113621
    fn transform_import_equals(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::ImportEqualsDeclarationData,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let module_reference = data
            .module_reference
            .and_then(|id| self.context.arena().node_ref(self.source, id))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ImportEqualsDeclaration,
                field: "module_reference",
            })?;
        if self.context.arena().node(module_reference)?.kind != SyntaxKind::ExternalModuleReference
        {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ImportEqualsDeclaration,
                field: "external module_reference",
            });
        }
        if self.module_kind < MODULE_NODE16 && self.module_kind != MODULE_PRESERVE {
            return Ok(Vec::new());
        }
        let name = data
            .name
            .and_then(|id| self.context.arena().node_ref(self.source, id))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ImportEqualsDeclaration,
                field: "name",
            })?;
        let module_specifier = match &self.context.arena().node(module_reference)?.data {
            NodeData::ExternalModuleReference(data) => data
                .expression
                .and_then(|id| self.context.arena().node_ref(self.source, id)),
            _ => None,
        }
        .ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::ExternalModuleReference,
            field: "expression",
        })?;
        let require = if self.module_kind == MODULE_PRESERVE {
            self.create_identifier("require")?
        } else {
            self.ensure_require_names();
            let require_name = self
                .require_name
                .clone()
                .expect("require helper name initialized");
            self.create_identifier(&require_name)?
        };
        let call = self.create_call(require, vec![module_specifier])?;
        let statement = self.create_variable_statement(name, call, NodeFlags::CONST)?;
        self.set_original_and_range(statement, original)?;
        let mut output = vec![statement];
        if has_modifier(
            self.context.arena(),
            self.source,
            data.modifiers,
            SyntaxKind::ExportKeyword,
        )? {
            let name = identifier_or_literal_text(self.context.arena(), name)?;
            output.push(self.create_named_export(&name)?);
        }
        Ok(output)
    }

    /// tsc-port: visitExportAssignment @6.0.3
    /// tsc-hash: 4078d37c3316121a91a2bb7c8fcbd1dbf41fe8d218eba426cc06c49c602a78ff
    /// tsc-span: _tsc.js:113622-113642
    fn transform_preserve_export_equals(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::ExportAssignmentData,
    ) -> Result<TransformNode, TransformError> {
        let expression = data
            .expression
            .and_then(|id| self.context.arena().node_ref(self.source, id))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ExportAssignment,
                field: "expression",
            })?;
        let module = self.create_identifier("module")?;
        let target = self.create_property_access(module, "exports")?;
        let assignment = self.create_assignment(target, expression)?;
        let statement = self.create_expression_statement(assignment)?;
        self.context
            .arena_mut()?
            .set_original_node(statement, Some(original))?;
        self.context
            .arena_mut()?
            .metadata_mut(statement)
            .add_flags(EmitFlags::NO_COMMENTS);
        Ok(statement)
    }

    fn create_require_helpers(&mut self) -> Result<Vec<TransformNode>, TransformError> {
        let create_require_name = self
            .create_require_name
            .clone()
            .expect("createRequire helper name initialized");
        let require_name = self
            .require_name
            .clone()
            .expect("require helper name initialized");
        let property = self.create_identifier("createRequire")?;
        let local = self.create_identifier(&create_require_name)?;
        let specifier = self.context.factory()?.create_node(
            self.source,
            NodeData::ImportSpecifier(tsc_syntax::nodes::ImportSpecifierData {
                name: Some(local.node()),
                property_name: Some(property.node()),
                is_type_only: false,
            }),
            TransformFlags::NONE,
        )?;
        let elements = self
            .context
            .factory()?
            .create_node_array(self.source, vec![specifier])?;
        let named = self.context.factory()?.create_node(
            self.source,
            NodeData::NamedImports(tsc_syntax::nodes::NamedImportsData {
                elements: Some(elements.array()),
            }),
            TransformFlags::NONE,
        )?;
        let clause = self.context.factory()?.create_node(
            self.source,
            NodeData::ImportClause(tsc_syntax::nodes::ImportClauseData {
                name: None,
                is_type_only: false,
                phase_modifier: None,
                named_bindings: Some(named.node()),
            }),
            TransformFlags::NONE,
        )?;
        let module = self.create_string_literal("module")?;
        let import = self.context.factory()?.create_node(
            self.source,
            NodeData::ImportDeclaration(tsc_syntax::nodes::ImportDeclarationData {
                modifiers: None,
                import_clause: Some(clause.node()),
                module_specifier: Some(module.node()),
                attributes: None,
            }),
            TransformFlags::NONE,
        )?;

        let create_require = self.create_identifier(&create_require_name)?;
        // The printer treats synthetic identifier text as emitted text. This
        // keeps the helper expression atomic while the parser-owned
        // MetaProperty node remains available for source syntax.
        let import_meta_url = self.create_identifier("import.meta.url")?;
        let initializer = self.create_call(create_require, vec![import_meta_url])?;
        let name = self.create_identifier(&require_name)?;
        let declaration = self.create_variable_statement(name, initializer, NodeFlags::CONST)?;
        Ok(vec![import, declaration])
    }

    fn create_named_export(&mut self, name: &str) -> Result<TransformNode, TransformError> {
        let name = self.create_identifier(name)?;
        let specifier = self.context.factory()?.create_node(
            self.source,
            NodeData::ExportSpecifier(tsc_syntax::nodes::ExportSpecifierData {
                name: Some(name.node()),
                property_name: None,
                is_type_only: false,
            }),
            TransformFlags::NONE,
        )?;
        let elements = self
            .context
            .factory()?
            .create_node_array(self.source, vec![specifier])?;
        let named = self.context.factory()?.create_node(
            self.source,
            NodeData::NamedExports(tsc_syntax::nodes::NamedExportsData {
                elements: Some(elements.array()),
            }),
            TransformFlags::NONE,
        )?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ExportDeclaration(tsc_syntax::nodes::ExportDeclarationData {
                modifiers: None,
                is_type_only: false,
                export_clause: Some(named.node()),
                module_specifier: None,
                attributes: None,
            }),
            TransformFlags::NONE,
        )
    }

    fn create_variable_statement(
        &mut self,
        name: TransformNode,
        initializer: TransformNode,
        flags: NodeFlags,
    ) -> Result<TransformNode, TransformError> {
        let declaration = self.context.factory()?.create_node(
            self.source,
            NodeData::VariableDeclaration(tsc_syntax::nodes::VariableDeclarationData {
                name: Some(name.node()),
                exclamation_token: None,
                r#type: None,
                initializer: Some(initializer.node()),
            }),
            TransformFlags::NONE,
        )?;
        let declarations = self
            .context
            .factory()?
            .create_node_array(self.source, vec![declaration])?;
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

    fn ensure_require_names(&mut self) {
        if self.create_require_name.is_none() {
            let create_require = self.fresh_name("_createRequire");
            let require = self.fresh_name("__require");
            self.create_require_name = Some(create_require);
            self.require_name = Some(require);
        }
    }

    fn fresh_name(&mut self, base: &str) -> String {
        if self.used_names.insert(base.to_owned()) {
            return base.to_owned();
        }
        let mut ordinal = 1usize;
        loop {
            let candidate = format!("{base}_{ordinal}");
            if self.used_names.insert(candidate.clone()) {
                return candidate;
            }
            ordinal += 1;
        }
    }

    fn set_original_and_range(
        &mut self,
        node: TransformNode,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.set_text_range(node, original)?;
        self.context
            .arena_mut()?
            .set_original_node(node, Some(original))?;
        Ok(node)
    }
}

struct RelativeModuleSpecifierVisitor<'context> {
    context: &'context mut TransformationContext,
    source: TransformSourceId,
    nodes: BTreeMap<NodeId, NodeId>,
    arrays: BTreeMap<NodeArrayId, NodeArrayId>,
}

impl<'context> RelativeModuleSpecifierVisitor<'context> {
    fn new(context: &'context mut TransformationContext, source: TransformSourceId) -> Self {
        Self {
            context,
            source,
            nodes: BTreeMap::new(),
            arrays: BTreeMap::new(),
        }
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
        let mut data = self.context.arena().node(original)?.data.clone();
        if matches!(data, NodeData::Token) {
            self.nodes.insert(id, original.node());
            return Ok(original);
        }
        try_visit_each_child(&mut data, self)?;
        match &mut data {
            NodeData::ImportDeclaration(declaration) => {
                declaration.module_specifier = declaration
                    .module_specifier
                    .map(|specifier| self.rewrite_literal(specifier).map(TransformNode::node))
                    .transpose()?;
            }
            NodeData::ExportDeclaration(declaration) => {
                declaration.module_specifier = declaration
                    .module_specifier
                    .map(|specifier| self.rewrite_literal(specifier).map(TransformNode::node))
                    .transpose()?;
            }
            NodeData::CallExpression(call) if self.is_dynamic_import(call.expression) => {
                if let Some(arguments) = call
                    .arguments
                    .and_then(|array| self.context.arena().node_array_ref(self.source, array))
                {
                    let original_array = arguments;
                    let mut arguments = self
                        .context
                        .arena()
                        .node_array(original_array)?
                        .nodes
                        .iter()
                        .filter_map(|id| self.context.arena().node_ref(self.source, *id))
                        .collect::<Vec<_>>();
                    if let Some(first) = arguments.first_mut() {
                        *first = self.rewrite_dynamic_argument(*first)?;
                    }
                    call.arguments = Some(
                        self.context
                            .factory()?
                            .update_node_array(original_array, arguments)?
                            .array(),
                    );
                }
            }
            _ => {}
        }
        let flags = flags_after_update(self.context.arena(), original, &data)?;
        let updated = self.context.factory()?.update_node(original, data, flags)?;
        self.nodes.insert(id, updated.node());
        Ok(updated)
    }

    fn rewrite_literal(&mut self, id: NodeId) -> Result<TransformNode, TransformError> {
        let original = self.node(id);
        let NodeData::StringLiteral(literal) = self.context.arena().node(original)?.data.clone()
        else {
            return Ok(original);
        };
        let Some(text) = rewrite_relative_module_specifier(&literal.text) else {
            return Ok(original);
        };
        let flags = self.context.arena().transform_flags(original);
        self.context.factory()?.update_node(
            original,
            NodeData::StringLiteral(tsc_syntax::nodes::StringLiteralData {
                text,
                has_extended_unicode_escape: literal.has_extended_unicode_escape,
            }),
            flags,
        )
    }

    fn rewrite_dynamic_argument(
        &mut self,
        argument: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if matches!(
            self.context.arena().node(argument)?.data,
            NodeData::StringLiteral(_)
        ) {
            return self.rewrite_literal(argument.node());
        }
        self.request_rewrite_helper()?;
        let helper = self.context.factory()?.create_unscoped_helper_identifier(
            self.source,
            EmitHelperName::RewriteRelativeImportExtension,
        )?;
        let arguments = self
            .context
            .factory()?
            .create_node_array(self.source, vec![argument])?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::CallExpression(tsc_syntax::nodes::CallExpressionData {
                expression: Some(helper.node()),
                question_dot_token: None,
                type_arguments: None,
                arguments: Some(arguments.array()),
            }),
            TransformFlags::NONE,
        )
    }

    fn request_rewrite_helper(&mut self) -> Result<(), TransformError> {
        self.context
            .request_emit_helper(crate::EmitHelper::with_text(
                "typescript:rewriteRelativeImportExtensions",
                false,
                REWRITE_RELATIVE_IMPORT_EXTENSIONS_HELPER_TEXT,
                None,
                Vec::new(),
            ))
    }

    fn is_dynamic_import(&self, expression: Option<NodeId>) -> bool {
        expression
            .and_then(|id| self.context.arena().node_ref(self.source, id))
            .is_some_and(|expression| {
                self.context
                    .arena()
                    .node(expression)
                    .is_ok_and(|node| node.kind == SyntaxKind::ImportKeyword)
            })
    }

    const fn node(&self, id: NodeId) -> TransformNode {
        TransformNode::new(self.source, id)
    }
}

impl NodeDataChildVisitor for RelativeModuleSpecifierVisitor<'_> {
    type Error = TransformError;

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.context
            .arena()
            .node(self.node(id))
            .expect("relative-module child belongs to its transform source")
            .kind
    }

    fn visit_node(&mut self, id: NodeId) -> Result<Option<NodeId>, Self::Error> {
        self.visit(id).map(|node| Some(node.node()))
    }

    fn visit_nodes(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, Self::Error> {
        if let Some(mapped) = self.arrays.get(&id) {
            return Ok(Some(*mapped));
        }
        let original = TransformNodeArray::new(self.source, id);
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
        context: &mut TransformationContext,
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
        preserves_native_parameter_defaults: options.emit_script_target() >= ScriptTarget::ES2015,
        rewrite_relative_import_extensions: options
            .rewrite_relative_import_extensions
            .unwrap_or(false),
    })
}

struct CommonJsModuleTransformer<'resolver> {
    resolver: &'resolver dyn EmitResolver,
    module_kind: i32,
    always_strict: bool,
    es_module_interop: bool,
    preserves_native_parameter_defaults: bool,
    rewrite_relative_import_extensions: bool,
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
        let has_import_reference_substitution =
            source_contains_import_reference_substitution(context.arena(), current_root)?;
        let requires_module_rewrite = is_external || has_dynamic_import;
        if !requires_module_rewrite && !has_import_reference_substitution {
            return Ok(TransformRoot::SourceFile(source));
        }
        if requires_module_rewrite
            && (matches!(self.module_kind, MODULE_AMD | MODULE_UMD)
                || self.always_strict
                || is_external)
        {
            let strict_root = context.arena().root(source)?;
            let strict_root = ensure_use_strict_prologue(context, source, strict_root)?;
            context.arena_mut()?.replace_root(source, strict_root)?;
        }

        let current_root = context.arena().root(source)?;
        let info = CommonJsModuleInfo::collect(
            context.arena(),
            source,
            current_root,
            self.resolver,
            self.module_kind,
        )?;
        let mut visitor = CommonJsVisitor::new(
            context,
            source,
            self.resolver,
            CommonJsVisitorOptions {
                module_kind: self.module_kind,
                es_module_interop: self.es_module_interop,
                has_dynamic_import,
                preserves_native_parameter_defaults: self.preserves_native_parameter_defaults,
                rewrite_relative_import_extensions: self.rewrite_relative_import_extensions,
            },
            info,
        );
        let mut updated = visitor.transform_source_file(current_root)?;
        if requires_module_rewrite && matches!(self.module_kind, MODULE_AMD | MODULE_UMD) {
            updated = visitor.wrap_asynchronous_module(updated)?;
        }
        visitor.context.arena_mut()?.replace_root(source, updated)?;
        Ok(TransformRoot::SourceFile(source))
    }

    fn substitute_node(
        &mut self,
        _context: &mut TransformationContext,
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
    runtime_name: Option<Box<str>>,
    namespace_alias: Option<Box<str>>,
    module_specifier: Box<str>,
    helper: ImportHelperKind,
    import_equals_publication: Option<ImportEqualsPublication>,
}

/// Runtime owner selected by `visitTopLevelImportEqualsDeclaration`.
///
/// A plain external import-equals owns a local declaration (or an AMD
/// parameter), while a syntactically exported import-equals owns
/// `exports.name` after its initial publication. Keeping that distinction in
/// the collected plan is important for later `export { local as alias }`
/// statements: the latter must read `exports.name` when no local binding was
/// materialized by the module format.
#[derive(Clone, Debug)]
enum ImportEqualsPublication {
    LocalBinding,
    ExportObject { exported_name: Box<str> },
}

impl ImportEqualsPublication {
    fn exported_name(&self) -> Option<&str> {
        match self {
            Self::LocalBinding => None,
            Self::ExportObject { exported_name } => Some(exported_name),
        }
    }
}

#[derive(Debug)]
struct GeneratedModuleNameAllocator {
    used_names: BTreeSet<String>,
}

impl GeneratedModuleNameAllocator {
    fn new(arena: &TransformArena, source: TransformSourceId) -> Self {
        Self {
            used_names: system::collect_identifier_texts(arena, source),
        }
    }

    fn allocate(&mut self, module_specifier: &str) -> Box<str> {
        let mut base = generated_module_name(module_specifier);
        if !base.ends_with('_') {
            base.push('_');
        }
        for ordinal in 1usize.. {
            let candidate = format!("{base}{ordinal}");
            if self.used_names.insert(candidate.clone()) {
                return candidate.into_boxed_str();
            }
        }
        unreachable!("the generated module-name ordinal space is unbounded")
    }
}

#[derive(Clone, Debug)]
struct ImportBinding {
    generated_name: Box<str>,
    property: Option<Box<str>>,
}

#[derive(Clone, Debug)]
struct ImportReExportPlan {
    exported_name: Box<str>,
    binding: ImportBinding,
    live_binding: bool,
    location: TransformNode,
}

/// Runtime storage selected by module.ts for a syntactically exported scalar
/// variable declaration. `LOCAL_NAME` is an emit-time ownership marker: a
/// transformed declaration carrying it keeps a local binding, while an
/// ordinary parsed declaration writes directly into the module export object.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CommonJsDirectVariableStorage {
    LocalBinding,
    ExportObject,
}

/// Ordered publication plan for one syntactically exported scalar variable.
///
/// The direct export name is declaration behavior and therefore exists even
/// when `collectExternalModuleInfo` did not see the declaration (for example,
/// parser-recovery syntax embedded in an `if`). Collector-owned aliases follow
/// that primary target without being allowed to decide the storage owner.
#[derive(Clone, Debug)]
struct CommonJsDirectVariablePlan {
    local_name: Box<str>,
    storage: CommonJsDirectVariableStorage,
    publication_targets: Vec<Box<str>>,
}

impl CommonJsDirectVariablePlan {
    fn new(
        local_name: &str,
        storage: CommonJsDirectVariableStorage,
        collector_targets: impl IntoIterator<Item = Box<str>>,
    ) -> Self {
        let local_name: Box<str> = local_name.into();
        let mut publication_targets = vec![local_name.clone()];
        for target in collector_targets {
            if !publication_targets
                .iter()
                .any(|existing| existing.as_ref() == target.as_ref())
            {
                publication_targets.push(target);
            }
        }
        Self {
            local_name,
            storage,
            publication_targets,
        }
    }

    fn alias_targets(&self) -> impl Iterator<Item = &str> {
        self.publication_targets
            .iter()
            .skip(1)
            .map(|target| target.as_ref())
    }
}

/// Typed identity admitted by CommonJS's file-level generated-export path.
///
/// This is the Rust boundary for tsc's
/// `isFileLevelReservedGeneratedIdentifier`: a generated binding ID alone is
/// insufficient because ordinary temps and scoped optimistic names must never
/// acquire export-specifier semantics.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct CommonJsFileLevelGeneratedBindingId(GeneratedBindingId);

impl CommonJsFileLevelGeneratedBindingId {
    fn from_identifier(arena: &TransformArena, identifier: TransformNode) -> Option<Self> {
        let metadata = arena.metadata(identifier)?;
        if !metadata.generated_binding_is_file_level_optimistic()
            || !metadata.generated_binding_reserved_in_nested_scopes()
        {
            return None;
        }
        metadata.generated_binding_id().map(Self)
    }
}

/// Export names attached only to file-level optimistic generated bindings.
///
/// Those identifiers deliberately have no checker-owned declaration to
/// resolve. The target transform instead projects one typed generated identity
/// onto both the synthetic local export specifier and every later assignment
/// target. Admission and lookup both re-read node metadata so a non-file-level
/// `GeneratedBindingId` is rejected in release builds as well as debug builds.
#[derive(Debug, Default)]
struct CommonJsFileLevelGeneratedBindingExports {
    by_binding: BTreeMap<CommonJsFileLevelGeneratedBindingId, Vec<Box<str>>>,
}

impl CommonJsFileLevelGeneratedBindingExports {
    fn add_for_identifier(
        &mut self,
        arena: &TransformArena,
        identifier: TransformNode,
        export: &str,
    ) -> bool {
        let Some(binding) = CommonJsFileLevelGeneratedBindingId::from_identifier(arena, identifier)
        else {
            return false;
        };
        let exports = self.by_binding.entry(binding).or_default();
        if !exports.iter().any(|existing| existing.as_ref() == export) {
            exports.push(export.into());
        }
        true
    }

    fn get_for_identifier(
        &self,
        arena: &TransformArena,
        identifier: TransformNode,
    ) -> Option<&[Box<str>]> {
        let binding = CommonJsFileLevelGeneratedBindingId::from_identifier(arena, identifier)?;
        self.by_binding.get(&binding).map(Vec::as_slice)
    }
}

#[derive(Debug)]
struct CommonJsModuleInfo {
    is_external: bool,
    export_equals: Option<NodeId>,
    /// `getGeneratedNameForNode` owns one source-wide module-name namespace.
    /// Keep the allocator after collection as well: JSX can retain an
    /// original import identity for substitution even when transformTypeScript
    /// elided that import from the statement list collected below.
    generated_module_names: GeneratedModuleNameAllocator,
    /// Generated owners for imports which no longer have an ImportPlan.
    /// These names are substitution-only and must never make the import a
    /// runtime dependency again.
    elided_import_runtime_names: BTreeMap<NodeId, Box<str>>,
    /// Stable local identities for anonymous default function/class
    /// declarations. Module formats materialize these identities differently,
    /// but collection and later declaration/export consumers must agree on
    /// the same binding.
    generated_declaration_names: BTreeMap<NodeId, Box<str>>,
    imports: BTreeMap<NodeId, ImportPlan>,
    external_imports: Vec<NodeId>,
    import_bindings: BTreeMap<NodeId, ImportBinding>,
    /// tsc ExternalModuleInfo.exportSpecifiers: only explicit
    /// `export { local as name }` declarations. Keep this separate
    /// from exports_by_local, which also includes a declaration's own
    /// `export` modifier; import declarations must publish only the
    /// former after their runtime binding is initialized.
    export_specifiers_by_local: BTreeMap<Box<str>, Vec<Box<str>>>,
    exports_by_local: BTreeMap<Box<str>, Vec<Box<str>>>,
    file_level_generated_binding_exports: CommonJsFileLevelGeneratedBindingExports,
    exported_bindings: BTreeMap<NodeId, Vec<Box<str>>>,
    export_specifier_locations: BTreeMap<(Box<str>, Box<str>), NodeId>,
    exported_names: Vec<Box<str>>,
    hoisted_function_exports: Vec<(Box<str>, Box<str>)>,
    direct_exported_variable_names: BTreeSet<Box<str>>,
}

impl CommonJsModuleInfo {
    /// `export =` owns the module's final runtime value. tsc still emits the
    /// ordinary export-name preinitializers (and lowers a directly exported
    /// variable into `exports.name` storage), but every `appendExportsOf*`
    /// path becomes inert. Keep that boundary explicit so declaration and
    /// import visitors do not each invent a slightly different exception.
    fn appends_declaration_exports(&self) -> bool {
        self.export_equals.is_none()
    }

    /// Typed publication plan for tsc's `appendExportsOfHoistedDeclaration`.
    /// Direct `export` modifiers are declaration behavior and therefore do
    /// not inherit collector-level export-name uniqueness. Explicit export
    /// specifiers remain collection data and follow the direct publication.
    fn hoisted_declaration_exports(
        &self,
        arena: &TransformArena,
        source: TransformSourceId,
        modifiers: Option<NodeArrayId>,
        local: &str,
    ) -> Result<Vec<Box<str>>, TransformError> {
        if !self.appends_declaration_exports() {
            return Ok(Vec::new());
        }
        let mut exports = Vec::<Box<str>>::new();
        if has_modifier(arena, source, modifiers, SyntaxKind::ExportKeyword)? {
            exports.push(
                if has_modifier(arena, source, modifiers, SyntaxKind::DefaultKeyword)? {
                    "default".into()
                } else {
                    local.into()
                },
            );
        }
        for export in self
            .export_specifiers_by_local
            .get(local)
            .cloned()
            .unwrap_or_default()
        {
            if !exports
                .iter()
                .any(|existing| existing.as_ref() == export.as_ref())
            {
                exports.push(export);
            }
        }
        Ok(exports)
    }

    fn collect(
        arena: &TransformArena,
        source: TransformSourceId,
        root: TransformNode,
        resolver: &dyn EmitResolver,
        module_kind: i32,
    ) -> Result<Self, TransformError> {
        let is_external = arena
            .source(source)?
            .syntax()
            .external_module_indicator
            .is_some();
        let statements = source_file_statement_nodes(arena, source, root)?;
        let mut generated_names = GeneratedModuleNameAllocator::new(arena, source);
        let mut generated_declaration_names = BTreeMap::new();
        // `getDeclarationName` establishes anonymous default-declaration
        // identities while external-module information is collected. Do this
        // before planning import aliases, whose concrete spelling must not
        // steal the declaration's `default_1` slot.
        for statement in &statements {
            let (name, modifiers) = match &arena.node(*statement)?.data {
                NodeData::FunctionDeclaration(data) => (data.name, data.modifiers),
                NodeData::ClassDeclaration(data) => (data.name, data.modifiers),
                _ => continue,
            };
            if name.is_none()
                && has_modifier(arena, source, modifiers, SyntaxKind::ExportKeyword)?
                && has_modifier(arena, source, modifiers, SyntaxKind::DefaultKeyword)?
            {
                generated_declaration_names.insert(
                    arena.get_original_node(*statement).node(),
                    generated_names.allocate("default"),
                );
            }
        }
        let mut info = Self {
            is_external,
            export_equals: None,
            generated_module_names: generated_names,
            elided_import_runtime_names: BTreeMap::new(),
            generated_declaration_names,
            imports: BTreeMap::new(),
            external_imports: Vec::new(),
            import_bindings: BTreeMap::new(),
            export_specifiers_by_local: BTreeMap::new(),
            exports_by_local: BTreeMap::new(),
            file_level_generated_binding_exports: CommonJsFileLevelGeneratedBindingExports::default(
            ),
            exported_bindings: BTreeMap::new(),
            export_specifier_locations: BTreeMap::new(),
            exported_names: Vec::new(),
            hoisted_function_exports: Vec::new(),
            direct_exported_variable_names: BTreeSet::new(),
        };
        // tsc keeps export-name uniqueness and default-declaration ownership
        // separate from exportedNames (the list that receives `void 0`
        // preinitializers). In particular, hoisted functions publish before
        // their declaration without entering exportedNames, while a later
        // duplicate default re-export can still enter that list.
        let mut unique_exports = BTreeSet::<Box<str>>::new();
        let mut has_export_default = false;
        // tsc's exportedFunctions is deliberately independent from
        // exportedBindings. The latter applies export-name uniqueness to
        // substitution, while every syntactically exported function still
        // owns a hoisted publication (including duplicate default exports).
        // Keep insertion order alongside typed identity uniqueness because
        // the order is observable in the module prelude.
        let mut exported_functions = Vec::<NodeId>::new();
        let mut exported_function_set = BTreeSet::<NodeId>::new();

        for statement in &statements {
            let record = arena.node(*statement)?;
            match &record.data {
                NodeData::ExportAssignment(data) if data.is_export_equals == Some(true) => {
                    // tsc's collectExternalModuleInfo records the first
                    // `export =` declaration and leaves later duplicate
                    // recovery nodes inert. Diagnostics still report every
                    // duplicate, while runtime publication retains the first
                    // declaration's expression.
                    if info.export_equals.is_none() {
                        info.export_equals = Some(statement.node());
                    }
                }
                NodeData::ImportDeclaration(data) => {
                    let Some(module_specifier) = data
                        .module_specifier
                        .and_then(|id| arena.node_ref(source, id))
                    else {
                        continue;
                    };
                    let module_text = string_literal_text(arena, module_specifier)?;
                    let mut runtime_name = None;
                    let mut namespace_alias = None;
                    let mut helper = ImportHelperKind::None;
                    if let Some(clause) =
                        data.import_clause.and_then(|id| arena.node_ref(source, id))
                    {
                        if let NodeData::ImportClause(clause_data) = &arena.node(clause)?.data {
                            let has_default = clause_data.name.is_some();
                            let namespace_name = clause_data
                                .named_bindings
                                .and_then(|id| arena.node_ref(source, id))
                                .and_then(|node| match &arena.node(node).ok()?.data {
                                    NodeData::NamespaceImport(namespace) => namespace
                                        .name
                                        .and_then(|id| arena.node_ref(source, id))
                                        .and_then(|name| {
                                            identifier_or_literal_text(arena, name).ok()
                                        })
                                        .map(String::into_boxed_str),
                                    _ => None,
                                });
                            let has_namespace = namespace_name.is_some();
                            let module_binding = if has_namespace && !has_default {
                                namespace_name.clone().expect("namespace name was observed")
                            } else {
                                info.generated_module_names.allocate(module_text)
                            };
                            if has_namespace && has_default {
                                namespace_alias = namespace_name.clone();
                            }
                            runtime_name = Some(module_binding.clone());
                            let (named_count, named_default_count) = clause_data
                                .named_bindings
                                .and_then(|id| arena.node_ref(source, id))
                                .and_then(|bindings| match &arena.node(bindings).ok()?.data {
                                    NodeData::NamedImports(named) => {
                                        let elements =
                                            node_array_nodes(arena, source, named.elements).ok()?;
                                        let default_count = elements
                                            .iter()
                                            .filter(|specifier| {
                                                let Ok(NodeData::ImportSpecifier(data)) = arena
                                                    .node(**specifier)
                                                    .map(|record| &record.data)
                                                else {
                                                    return false;
                                                };
                                                data.property_name
                                                    .or(data.name)
                                                    .and_then(|name| arena.node_ref(source, name))
                                                    .and_then(|name| {
                                                        identifier_or_literal_text(arena, name).ok()
                                                    })
                                                    .is_some_and(|name| name == "default")
                                            })
                                            .count();
                                        Some((elements.len(), default_count))
                                    }
                                    _ => None,
                                })
                                .unwrap_or((0, 0));
                            let needs_star = has_namespace
                                || named_default_count > 0 && named_default_count != named_count
                                || named_count.saturating_sub(named_default_count) > 0
                                    && has_default;
                            let needs_default =
                                !needs_star && (has_default || named_default_count > 0);
                            helper = if needs_star {
                                ImportHelperKind::Star
                            } else if needs_default {
                                ImportHelperKind::Default
                            } else {
                                ImportHelperKind::None
                            };
                            if let Some(_name) = clause_data.name {
                                info.import_bindings.insert(
                                    arena.get_original_node(clause).node(),
                                    ImportBinding {
                                        generated_name: module_binding.clone(),
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
                                        let namespace_name = namespace
                                            .name
                                            .and_then(|id| arena.node_ref(source, id))
                                            .and_then(|name| {
                                                identifier_or_literal_text(arena, name).ok()
                                            })
                                            .map(String::into_boxed_str)
                                            .unwrap_or_else(|| module_binding.clone());
                                        info.import_bindings.insert(
                                            arena.get_original_node(bindings).node(),
                                            ImportBinding {
                                                generated_name: namespace_name,
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
                                                    generated_name: module_binding.clone(),
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
                            runtime_name,
                            namespace_alias,
                            module_specifier: module_text.to_owned().into_boxed_str(),
                            helper,
                            import_equals_publication: None,
                        },
                    );
                    info.external_imports
                        .push(arena.get_original_node(*statement).node());
                }
                NodeData::ImportEqualsDeclaration(data) => {
                    let Some(module_reference) = data
                        .module_reference
                        .and_then(|id| arena.node_ref(source, id))
                    else {
                        continue;
                    };
                    let NodeData::ExternalModuleReference(reference) =
                        &arena.node(module_reference)?.data
                    else {
                        continue;
                    };
                    let Some(module_specifier) = reference
                        .expression
                        .and_then(|id| arena.node_ref(source, id))
                    else {
                        continue;
                    };
                    let module_text = string_literal_text(arena, module_specifier)?;
                    let Some(name) = data
                        .name
                        .and_then(|id| arena.node_ref(source, id))
                        .and_then(|name| identifier_or_literal_text(arena, name).ok())
                    else {
                        continue;
                    };
                    let exported =
                        has_modifier(arena, source, data.modifiers, SyntaxKind::ExportKeyword)?;
                    let key = arena.get_original_node(*statement).node();
                    info.imports.insert(
                        key,
                        ImportPlan {
                            runtime_name: Some(name.clone().into_boxed_str()),
                            namespace_alias: None,
                            module_specifier: module_text.to_owned().into_boxed_str(),
                            helper: ImportHelperKind::None,
                            import_equals_publication: Some(if exported {
                                ImportEqualsPublication::ExportObject {
                                    exported_name: name.clone().into_boxed_str(),
                                }
                            } else {
                                ImportEqualsPublication::LocalBinding
                            }),
                        },
                    );
                    info.import_bindings.insert(
                        key,
                        ImportBinding {
                            generated_name: name.into_boxed_str(),
                            property: None,
                        },
                    );
                    info.external_imports.push(key);
                }
                NodeData::FunctionDeclaration(data) => {
                    let key = arena.get_original_node(*statement).node();
                    let directly_exported =
                        has_modifier(arena, source, data.modifiers, SyntaxKind::ExportKeyword)?;
                    if directly_exported && exported_function_set.insert(key) {
                        exported_functions.push(key);
                    }
                    let local = data
                        .name
                        .and_then(|id| arena.node_ref(source, id))
                        .and_then(|name| identifier_or_literal_text(arena, name).ok())
                        .map(String::into_boxed_str)
                        .or_else(|| info.generated_declaration_names.get(&key).cloned());
                    if let Some(local) = local {
                        if directly_exported {
                            let is_default = has_modifier(
                                arena,
                                source,
                                data.modifiers,
                                SyntaxKind::DefaultKeyword,
                            )?;
                            let export = if is_default {
                                "default"
                            } else {
                                local.as_ref()
                            };
                            let admitted = if is_default {
                                if has_export_default {
                                    false
                                } else {
                                    has_export_default = true;
                                    true
                                }
                            } else {
                                unique_exports.insert(export.into())
                            };
                            if admitted {
                                info.add_export_mapping(&local, export);
                                // collectModuleInfo's default arm records the
                                // declaration name in exportedBindings
                                // (_tsc.js:92906-92908); "default" stays only
                                // in the export mapping, so a merged namespace
                                // initializer substitutes `exports.<name>`.
                                info.add_exported_binding(
                                    key,
                                    if is_default { local.as_ref() } else { export },
                                );
                            }
                        }
                    }
                }
                NodeData::ClassDeclaration(data)
                    if has_modifier(arena, source, data.modifiers, SyntaxKind::ExportKeyword)? =>
                {
                    let key = arena.get_original_node(*statement).node();
                    let is_default =
                        has_modifier(arena, source, data.modifiers, SyntaxKind::DefaultKeyword)?;
                    let has_syntactic_name = data.name.is_some();
                    if !has_syntactic_name
                        && !is_default
                        && !info.generated_declaration_names.contains_key(&key)
                    {
                        // Invalid-but-emittable `export class {}` reaches
                        // getDeclarationName lazily from visitClassDeclaration.
                        // Materialize it here, in statement order relative to
                        // import aliases, while keeping it out of
                        // collectExternalModuleInfo.exportedNames.
                        let generated_name = info.generated_module_names.allocate("default");
                        info.generated_declaration_names.insert(key, generated_name);
                    }
                    let local = data
                        .name
                        .and_then(|id| arena.node_ref(source, id))
                        .and_then(|name| identifier_or_literal_text(arena, name).ok())
                        .map(String::into_boxed_str)
                        .or_else(|| info.generated_declaration_names.get(&key).cloned());
                    if let Some(local) = local {
                        let export = if is_default {
                            "default"
                        } else {
                            local.as_ref()
                        };
                        // collectExternalModuleInfo deliberately omits a
                        // directly exported default class from exportedNames:
                        // its declaration is followed immediately by the
                        // export assignment, so no undefined preinitializer
                        // is required. Keep the local/export mapping because
                        // visitClassDeclaration still owns that final
                        // assignment.
                        let admitted = if is_default {
                            if has_export_default {
                                false
                            } else {
                                has_export_default = true;
                                true
                            }
                        } else if has_syntactic_name {
                            unique_exports.insert(export.into())
                        } else {
                            // Invalid-but-emittable anonymous named export is
                            // absent from collectExternalModuleInfo but still
                            // materialized by visitClassDeclaration.
                            true
                        };
                        if admitted {
                            info.add_export_mapping(&local, export);
                            if !is_default && has_syntactic_name {
                                info.add_exported_name(export);
                            }
                            if is_default || has_syntactic_name {
                                // The default arm mirrors the function case:
                                // exportedBindings receives the declaration
                                // name (_tsc.js:92859-92861).
                                info.add_exported_binding(
                                    key,
                                    if is_default { local.as_ref() } else { export },
                                );
                            }
                        }
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
                            for leaf in
                                binding_name_leaves(arena, source, variable.name, declaration)?
                            {
                                let local = identifier_or_literal_text(arena, leaf.name)?;
                                if !unique_exports.insert(local.clone().into_boxed_str()) {
                                    continue;
                                }
                                info.add_export_mapping(&local, &local);
                                info.add_exported_name(&local);
                                info.add_exported_binding(
                                    arena.get_original_node(leaf.declaration).node(),
                                    &local,
                                );
                                if !arena.metadata(leaf.name).is_some_and(|metadata| {
                                    metadata.flags().contains(EmitFlags::LOCAL_NAME)
                                }) {
                                    info.direct_exported_variable_names
                                        .insert(local.clone().into_boxed_str());
                                }
                            }
                        }
                    }
                }
                NodeData::ExportDeclaration(data) if data.module_specifier.is_some() => {
                    let module_specifier = data
                        .module_specifier
                        .and_then(|id| arena.node_ref(source, id))
                        .ok_or(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::ExportDeclaration,
                            field: "module_specifier",
                        })?;
                    let module_text = string_literal_text(arena, module_specifier)?;
                    let key = arena.get_original_node(*statement).node();
                    // `getGeneratedNameForNode` is lazy in tsc: a synchronous
                    // export-star or namespace export constructs an identity
                    // but never materializes it, so it must not consume an
                    // ordinal. Named re-exports own a local require binding;
                    // AMD also needs a setter parameter for every external
                    // export declaration.
                    let has_named_exports = data
                        .export_clause
                        .and_then(|clause| arena.node_ref(source, clause))
                        .is_some_and(|clause| {
                            matches!(arena.node(clause), Ok(record) if matches!(record.data, NodeData::NamedExports(_)))
                        });
                    let runtime_name = (module_kind == MODULE_AMD || has_named_exports)
                        .then(|| info.generated_module_names.allocate(module_text));
                    info.imports.insert(
                        key,
                        ImportPlan {
                            runtime_name,
                            namespace_alias: None,
                            module_specifier: module_text.to_owned().into_boxed_str(),
                            helper: ImportHelperKind::None,
                            import_equals_publication: None,
                        },
                    );
                    info.external_imports.push(key);

                    if let Some(clause) =
                        data.export_clause.and_then(|id| arena.node_ref(source, id))
                    {
                        match &arena.node(clause)?.data {
                            NodeData::NamedExports(named) => {
                                for specifier in node_array_nodes(arena, source, named.elements)? {
                                    if let NodeData::ExportSpecifier(specifier) =
                                        &arena.node(specifier)?.data
                                    {
                                        if let Some(export) = specifier
                                            .name
                                            .and_then(|id| arena.node_ref(source, id))
                                            .and_then(|name| {
                                                identifier_or_literal_text(arena, name).ok()
                                            })
                                        {
                                            if !unique_exports
                                                .insert(export.clone().into_boxed_str())
                                            {
                                                continue;
                                            }
                                            info.add_exported_name(&export);
                                        }
                                    }
                                }
                            }
                            NodeData::NamespaceExport(namespace) => {
                                if let Some(export) = namespace
                                    .name
                                    .and_then(|id| arena.node_ref(source, id))
                                    .and_then(|name| identifier_or_literal_text(arena, name).ok())
                                {
                                    if !unique_exports.insert(export.clone().into_boxed_str()) {
                                        continue;
                                    }
                                    info.add_exported_name(&export);
                                }
                            }
                            _ => {}
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
                    for specifier_node in node_array_nodes(arena, source, named.elements)? {
                        let NodeData::ExportSpecifier(specifier) =
                            &arena.node(specifier_node)?.data
                        else {
                            continue;
                        };
                        let Some(local_name) = specifier
                            .property_name
                            .or(specifier.name)
                            .and_then(|id| arena.node_ref(source, id))
                        else {
                            continue;
                        };
                        let local = identifier_or_literal_text(arena, local_name)?;
                        let Some(export) = specifier
                            .name
                            .and_then(|id| arena.node_ref(source, id))
                            .and_then(|name| identifier_or_literal_text(arena, name).ok())
                        else {
                            continue;
                        };
                        let declaration = if let Some(resolver_node) =
                            arena.parse_tree_resolver_node(local_name)?
                        {
                            resolver.get_referenced_value_declaration(resolver_node)?
                        } else {
                            None
                        };
                        let function_declaration = declaration.is_some_and(|declaration| {
                            arena
                                .node_ref(source, declaration.node())
                                .and_then(|declaration| arena.node(declaration).ok())
                                .is_some_and(|declaration| {
                                    declaration.kind == SyntaxKind::FunctionDeclaration
                                })
                        });
                        // addExportedNamesForExportDeclaration gates the
                        // whole specifier on uniqueExports. A default
                        // function export is special: addExportedFunction-
                        // Declaration records the declaration and specifier
                        // even when hasExportDefault suppresses its exported
                        // binding, and it does not enter exportedNames.
                        if unique_exports.contains(export.as_str()) {
                            continue;
                        }
                        let location = arena.get_original_node(specifier_node).node();
                        if function_declaration {
                            info.add_export_specifier_mapping(&local, &export, location);
                            if let Some(declaration) = declaration {
                                let key = declaration.node();
                                if exported_function_set.insert(key) {
                                    exported_functions.push(key);
                                }
                                if export == "default" {
                                    if !has_export_default {
                                        has_export_default = true;
                                        info.add_exported_binding(key, &export);
                                    }
                                } else {
                                    unique_exports.insert(export.clone().into_boxed_str());
                                    info.add_exported_binding(key, &export);
                                }
                            }
                        } else {
                            unique_exports.insert(export.clone().into_boxed_str());
                            info.add_export_specifier(&local, &export, location);
                            let _ = info
                                .file_level_generated_binding_exports
                                .add_for_identifier(arena, local_name, &export);
                            if let Some(declaration) = declaration {
                                info.add_exported_binding(declaration.node(), &export);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let functions_by_original = statements
            .iter()
            .filter(|&statement| {
                matches!(
                    arena.node(*statement),
                    Ok(record) if record.kind == SyntaxKind::FunctionDeclaration
                )
            })
            .map(|statement| (arena.get_original_node(*statement).node(), *statement))
            .collect::<BTreeMap<_, _>>();
        for key in exported_functions {
            // An ambient function is replaced by a NotEmittedStatement in
            // the preceding TypeScript transform, but
            // collectExternalModuleInfo still keeps the original function
            // declaration in `exportedFunctions`. The module prelude must
            // therefore be able to publish an `export { fn }` alias from the
            // parse-tree declaration even when no transformed function
            // statement remains in the source-file list.
            let Some(statement) = functions_by_original
                .get(&key)
                .copied()
                .or_else(|| arena.node_ref(source, key))
            else {
                continue;
            };
            let NodeData::FunctionDeclaration(data) = &arena.node(statement)?.data else {
                continue;
            };
            let Some(local) = data
                .name
                .and_then(|id| arena.node_ref(source, id))
                .and_then(|name| identifier_or_literal_text(arena, name).ok())
                .map(String::into_boxed_str)
                .or_else(|| info.generated_declaration_names.get(&key).cloned())
            else {
                continue;
            };
            for export in info.hoisted_declaration_exports(arena, source, data.modifiers, &local)? {
                info.hoisted_function_exports.push((export, local.clone()));
            }
        }
        Ok(info)
    }

    fn add_export_mapping(&mut self, local: &str, export: &str) {
        let exports = self.exports_by_local.entry(local.into()).or_default();
        if !exports.iter().any(|existing| existing.as_ref() == export) {
            exports.push(export.into());
        }
    }

    fn add_export_specifier(&mut self, local: &str, export: &str, location: NodeId) {
        self.add_export_specifier_mapping(local, export, location);
        self.add_exported_name(export);
    }

    fn add_export_specifier_mapping(&mut self, local: &str, export: &str, location: NodeId) {
        let exports = self
            .export_specifiers_by_local
            .entry(local.into())
            .or_default();
        if !exports.iter().any(|existing| existing.as_ref() == export) {
            exports.push(export.into());
        }
        self.export_specifier_locations
            .entry((local.into(), export.into()))
            .or_insert(location);
        self.add_export_mapping(local, export);
    }

    fn add_exported_name(&mut self, export: &str) {
        if !self
            .exported_names
            .iter()
            .any(|existing| existing.as_ref() == export)
        {
            self.exported_names.push(export.into());
        }
    }

    fn add_exported_binding(&mut self, declaration: NodeId, export: &str) {
        let exports = self.exported_bindings.entry(declaration).or_default();
        if !exports.iter().any(|existing| existing.as_ref() == export) {
            exports.push(export.into());
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

/// The parsed statement-list owner behind a transformed SourceFile.
///
/// Module transforms can update the SourceFile more than once before moving
/// its runtime statements into a synthetic function body.  The current array
/// can therefore begin with a synthesized prologue even though detached
/// comments are still owned by the first parse-tree statement.  Follow the
/// SourceFile's original chain once at this boundary instead of asking each
/// wrapper transform to infer comment ownership from its current children.
fn parsed_source_file_statement_array(
    arena: &TransformArena,
    root: TransformNode,
) -> Result<Option<TransformNodeArray>, TransformError> {
    let parsed_root = arena.get_original_node(root);
    let NodeData::SourceFile(data) = &arena.node(parsed_root)?.data else {
        return Err(TransformError::RootKindExpected {
            actual: arena.node(parsed_root)?.kind,
        });
    };
    Ok(data
        .statements
        .and_then(|array| arena.node_array_ref(parsed_root.source(), array)))
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

/// The upstream module transform installs expression substitutions even for a
/// source file that does not need a module statement rewrite. In particular,
/// automatic JSX can retain a generated import-specifier identity without
/// inserting its implicit import into a legacy script. Our mutable-AST
/// substitution model must still visit that script so the reference receives
/// the same generated namespace access at emit time.
fn source_contains_import_reference_substitution(
    arena: &TransformArena,
    root: TransformNode,
) -> Result<bool, TransformError> {
    let mut stack = vec![root.node()];
    while let Some(id) = stack.pop() {
        let node = arena
            .node_ref(root.source(), id)
            .ok_or_else(|| TransformError::UnknownNode(TransformNode::new(root.source(), id)))?;
        if arena
            .metadata(node)
            .and_then(crate::EmitMetadata::referenced_import_declaration)
            .is_some()
        {
            return Ok(true);
        }
        let record = arena.node(node)?;
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

fn source_contains_import_attributes(source: &tsc_syntax::SourceFile) -> bool {
    let mut stack = vec![source.root];
    while let Some(id) = stack.pop() {
        let record = source.arena.node(id);
        let static_attributes = matches!(
            &record.data,
            NodeData::ImportDeclaration(data) if data.attributes.is_some()
        ) || matches!(
            &record.data,
            NodeData::ExportDeclaration(data) if data.attributes.is_some()
        );
        let dynamic_attributes = match &record.data {
            NodeData::CallExpression(data) => {
                let is_dynamic_import = data.expression.is_some_and(|expression| {
                    source.arena.node(expression).kind == SyntaxKind::ImportKeyword
                });
                let argument_count = data
                    .arguments
                    .map(|arguments| source.arena.node_array(arguments).nodes.len())
                    .unwrap_or(0);
                is_dynamic_import && argument_count > 1
            }
            _ => false,
        };
        if static_attributes || dynamic_attributes {
            return true;
        }
        for_each_child(&source.arena, record, |child| {
            stack.push(child);
            false
        });
    }
    false
}

fn source_contains_runtime_enum(source: &tsc_syntax::SourceFile) -> bool {
    let mut stack = vec![source.root];
    while let Some(id) = stack.pop() {
        let record = source.arena.node(id);
        if record.kind == SyntaxKind::EnumDeclaration {
            return true;
        }
        for_each_child(&source.arena, record, |child| {
            stack.push(child);
            false
        });
    }
    false
}

fn source_contains_runtime_namespace(source: &tsc_syntax::SourceFile) -> bool {
    let mut stack = vec![source.root];
    while let Some(id) = stack.pop() {
        let record = source.arena.node(id);
        if let NodeData::ModuleDeclaration(data) = &record.data {
            let flags = NodeFlags::from_bits(record.flags);
            let declared = data.modifiers.is_some_and(|modifiers| {
                source
                    .arena
                    .node_array(modifiers)
                    .nodes
                    .iter()
                    .any(|modifier| source.arena.node(*modifier).kind == SyntaxKind::DeclareKeyword)
            });
            let identifier_named = data
                .name
                .is_some_and(|name| source.arena.node(name).kind == SyntaxKind::Identifier);
            if identifier_named
                && !declared
                && !flags.contains(NodeFlags::AMBIENT)
                && !flags.contains(NodeFlags::GLOBAL_AUGMENTATION)
            {
                return true;
            }
        }
        for_each_child(&source.arena, record, |child| {
            stack.push(child);
            false
        });
    }
    false
}

fn source_contains_parameter_property(source: &tsc_syntax::SourceFile) -> bool {
    let mut stack = vec![source.root];
    while let Some(id) = stack.pop() {
        let record = source.arena.node(id);
        if parameter_has_property_modifier(source, record) {
            return true;
        }
        for_each_child(&source.arena, record, |child| {
            stack.push(child);
            false
        });
    }
    false
}

fn source_contains_import_or_export_equals(source: &tsc_syntax::SourceFile) -> bool {
    let mut stack = vec![source.root];
    while let Some(id) = stack.pop() {
        let record = source.arena.node(id);
        if record.kind == SyntaxKind::ImportEqualsDeclaration
            || matches!(
                &record.data,
                NodeData::ExportAssignment(data) if data.is_export_equals == Some(true)
            )
        {
            return true;
        }
        for_each_child(&source.arena, record, |child| {
            stack.push(child);
            false
        });
    }
    false
}

fn source_contains_decorator(source: &tsc_syntax::SourceFile) -> bool {
    let mut stack = vec![source.root];
    while let Some(id) = stack.pop() {
        let record = source.arena.node(id);
        if record.kind == SyntaxKind::Decorator {
            return true;
        }
        for_each_child(&source.arena, record, |child| {
            stack.push(child);
            false
        });
    }
    false
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

/// tsc-port: shouldRewriteModuleSpecifier @6.0.3
/// tsc-hash: a93fcbefd630ae756d3a55367bf8884b7835c8836f7a8c12fec4e489872dfff3
/// tsc-span: _tsc.js:15250-15252
///
/// tsc-port: rewriteModuleSpecifier @6.0.3
/// tsc-hash: f922e640861acb3c4f3e223a052ecf480ebdc989e9c1d4b545efca742e40aace
/// tsc-span: _tsc.js:93242-93248
pub(crate) fn rewrite_relative_module_specifier(text: &str) -> Option<String> {
    if !(text.starts_with("./") || text.starts_with("../")) {
        return None;
    }
    let base = text.rsplit('/').next().unwrap_or(text);
    if base.ends_with(".d.ts")
        || base.ends_with(".d.mts")
        || base.ends_with(".d.cts")
        || base.contains(".d.") && base.ends_with(".ts")
    {
        return None;
    }
    let (suffix_len, output) = if text.ends_with(".mts") {
        (4, ".mjs")
    } else if text.ends_with(".cts") {
        (4, ".cjs")
    } else if text.ends_with(".tsx") {
        (4, ".js")
    } else if text.ends_with(".ts") {
        (3, ".js")
    } else {
        return None;
    };
    let mut rewritten = String::with_capacity(text.len() - suffix_len + output.len());
    rewritten.push_str(&text[..text.len() - suffix_len]);
    rewritten.push_str(output);
    Some(rewritten)
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
    // This is tsc's `makeIdentifierFromModuleName` boundary: normalize path
    // separators, remove one trailing directory separator as
    // `getBaseFileName` does, then make the basename identifier-safe. Keep an
    // empty result empty; `makeUniqueName` owns the `_1` fallback.
    let normalized = module_specifier.replace('\\', "/");
    let without_trailing_separator = normalized.strip_suffix('/').unwrap_or(&normalized);
    let segment = without_trailing_separator
        .rsplit('/')
        .next()
        .unwrap_or(without_trailing_separator);
    let mut generated = segment
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if generated.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        generated.insert(0, '_');
    }
    generated
}

/// tsc-port: safeMultiLineComment @6.0.3
/// tsc-hash: 667604036376999e00db3f0b459cb4f4a9374b06d579b6efe0178e5241547ac0
/// tsc-span: _tsc.js:95824-95826
///
/// Preserve the slash from a closing delimiter after breaking the delimiter
/// itself. Const-enum substitutions embed the original access text in a
/// synthetic multi-line comment, so dropping that slash would change the
/// provenance text visible in JavaScript output.
fn safe_multi_line_comment(text: &str) -> String {
    text.replace("*/", "*_/")
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

#[derive(Clone, Copy, Debug)]
struct BindingNameLeaf {
    name: TransformNode,
    declaration: TransformNode,
}

/// Collects the identifier leaves of a declaration name while retaining the
/// declaration node which owns each symbol. For a simple variable this is the
/// VariableDeclaration; for a destructuring leaf it is the BindingElement.
/// That distinction is observable through getReferencedValueDeclaration and
/// must remain stable across module-information collection and substitution.
fn binding_name_leaves(
    arena: &TransformArena,
    source: TransformSourceId,
    name: Option<NodeId>,
    declaration: TransformNode,
) -> Result<Vec<BindingNameLeaf>, TransformError> {
    let Some(name) = name.and_then(|name| arena.node_ref(source, name)) else {
        return Ok(Vec::new());
    };
    let mut leaves = Vec::new();
    collect_binding_name_leaves(arena, source, name, declaration, &mut leaves)?;
    Ok(leaves)
}

fn collect_binding_name_leaves(
    arena: &TransformArena,
    source: TransformSourceId,
    name: TransformNode,
    declaration: TransformNode,
    leaves: &mut Vec<BindingNameLeaf>,
) -> Result<(), TransformError> {
    match &arena.node(name)?.data {
        NodeData::Identifier(_) => leaves.push(BindingNameLeaf { name, declaration }),
        NodeData::ObjectBindingPattern(data) => {
            for element in node_array_nodes(arena, source, data.elements)? {
                let NodeData::BindingElement(data) = &arena.node(element)?.data else {
                    continue;
                };
                let Some(name) = data.name.and_then(|name| arena.node_ref(source, name)) else {
                    continue;
                };
                collect_binding_name_leaves(arena, source, name, element, leaves)?;
            }
        }
        NodeData::ArrayBindingPattern(data) => {
            for element in node_array_nodes(arena, source, data.elements)? {
                let NodeData::BindingElement(data) = &arena.node(element)?.data else {
                    continue;
                };
                let Some(name) = data.name.and_then(|name| arena.node_ref(source, name)) else {
                    continue;
                };
                collect_binding_name_leaves(arena, source, name, element, leaves)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn is_non_reference_identifier_node(
    arena: &TransformArena,
    node: TransformNode,
) -> Result<bool, TransformError> {
    if arena.metadata(node).is_some_and(|metadata| {
        metadata
            .internal_flags()
            .contains(InternalEmitFlags::DECLARATION_NAME_REFERENCE)
    }) {
        return Ok(false);
    }
    let node = arena.get_original_node(node);
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
        NodeData::FunctionExpression(data) => data.name == Some(node.node()),
        NodeData::ClassDeclaration(data) => data.name == Some(node.node()),
        NodeData::ClassExpression(data) => data.name == Some(node.node()),
        NodeData::Parameter(data) => data.name == Some(node.node()),
        NodeData::BindingElement(data) => {
            data.name == Some(node.node()) || data.property_name == Some(node.node())
        }
        NodeData::ImportClause(data) => data.name == Some(node.node()),
        NodeData::ImportEqualsDeclaration(data) => data.name == Some(node.node()),
        NodeData::ImportSpecifier(data) => {
            data.name == Some(node.node()) || data.property_name == Some(node.node())
        }
        NodeData::NamespaceImport(data) => data.name == Some(node.node()),
        NodeData::PropertyDeclaration(data) => data.name == Some(node.node()),
        NodeData::PropertySignature(data) => data.name == Some(node.node()),
        NodeData::MethodDeclaration(data) => data.name == Some(node.node()),
        NodeData::MethodSignature(data) => data.name == Some(node.node()),
        NodeData::GetAccessor(data) => data.name == Some(node.node()),
        NodeData::SetAccessor(data) => data.name == Some(node.node()),
        NodeData::EnumMember(data) => data.name == Some(node.node()),
        NodeData::PropertyAssignment(data) => data.name == Some(node.node()),
        NodeData::PropertyAccessExpression(data) => data.name == Some(node.node()),
        NodeData::QualifiedName(data) => data.right == Some(node.node()),
        NodeData::TypeParameter(data) => data.name == Some(node.node()),
        NodeData::InterfaceDeclaration(data) => data.name == Some(node.node()),
        NodeData::TypeAliasDeclaration(data) => data.name == Some(node.node()),
        NodeData::EnumDeclaration(data) => data.name == Some(node.node()),
        NodeData::ModuleDeclaration(data) => data.name == Some(node.node()),
        NodeData::LabeledStatement(data) => data.label == Some(node.node()),
        NodeData::BreakStatement(data) => data.label == Some(node.node()),
        NodeData::ContinueStatement(data) => data.label == Some(node.node()),
        NodeData::ExportSpecifier(data) => {
            data.name == Some(node.node()) || data.property_name == Some(node.node())
        }
        NodeData::JsxAttribute(data) => data.name == Some(node.node()),
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

/// The two prefix boundaries owned by `copyPrologue` for a constructor body.
///
/// Standard directives and custom prologues are deliberately represented as
/// separate phases. A string-literal statement following a custom prologue is
/// ordinary body syntax; folding both predicates into one loop would consume
/// it incorrectly.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ConstructorPrologue {
    standard_end: usize,
    custom_end: usize,
}

impl ConstructorPrologue {
    pub(super) const fn standard_end(self) -> usize {
        self.standard_end
    }

    pub(super) const fn custom_end(self) -> usize {
        self.custom_end
    }

    pub(super) const fn body_start(self) -> usize {
        self.custom_end
    }
}

/// Finds the already-transformed statement prefix corresponding to tsc's
/// `copyPrologue`. The caller owns placement: class-field transforms retain
/// the prefix once, while the standard-decorator no-super path replays it to
/// match upstream's observable constructor body.
///
/// tsc-port: copyPrologue/copyStandardPrologue/copyCustomPrologue @6.0.3
/// tsc-hash: 555445a3fd02a4b53bbc05f05e48729ca0f7208892d66dbc7985f51f3e897a8e
/// tsc-span: _tsc.js:24827-24869
pub(super) fn constructor_prologue(
    arena: &TransformArena,
    statements: &[TransformNode],
) -> Result<ConstructorPrologue, TransformError> {
    let mut standard_end = 0usize;
    while standard_end < statements.len() && is_prologue_statement(arena, statements[standard_end])?
    {
        standard_end += 1;
    }

    let mut custom_end = standard_end;
    while custom_end < statements.len() {
        let statement = statements[custom_end];
        arena.node(statement)?;
        if !arena
            .metadata(statement)
            .is_some_and(|metadata| metadata.flags().contains(EmitFlags::CUSTOM_PROLOGUE))
        {
            break;
        }
        custom_end += 1;
    }

    Ok(ConstructorPrologue {
        standard_end,
        custom_end,
    })
}

pub(super) fn first_runtime_declaration_original(
    arena: &TransformArena,
    source: TransformSourceId,
    statements: Option<NodeArrayId>,
) -> Result<Option<TransformNode>, TransformError> {
    let Some(statements) = statements.and_then(|id| arena.node_array_ref(source, id)) else {
        return Ok(None);
    };
    for id in &arena.node_array(statements)?.nodes {
        let Some(statement) = arena.node_ref(source, *id) else {
            continue;
        };
        if is_prologue_statement(arena, statement)? {
            continue;
        }
        // A TypeScript-erasure anchor owns a source range but no runtime
        // declaration. In particular, it must not donate an ambient module's
        // leading comments to a generated `__esModule` marker.
        if arena.node(statement)?.kind == SyntaxKind::NotEmittedStatement {
            continue;
        }
        let original = arena.get_original_node(statement);
        return Ok(matches!(
            arena.node(original)?.kind,
            SyntaxKind::EnumDeclaration | SyntaxKind::ModuleDeclaration
        )
        .then_some(original));
    }
    Ok(None)
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

fn variable_list_has_initialized_export_binding(
    arena: &TransformArena,
    source: TransformSourceId,
    list: TransformNode,
    exported_bindings: &BTreeMap<NodeId, Vec<Box<str>>>,
) -> Result<bool, TransformError> {
    let NodeData::VariableDeclarationList(data) = &arena.node(list)?.data else {
        return Ok(false);
    };
    for declaration in variable_declarations_from_array(arena, source, data.declarations)? {
        if let NodeData::VariableDeclaration(data) = &arena.node(declaration)?.data {
            if data.initializer.is_none() {
                continue;
            }
            for leaf in binding_name_leaves(arena, source, data.name, declaration)? {
                let declaration = arena.get_original_node(leaf.declaration).node();
                if exported_bindings
                    .get(&declaration)
                    .is_some_and(|exports| !exports.is_empty())
                {
                    return Ok(true);
                }
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
    preserves_native_parameter_defaults: bool,
    rewrite_relative_import_extensions: bool,
}

/// The two function traversal routes exposed by transformModule. Ordinary
/// function-like nodes use visitEachChild's nearest-function lexical
/// environment. A syntactically exported top-level function is the deliberate
/// exception: transformModule's declaration visitor walks its parameters and
/// body directly, so its generated temporaries remain module-owned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CommonJsFunctionLexicalOwner {
    Module,
    Function {
        kind: SyntaxKind,
        concise_body: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CommonJsExpressionValueUse {
    Required,
    Unused,
}

/// Materialization policy when a module rewrite removes every statement from
/// a required embedded-statement slot. Most control-flow owners use
/// `liftToBlock`; a label instead retains the erased statement's source range
/// on an `EmptyStatement`, matching transformModule's recovery path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CommonJsEmptyEmbeddedStatement {
    LiftToBlock,
    EmptyStatementWithSourceRange,
}

#[derive(Clone, Copy, Debug)]
struct ModuleDestructuringElement {
    original: TransformNode,
    target: TransformNode,
    property_name: Option<TransformNode>,
    initializer: Option<TransformNode>,
    rest: bool,
}

#[derive(Clone, Copy, Debug)]
enum ModuleDestructuringExcludedProperty {
    Named(TransformNode),
    Computed(TransformNode),
}

/// A generated temporary whose declaration is owned by transformModule's
/// active lexical environment. The binding text is stable, while declaration
/// and reference identifiers remain distinct AST nodes.
#[derive(Clone, Debug, Eq, PartialEq)]
struct CommonJsTempBinding(Box<str>);

/// Generated parameter identity used when a binding-pattern default must move
/// into a function body. This is a parameter binding, never a hoisted temp.
#[derive(Clone, Debug, Eq, PartialEq)]
struct CommonJsParameterBinding(Box<str>);

/// A generated binding owned by the lexical environment of a transformed
/// namespace body. The binding text is allocated once; declaration, write,
/// and read identifiers are materialized as distinct AST nodes.
#[derive(Clone, Debug, Eq, PartialEq)]
struct NamespaceTempBinding(Box<str>);

#[derive(Clone, Debug)]
enum NamespaceDestructuringTarget {
    Temp(NamespaceTempBinding),
    Export(Box<str>),
}

#[derive(Clone, Debug)]
struct NamespaceDestructuringStep {
    target: NamespaceDestructuringTarget,
    value: TransformNode,
    original: Option<TransformNode>,
}

/// Rust ownership form of tsc's `flattenDestructuringAssignment` expression
/// accumulator. The plan makes evaluation order explicit and keeps generated
/// temporary bindings distinct from namespace publication leaves.
#[derive(Default, Debug)]
struct NamespaceDestructuringPlan {
    steps: Vec<NamespaceDestructuringStep>,
}

impl NamespaceDestructuringPlan {
    fn push(
        &mut self,
        target: NamespaceDestructuringTarget,
        value: TransformNode,
        original: Option<TransformNode>,
    ) {
        self.steps.push(NamespaceDestructuringStep {
            target,
            value,
            original,
        });
    }
}

#[derive(Clone, Debug)]
struct ExportAssignmentPlan {
    local_name: String,
    exports: Vec<Box<str>>,
    direct_export_storage: bool,
}

/// One explicit `export { local as exported }` publication owned by a
/// declaration leaf. Keeping the declaration identity and export-specifier
/// location in the plan avoids the shadowing bugs of a text-only lookup and
/// lets variable statements and loop initializers share one materializer.
#[derive(Clone, Debug)]
struct DeclarationExportPlan {
    local: TransformNode,
    exported_name: Box<str>,
    location: TransformNode,
}

struct AliasedAsynchronousDependency {
    path: String,
    parameter: String,
}

struct AsynchronousDependencies {
    aliased: Vec<AliasedAsynchronousDependency>,
    unaliased: Vec<String>,
}

/// Names owned by one AMD dynamic-import executor. Reserving the pair before
/// visiting an AMD dependency mirrors the printer-order allocation of tsc's
/// generated identifiers: the outer executor is emitted before a nested
/// dependency expression and therefore owns the lower ordinal.
#[derive(Clone, Debug, Eq, PartialEq)]
struct AmdDynamicImportBindings {
    resolve: Box<str>,
    reject: Box<str>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StringLiteralQuote {
    Single,
    Double,
}

impl StringLiteralQuote {
    const fn is_single(self) -> bool {
        matches!(self, Self::Single)
    }
}

struct CommonJsVisitor<'context, 'resolver> {
    context: &'context mut TransformationContext,
    source: TransformSourceId,
    resolver: &'resolver dyn EmitResolver,
    module_kind: i32,
    es_module_interop: bool,
    has_dynamic_import: bool,
    preserves_native_parameter_defaults: bool,
    rewrite_relative_import_extensions: bool,
    info: CommonJsModuleInfo,
    nodes: BTreeMap<NodeId, NodeId>,
    arrays: BTreeMap<NodeArrayId, NodeArrayId>,
    dynamic_import_ordinal: usize,
    used_names: BTreeSet<String>,
    temp_ordinal: usize,
    expression_value_use: CommonJsExpressionValueUse,
}

impl<'context, 'resolver> CommonJsVisitor<'context, 'resolver> {
    fn new(
        context: &'context mut TransformationContext,
        source: TransformSourceId,
        resolver: &'resolver dyn EmitResolver,
        options: CommonJsVisitorOptions,
        info: CommonJsModuleInfo,
    ) -> Self {
        let used_names = system::collect_identifier_texts(context.arena(), source);
        Self {
            context,
            source,
            resolver,
            module_kind: options.module_kind,
            es_module_interop: options.es_module_interop,
            has_dynamic_import: options.has_dynamic_import,
            preserves_native_parameter_defaults: options.preserves_native_parameter_defaults,
            rewrite_relative_import_extensions: options.rewrite_relative_import_extensions,
            info,
            nodes: BTreeMap::new(),
            arrays: BTreeMap::new(),
            dynamic_import_ordinal: 0,
            used_names,
            temp_ordinal: 0,
            expression_value_use: CommonJsExpressionValueUse::Required,
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
        // `transformAsynchronousModuleBody` and `transformCommonJSModule`
        // each own one lexical environment in tsc. transformModule keeps that
        // environment active while it visits nested functions as well, so its
        // export-update and destructuring temporaries remain module-scoped.
        self.context.start_lexical_environment()?;
        let visit_result = (|| -> Result<(Vec<TransformNode>, usize), TransformError> {
            let mut output = Vec::new();
            let mut offset = 0usize;
            while offset < input.len()
                && is_prologue_statement(self.context.arena(), input[offset])?
            {
                output.push(self.visit(input[offset].node())?);
                offset += 1;
            }
            while offset < input.len()
                && self
                    .context
                    .arena()
                    .metadata(input[offset])
                    .is_some_and(|metadata| metadata.flags().contains(EmitFlags::CUSTOM_PROLOGUE))
            {
                output.push(self.visit(input[offset].node())?);
                offset += 1;
            }

            if self.module_kind == MODULE_UMD && self.has_dynamic_import {
                output.push(self.create_sync_require_declaration()?);
            }
            let temp_insertion = output.len();
            if self.info.is_external && self.info.export_equals.is_none() {
                output.push(self.create_es_module_marker()?);
            }
            let hoisted_function_exports = self.info.hoisted_function_exports.clone();
            let preinitialized = self.info.exported_names.clone();
            for chunk in preinitialized.chunks(50) {
                let mut expression = self.create_void_zero()?;
                for name in chunk {
                    let target = self.create_export_access(name)?;
                    expression = self.create_assignment(target, expression)?;
                }
                output.push(self.create_expression_statement(expression)?);
            }
            if self.info.appends_declaration_exports() {
                for (export, local) in hoisted_function_exports {
                    let target = self.create_export_access(&export)?;
                    let value = self.create_identifier(&local)?;
                    let assignment = self.create_assignment(target, value)?;
                    output.push(self.create_expression_statement(assignment)?);
                }
            }

            if self.module_kind == MODULE_AMD {
                output.extend(self.create_amd_import_initializers()?);
            }
            for statement in input.into_iter().skip(offset) {
                output.extend(self.visit_top_level_statement(statement)?);
            }
            if let Some(export_equals) = self.info.export_equals {
                output.push(self.create_export_equals_statement(export_equals)?);
            }
            Ok((output, temp_insertion))
        })();
        let lexical_environment = self.context.end_lexical_environment();
        let (mut output, temp_insertion) = visit_result?;
        self.insert_source_lexical_environment(&mut output, temp_insertion, lexical_environment?)?;
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

    /// The UMD sync-require helper is represented as an ordinary statement in
    /// this arena, while tsc carries it as an emit helper. Preserve the same
    /// observable order by inserting lexical declarations after that helper
    /// and before the module marker at the source-body ownership boundary.
    fn insert_source_lexical_environment(
        &mut self,
        statements: &mut Vec<TransformNode>,
        insertion: usize,
        lexical_environment: LexicalEnvironment,
    ) -> Result<(), TransformError> {
        if lexical_environment.is_empty() {
            return Ok(());
        }
        let mut declarations = Vec::new();
        declarations.extend_from_slice(lexical_environment.function_declarations());
        if !lexical_environment.variable_declarations().is_empty() {
            declarations.push(
                self.create_hoisted_variable_statement(
                    lexical_environment.variable_declarations(),
                )?,
            );
        }
        declarations.extend_from_slice(lexical_environment.initialization_statements());
        statements.splice(insertion..insertion, declarations);
        Ok(())
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
            // Import-equals publication and the namespace alias of a combined
            // default/namespace import belong to their original source
            // positions. This phase initializes only AMD dependency
            // parameters with interop helpers, before ordinary statements.
            if plan.import_equals_publication.is_some() {
                continue;
            }
            let Some(runtime_name) = plan.runtime_name.as_deref() else {
                continue;
            };
            if self.es_module_interop {
                let helper_name = match plan.helper {
                    ImportHelperKind::Star => Some(EmitHelperName::ImportStar),
                    ImportHelperKind::Default => Some(EmitHelperName::ImportDefault),
                    ImportHelperKind::None => None,
                };
                if let Some(helper_name) = helper_name {
                    match plan.helper {
                        ImportHelperKind::Star => self.request_import_star_helper()?,
                        ImportHelperKind::Default => self.request_import_default_helper()?,
                        ImportHelperKind::None => {}
                    }
                    let target = self.create_identifier(runtime_name)?;
                    let helper = self
                        .context
                        .factory()?
                        .create_unscoped_helper_identifier(self.source, helper_name)?;
                    let argument = self.create_identifier(runtime_name)?;
                    let value = self.create_call(helper, vec![argument])?;
                    let assignment = self.create_assignment(target, value)?;
                    statements.push(self.create_expression_statement(assignment)?);
                }
            }
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

    /// tsc-port: addExportEqualsIfNeeded @6.0.3
    /// tsc-hash: d04181926d998191c370e6a96184e7700d2a60a17d62275755ef3fdcaa9ac74d
    /// tsc-span: _tsc.js:110535-110562
    fn create_export_equals_statement(
        &mut self,
        export_equals: NodeId,
    ) -> Result<TransformNode, TransformError> {
        let original = self.node(export_equals);
        let expression = match &self.context.arena().node(original)?.data {
            NodeData::ExportAssignment(data) => data.expression,
            _ => None,
        }
        .ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::ExportAssignment,
            field: "expression",
        })?;
        let expression = self.visit(expression)?;
        let statement = if matches!(self.module_kind, MODULE_AMD | MODULE_UMD) {
            self.context.factory()?.create_node(
                self.source,
                NodeData::ReturnStatement(tsc_syntax::nodes::ReturnStatementData {
                    expression: Some(expression.node()),
                }),
                TransformFlags::NONE,
            )?
        } else {
            let module = self.create_identifier("module")?;
            let target = self.create_property_access(module, "exports")?;
            let assignment = self.create_assignment(target, expression)?;
            self.create_expression_statement(assignment)?
        };
        self.set_original_and_range(statement, original)?;
        self.context
            .arena_mut()?
            .metadata_mut(statement)
            .add_flags(EmitFlags::NO_COMMENTS | EmitFlags::NO_TOKEN_SOURCE_MAPS);
        Ok(statement)
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
        for dependency in amd_dependencies {
            let path = dependency.path;
            if let Some(name) = dependency.name {
                aliased.push(AliasedAsynchronousDependency {
                    path,
                    parameter: name,
                });
            } else {
                unaliased.push(path);
            }
        }
        for plan in import_plans {
            let module_specifier = if self.rewrite_relative_import_extensions {
                rewrite_relative_module_specifier(&plan.module_specifier)
                    .unwrap_or_else(|| plan.module_specifier.to_string())
            } else {
                plan.module_specifier.to_string()
            };
            if self.module_kind == MODULE_AMD && plan.runtime_name.is_some() {
                aliased.push(AliasedAsynchronousDependency {
                    path: module_specifier,
                    parameter: plan
                        .runtime_name
                        .as_deref()
                        .expect("an aliased AMD dependency owns a runtime binding")
                        .to_owned(),
                });
            } else {
                unaliased.push(module_specifier);
            }
        }
        Ok(AsynchronousDependencies { aliased, unaliased })
    }

    fn wrap_asynchronous_module(
        &mut self,
        root: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let parsed_statement_array =
            parsed_source_file_statement_array(self.context.arena(), root)?;
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
        // Only the outer wrapper list retains the parsed statement-list range.
        // The body is a new lexical container, but carries typed provenance so
        // the printer can seed the original detached-comment boundary without
        // emitting the SourceFile-owned prefix a second time.
        let relocated_comments = parsed_statement_array
            .map(crate::metadata::RelocatedStatementListComments::owned_by_source_file);
        let body = self.create_block_from_array(body_statements, None, true)?;
        if let Some(relocated_comments) = relocated_comments {
            self.context
                .arena_mut()?
                .metadata_mut(body)
                .set_relocated_statement_list_comments(relocated_comments);
        }
        let asynchronous_dependencies = self.asynchronous_dependencies()?;

        let mut body_parameters = vec![
            self.create_parameter("require")?,
            self.create_parameter("exports")?,
        ];
        for dependency in &asynchronous_dependencies.aliased {
            body_parameters.push(self.create_parameter(&dependency.parameter)?);
        }
        let body_function = self.create_function_expression(body_parameters, body)?;
        let mut dependency_elements = vec![
            self.create_string_literal("require")?,
            self.create_string_literal("exports")?,
        ];
        for path in asynchronous_dependencies
            .aliased
            .into_iter()
            .map(|dependency| dependency.path)
            .chain(asynchronous_dependencies.unaliased)
        {
            dependency_elements.push(self.create_string_literal(&path)?);
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
            NodeData::ImportEqualsDeclaration(data) => {
                self.transform_import_equals(statement, data)
            }
            NodeData::ExportDeclaration(data) => self.transform_export_declaration(statement, data),
            NodeData::ExportAssignment(data) => {
                if data.is_export_equals == Some(true) {
                    return Ok(Vec::new());
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
                let syntactic_export = has_modifier(
                    self.context.arena(),
                    self.source,
                    data.modifiers,
                    SyntaxKind::ExportKeyword,
                )?;
                if data.name.is_none() {
                    let key = self.context.arena().get_original_node(statement).node();
                    if let Some(name) = self
                        .info
                        .generated_declaration_names
                        .get(&key)
                        .map(ToString::to_string)
                    {
                        data.name = Some(self.create_identifier(&name)?.node());
                    }
                }
                data.modifiers = self.remove_export_modifiers(data.modifiers)?;
                let owner = if syntactic_export {
                    CommonJsFunctionLexicalOwner::Module
                } else {
                    CommonJsFunctionLexicalOwner::Function {
                        kind: SyntaxKind::FunctionDeclaration,
                        concise_body: false,
                    }
                };
                let function = self.visit_function_declaration(statement, data, owner)?;
                self.nodes.insert(statement.node(), function.node());
                Ok(vec![function])
            }
            NodeData::ClassDeclaration(mut data) => {
                if data.name.is_none() {
                    let key = self.context.arena().get_original_node(statement).node();
                    if let Some(name) = self
                        .info
                        .generated_declaration_names
                        .get(&key)
                        .map(ToString::to_string)
                    {
                        data.name = Some(self.create_identifier(&name)?.node());
                    }
                }
                let name = data
                    .name
                    .and_then(|id| self.context.arena().node_ref(self.source, id))
                    .and_then(|name| identifier_or_literal_text(self.context.arena(), name).ok());
                let exported = name
                    .as_deref()
                    .map(|name| {
                        self.info.hoisted_declaration_exports(
                            self.context.arena(),
                            self.source,
                            data.modifiers,
                            name,
                        )
                    })
                    .transpose()?
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
            let mut statements = Vec::new();
            // tsc materializes the namespace side of
            // `import default, * as namespace` while visiting the import,
            // after the parameter-wide interop initialization phase.
            if let Some(namespace_alias) = plan.namespace_alias.as_deref() {
                let runtime_name = plan
                    .runtime_name
                    .as_deref()
                    .expect("an AMD namespace alias owns a dependency parameter");
                let value = self.create_identifier(runtime_name)?;
                let declaration = self.create_variable_declaration(namespace_alias, value)?;
                let declaration = self.set_original_and_range(declaration, original)?;
                statements
                    .push(self.create_variable_statement(vec![declaration], NodeFlags::CONST)?);
            }
            for re_export in self.import_re_exports(&data)? {
                statements.push(self.create_import_re_export_statement(re_export)?);
            }
            return Ok(statements);
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
                let helper = self
                    .context
                    .factory()?
                    .create_unscoped_helper_identifier(self.source, EmitHelperName::ImportStar)?;
                self.create_call(helper, vec![require])?
            }
            ImportHelperKind::Default if self.es_module_interop => {
                self.request_import_default_helper()?;
                let helper = self.context.factory()?.create_unscoped_helper_identifier(
                    self.source,
                    EmitHelperName::ImportDefault,
                )?;
                self.create_call(helper, vec![require])?
            }
            ImportHelperKind::Star | ImportHelperKind::Default => require,
        };
        let runtime_name = plan
            .runtime_name
            .as_deref()
            .expect("an import declaration with a clause owns a runtime binding");
        let mut declarations = vec![self.create_variable_declaration(runtime_name, initializer)?];
        if let Some(namespace_alias) = plan.namespace_alias.as_deref() {
            let value = self.create_identifier(runtime_name)?;
            declarations.push(self.create_variable_declaration(namespace_alias, value)?);
        }
        let statement = self.create_variable_statement(declarations, NodeFlags::CONST)?;
        self.set_original_and_range(statement, original)?;
        let mut statements = vec![statement];
        let re_exports = self.import_re_exports(&data)?;
        for re_export in re_exports {
            statements.push(self.create_import_re_export_statement(re_export)?);
        }
        Ok(statements)
    }

    fn import_re_exports(
        &self,
        import: &tsc_syntax::nodes::ImportDeclarationData,
    ) -> Result<Vec<ImportReExportPlan>, TransformError> {
        if !self.info.appends_declaration_exports() {
            return Ok(Vec::new());
        }
        let Some(clause) = import
            .import_clause
            .and_then(|clause| self.context.arena().node_ref(self.source, clause))
        else {
            return Ok(Vec::new());
        };
        let NodeData::ImportClause(clause_data) = &self.context.arena().node(clause)?.data else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ImportDeclaration,
                field: "import_clause",
            });
        };
        let mut plans = Vec::new();
        if let Some(name) = clause_data
            .name
            .and_then(|name| self.context.arena().node_ref(self.source, name))
        {
            plans.extend(self.import_binding_re_exports(clause, name, false)?);
        }
        let Some(bindings) = clause_data
            .named_bindings
            .and_then(|bindings| self.context.arena().node_ref(self.source, bindings))
        else {
            return Ok(plans);
        };
        match &self.context.arena().node(bindings)?.data {
            NodeData::NamespaceImport(namespace) => {
                if let Some(name) = namespace
                    .name
                    .and_then(|name| self.context.arena().node_ref(self.source, name))
                {
                    plans.extend(self.import_binding_re_exports(bindings, name, false)?);
                }
            }
            NodeData::NamedImports(named) => {
                for specifier in
                    node_array_nodes(self.context.arena(), self.source, named.elements)?
                {
                    let NodeData::ImportSpecifier(data) =
                        &self.context.arena().node(specifier)?.data
                    else {
                        continue;
                    };
                    if let Some(name) = data
                        .name
                        .and_then(|name| self.context.arena().node_ref(self.source, name))
                    {
                        plans.extend(self.import_binding_re_exports(specifier, name, true)?);
                    }
                }
            }
            _ => {}
        }
        Ok(plans)
    }

    fn import_binding_re_exports(
        &self,
        declaration: TransformNode,
        local_name: TransformNode,
        live_binding: bool,
    ) -> Result<Vec<ImportReExportPlan>, TransformError> {
        let local = identifier_or_literal_text(self.context.arena(), local_name)?;
        let key = self.context.arena().get_original_node(declaration).node();
        let Some(binding) = self.info.import_bindings.get(&key).cloned() else {
            return Ok(Vec::new());
        };
        let exports = self
            .info
            .export_specifiers_by_local
            .get(local.as_str())
            .cloned()
            .unwrap_or_default();
        exports
            .into_iter()
            .map(|exported_name| {
                let location = self
                    .info
                    .export_specifier_locations
                    .get(&(local.clone().into_boxed_str(), exported_name.clone()))
                    .and_then(|location| self.context.arena().node_ref(self.source, *location))
                    .unwrap_or(local_name);
                Ok(ImportReExportPlan {
                    exported_name,
                    binding: binding.clone(),
                    live_binding,
                    location,
                })
            })
            .collect()
    }

    fn create_import_re_export_statement(
        &mut self,
        plan: ImportReExportPlan,
    ) -> Result<TransformNode, TransformError> {
        let target = self.create_identifier(&plan.binding.generated_name)?;
        let value = if let Some(property) = plan.binding.property.as_deref() {
            self.create_property_access(target, property)?
        } else {
            target
        };
        let statement = if plan.live_binding {
            self.create_live_export_statement(&plan.exported_name, value)?
        } else {
            let target = self.create_export_access(&plan.exported_name)?;
            let assignment = self.create_assignment(target, value)?;
            self.create_expression_statement(assignment)?
        };
        self.set_original_and_range(statement, plan.location)?;
        self.context
            .arena_mut()?
            .metadata_mut(statement)
            .add_flags(EmitFlags::NO_COMMENTS);
        Ok(statement)
    }

    fn create_live_export_statement(
        &mut self,
        exported_name: &str,
        value: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let return_statement = self.context.factory()?.create_node(
            self.source,
            NodeData::ReturnStatement(tsc_syntax::nodes::ReturnStatementData {
                expression: Some(value.node()),
            }),
            TransformFlags::NONE,
        )?;
        let body = self.create_block(vec![return_statement], false)?;
        let getter = self.create_function_expression(Vec::new(), body)?;
        let enumerable_name = self.create_identifier("enumerable")?;
        let enumerable_value = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::TrueKeyword,
            TransformFlags::NONE,
        )?;
        let enumerable = self.context.factory()?.create_node(
            self.source,
            NodeData::PropertyAssignment(tsc_syntax::nodes::PropertyAssignmentData {
                name: Some(enumerable_name.node()),
                initializer: Some(enumerable_value.node()),
                modifiers: None,
                question_token: None,
                exclamation_token: None,
            }),
            TransformFlags::NONE,
        )?;
        let get_name = self.create_identifier("get")?;
        let get = self.context.factory()?.create_node(
            self.source,
            NodeData::PropertyAssignment(tsc_syntax::nodes::PropertyAssignmentData {
                name: Some(get_name.node()),
                initializer: Some(getter.node()),
                modifiers: None,
                question_token: None,
                exclamation_token: None,
            }),
            TransformFlags::NONE,
        )?;
        let properties = self
            .context
            .factory()?
            .create_node_array(self.source, vec![enumerable, get])?;
        let descriptor = self.context.factory()?.create_node(
            self.source,
            NodeData::ObjectLiteralExpression(tsc_syntax::nodes::ObjectLiteralExpressionData {
                properties: Some(properties.array()),
            }),
            TransformFlags::NONE,
        )?;
        let object = self.create_identifier("Object")?;
        let define_property = self.create_property_access(object, "defineProperty")?;
        let exports = self.create_identifier("exports")?;
        let name = self.create_string_literal(exported_name)?;
        let call = self.create_call(define_property, vec![exports, name, descriptor])?;
        self.create_expression_statement(call)
    }

    /// tsc-port: visitTopLevelImportEqualsDeclaration @6.0.3
    /// tsc-hash: 8577823442eb4668d4144ba8be82838ed14b0c7ebd76f51e2b82380cd29d4406
    /// tsc-span: _tsc.js:111298-111365
    fn transform_import_equals(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::ImportEqualsDeclarationData,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let key = self.context.arena().get_original_node(original).node();
        let plan =
            self.info
                .imports
                .get(&key)
                .cloned()
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ImportEqualsDeclaration,
                    field: "module plan",
                })?;
        let publication =
            plan.import_equals_publication
                .clone()
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ImportEqualsDeclaration,
                    field: "import-equals publication plan",
                })?;
        let module_reference = data
            .module_reference
            .and_then(|id| self.context.arena().node_ref(self.source, id))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ImportEqualsDeclaration,
                field: "module_reference",
            })?;
        let module_specifier = match &self.context.arena().node(module_reference)?.data {
            NodeData::ExternalModuleReference(reference) => reference
                .expression
                .and_then(|id| self.context.arena().node_ref(self.source, id)),
            _ => None,
        }
        .ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::ExternalModuleReference,
            field: "expression",
        })?;
        let runtime_name = plan
            .runtime_name
            .as_deref()
            .expect("an external import-equals plan owns a runtime binding");
        let mut statements = Vec::new();
        match (&publication, self.module_kind == MODULE_AMD) {
            (ImportEqualsPublication::LocalBinding, true) => {
                // The AMD dependency parameter is the local binding.
            }
            (ImportEqualsPublication::ExportObject { exported_name }, true) => {
                let target = self.create_export_access(exported_name)?;
                let value = self.create_identifier(runtime_name)?;
                let assignment = self.create_assignment(target, value)?;
                let statement = self.create_expression_statement(assignment)?;
                statements.push(self.set_original_and_range(statement, original)?);
            }
            (ImportEqualsPublication::LocalBinding, false) => {
                let require = self.create_require_call(module_specifier)?;
                let declaration = self.create_variable_declaration(runtime_name, require)?;
                let statement =
                    self.create_variable_statement(vec![declaration], NodeFlags::CONST)?;
                statements.push(self.set_original_and_range(statement, original)?);
            }
            (ImportEqualsPublication::ExportObject { exported_name }, false) => {
                let require = self.create_require_call(module_specifier)?;
                let target = self.create_export_access(exported_name)?;
                let assignment = self.create_assignment(target, require)?;
                let statement = self.create_expression_statement(assignment)?;
                statements.push(self.set_original_and_range(statement, original)?);
            }
        }

        let name = data
            .name
            .and_then(|name| self.context.arena().node_ref(self.source, name))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ImportEqualsDeclaration,
                field: "name",
            })?;
        let mut re_exports = self.import_binding_re_exports(original, name, false)?;
        if let ImportEqualsPublication::ExportObject { exported_name } = publication {
            for re_export in &mut re_exports {
                re_export.binding = ImportBinding {
                    generated_name: "exports".into(),
                    property: Some(exported_name.clone()),
                };
            }
        }
        for re_export in re_exports {
            statements.push(self.create_import_re_export_statement(re_export)?);
        }
        Ok(statements)
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
        let key = self.context.arena().get_original_node(original).node();
        let plan =
            self.info
                .imports
                .get(&key)
                .cloned()
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ExportDeclaration,
                    field: "module plan",
                })?;
        let Some(clause) = data
            .export_clause
            .and_then(|id| self.context.arena().node_ref(self.source, id))
        else {
            self.request_export_star_helper()?;
            let helper = self
                .context
                .factory()?
                .create_unscoped_helper_identifier(self.source, EmitHelperName::ExportStar)?;
            let exports = self.create_identifier("exports")?;
            let module = if self.module_kind == MODULE_AMD {
                let generated_name = plan
                    .runtime_name
                    .as_deref()
                    .expect("an AMD export-star declaration owns a generated module binding");
                self.create_identifier(generated_name)?
            } else {
                self.create_require_call(module_specifier)?
            };
            let call = self.create_call(helper, vec![module, exports])?;
            let statement = self.create_expression_statement(call)?;
            self.set_original_and_range(statement, original)?;
            return Ok(vec![statement]);
        };

        match self.context.arena().node(clause)?.data.clone() {
            NodeData::NamedExports(named) => {
                let generated_name = plan
                    .runtime_name
                    .as_deref()
                    .expect("a named external export owns a generated module binding")
                    .to_owned();
                let mut statements = Vec::new();
                if self.module_kind != MODULE_AMD {
                    let require = self.create_require_call(module_specifier)?;
                    let declaration = self.create_variable_declaration(&generated_name, require)?;
                    let statement =
                        self.create_variable_statement(vec![declaration], NodeFlags::NONE)?;
                    self.set_original_and_range(statement, original)?;
                    statements.push(statement);
                }

                let apply_import_helpers = self.es_module_interop
                    && !self
                        .context
                        .arena()
                        .metadata(original)
                        .is_some_and(|metadata| {
                            metadata
                                .internal_flags()
                                .contains(crate::InternalEmitFlags::NEVER_APPLY_IMPORT_HELPER)
                        });
                for specifier in
                    node_array_nodes(self.context.arena(), self.source, named.elements)?
                {
                    let NodeData::ExportSpecifier(specifier_data) =
                        self.context.arena().node(specifier)?.data.clone()
                    else {
                        continue;
                    };
                    let property = specifier_data
                        .property_name
                        .or(specifier_data.name)
                        .and_then(|id| self.context.arena().node_ref(self.source, id))
                        .and_then(|name| {
                            identifier_or_literal_text(self.context.arena(), name).ok()
                        })
                        .ok_or(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::ExportSpecifier,
                            field: "property_name or name",
                        })?;
                    let exported = specifier_data
                        .name
                        .and_then(|id| self.context.arena().node_ref(self.source, id))
                        .and_then(|name| {
                            identifier_or_literal_text(self.context.arena(), name).ok()
                        })
                        .ok_or(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::ExportSpecifier,
                            field: "name",
                        })?;
                    let mut target = self.create_identifier(&generated_name)?;
                    if apply_import_helpers && property == "default" {
                        self.request_import_default_helper()?;
                        let helper = self.context.factory()?.create_unscoped_helper_identifier(
                            self.source,
                            EmitHelperName::ImportDefault,
                        )?;
                        target = self.create_call(helper, vec![target])?;
                    }
                    let value = if is_identifier_export_name(&property) {
                        self.create_property_access(target, &property)?
                    } else {
                        let property = self.create_string_literal(&property)?;
                        self.create_element_access(target, property)?
                    };
                    let statement = self.create_live_export_statement(&exported, value)?;
                    self.set_original_and_range(statement, specifier)?;
                    statements.push(statement);
                }
                Ok(statements)
            }
            NodeData::NamespaceExport(namespace) => {
                let exported = namespace
                    .name
                    .and_then(|id| self.context.arena().node_ref(self.source, id))
                    .and_then(|name| identifier_or_literal_text(self.context.arena(), name).ok())
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::NamespaceExport,
                        field: "name",
                    })?;
                let mut value = if self.module_kind == MODULE_AMD {
                    let generated_name = plan
                        .runtime_name
                        .as_deref()
                        .expect("an AMD namespace export owns a generated module binding");
                    self.create_identifier(generated_name)?
                } else {
                    self.create_require_call(module_specifier)?
                };
                if self.es_module_interop {
                    self.request_import_star_helper()?;
                    let helper = self.context.factory()?.create_unscoped_helper_identifier(
                        self.source,
                        EmitHelperName::ImportStar,
                    )?;
                    value = self.create_call(helper, vec![value])?;
                }
                let target = self.create_export_access(&exported)?;
                let assignment = self.create_assignment(target, value)?;
                let statement = self.create_expression_statement(assignment)?;
                self.set_original_and_range(statement, original)?;
                Ok(vec![statement])
            }
            _ => Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ExportDeclaration,
                field: "export_clause",
            }),
        }
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
        let declarations =
            variable_declarations(self.context.arena(), self.source, data.declaration_list)?;
        if !direct_export && declarations.is_empty() {
            // tsc's non-export branch is a plain visitEachChild. In recovery
            // trees Rust can represent the missing identifier in `var;` or
            // `const;` as an empty declaration array, so an empty array must
            // not be confused with an elided variable statement.
            return Ok(vec![
                self.update_generic(original, NodeData::VariableStatement(data))?
            ]);
        }
        data.modifiers = self.remove_export_modifiers(data.modifiers)?;
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
            let local_name = variable
                .name
                .and_then(|id| self.context.arena().node_ref(self.source, id));
            let binding_pattern = local_name.is_some_and(|name| {
                self.context.arena().node(name).is_ok_and(|node| {
                    matches!(
                        node.kind,
                        SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern
                    )
                })
            });
            if direct_export && binding_pattern {
                if let (Some(pattern), Some(initializer)) = (local_name, variable.initializer) {
                    let expression = self.flatten_module_destructuring_declaration(
                        pattern,
                        self.node(initializer),
                        declaration,
                    )?;
                    let statement = self.create_expression_statement(expression)?;
                    self.set_original_and_range(statement, original)?;
                    trailing.push(statement);
                }
                continue;
            }
            let direct_plan = if direct_export {
                local.as_deref().zip(local_name).map(|(local, name)| {
                    let storage = if self.is_local_name(name) {
                        CommonJsDirectVariableStorage::LocalBinding
                    } else {
                        CommonJsDirectVariableStorage::ExportObject
                    };
                    let collector_targets = if self.info.appends_declaration_exports() {
                        exports.clone()
                    } else {
                        Default::default()
                    };
                    CommonJsDirectVariablePlan::new(local, storage, collector_targets)
                })
            } else {
                None
            };
            let direct_export_storage = direct_plan
                .as_ref()
                .is_some_and(|plan| plan.storage == CommonJsDirectVariableStorage::ExportObject);
            let local_export_initializer = variable
                .initializer
                .and_then(|initializer| self.context.arena().node_ref(self.source, initializer))
                .is_some_and(|initializer| {
                    self.context.arena().node(initializer).is_ok_and(|node| {
                        matches!(
                            node.kind,
                            SyntaxKind::ArrowFunction
                                | SyntaxKind::FunctionExpression
                                | SyntaxKind::ClassExpression
                        )
                    })
                });
            if direct_export_storage && local_export_initializer {
                let initializer = variable
                    .initializer
                    .map(|initializer| self.visit(initializer).map(TransformNode::node))
                    .transpose()?;
                variable.initializer = initializer;
                let updated =
                    self.update_variable_declaration_after_initializer(declaration, variable)?;
                retained.push(updated);
                let plan = direct_plan
                    .as_ref()
                    .expect("direct export-object storage owns a publication plan");
                let target = self.create_export_access(&plan.local_name)?;
                let value = self.create_identifier(&plan.local_name)?;
                let assignment = self.create_assignment(target, value)?;
                let statement = self.create_expression_statement(assignment)?;
                self.set_original_and_range(statement, original)?;
                self.context
                    .arena_mut()?
                    .metadata_mut(statement)
                    .add_flags(EmitFlags::NO_COMMENTS);
                trailing.push(statement);
                for export in plan.alias_targets() {
                    let target = self.create_export_access(export)?;
                    let value = self.create_identifier(&plan.local_name)?;
                    let assignment = self.create_assignment(target, value)?;
                    trailing.push(self.create_expression_statement(assignment)?);
                }
                continue;
            }
            if direct_export_storage {
                if let Some(initializer) = variable.initializer {
                    let initializer = self.visit(initializer)?;
                    let plan = direct_plan
                        .as_ref()
                        .expect("direct export-object storage owns a publication plan");
                    let target = self.create_export_access(&plan.local_name)?;
                    let assignment = self.create_assignment(target, initializer)?;
                    let statement = self.create_expression_statement(assignment)?;
                    self.set_original_and_range(statement, original)?;
                    trailing.push(statement);
                    for export in plan.alias_targets() {
                        let target = self.create_export_access(export)?;
                        let value = self.create_export_access(&plan.local_name)?;
                        let assignment = self.create_assignment(target, value)?;
                        trailing.push(self.create_expression_statement(assignment)?);
                    }
                }
                continue;
            }
            if let Some(initializer) = variable.initializer {
                let mut initializer = self.visit(initializer)?;
                if let Some(plan) = direct_plan
                    .as_ref()
                    .filter(|plan| plan.storage == CommonJsDirectVariableStorage::LocalBinding)
                {
                    for export in &plan.publication_targets {
                        let target = self.create_export_access(export)?;
                        initializer = self.create_assignment(target, initializer)?;
                    }
                }
                variable.initializer = Some(initializer.node());
            }
            let updated =
                self.update_variable_declaration_after_initializer(declaration, variable)?;
            retained.push(updated);
            if !direct_export
                && self.info.appends_declaration_exports()
                && variable_has_initializer(self.context.arena(), updated)?
            {
                trailing.extend(self.create_declaration_export_statements(declaration)?);
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

    /// Update a variable declaration after its initializer has already gone
    /// through the expression visitor. Calling the generic child visitor at
    /// this point would transform that initializer twice, which is observable
    /// for context-sensitive rewrites such as postfix export publication.
    fn update_variable_declaration_after_initializer(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::VariableDeclarationData,
    ) -> Result<TransformNode, TransformError> {
        data.name = self.visit_optional_node(data.name)?;
        data.exclamation_token = self.visit_optional_node(data.exclamation_token)?;
        data.r#type = self.visit_optional_node(data.r#type)?;
        self.update_generic_without_visit(original, NodeData::VariableDeclaration(data))
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
        self.transform_embedded_statement_with_empty_result(
            statement,
            parent,
            CommonJsEmptyEmbeddedStatement::LiftToBlock,
        )
    }

    fn transform_embedded_statement_with_empty_result(
        &mut self,
        statement: Option<NodeId>,
        parent: SyntaxKind,
        empty_result: CommonJsEmptyEmbeddedStatement,
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
        if statements.is_empty()
            && matches!(
                empty_result,
                CommonJsEmptyEmbeddedStatement::EmptyStatementWithSourceRange
            )
        {
            let empty = self.context.factory()?.create_node(
                self.source,
                NodeData::EmptyStatement(tsc_syntax::nodes::EmptyStatementData {}),
                TransformFlags::NONE,
            )?;
            return self.context.factory()?.set_text_range(empty, statement);
        }
        // `factory.liftToBlock` creates a block without the multi-line role.
        // Non-empty lifted lists are still printed vertically by the regular
        // block-list rules, while an erased embedded statement remains `{ }`.
        self.create_block(statements, false)
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
                && variable_list_has_initialized_export_binding(
                    self.context.arena(),
                    self.source,
                    initializer,
                    &self.info.exported_bindings,
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
        if !self.info.appends_declaration_exports() {
            return Ok(Vec::new());
        }
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
            assignments.extend(self.create_declaration_export_statements(declaration)?);
        }
        Ok(assignments)
    }

    /// Rust ownership form of tsc's `appendExportsOfBindingElement`: recurse
    /// through the declaration name once and materialize the explicit export
    /// specifiers registered for that local name. `collectExternalModuleInfo`
    /// deliberately records this name map even when the resolver cannot find
    /// a parse-tree declaration (for example, `export { C }` synthesized by
    /// the standard-decorator transform for its generated `let C = ...`).
    /// Resolver-backed `exported_bindings` remains the substitution plan; it
    /// is not the declaration-append plan.
    ///
    /// tsc-port: appendExportsOfDeclaration @6.0.3
    /// tsc-hash: b1e3c0856abab75bf412486742c157c5a6b6a7a41fd293c039ba0936fa70cad2
    /// tsc-span: _tsc.js:111743-111762
    fn declaration_export_plans(
        &self,
        declaration: TransformNode,
    ) -> Result<Vec<DeclarationExportPlan>, TransformError> {
        let NodeData::VariableDeclaration(data) = &self.context.arena().node(declaration)?.data
        else {
            return Ok(Vec::new());
        };
        let leaves =
            binding_name_leaves(self.context.arena(), self.source, data.name, declaration)?;
        let mut plans = Vec::new();
        for leaf in leaves {
            let local_name = identifier_or_literal_text(self.context.arena(), leaf.name)?;
            let exports = self
                .info
                .export_specifiers_by_local
                .get(local_name.as_str())
                .cloned()
                .unwrap_or_default();
            for exported_name in exports {
                let Some(location) = self
                    .info
                    .export_specifier_locations
                    .get(&(local_name.clone().into_boxed_str(), exported_name.clone()))
                    .and_then(|location| self.context.arena().node_ref(self.source, *location))
                else {
                    // A syntactic `export` modifier is published by the
                    // declaration lowering itself. This append phase owns
                    // only explicit export specifiers.
                    continue;
                };
                plans.push(DeclarationExportPlan {
                    local: leaf.name,
                    exported_name,
                    location,
                });
            }
        }
        Ok(plans)
    }

    fn create_declaration_export_statements(
        &mut self,
        declaration: TransformNode,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let plans = self.declaration_export_plans(declaration)?;
        let mut statements = Vec::with_capacity(plans.len());
        for plan in plans {
            let value = self.context.factory()?.clone_node(plan.local)?;
            self.context.factory()?.set_text_range(value, plan.local)?;
            let target = self.create_export_access(&plan.exported_name)?;
            let assignment = self.create_assignment(target, value)?;
            let statement = self.create_expression_statement(assignment)?;
            self.context
                .factory()?
                .set_text_range(statement, plan.location)?;
            self.context
                .arena_mut()?
                .metadata_mut(statement)
                .add_flags(EmitFlags::NO_COMMENTS);
            statements.push(statement);
        }
        Ok(statements)
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
            self.transform_embedded_statement_with_empty_result(
                data.statement,
                SyntaxKind::LabeledStatement,
                CommonJsEmptyEmbeddedStatement::EmptyStatementWithSourceRange,
            )?
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
            NodeData::Identifier(_)
                if !is_non_reference_identifier_node(self.context.arena(), original)? =>
            {
                self.substitute_identifier(original)?
            }
            NodeData::Identifier(_) => original,
            NodeData::ExpressionStatement(data) => {
                self.visit_expression_statement(original, data)?
            }
            NodeData::ParenthesizedExpression(data) => {
                self.visit_parenthesized_expression(original, data)?
            }
            NodeData::ShorthandPropertyAssignment(data) => {
                self.visit_shorthand_property_assignment(original, data)?
            }
            NodeData::BinaryExpression(data)
                if self.module_destructuring_assignment_needs_flattening(&data)? =>
            {
                self.flatten_module_destructuring_assignment(original, data)?
            }
            NodeData::BinaryExpression(data) => self
                .with_expression_value_use(CommonJsExpressionValueUse::Required, |visitor| {
                    visitor.visit_binary_expression(original, data)
                })?,
            NodeData::PrefixUnaryExpression(data) => {
                self.visit_prefix_unary_expression(original, data)?
            }
            NodeData::PostfixUnaryExpression(data) => {
                self.visit_postfix_unary_expression(original, data)?
            }
            NodeData::CallExpression(data) => self
                .with_expression_value_use(CommonJsExpressionValueUse::Required, |visitor| {
                    visitor.visit_call_expression(original, data)
                })?,
            NodeData::TaggedTemplateExpression(data) => self
                .with_expression_value_use(CommonJsExpressionValueUse::Required, |visitor| {
                    visitor.visit_tagged_template_expression(original, data)
                })?,
            NodeData::FunctionDeclaration(data) => self.visit_function_declaration(
                original,
                data,
                CommonJsFunctionLexicalOwner::Function {
                    kind: SyntaxKind::FunctionDeclaration,
                    concise_body: false,
                },
            )?,
            NodeData::FunctionExpression(data) => self.visit_function_expression(original, data)?,
            NodeData::ArrowFunction(data) => self.visit_arrow_function(original, data)?,
            NodeData::MethodDeclaration(data) => self.visit_method_declaration(original, data)?,
            NodeData::GetAccessor(data) => self.visit_get_accessor(original, data)?,
            NodeData::SetAccessor(data) => self.visit_set_accessor(original, data)?,
            NodeData::Constructor(data) => self.visit_constructor(original, data)?,
            NodeData::ClassStaticBlockDeclaration(data) => {
                self.visit_class_static_block(original, data)?
            }
            data => self
                .with_expression_value_use(CommonJsExpressionValueUse::Required, |visitor| {
                    visitor.update_generic(original, data)
                })?,
        };
        self.nodes.insert(id, transformed.node());
        Ok(transformed)
    }

    fn visit_function_declaration(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::FunctionDeclarationData,
        owner: CommonJsFunctionLexicalOwner,
    ) -> Result<TransformNode, TransformError> {
        data.modifiers = self.visit_optional_nodes(data.modifiers)?;
        data.asterisk_token = self.visit_optional_node(data.asterisk_token)?;
        data.name = self.visit_optional_node(data.name)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.r#type = self.visit_optional_node(data.r#type)?;
        (data.parameters, data.body) =
            self.visit_module_function_children(data.parameters, data.body, owner)?;
        self.update_generic_without_visit(original, NodeData::FunctionDeclaration(data))
    }

    fn visit_function_expression(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::FunctionExpressionData,
    ) -> Result<TransformNode, TransformError> {
        data.modifiers = self.visit_optional_nodes(data.modifiers)?;
        data.asterisk_token = self.visit_optional_node(data.asterisk_token)?;
        data.name = self.visit_optional_node(data.name)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.r#type = self.visit_optional_node(data.r#type)?;
        (data.parameters, data.body) = self.visit_module_function_children(
            data.parameters,
            data.body,
            CommonJsFunctionLexicalOwner::Function {
                kind: SyntaxKind::FunctionExpression,
                concise_body: false,
            },
        )?;
        self.update_generic_without_visit(original, NodeData::FunctionExpression(data))
    }

    fn visit_arrow_function(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ArrowFunctionData,
    ) -> Result<TransformNode, TransformError> {
        data.modifiers = self.visit_optional_nodes(data.modifiers)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.r#type = self.visit_optional_node(data.r#type)?;
        data.equals_greater_than_token =
            self.visit_optional_node(data.equals_greater_than_token)?;
        let concise_body = data
            .body
            .and_then(|body| self.context.arena().node_ref(self.source, body))
            .is_some_and(|body| {
                self.context
                    .arena()
                    .node(body)
                    .is_ok_and(|node| node.kind != SyntaxKind::Block)
            });
        (data.parameters, data.body) = self.visit_module_function_children(
            data.parameters,
            data.body,
            CommonJsFunctionLexicalOwner::Function {
                kind: SyntaxKind::ArrowFunction,
                concise_body,
            },
        )?;
        self.update_generic_without_visit(original, NodeData::ArrowFunction(data))
    }

    fn visit_method_declaration(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::MethodDeclarationData,
    ) -> Result<TransformNode, TransformError> {
        data.modifiers = self.visit_optional_nodes(data.modifiers)?;
        data.asterisk_token = self.visit_optional_node(data.asterisk_token)?;
        data.name = self.visit_optional_node(data.name)?;
        data.question_token = self.visit_optional_node(data.question_token)?;
        data.exclamation_token = self.visit_optional_node(data.exclamation_token)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.r#type = self.visit_optional_node(data.r#type)?;
        (data.parameters, data.body) = self.visit_module_function_children(
            data.parameters,
            data.body,
            CommonJsFunctionLexicalOwner::Function {
                kind: SyntaxKind::MethodDeclaration,
                concise_body: false,
            },
        )?;
        self.update_generic_without_visit(original, NodeData::MethodDeclaration(data))
    }

    fn visit_get_accessor(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::GetAccessorData,
    ) -> Result<TransformNode, TransformError> {
        data.modifiers = self.visit_optional_nodes(data.modifiers)?;
        data.name = self.visit_optional_node(data.name)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.r#type = self.visit_optional_node(data.r#type)?;
        (data.parameters, data.body) = self.visit_module_function_children(
            data.parameters,
            data.body,
            CommonJsFunctionLexicalOwner::Function {
                kind: SyntaxKind::GetAccessor,
                concise_body: false,
            },
        )?;
        self.update_generic_without_visit(original, NodeData::GetAccessor(data))
    }

    fn visit_set_accessor(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::SetAccessorData,
    ) -> Result<TransformNode, TransformError> {
        data.modifiers = self.visit_optional_nodes(data.modifiers)?;
        data.name = self.visit_optional_node(data.name)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.r#type = self.visit_optional_node(data.r#type)?;
        (data.parameters, data.body) = self.visit_module_function_children(
            data.parameters,
            data.body,
            CommonJsFunctionLexicalOwner::Function {
                kind: SyntaxKind::SetAccessor,
                concise_body: false,
            },
        )?;
        self.update_generic_without_visit(original, NodeData::SetAccessor(data))
    }

    fn visit_constructor(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ConstructorData,
    ) -> Result<TransformNode, TransformError> {
        data.modifiers = self.visit_optional_nodes(data.modifiers)?;
        data.name = self.visit_optional_node(data.name)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.r#type = self.visit_optional_node(data.r#type)?;
        (data.parameters, data.body) = self.visit_module_function_children(
            data.parameters,
            data.body,
            CommonJsFunctionLexicalOwner::Function {
                kind: SyntaxKind::Constructor,
                concise_body: false,
            },
        )?;
        self.update_generic_without_visit(original, NodeData::Constructor(data))
    }

    fn visit_class_static_block(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ClassStaticBlockDeclarationData,
    ) -> Result<TransformNode, TransformError> {
        data.modifiers = self.visit_optional_nodes(data.modifiers)?;
        (_, data.body) = self.visit_module_function_children(
            None,
            data.body,
            CommonJsFunctionLexicalOwner::Function {
                kind: SyntaxKind::ClassStaticBlockDeclaration,
                concise_body: false,
            },
        )?;
        self.update_generic_without_visit(original, NodeData::ClassStaticBlockDeclaration(data))
    }

    /// Preserve transformModule's two function traversal routes. Generic
    /// visitEachChild gives function-like nodes a fresh parameter/body
    /// lexical environment. The syntactic-export function path deliberately
    /// inherits the module environment instead.
    ///
    /// tsc-port: visitParameterList/visitFunctionBody @6.0.3
    /// tsc-hash: 6a717ad369016d425e264b0ec5d8a2917193fb0aca6afffa9cfb2b64aae302ca
    /// tsc-span: _tsc.js:91168-91287
    /// tsc-port: transformModule/visitFunctionDeclaration @6.0.3
    /// tsc-hash: 9261703288385c48070d74de75cf341f5a6423e0821c0d7035783381f2cdb948
    /// tsc-span: _tsc.js:111469-111502
    fn visit_module_function_children(
        &mut self,
        parameters: Option<NodeArrayId>,
        body: Option<NodeId>,
        owner: CommonJsFunctionLexicalOwner,
    ) -> Result<(Option<NodeArrayId>, Option<NodeId>), TransformError> {
        match owner {
            CommonJsFunctionLexicalOwner::Module => Ok((
                self.visit_optional_nodes(parameters)?,
                self.visit_optional_node(body)?,
            )),
            CommonJsFunctionLexicalOwner::Function { kind, concise_body } => {
                self.visit_owned_module_function_children(kind, concise_body, parameters, body)
            }
        }
    }

    fn visit_owned_module_function_children(
        &mut self,
        kind: SyntaxKind,
        concise_body: bool,
        parameters: Option<NodeArrayId>,
        body: Option<NodeId>,
    ) -> Result<(Option<NodeArrayId>, Option<NodeId>), TransformError> {
        let original_parameters = parameters;
        self.context.start_lexical_environment()?;
        let operation = (|| -> Result<(Option<NodeArrayId>, Option<NodeId>), TransformError> {
            self.context
                .set_lexical_environment_flags(LexicalEnvironmentFlags::IN_PARAMETERS, true)?;
            let parameters_result = self.visit_optional_nodes(original_parameters);
            let variables_hoisted_in_parameters = self
                .context
                .lexical_environment_flags()
                .contains(LexicalEnvironmentFlags::VARIABLES_HOISTED_IN_PARAMETERS);
            let clear_parameter_flag = self
                .context
                .set_lexical_environment_flags(LexicalEnvironmentFlags::IN_PARAMETERS, false);
            let parameters = parameters_result?;
            clear_parameter_flag?;
            let parameters =
                if variables_hoisted_in_parameters && self.preserves_native_parameter_defaults {
                    self.lower_module_parameter_defaults(original_parameters, parameters)?
                } else {
                    parameters
                };
            let body = self.visit_optional_node(body)?;
            Ok((parameters, body))
        })();
        let lexical_environment = self.context.end_lexical_environment();
        let (parameters, body) = operation?;
        let body = self.merge_module_function_lexical_environment(
            kind,
            concise_body,
            body,
            lexical_environment?,
        )?;
        Ok((parameters, body))
    }

    /// Move native parameter defaults into the function body when visiting a
    /// parameter allocated a hoisted temporary. This is the typed Rust form
    /// of `addDefaultValueAssignmentsIfNeeded`: every default is moved once,
    /// while binding patterns receive a distinct generated parameter binding.
    fn lower_module_parameter_defaults(
        &mut self,
        original_parameters: Option<NodeArrayId>,
        parameters: Option<NodeArrayId>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let (Some(original_parameters), Some(parameters)) = (original_parameters, parameters)
        else {
            return Ok(parameters);
        };
        let array = self.array(parameters);
        let nodes = self.context.arena().node_array(array)?.nodes.clone();
        let mut lowered = Vec::with_capacity(nodes.len());
        for parameter in nodes {
            lowered.push(self.lower_module_parameter_default(parameter)?);
        }
        let updated = self.context.factory()?.update_node_array(array, lowered)?;
        self.arrays.insert(original_parameters, updated.array());
        Ok(Some(updated.array()))
    }

    fn lower_module_parameter_default(
        &mut self,
        parameter: NodeId,
    ) -> Result<TransformNode, TransformError> {
        let parameter = self.node(parameter);
        let NodeData::Parameter(mut data) = self.context.arena().node(parameter)?.data.clone()
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::Parameter,
                field: "parameter data",
            });
        };
        if data.dot_dot_dot_token.is_some() {
            return Ok(parameter);
        }
        let name = data
            .name
            .and_then(|name| self.context.arena().node_ref(self.source, name))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::Parameter,
                field: "name",
            })?;
        let name_kind = self.context.arena().node(name)?.kind;

        if matches!(
            name_kind,
            SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern
        ) {
            let binding = CommonJsParameterBinding(self.next_generated_name().into_boxed_str());
            let alias_for_condition = self.create_identifier(&binding.0)?;
            let value = if let Some(initializer) = data
                .initializer
                .and_then(|initializer| self.context.arena().node_ref(self.source, initializer))
            {
                let undefined = self.create_void_zero()?;
                let condition = self.create_binary(
                    alias_for_condition,
                    SyntaxKind::EqualsEqualsEqualsToken,
                    undefined,
                )?;
                let alias_for_fallback = self.create_identifier(&binding.0)?;
                self.create_conditional(condition, initializer, alias_for_fallback)?
            } else {
                alias_for_condition
            };
            let declaration = self.create_variable_declaration_from_name(name, Some(value))?;
            let statement = self.create_variable_statement(vec![declaration], NodeFlags::NONE)?;
            self.context.add_initialization_statement(statement)?;
            data.name = Some(self.create_identifier(&binding.0)?.node());
            data.initializer = None;
        } else if let Some(initializer) = data
            .initializer
            .and_then(|initializer| self.context.arena().node_ref(self.source, initializer))
        {
            let name_text = identifier_or_literal_text(self.context.arena(), name)?;
            let condition_name = self.create_identifier(&name_text)?;
            let undefined = self.create_void_zero()?;
            let condition = self.create_binary(
                condition_name,
                SyntaxKind::EqualsEqualsEqualsToken,
                undefined,
            )?;
            let assignment_name = self.create_identifier(&name_text)?;
            let assignment = self.create_assignment(assignment_name, initializer)?;
            self.context
                .arena_mut()?
                .metadata_mut(assignment)
                .add_flags(EmitFlags::NO_COMMENTS | EmitFlags::NO_SOURCE_MAP);
            let statement = self.create_expression_statement(assignment)?;
            self.context
                .arena_mut()?
                .metadata_mut(statement)
                .add_flags(EmitFlags::NO_COMMENTS);
            let block = self.create_block(vec![statement], false)?;
            self.context.arena_mut()?.metadata_mut(block).add_flags(
                EmitFlags::SINGLE_LINE
                    | EmitFlags::NO_TRAILING_SOURCE_MAP
                    | EmitFlags::NO_TOKEN_SOURCE_MAPS
                    | EmitFlags::NO_COMMENTS,
            );
            let if_statement = self.create_if_statement(condition, block, None)?;
            self.context.add_initialization_statement(if_statement)?;
            data.initializer = None;
        }

        let flags = flags_after_update(
            self.context.arena(),
            parameter,
            &NodeData::Parameter(data.clone()),
        )?;
        self.context
            .factory()?
            .update_node(parameter, NodeData::Parameter(data), flags)
    }

    fn merge_module_function_lexical_environment(
        &mut self,
        function_kind: SyntaxKind,
        concise_body: bool,
        body: Option<NodeId>,
        lexical_environment: LexicalEnvironment,
    ) -> Result<Option<NodeId>, TransformError> {
        if lexical_environment.is_empty() {
            return Ok(body);
        }
        let body = body
            .and_then(|body| self.context.arena().node_ref(self.source, body))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: function_kind,
                field: "body for lexical declarations",
            })?;
        let block = if self.context.arena().node(body)?.kind == SyntaxKind::Block {
            body
        } else if concise_body && function_kind == SyntaxKind::ArrowFunction {
            let return_statement = self.context.factory()?.create_node(
                self.source,
                NodeData::ReturnStatement(tsc_syntax::nodes::ReturnStatementData {
                    expression: Some(body.node()),
                }),
                TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION,
            )?;
            self.create_block(vec![return_statement], false)?
        } else {
            return Err(TransformError::RequiredChildRemoved {
                parent: function_kind,
                field: "block function body for lexical declarations",
            });
        };
        let NodeData::Block(mut data) = self.context.arena().node(block)?.data.clone() else {
            unreachable!("function lexical body is a block")
        };
        let original_statements = data.statements;
        let mut statements =
            node_array_nodes(self.context.arena(), self.source, original_statements)?;
        let insertion = statements
            .iter()
            .take_while(|statement| {
                is_prologue_statement(self.context.arena(), **statement).unwrap_or(false)
            })
            .count();
        let mut declarations = lexical_environment.function_declarations().to_vec();
        if !lexical_environment.variable_declarations().is_empty() {
            declarations.push(
                self.create_hoisted_variable_statement(
                    lexical_environment.variable_declarations(),
                )?,
            );
        }
        declarations.extend_from_slice(lexical_environment.initialization_statements());
        statements.splice(insertion..insertion, declarations);
        data.statements = Some(
            if let Some(original) = original_statements
                .and_then(|array| self.context.arena().node_array_ref(self.source, array))
            {
                self.context
                    .factory()?
                    .update_node_array(original, statements)?
            } else {
                self.context
                    .factory()?
                    .create_node_array(self.source, statements)?
            }
            .array(),
        );
        let flags =
            flags_after_update(self.context.arena(), block, &NodeData::Block(data.clone()))?;
        Ok(Some(
            self.context
                .factory()?
                .update_node(block, NodeData::Block(data), flags)?
                .node(),
        ))
    }

    fn visit_optional_node(
        &mut self,
        node: Option<NodeId>,
    ) -> Result<Option<NodeId>, TransformError> {
        node.map(|node| self.visit(node).map(TransformNode::node))
            .transpose()
    }

    fn visit_optional_nodes(
        &mut self,
        nodes: Option<NodeArrayId>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        nodes
            .map(|nodes| self.visit_nodes(nodes))
            .transpose()
            .map(Option::flatten)
    }

    fn create_hoisted_variable_statement(
        &mut self,
        names: &[TransformNode],
    ) -> Result<TransformNode, TransformError> {
        let declarations = names
            .iter()
            .copied()
            .map(|name| self.create_variable_declaration_from_name(name, None))
            .collect::<Result<Vec<_>, _>>()?;
        let statement = self.create_variable_statement(declarations, NodeFlags::NONE)?;
        self.context
            .arena_mut()?
            .metadata_mut(statement)
            .add_flags(EmitFlags::CUSTOM_PROLOGUE);
        Ok(statement)
    }

    fn visit_expression_statement(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ExpressionStatementData,
    ) -> Result<TransformNode, TransformError> {
        data.expression =
            self.with_expression_value_use(CommonJsExpressionValueUse::Unused, |visitor| {
                data.expression
                    .map(|expression| visitor.visit(expression).map(TransformNode::node))
                    .transpose()
            })?;
        self.update_generic_without_visit(original, NodeData::ExpressionStatement(data))
    }

    fn visit_parenthesized_expression(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ParenthesizedExpressionData,
    ) -> Result<TransformNode, TransformError> {
        data.expression = data
            .expression
            .map(|expression| self.visit(expression).map(TransformNode::node))
            .transpose()?;
        self.update_generic_without_visit(original, NodeData::ParenthesizedExpression(data))
    }

    fn visit_shorthand_property_assignment(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ShorthandPropertyAssignmentData,
    ) -> Result<TransformNode, TransformError> {
        let name = data
            .name
            .and_then(|name| self.context.arena().node_ref(self.source, name))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ShorthandPropertyAssignment,
                field: "name",
            })?;
        let value = self.substitute_identifier(name)?;
        data.object_assignment_initializer = data
            .object_assignment_initializer
            .map(|initializer| self.visit(initializer).map(TransformNode::node))
            .transpose()?;
        data.modifiers = data
            .modifiers
            .map(|modifiers| self.visit_nodes(modifiers))
            .transpose()?
            .flatten();
        if value == name {
            return self.update_generic_without_visit(
                original,
                NodeData::ShorthandPropertyAssignment(data),
            );
        }
        let value = if let Some(initializer) = data
            .object_assignment_initializer
            .and_then(|initializer| self.context.arena().node_ref(self.source, initializer))
        {
            self.create_assignment(value, initializer)?
        } else {
            value
        };
        let property = self.context.factory()?.create_node(
            self.source,
            NodeData::PropertyAssignment(tsc_syntax::nodes::PropertyAssignmentData {
                name: Some(name.node()),
                initializer: Some(value.node()),
                modifiers: data.modifiers,
                question_token: None,
                exclamation_token: None,
            }),
            TransformFlags::NONE,
        )?;
        self.set_original_and_range(property, original)?;
        Ok(property)
    }

    /// tsc-port: transformModule/substituteBinaryExpression @6.0.3
    /// tsc-hash: fc6c4ddb37ad5d8398d7a3dba5ee822c7dfdad5bf009e78213a47a04f6a0f4e0
    /// tsc-span: _tsc.js:111990-112028
    fn visit_binary_expression(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::BinaryExpressionData,
    ) -> Result<TransformNode, TransformError> {
        let exports = self.exports_for_assignment(&data)?;
        let mut expression = self.update_generic(original, NodeData::BinaryExpression(data))?;
        for export in exports {
            let target = self.create_export_access(&export)?;
            expression = self.create_assignment(target, expression)?;
            self.set_original_and_range(expression, original)?;
        }
        Ok(expression)
    }

    /// tsc-port: transformModule/visitPreOrPostfixUnaryExpression @6.0.3 (prefix)
    /// tsc-hash: b8333c6cd4367aa604bc119a8ceb0bb1998ccff1484f8746490c95a48fda94e9
    /// tsc-span: _tsc.js:110905-110937
    fn visit_prefix_unary_expression(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::PrefixUnaryExpressionData,
    ) -> Result<TransformNode, TransformError> {
        let plan = if matches!(
            data.operator,
            SyntaxKind::PlusPlusToken | SyntaxKind::MinusMinusToken
        ) {
            data.operand
                .and_then(|operand| self.context.arena().node_ref(self.source, operand))
                .map(|operand| self.export_assignment_plan(operand))
                .transpose()?
                .flatten()
        } else {
            None
        };
        data.operand = data
            .operand
            .map(|operand| {
                self.with_expression_value_use(CommonJsExpressionValueUse::Required, |visitor| {
                    visitor.visit(operand).map(TransformNode::node)
                })
            })
            .transpose()?;
        let mut expression =
            self.update_generic_without_visit(original, NodeData::PrefixUnaryExpression(data))?;
        if let Some(plan) = plan {
            for export in plan.exports {
                let target = self.create_export_access(&export)?;
                expression = self.create_assignment(target, expression)?;
                self.set_original_and_range(expression, original)?;
            }
        }
        Ok(expression)
    }

    /// tsc-port: transformModule/visitPreOrPostfixUnaryExpression @6.0.3 (postfix)
    /// tsc-hash: b8333c6cd4367aa604bc119a8ceb0bb1998ccff1484f8746490c95a48fda94e9
    /// tsc-span: _tsc.js:110905-110937
    ///
    /// Publishing the updated value and preserving the postfix expression's
    /// old value are separate obligations. A discarded expression needs no
    /// temporary; a value-producing expression saves the old value in the
    /// active lexical environment before publishing the new one.
    fn visit_postfix_unary_expression(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::PostfixUnaryExpressionData,
    ) -> Result<TransformNode, TransformError> {
        let plan = if matches!(
            data.operator,
            SyntaxKind::PlusPlusToken | SyntaxKind::MinusMinusToken
        ) {
            data.operand
                .and_then(|operand| self.context.arena().node_ref(self.source, operand))
                .map(|operand| self.export_assignment_plan(operand))
                .transpose()?
                .flatten()
        } else {
            None
        };
        let visited_operand = data
            .operand
            .map(|operand| {
                self.with_expression_value_use(CommonJsExpressionValueUse::Required, |visitor| {
                    visitor.visit(operand)
                })
            })
            .transpose()?;
        data.operand = visited_operand.map(TransformNode::node);
        let update =
            self.update_generic_without_visit(original, NodeData::PostfixUnaryExpression(data))?;
        let Some(plan) = plan else {
            return Ok(update);
        };
        let Some(visited_operand) = visited_operand else {
            return Ok(update);
        };

        let mut expression = update;
        let saved_result = if self.expression_value_use == CommonJsExpressionValueUse::Required {
            let binding = self.allocate_temp_binding()?;
            let target = self.create_temp_reference(&binding)?;
            expression = self.create_assignment(target, expression)?;
            self.set_original_and_range(expression, original)?;
            Some(binding)
        } else {
            None
        };
        let current_value = self.context.factory()?.clone_node(visited_operand)?;
        expression = self.create_binary(expression, SyntaxKind::CommaToken, current_value)?;
        self.set_original_and_range(expression, original)?;
        for export in plan.exports {
            let target = self.create_export_access(&export)?;
            expression = self.create_assignment(target, expression)?;
            self.set_original_and_range(expression, original)?;
        }
        if let Some(binding) = saved_result {
            let saved_value = self.create_temp_reference(&binding)?;
            expression = self.create_binary(expression, SyntaxKind::CommaToken, saved_value)?;
            self.set_original_and_range(expression, original)?;
            expression = self.create_parenthesized(expression)?;
            self.set_original_and_range(expression, original)?;
        }
        Ok(expression)
    }

    fn exports_for_assignment(
        &self,
        data: &tsc_syntax::nodes::BinaryExpressionData,
    ) -> Result<Vec<Box<str>>, TransformError> {
        let Some(operator) = data
            .operator_token
            .and_then(|id| self.context.arena().node_ref(self.source, id))
        else {
            return Ok(Vec::new());
        };
        let operator = self.context.arena().node(operator)?.kind;
        if operator.value() < SyntaxKind::FirstAssignment.value()
            || operator.value() > SyntaxKind::LastAssignment.value()
        {
            return Ok(Vec::new());
        }
        let Some(left) = data
            .left
            .and_then(|id| self.context.arena().node_ref(self.source, id))
        else {
            return Ok(Vec::new());
        };
        let Some(plan) = self.export_assignment_plan(left)? else {
            return Ok(Vec::new());
        };
        Ok(plan
            .exports
            .into_iter()
            .filter(|export| {
                !plan.direct_export_storage || export.as_ref() != plan.local_name.as_str()
            })
            .collect())
    }

    fn export_assignment_plan(
        &self,
        identifier: TransformNode,
    ) -> Result<Option<ExportAssignmentPlan>, TransformError> {
        let NodeData::Identifier(data) = &self.context.arena().node(identifier)?.data else {
            return Ok(None);
        };
        if self.is_local_name(identifier) {
            return Ok(None);
        }
        if let Some(metadata) = self.context.arena().metadata(identifier) {
            if metadata.generated_binding_id().is_some() {
                if !metadata.generated_binding_is_file_level_optimistic()
                    || !metadata.generated_binding_reserved_in_nested_scopes()
                {
                    return Ok(None);
                }
                return Ok(self
                    .info
                    .file_level_generated_binding_exports
                    .get_for_identifier(self.context.arena(), identifier)
                    .map(|exports| ExportAssignmentPlan {
                        local_name: data.text.clone(),
                        exports: exports.to_vec(),
                        direct_export_storage: false,
                    }));
            }
        }
        let original = self.context.arena().get_original_node(identifier);
        if self.context.arena().node(original)?.pos == u32::MAX {
            return Ok(None);
        }
        let declarations = self
            .resolver
            .get_referenced_value_declarations(self.resolver_node(identifier)?)?;
        let mut exports = Vec::new();
        let mut seen = BTreeSet::new();
        for declaration in declarations {
            if let Some(bindings) = self.info.exported_bindings.get(&declaration.node()) {
                for binding in bindings {
                    if seen.insert(binding.clone()) {
                        exports.push(binding.clone());
                    }
                }
            }
        }
        if exports.is_empty() {
            return Ok(None);
        }
        let local_name = data.text.clone();
        let direct_export_storage = self
            .info
            .direct_exported_variable_names
            .contains(local_name.as_str())
            && exports
                .iter()
                .any(|export| export.as_ref() == local_name.as_str());
        Ok(Some(ExportAssignmentPlan {
            local_name,
            exports,
            direct_export_storage,
        }))
    }

    /// tsc-port: transformModule/destructuringNeedsFlattening/visitDestructuringAssignment @6.0.3
    /// tsc-hash: 99f75e4d73bb612262cbb9a3c63008122bcda1af43caed778f298a257e69ff47
    /// tsc-span: _tsc.js:110674-110721
    fn module_destructuring_assignment_needs_flattening(
        &self,
        data: &tsc_syntax::nodes::BinaryExpressionData,
    ) -> Result<bool, TransformError> {
        let operator = data
            .operator_token
            .and_then(|operator| self.context.arena().node_ref(self.source, operator))
            .map(|operator| self.context.arena().node(operator).map(|node| node.kind))
            .transpose()?;
        if operator != Some(SyntaxKind::EqualsToken) {
            return Ok(false);
        }
        let Some(left) = data
            .left
            .and_then(|left| self.context.arena().node_ref(self.source, left))
        else {
            return Ok(false);
        };
        if !matches!(
            self.context.arena().node(left)?.kind,
            SyntaxKind::ObjectLiteralExpression | SyntaxKind::ArrayLiteralExpression
        ) {
            return Ok(false);
        }
        self.module_destructuring_pattern_needs_flattening(left)
    }

    fn module_destructuring_pattern_needs_flattening(
        &self,
        pattern: TransformNode,
    ) -> Result<bool, TransformError> {
        match &self.context.arena().node(pattern)?.data {
            NodeData::Identifier(_) => {
                let Some(plan) = self.export_assignment_plan(pattern)? else {
                    return Ok(false);
                };
                Ok(plan.exports.len() > usize::from(plan.direct_export_storage))
            }
            NodeData::ObjectLiteralExpression(data) => {
                for element in node_array_nodes(self.context.arena(), self.source, data.properties)?
                {
                    if self.module_destructuring_pattern_needs_flattening(
                        self.module_destructuring_element(element)?.target,
                    )? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            NodeData::ObjectBindingPattern(data) => {
                for element in node_array_nodes(self.context.arena(), self.source, data.elements)? {
                    if self.module_destructuring_pattern_needs_flattening(
                        self.module_destructuring_element(element)?.target,
                    )? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            NodeData::ArrayLiteralExpression(data) => {
                for element in node_array_nodes(self.context.arena(), self.source, data.elements)? {
                    if self.context.arena().node(element)?.kind == SyntaxKind::OmittedExpression {
                        continue;
                    }
                    if self.module_destructuring_pattern_needs_flattening(
                        self.module_destructuring_element(element)?.target,
                    )? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            NodeData::ArrayBindingPattern(data) => {
                for element in node_array_nodes(self.context.arena(), self.source, data.elements)? {
                    if self.context.arena().node(element)?.kind == SyntaxKind::OmittedExpression {
                        continue;
                    }
                    if self.module_destructuring_pattern_needs_flattening(
                        self.module_destructuring_element(element)?.target,
                    )? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            NodeData::ParenthesizedExpression(data) => data
                .expression
                .and_then(|expression| self.context.arena().node_ref(self.source, expression))
                .map(|expression| self.module_destructuring_pattern_needs_flattening(expression))
                .transpose()
                .map(Option::unwrap_or_default),
            _ => Ok(false),
        }
    }

    /// Rust ownership adaptation of the shared destructuring flattener used
    /// by transformModule. A typed expression plan owns evaluation order;
    /// export publication is applied only when each leaf is materialized.
    fn flatten_module_destructuring_assignment(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::BinaryExpressionData,
    ) -> Result<TransformNode, TransformError> {
        let pattern = data
            .left
            .and_then(|left| self.context.arena().node_ref(self.source, left))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::BinaryExpression,
                field: "left",
            })?;
        let right =
            self.with_expression_value_use(CommonJsExpressionValueUse::Required, |visitor| {
                data.right
                    .map(|right| visitor.visit(right))
                    .transpose()?
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::BinaryExpression,
                        field: "right",
                    })
            })?;
        let mut expressions = Vec::new();
        let force_fresh_right = match &self.context.arena().node(right)?.data {
            NodeData::Identifier(identifier) => {
                self.module_pattern_assigns_to_identifier(pattern, &identifier.text)?
            }
            _ => false,
        } || self
            .module_pattern_contains_nonliteral_computed_name(pattern)?;
        let mut value = if force_fresh_right {
            self.ensure_module_destructuring_identifier(
                &mut expressions,
                right,
                false,
                Some(original),
            )?
        } else {
            right
        };
        if self.expression_value_use == CommonJsExpressionValueUse::Required {
            value = self.ensure_module_destructuring_identifier(
                &mut expressions,
                value,
                true,
                Some(original),
            )?;
        }
        self.flatten_module_destructuring_target(&mut expressions, pattern, value, Some(original))?;
        if self.expression_value_use == CommonJsExpressionValueUse::Required {
            expressions.push(value);
        }
        let expression = self.inline_module_destructuring_expressions(expressions, value)?;
        self.set_original_and_range(expression, original)?;
        Ok(expression)
    }

    /// Declaration-form entry to the same leaf publication pipeline used by
    /// assignment expressions. The caller owns statement placement while this
    /// method owns initializer evaluation order and declaration identity.
    fn flatten_module_destructuring_declaration(
        &mut self,
        pattern: TransformNode,
        initializer: TransformNode,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let value = self
            .with_expression_value_use(CommonJsExpressionValueUse::Required, |visitor| {
                visitor.visit(initializer.node())
            })?;
        let mut expressions = Vec::new();
        self.flatten_module_destructuring_target(&mut expressions, pattern, value, Some(original))?;
        let expression = self.inline_module_destructuring_expressions(expressions, value)?;
        self.set_original_and_range(expression, original)?;
        Ok(expression)
    }

    fn flatten_module_destructuring_target(
        &mut self,
        expressions: &mut Vec<TransformNode>,
        target: TransformNode,
        value: TransformNode,
        original: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        match self.context.arena().node(target)?.data.clone() {
            NodeData::ObjectLiteralExpression(data) => {
                self.flatten_module_object_pattern(expressions, data.properties, value, original)
            }
            NodeData::ObjectBindingPattern(data) => {
                self.flatten_module_object_pattern(expressions, data.elements, value, original)
            }
            NodeData::ArrayLiteralExpression(data) => {
                self.flatten_module_array_pattern(expressions, data.elements, value, original)
            }
            NodeData::ArrayBindingPattern(data) => {
                self.flatten_module_array_pattern(expressions, data.elements, value, original)
            }
            NodeData::ParenthesizedExpression(data) => {
                let target = data
                    .expression
                    .and_then(|expression| self.context.arena().node_ref(self.source, expression))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ParenthesizedExpression,
                        field: "expression",
                    })?;
                self.flatten_module_destructuring_target(expressions, target, value, original)
            }
            _ => {
                let assignment = self.create_module_export_assignment(target, value)?;
                if let Some(original) = original {
                    self.set_original_and_range(assignment, original)?;
                }
                expressions.push(assignment);
                Ok(())
            }
        }
    }

    fn flatten_module_destructuring_element(
        &mut self,
        expressions: &mut Vec<TransformNode>,
        element: ModuleDestructuringElement,
        mut value: TransformNode,
    ) -> Result<(), TransformError> {
        if let Some(initializer) = element.initializer {
            let initializer = self
                .with_expression_value_use(CommonJsExpressionValueUse::Required, |visitor| {
                    visitor.visit(initializer.node())
                })?;
            let initializer_is_simple = self.is_simple_inlineable_expression(initializer)?;
            value = self.create_module_destructuring_default(
                expressions,
                value,
                initializer,
                element.original,
            )?;
            if self.module_is_destructuring_pattern(element.target)? && !initializer_is_simple {
                value = self.ensure_module_destructuring_identifier(
                    expressions,
                    value,
                    true,
                    Some(element.original),
                )?;
            }
        }
        self.flatten_module_destructuring_target(
            expressions,
            element.target,
            value,
            Some(element.original),
        )
    }

    fn flatten_module_object_pattern(
        &mut self,
        expressions: &mut Vec<TransformNode>,
        elements: Option<NodeArrayId>,
        mut value: TransformNode,
        original: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        let elements = node_array_nodes(self.context.arena(), self.source, elements)?;
        if elements.len() != 1 {
            value =
                self.ensure_module_destructuring_identifier(expressions, value, true, original)?;
        }
        let mut excluded = Vec::new();
        for (index, node) in elements.iter().copied().enumerate() {
            let element = self.module_destructuring_element(node)?;
            if element.rest {
                if index + 1 == elements.len() {
                    let rest =
                        self.create_module_object_rest(value, &excluded, element.original)?;
                    self.flatten_module_destructuring_element(expressions, element, rest)?;
                }
                continue;
            }
            let (property_value, exclusion) =
                self.create_module_destructuring_property_access(expressions, value, element)?;
            excluded.push(exclusion);
            self.flatten_module_destructuring_element(expressions, element, property_value)?;
        }
        Ok(())
    }

    fn flatten_module_array_pattern(
        &mut self,
        expressions: &mut Vec<TransformNode>,
        elements: Option<NodeArrayId>,
        mut value: TransformNode,
        original: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        let elements = node_array_nodes(self.context.arena(), self.source, elements)?;
        let all_omitted = !elements.is_empty()
            && elements.iter().all(|element| {
                self.context
                    .arena()
                    .node(*element)
                    .is_ok_and(|node| node.kind == SyntaxKind::OmittedExpression)
            });
        if elements.len() != 1 || all_omitted {
            value =
                self.ensure_module_destructuring_identifier(expressions, value, true, original)?;
        }
        for (index, node) in elements.into_iter().enumerate() {
            if self.context.arena().node(node)?.kind == SyntaxKind::OmittedExpression {
                continue;
            }
            let element = self.module_destructuring_element(node)?;
            let element_value = if element.rest {
                let base = self.context.factory()?.clone_node(value)?;
                let slice = self.create_property_access(base, "slice")?;
                let index = self.create_numeric_literal(&index.to_string())?;
                self.create_call(slice, vec![index])?
            } else {
                let base = self.context.factory()?.clone_node(value)?;
                let index = self.create_numeric_literal(&index.to_string())?;
                self.create_element_access(base, index)?
            };
            self.flatten_module_destructuring_element(expressions, element, element_value)?;
        }
        Ok(())
    }

    fn create_module_export_assignment(
        &mut self,
        target: TransformNode,
        value: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let Some(plan) = self.export_assignment_plan(target)? else {
            let target = self
                .with_expression_value_use(CommonJsExpressionValueUse::Required, |visitor| {
                    visitor.visit(target.node())
                })?;
            return self.create_assignment(target, value);
        };
        let mut expression = if plan.direct_export_storage {
            value
        } else {
            let target = self.create_identifier(&plan.local_name)?;
            self.create_assignment(target, value)?
        };
        for export in plan.exports {
            let target = self.create_export_access(&export)?;
            expression = self.create_assignment(target, expression)?;
        }
        Ok(expression)
    }

    fn create_module_destructuring_property_access(
        &mut self,
        expressions: &mut Vec<TransformNode>,
        value: TransformNode,
        element: ModuleDestructuringElement,
    ) -> Result<(TransformNode, ModuleDestructuringExcludedProperty), TransformError> {
        let property_name = element
            .property_name
            .ok_or(TransformError::RequiredChildRemoved {
                parent: self.context.arena().node(element.original)?.kind,
                field: "property name",
            })?;
        let base = self.context.factory()?.clone_node(value)?;
        if let NodeData::ComputedPropertyName(data) =
            self.context.arena().node(property_name)?.data.clone()
        {
            let argument =
                self.with_expression_value_use(CommonJsExpressionValueUse::Required, |visitor| {
                    data.expression
                        .map(|expression| visitor.visit(expression))
                        .transpose()?
                        .ok_or(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::ComputedPropertyName,
                            field: "expression",
                        })
                })?;
            let argument = self.ensure_module_destructuring_identifier(
                expressions,
                argument,
                false,
                Some(property_name),
            )?;
            let access_argument = self.context.factory()?.clone_node(argument)?;
            let access = self.create_element_access(base, access_argument)?;
            return Ok((
                access,
                ModuleDestructuringExcludedProperty::Computed(argument),
            ));
        }
        let kind = self.context.arena().node(property_name)?.kind;
        if matches!(
            kind,
            SyntaxKind::StringLiteral | SyntaxKind::NumericLiteral | SyntaxKind::BigIntLiteral
        ) {
            let argument = self.context.factory()?.clone_node(property_name)?;
            return Ok((
                self.create_element_access(base, argument)?,
                ModuleDestructuringExcludedProperty::Named(property_name),
            ));
        }
        let property = identifier_or_literal_text(self.context.arena(), property_name)?;
        Ok((
            self.create_property_access(base, &property)?,
            ModuleDestructuringExcludedProperty::Named(property_name),
        ))
    }

    fn create_module_destructuring_default(
        &mut self,
        expressions: &mut Vec<TransformNode>,
        value: TransformNode,
        initializer: TransformNode,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let value =
            self.ensure_module_destructuring_identifier(expressions, value, true, Some(original))?;
        let condition_value = self.context.factory()?.clone_node(value)?;
        let undefined = self.create_void_zero()?;
        let condition = self.create_binary(
            condition_value,
            SyntaxKind::EqualsEqualsEqualsToken,
            undefined,
        )?;
        let fallback = self.context.factory()?.clone_node(value)?;
        self.create_conditional(condition, initializer, fallback)
    }

    fn create_module_object_rest(
        &mut self,
        value: TransformNode,
        excluded: &[ModuleDestructuringExcludedProperty],
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.request_emit_helper(helpers::object_rest())?;
        let mut properties = Vec::with_capacity(excluded.len());
        for property in excluded {
            let property = match *property {
                ModuleDestructuringExcludedProperty::Named(name) => {
                    let text = identifier_or_literal_text(self.context.arena(), name)?;
                    self.create_string_literal(&text)?
                }
                ModuleDestructuringExcludedProperty::Computed(temp) => {
                    let type_value = self.context.factory()?.clone_node(temp)?;
                    let type_of = self.create_typeof(type_value)?;
                    let symbol = self.create_string_literal("symbol")?;
                    let condition =
                        self.create_binary(type_of, SyntaxKind::EqualsEqualsEqualsToken, symbol)?;
                    let symbol_value = self.context.factory()?.clone_node(temp)?;
                    let string_value = self.context.factory()?.clone_node(temp)?;
                    let empty = self.create_string_literal("")?;
                    let as_string =
                        self.create_binary(string_value, SyntaxKind::PlusToken, empty)?;
                    self.create_conditional(condition, symbol_value, as_string)?
                }
            };
            properties.push(property);
        }
        let excluded = self.create_array_literal(properties)?;
        self.context.factory()?.set_text_range(excluded, original)?;
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(self.source, EmitHelperName::Rest)?;
        let value = self.context.factory()?.clone_node(value)?;
        self.create_call(helper, vec![value, excluded])
    }

    fn ensure_module_destructuring_identifier(
        &mut self,
        expressions: &mut Vec<TransformNode>,
        value: TransformNode,
        reuse_identifier: bool,
        original: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        if reuse_identifier && self.context.arena().node(value)?.kind == SyntaxKind::Identifier {
            return Ok(value);
        }
        let binding = self.allocate_temp_binding()?;
        let target = self.create_temp_reference(&binding)?;
        let assignment = self.create_assignment(target, value)?;
        if let Some(original) = original {
            self.context
                .factory()?
                .set_text_range(assignment, original)?;
        }
        expressions.push(assignment);
        self.create_temp_reference(&binding)
    }

    fn inline_module_destructuring_expressions(
        &mut self,
        expressions: Vec<TransformNode>,
        fallback: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let mut expressions = expressions.into_iter();
        let Some(mut expression) = expressions.next() else {
            return Ok(fallback);
        };
        for next in expressions {
            expression = self.create_binary(expression, SyntaxKind::CommaToken, next)?;
        }
        Ok(expression)
    }

    fn module_destructuring_element(
        &self,
        element: TransformNode,
    ) -> Result<ModuleDestructuringElement, TransformError> {
        match &self.context.arena().node(element)?.data {
            NodeData::BindingElement(data) => {
                let target = data
                    .name
                    .and_then(|name| self.context.arena().node_ref(self.source, name))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::BindingElement,
                        field: "name",
                    })?;
                Ok(ModuleDestructuringElement {
                    original: element,
                    target,
                    property_name: data
                        .property_name
                        .and_then(|name| self.context.arena().node_ref(self.source, name))
                        .or_else(|| data.dot_dot_dot_token.is_none().then_some(target)),
                    initializer: data.initializer.and_then(|initializer| {
                        self.context.arena().node_ref(self.source, initializer)
                    }),
                    rest: data.dot_dot_dot_token.is_some(),
                })
            }
            NodeData::PropertyAssignment(data) => {
                let initializer = data
                    .initializer
                    .and_then(|initializer| self.context.arena().node_ref(self.source, initializer))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::PropertyAssignment,
                        field: "initializer",
                    })?;
                let (target, initializer) = self.split_module_destructuring_default(initializer)?;
                Ok(ModuleDestructuringElement {
                    original: element,
                    target,
                    property_name: data
                        .name
                        .and_then(|name| self.context.arena().node_ref(self.source, name)),
                    initializer,
                    rest: false,
                })
            }
            NodeData::ShorthandPropertyAssignment(data) => {
                let target = data
                    .name
                    .and_then(|name| self.context.arena().node_ref(self.source, name))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ShorthandPropertyAssignment,
                        field: "name",
                    })?;
                Ok(ModuleDestructuringElement {
                    original: element,
                    target,
                    property_name: Some(target),
                    initializer: data.object_assignment_initializer.and_then(|initializer| {
                        self.context.arena().node_ref(self.source, initializer)
                    }),
                    rest: false,
                })
            }
            NodeData::SpreadAssignment(data) => Ok(ModuleDestructuringElement {
                original: element,
                target: data
                    .expression
                    .and_then(|expression| self.context.arena().node_ref(self.source, expression))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::SpreadAssignment,
                        field: "expression",
                    })?,
                property_name: None,
                initializer: None,
                rest: true,
            }),
            NodeData::SpreadElement(data) => Ok(ModuleDestructuringElement {
                original: element,
                target: data
                    .expression
                    .and_then(|expression| self.context.arena().node_ref(self.source, expression))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::SpreadElement,
                        field: "expression",
                    })?,
                property_name: None,
                initializer: None,
                rest: true,
            }),
            NodeData::BinaryExpression(_) => {
                let (target, initializer) = self.split_module_destructuring_default(element)?;
                Ok(ModuleDestructuringElement {
                    original: element,
                    target,
                    property_name: None,
                    initializer,
                    rest: false,
                })
            }
            NodeData::OmittedExpression(_) => Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::OmittedExpression,
                field: "pattern target",
            }),
            _ => Ok(ModuleDestructuringElement {
                original: element,
                target: element,
                property_name: None,
                initializer: None,
                rest: false,
            }),
        }
    }

    fn split_module_destructuring_default(
        &self,
        expression: TransformNode,
    ) -> Result<(TransformNode, Option<TransformNode>), TransformError> {
        let NodeData::BinaryExpression(data) = &self.context.arena().node(expression)?.data else {
            return Ok((expression, None));
        };
        let operator = data
            .operator_token
            .and_then(|operator| self.context.arena().node_ref(self.source, operator))
            .map(|operator| self.context.arena().node(operator).map(|node| node.kind))
            .transpose()?;
        if operator != Some(SyntaxKind::EqualsToken) {
            return Ok((expression, None));
        }
        let target = data
            .left
            .and_then(|left| self.context.arena().node_ref(self.source, left))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::BinaryExpression,
                field: "left",
            })?;
        let initializer = data
            .right
            .and_then(|right| self.context.arena().node_ref(self.source, right))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::BinaryExpression,
                field: "right",
            })?;
        Ok((target, Some(initializer)))
    }

    fn module_pattern_assigns_to_identifier(
        &self,
        pattern: TransformNode,
        identifier: &str,
    ) -> Result<bool, TransformError> {
        if let NodeData::Identifier(data) = &self.context.arena().node(pattern)?.data {
            return Ok(data.text == identifier);
        }
        let elements = match &self.context.arena().node(pattern)?.data {
            NodeData::ObjectLiteralExpression(data) => {
                node_array_nodes(self.context.arena(), self.source, data.properties)?
            }
            NodeData::ObjectBindingPattern(data) => {
                node_array_nodes(self.context.arena(), self.source, data.elements)?
            }
            NodeData::ArrayLiteralExpression(data) => {
                node_array_nodes(self.context.arena(), self.source, data.elements)?
            }
            NodeData::ArrayBindingPattern(data) => {
                node_array_nodes(self.context.arena(), self.source, data.elements)?
            }
            _ => return Ok(false),
        };
        for element in elements {
            if self.context.arena().node(element)?.kind == SyntaxKind::OmittedExpression {
                continue;
            }
            if self.module_pattern_assigns_to_identifier(
                self.module_destructuring_element(element)?.target,
                identifier,
            )? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn module_pattern_contains_nonliteral_computed_name(
        &self,
        pattern: TransformNode,
    ) -> Result<bool, TransformError> {
        let elements = match &self.context.arena().node(pattern)?.data {
            NodeData::ObjectLiteralExpression(data) => {
                node_array_nodes(self.context.arena(), self.source, data.properties)?
            }
            NodeData::ObjectBindingPattern(data) => {
                node_array_nodes(self.context.arena(), self.source, data.elements)?
            }
            NodeData::ArrayLiteralExpression(data) => {
                node_array_nodes(self.context.arena(), self.source, data.elements)?
            }
            NodeData::ArrayBindingPattern(data) => {
                node_array_nodes(self.context.arena(), self.source, data.elements)?
            }
            _ => return Ok(false),
        };
        for element in elements {
            if self.context.arena().node(element)?.kind == SyntaxKind::OmittedExpression {
                continue;
            }
            let element = self.module_destructuring_element(element)?;
            if let Some(property_name) = element.property_name {
                if let NodeData::ComputedPropertyName(data) =
                    &self.context.arena().node(property_name)?.data
                {
                    let literal = data
                        .expression
                        .and_then(|expression| {
                            self.context.arena().node_ref(self.source, expression)
                        })
                        .is_some_and(|expression| {
                            self.context.arena().node(expression).is_ok_and(|node| {
                                matches!(
                                    node.kind,
                                    SyntaxKind::StringLiteral
                                        | SyntaxKind::NumericLiteral
                                        | SyntaxKind::BigIntLiteral
                                        | SyntaxKind::NoSubstitutionTemplateLiteral
                                )
                            })
                        });
                    if !literal {
                        return Ok(true);
                    }
                }
            }
            if self.module_pattern_contains_nonliteral_computed_name(element.target)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn module_is_destructuring_pattern(&self, node: TransformNode) -> Result<bool, TransformError> {
        Ok(matches!(
            self.context.arena().node(node)?.kind,
            SyntaxKind::ObjectLiteralExpression
                | SyntaxKind::ArrayLiteralExpression
                | SyntaxKind::ObjectBindingPattern
                | SyntaxKind::ArrayBindingPattern
        ))
    }

    fn visit_call_expression(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::CallExpressionData,
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
            if matches!(
                self.module_kind,
                MODULE_NODE16 | MODULE_NODE18 | MODULE_NODE20 | MODULE_NODE_NEXT
            ) {
                let original_array = data
                    .arguments
                    .and_then(|array| self.context.arena().node_array_ref(self.source, array))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::CallExpression,
                        field: "arguments",
                    })?;
                let mut visited = Vec::with_capacity(arguments.len());
                for argument in arguments {
                    visited.push(self.visit(argument.node())?);
                }
                if let Some(argument) = visited.first_mut() {
                    *argument = self.rewrite_import_argument(*argument)?;
                }
                data.arguments = Some(
                    self.context
                        .factory()?
                        .update_node_array(original_array, visited)?
                        .array(),
                );
                data.type_arguments = None;
                return self.update_generic_without_visit(original, NodeData::CallExpression(data));
            }
            // AMD emits its executor parameters before the dependency
            // expression. Reserve those generated bindings before descending
            // into a nested import so ordinal assignment follows emit order.
            // UMD emits the copied argument before its AMD branch, so it
            // intentionally reserves later in `create_umd_dynamic_import`.
            let amd_bindings = (self.module_kind == MODULE_AMD)
                .then(|| self.reserve_amd_dynamic_import_bindings());
            // Grammar diagnostics do not suppress JavaScript emission. tsc's
            // module transform carries `firstOrUndefined(node.arguments)`
            // through this boundary: CommonJS emits `require()`, AMD uses an
            // omitted dependency element, and UMD stabilizes `void 0`.
            let argument = arguments
                .first()
                .copied()
                .map(|argument| self.visit(argument.node()))
                .transpose()?;
            if let Some(amd_bindings) = amd_bindings {
                let argument = argument
                    .map(|argument| self.rewrite_import_argument(argument))
                    .transpose()?;
                let transformed = self.create_amd_dynamic_import(argument, amd_bindings)?;
                self.set_original_and_range(transformed, original)?;
                return Ok(transformed);
            }
            if self.module_kind == MODULE_UMD {
                let transformed = self.create_umd_dynamic_import(argument)?;
                self.set_original_and_range(transformed, original)?;
                return Ok(transformed);
            }
            let argument = argument
                .map(|argument| self.rewrite_import_argument(argument))
                .transpose()?;
            let transformed = self.create_common_js_dynamic_import_value(argument, false)?;
            self.set_original_and_range(transformed, original)?;
            return Ok(transformed);
        }

        // tsc decides indirect-call syntax from the substitution result, not
        // merely from the fact that a symbol was imported. An AMD
        // import-equals reference substitutes to its local parameter
        // identifier and keeps `fn()`, while a named CommonJS import or a
        // direct export substitutes to a property access and needs
        // `(0, object.fn)()` to erase the receiver.
        let callee_requires_receiver_erasure = data
            .expression
            .and_then(|id| self.context.arena().node_ref(self.source, id))
            .is_some_and(|callee| {
                self.context
                    .arena()
                    .node(callee)
                    .is_ok_and(|node| node.kind == SyntaxKind::Identifier)
                    && !self
                        .context
                        .arena()
                        .metadata(callee)
                        .is_some_and(|metadata| metadata.flags().contains(EmitFlags::HELPER_NAME))
            });
        let mut node_data = NodeData::CallExpression(data);
        try_visit_each_child(&mut node_data, self)?;
        let NodeData::CallExpression(mut data) = node_data else {
            unreachable!("call expression visitor preserves kind")
        };
        let substituted_callee_is_non_identifier = data
            .expression
            .and_then(|id| self.context.arena().node_ref(self.source, id))
            .is_some_and(|callee| {
                self.context
                    .arena()
                    .node(callee)
                    .is_ok_and(|node| node.kind != SyntaxKind::Identifier)
            });
        if callee_requires_receiver_erasure && substituted_callee_is_non_identifier {
            let callee = data
                .expression
                .and_then(|id| self.context.arena().node_ref(self.source, id))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::CallExpression,
                    field: "expression",
                })?;
            data.expression = Some(self.create_receiver_erased_expression(callee)?.node());
        }
        self.update_generic_without_visit(original, NodeData::CallExpression(data))
    }

    /// Preserve the value-call semantics of an imported identifier after the
    /// module substitution turns it into a property access. A tagged template
    /// invokes its tag with the same receiver rules as a call expression, so
    /// `tag\`...\`` must become `(0, module.tag)\`...\`` rather than
    /// `module.tag\`...\``.
    ///
    /// tsc-port: substituteTaggedTemplateExpression @6.0.3
    /// tsc-hash: 7ae75d422344e617c1fff45272b14d44abe2582c0f4922f6d9e9078b090f038b
    /// tsc-span: _tsc.js:111927-111945
    fn visit_tagged_template_expression(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::TaggedTemplateExpressionData,
    ) -> Result<TransformNode, TransformError> {
        let tag_requires_receiver_erasure =
            data.tag
                .and_then(|id| self.context.arena().node_ref(self.source, id))
                .is_some_and(|tag| {
                    self.context
                        .arena()
                        .node(tag)
                        .is_ok_and(|node| node.kind == SyntaxKind::Identifier)
                        && !self.context.arena().metadata(tag).is_some_and(|metadata| {
                            metadata.flags().contains(EmitFlags::HELPER_NAME)
                        })
                });
        let mut node_data = NodeData::TaggedTemplateExpression(data);
        try_visit_each_child(&mut node_data, self)?;
        let NodeData::TaggedTemplateExpression(mut data) = node_data else {
            unreachable!("tagged-template visitor preserves kind")
        };
        let substituted_tag_is_non_identifier = data
            .tag
            .and_then(|id| self.context.arena().node_ref(self.source, id))
            .is_some_and(|tag| {
                self.context
                    .arena()
                    .node(tag)
                    .is_ok_and(|node| node.kind != SyntaxKind::Identifier)
            });
        if tag_requires_receiver_erasure && substituted_tag_is_non_identifier {
            let tag = data
                .tag
                .and_then(|id| self.context.arena().node_ref(self.source, id))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::TaggedTemplateExpression,
                    field: "tag",
                })?;
            data.tag = Some(self.create_receiver_erased_expression(tag)?.node());
        }
        self.update_generic_without_visit(original, NodeData::TaggedTemplateExpression(data))
    }

    fn create_receiver_erased_expression(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
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
                right: Some(expression.node()),
            }),
            TransformFlags::NONE,
        )?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ParenthesizedExpression(tsc_syntax::nodes::ParenthesizedExpressionData {
                expression: Some(indirect.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_common_js_dynamic_import_value(
        &mut self,
        argument: Option<TransformNode>,
        is_inlineable: bool,
    ) -> Result<TransformNode, TransformError> {
        let (resolve_arguments, require_argument, parameters) = match argument {
            Some(argument)
                if !is_inlineable && !self.is_simple_inlineable_expression(argument)? =>
            {
                let template = self.create_string_coercion_template(argument)?;
                let parameter = self.create_parameter("s")?;
                (
                    vec![template],
                    Some(self.create_identifier("s")?),
                    vec![parameter],
                )
            }
            argument => (Vec::new(), argument, Vec::new()),
        };
        let require = self.create_raw_require_call_optional(require_argument)?;
        let loaded = if self.es_module_interop {
            self.request_import_star_helper()?;
            let helper = self
                .context
                .factory()?
                .create_unscoped_helper_identifier(self.source, EmitHelperName::ImportStar)?;
            self.create_call(helper, vec![require])?
        } else {
            require
        };
        let arrow = self.create_arrow_function(parameters, loaded)?;
        let promise = self.create_identifier("Promise")?;
        let resolve = self.create_property_access(promise, "resolve")?;
        let resolved = self.create_call(resolve, resolve_arguments)?;
        let then = self.create_property_access(resolved, "then")?;
        self.create_call(then, vec![arrow])
    }

    fn reserve_amd_dynamic_import_bindings(&mut self) -> AmdDynamicImportBindings {
        self.dynamic_import_ordinal += 1;
        AmdDynamicImportBindings {
            resolve: format!("resolve_{}", self.dynamic_import_ordinal).into_boxed_str(),
            reject: format!("reject_{}", self.dynamic_import_ordinal).into_boxed_str(),
        }
    }

    fn create_amd_dynamic_import(
        &mut self,
        argument: Option<TransformNode>,
        bindings: AmdDynamicImportBindings,
    ) -> Result<TransformNode, TransformError> {
        let resolve_parameter = self.create_parameter(&bindings.resolve)?;
        let reject_parameter = self.create_parameter(&bindings.reject)?;
        let (dependency, has_trailing_comma) = match argument {
            Some(argument) => (argument, false),
            None => (self.create_omitted_expression()?, true),
        };
        let dependency =
            self.create_array_literal_with_trailing_comma(vec![dependency], has_trailing_comma)?;
        let require = self.create_identifier("require")?;
        let resolve = self.create_identifier(&bindings.resolve)?;
        let reject = self.create_identifier(&bindings.reject)?;
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
            let helper = self
                .context
                .factory()?
                .create_unscoped_helper_identifier(self.source, EmitHelperName::ImportStar)?;
            self.create_call(then, vec![helper])
        } else {
            Ok(loaded)
        }
    }

    fn create_umd_dynamic_import(
        &mut self,
        argument: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let argument = match argument {
            Some(argument) => self.rewrite_import_argument(argument)?,
            None => self.create_void_zero()?,
        };
        if self.is_simple_copiable_expression(argument)? {
            let asynchronous_argument = self.clone_umd_dynamic_import_argument(argument)?;
            let common_js = self.create_common_js_dynamic_import_value(Some(argument), false)?;
            let bindings = self.reserve_amd_dynamic_import_bindings();
            let amd = self.create_amd_dynamic_import(Some(asynchronous_argument), bindings)?;
            let condition = self.create_identifier("__syncRequire")?;
            return self.create_conditional(condition, common_js, amd);
        }

        let binding = self.allocate_temp_binding()?;
        let temp = self.create_temp_reference(&binding)?;
        let assignment = self.create_assignment(temp, argument)?;
        let common_argument = self.create_temp_reference(&binding)?;
        let common_js = self.create_common_js_dynamic_import_value(Some(common_argument), true)?;
        let asynchronous_argument = self.create_temp_reference(&binding)?;
        let bindings = self.reserve_amd_dynamic_import_bindings();
        let amd = self.create_amd_dynamic_import(Some(asynchronous_argument), bindings)?;
        let condition = self.create_identifier("__syncRequire")?;
        let conditional = self.create_conditional(condition, common_js, amd)?;
        self.create_binary(assignment, SyntaxKind::CommaToken, conditional)
    }

    /// tsc's UMD transform uses `createStringLiteralFromNode` for the AMD
    /// copy of a literal argument. Rust string nodes keep decoded text rather
    /// than a `textSourceNode`, so carry the source quote as typed emit
    /// metadata when cloning. Other copiable expressions retain the ordinary
    /// clone path.
    fn clone_umd_dynamic_import_argument(
        &mut self,
        argument: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let quote = self.string_literal_quote(argument)?;
        let clone = self.context.factory()?.clone_node(argument)?;
        if let Some(quote) = quote {
            self.context
                .arena_mut()?
                .metadata_mut(clone)
                .set_string_literal_single_quote(quote.is_single());
        }
        Ok(clone)
    }

    fn string_literal_quote(
        &self,
        literal: TransformNode,
    ) -> Result<Option<StringLiteralQuote>, TransformError> {
        if self.context.arena().node(literal)?.kind != SyntaxKind::StringLiteral {
            return Ok(None);
        }
        if let Some(single_quote) = self
            .context
            .arena()
            .metadata(literal)
            .and_then(crate::EmitMetadata::string_literal_single_quote)
        {
            return Ok(Some(if single_quote {
                StringLiteralQuote::Single
            } else {
                StringLiteralQuote::Double
            }));
        }

        let original = self.context.arena().get_original_node(literal);
        if self.context.arena().node(original)?.kind != SyntaxKind::StringLiteral {
            return Ok(None);
        }
        let source = self.context.arena().source(original.source())?.syntax();
        let record = self.context.arena().node(original)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions()).map_err(|error| {
                TransformError::InvalidSourceRange {
                    node: original,
                    error,
                }
            })?
        else {
            return Ok(None);
        };
        let range = range
            .without_leading_trivia(source.text(), source.positions())
            .map_err(|error| TransformError::InvalidSourceRange {
                node: original,
                error,
            })?;
        Ok(
            match source.text().as_bytes().get(range.start().value() as usize) {
                Some(b'\'') => Some(StringLiteralQuote::Single),
                Some(b'"') => Some(StringLiteralQuote::Double),
                _ => None,
            },
        )
    }

    fn is_simple_copiable_expression(
        &self,
        expression: TransformNode,
    ) -> Result<bool, TransformError> {
        let kind = self.context.arena().node(expression)?.kind;
        Ok(matches!(
            kind,
            SyntaxKind::StringLiteral
                | SyntaxKind::NoSubstitutionTemplateLiteral
                | SyntaxKind::NumericLiteral
                | SyntaxKind::Identifier
        ) || kind.value() >= SyntaxKind::FirstKeyword.value()
            && kind.value() <= SyntaxKind::LastKeyword.value())
    }

    fn is_simple_inlineable_expression(
        &self,
        expression: TransformNode,
    ) -> Result<bool, TransformError> {
        Ok(
            self.context.arena().node(expression)?.kind != SyntaxKind::Identifier
                && self.is_simple_copiable_expression(expression)?,
        )
    }

    fn create_string_coercion_template(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let head = self.context.factory()?.create_node(
            self.source,
            NodeData::TemplateHead(tsc_syntax::nodes::TemplateHeadData {
                text: String::new(),
                raw_text: Some(String::new()),
            }),
            TransformFlags::NONE,
        )?;
        let tail = self.context.factory()?.create_node(
            self.source,
            NodeData::TemplateTail(tsc_syntax::nodes::TemplateTailData {
                text: String::new(),
                raw_text: Some(String::new()),
            }),
            TransformFlags::NONE,
        )?;
        let span = self.context.factory()?.create_node(
            self.source,
            NodeData::TemplateSpan(tsc_syntax::nodes::TemplateSpanData {
                expression: Some(expression.node()),
                literal: Some(tail.node()),
            }),
            TransformFlags::NONE,
        )?;
        let spans = self
            .context
            .factory()?
            .create_node_array(self.source, vec![span])?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::TemplateExpression(tsc_syntax::nodes::TemplateExpressionData {
                head: Some(head.node()),
                template_spans: Some(spans.array()),
            }),
            TransformFlags::NONE,
        )
    }

    fn allocate_temp_binding(&mut self) -> Result<CommonJsTempBinding, TransformError> {
        let binding = CommonJsTempBinding(self.next_generated_name().into_boxed_str());
        let declaration = self.create_identifier(&binding.0)?;
        self.context.hoist_variable_declaration(declaration)?;
        Ok(binding)
    }

    fn create_temp_reference(
        &mut self,
        binding: &CommonJsTempBinding,
    ) -> Result<TransformNode, TransformError> {
        self.create_identifier(&binding.0)
    }

    fn next_generated_name(&mut self) -> String {
        loop {
            let ordinal = self.temp_ordinal;
            self.temp_ordinal += 1;
            let candidate = if ordinal < 26 {
                format!("_{}", (b'a' + ordinal as u8) as char)
            } else {
                format!("_{}", ordinal - 26)
            };
            if self.used_names.insert(candidate.clone()) {
                return candidate;
            }
        }
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

    fn substitute_identifier(
        &mut self,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if self
            .context
            .arena()
            .metadata(original)
            .is_some_and(|metadata| metadata.flags().contains(EmitFlags::NO_SUBSTITUTION))
        {
            return Ok(original);
        }
        if !self.is_local_name(original) {
            let parsed = self.context.arena().get_original_node(original);
            if self.context.arena().node(parsed)?.pos != u32::MAX {
                let resolver_node = self.resolver_node(original)?;
                let export_container_mode = if self
                    .context
                    .arena()
                    .metadata(original)
                    .is_some_and(|metadata| metadata.flags().contains(EmitFlags::EXPORT_NAME))
                {
                    EmitExportContainerMode::ExportName
                } else {
                    EmitExportContainerMode::Reference
                };
                let exported_from_source = self
                    .resolver
                    .get_referenced_export_container(resolver_node, export_container_mode)?
                    .and_then(|container| {
                        self.context.arena().node_ref(self.source, container.node())
                    })
                    .and_then(|container| self.context.arena().node(container).ok())
                    .is_some_and(|container| container.kind == SyntaxKind::SourceFile);
                if exported_from_source {
                    let name = identifier_or_literal_text(self.context.arena(), original)?;
                    let transformed = self.create_export_access(&name)?;
                    self.set_original_and_range(transformed, original)?;
                    return Ok(transformed);
                }
            }
        }
        self.substitute_import_identifier(original)
    }

    fn is_local_name(&self, node: TransformNode) -> bool {
        self.context
            .arena()
            .metadata(node)
            .is_some_and(|metadata| metadata.flags().contains(EmitFlags::LOCAL_NAME))
    }

    /// Resolve the runtime owner of a parsed import alias which survived only
    /// as emit metadata on a synthetic reference.
    ///
    /// transformTypeScript is allowed to remove that alias (and even its whole
    /// ImportDeclaration) before transformModule runs. tsc nevertheless uses
    /// the original declaration identity to substitute the generated JSX
    /// factory identifier. This binding is deliberately not an ImportPlan: it
    /// must provide a spelling without recreating a runtime dependency.
    fn elided_import_binding(
        &mut self,
        declaration: TransformNode,
    ) -> Result<Option<ImportBinding>, TransformError> {
        let declaration = self.context.arena().get_original_node(declaration);
        let property = match &self.context.arena().node(declaration)?.data {
            NodeData::ImportSpecifier(specifier) => {
                let Some(name) = specifier
                    .property_name
                    .or(specifier.name)
                    .and_then(|id| self.context.arena().node_ref(self.source, id))
                else {
                    return Ok(None);
                };
                Some(identifier_or_literal_text(self.context.arena(), name)?.into_boxed_str())
            }
            NodeData::ImportClause(clause) if clause.name.is_some() => Some("default".into()),
            NodeData::NamespaceImport(_) => None,
            _ => return Ok(None),
        };

        let mut ancestor = self.context.arena().node(declaration)?.parent;
        let import_declaration = loop {
            let Some(id) = ancestor else {
                return Ok(None);
            };
            let node = self
                .context
                .arena()
                .node_ref(self.source, id)
                .ok_or_else(|| TransformError::UnknownNode(self.node(id)))?;
            let record = self.context.arena().node(node)?;
            if record.kind == SyntaxKind::ImportDeclaration {
                break self.context.arena().get_original_node(node);
            }
            ancestor = record.parent;
        };
        let import_key = import_declaration.node();

        let planned_runtime_name = self
            .info
            .imports
            .get(&import_key)
            .and_then(|plan| plan.runtime_name.clone());
        let cached_runtime_name = self
            .info
            .elided_import_runtime_names
            .get(&import_key)
            .cloned();
        let runtime_name = if let Some(name) = planned_runtime_name.or(cached_runtime_name) {
            name
        } else {
            let module_text = match &self.context.arena().node(import_declaration)?.data {
                NodeData::ImportDeclaration(import) => {
                    let Some(specifier) = import
                        .module_specifier
                        .and_then(|id| self.context.arena().node_ref(self.source, id))
                    else {
                        return Ok(None);
                    };
                    string_literal_text(self.context.arena(), specifier)?.to_owned()
                }
                _ => return Ok(None),
            };
            let name = self
                .info
                .generated_module_names
                .allocate(module_text.as_str());
            self.info
                .elided_import_runtime_names
                .insert(import_key, name.clone());
            name
        };
        Ok(Some(ImportBinding {
            generated_name: runtime_name,
            property,
        }))
    }

    fn import_binding_for_reference(
        &mut self,
        node: TransformNode,
    ) -> Result<Option<ImportBinding>, TransformError> {
        if let Some(declaration) = self
            .context
            .arena()
            .metadata(node)
            .and_then(crate::EmitMetadata::referenced_import_declaration)
        {
            if let Some(export) = self
                .info
                .imports
                .get(&declaration.node())
                .and_then(|plan| plan.import_equals_publication.as_ref())
                .and_then(ImportEqualsPublication::exported_name)
            {
                return Ok(Some(ImportBinding {
                    generated_name: "exports".into(),
                    property: Some(export.into()),
                }));
            }
            if let Some(binding) = self.info.import_bindings.get(&declaration.node()).cloned() {
                return Ok(Some(binding));
            }

            if self.context.arena().node(declaration)?.pos != u32::MAX {
                let Some(binding) = self.elided_import_binding(declaration)? else {
                    return Ok(None);
                };
                self.info
                    .import_bindings
                    .insert(declaration.node(), binding.clone());
                return Ok(Some(binding));
            }

            // Automatic JSX records the synthetic ImportSpecifier on each
            // generated helper reference. In a legacy script tsc deliberately
            // does not attach that specifier to an emitted import statement,
            // but its substitution hook still asks the node factory for the
            // specifier's generated container name. Preserve that declaration
            // identity and allocate the binding lazily, when a reachable
            // reference is actually visited.
            let property = match &self.context.arena().node(declaration)?.data {
                NodeData::ImportSpecifier(specifier)
                    if self.context.arena().node(declaration)?.pos == u32::MAX =>
                {
                    specifier
                        .property_name
                        .or(specifier.name)
                        .and_then(|id| self.context.arena().node_ref(self.source, id))
                        .and_then(|name| {
                            identifier_or_literal_text(self.context.arena(), name).ok()
                        })
                }
                _ => None,
            };
            let Some(property) = property else {
                return Ok(None);
            };
            let binding = ImportBinding {
                generated_name: self.next_generated_name().into_boxed_str(),
                property: Some(property.into_boxed_str()),
            };
            self.info
                .import_bindings
                .insert(declaration.node(), binding.clone());
            return Ok(Some(binding));
        }
        let original = self.context.arena().get_original_node(node);
        if NodeFlags::from_bits(self.context.arena().node(original)?.flags)
            .contains(NodeFlags::SYNTHESIZED)
        {
            return Ok(None);
        }
        let resolver_node = self.resolver_node(node)?;
        let declaration = self
            .resolver
            .get_referenced_import_declaration(resolver_node)?;
        Ok(declaration.and_then(|declaration| {
            if let Some(export) = self
                .info
                .imports
                .get(&declaration.node())
                .and_then(|plan| plan.import_equals_publication.as_ref())
                .and_then(ImportEqualsPublication::exported_name)
            {
                return Some(ImportBinding {
                    generated_name: "exports".into(),
                    property: Some(export.into()),
                });
            }
            self.info.import_bindings.get(&declaration.node()).cloned()
        }))
    }

    fn update_generic(
        &mut self,
        original: TransformNode,
        mut data: NodeData,
    ) -> Result<TransformNode, TransformError> {
        try_visit_each_child(&mut data, self)?;
        self.update_generic_without_visit(original, data)
    }

    fn with_expression_value_use<T>(
        &mut self,
        value_use: CommonJsExpressionValueUse,
        operation: impl FnOnce(&mut Self) -> Result<T, TransformError>,
    ) -> Result<T, TransformError> {
        let previous = std::mem::replace(&mut self.expression_value_use, value_use);
        let result = operation(self);
        self.expression_value_use = previous;
        result
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
        self.create_array_literal_with_trailing_comma(elements, false)
    }

    fn create_array_literal_with_trailing_comma(
        &mut self,
        elements: Vec<TransformNode>,
        has_trailing_comma: bool,
    ) -> Result<TransformNode, TransformError> {
        let elements = self
            .context
            .factory()?
            .create_node_array_with_trailing_comma(self.source, elements, has_trailing_comma)?;
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

    fn create_element_access(
        &mut self,
        expression: TransformNode,
        argument_expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::ElementAccessExpression(tsc_syntax::nodes::ElementAccessExpressionData {
                expression: Some(expression.node()),
                argument_expression: Some(argument_expression.node()),
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
        let module_specifier = self.context.factory()?.clone_node(module_specifier)?;
        let module_specifier = self.rewrite_import_argument(module_specifier)?;
        self.create_raw_require_call(module_specifier)
    }

    fn create_raw_require_call(
        &mut self,
        module_specifier: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_raw_require_call_optional(Some(module_specifier))
    }

    fn create_raw_require_call_optional(
        &mut self,
        module_specifier: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let require = self.create_identifier("require")?;
        self.create_call(require, module_specifier.into_iter().collect())
    }

    fn rewrite_import_argument(
        &mut self,
        argument: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if !self.rewrite_relative_import_extensions {
            return Ok(argument);
        }
        if let NodeData::StringLiteral(literal) = self.context.arena().node(argument)?.data.clone()
        {
            let Some(text) = rewrite_relative_module_specifier(&literal.text) else {
                return Ok(argument);
            };
            let flags = self.context.arena().transform_flags(argument);
            return self.context.factory()?.update_node(
                argument,
                NodeData::StringLiteral(tsc_syntax::nodes::StringLiteralData {
                    text,
                    has_extended_unicode_escape: literal.has_extended_unicode_escape,
                }),
                flags,
            );
        }
        self.request_rewrite_relative_import_extensions_helper()?;
        let helper = self.context.factory()?.create_unscoped_helper_identifier(
            self.source,
            EmitHelperName::RewriteRelativeImportExtension,
        )?;
        self.create_call(helper, vec![argument])
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

    fn create_omitted_expression(&mut self) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::OmittedExpression(tsc_syntax::nodes::OmittedExpressionData {}),
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
        self.create_variable_declaration_from_name(name, Some(initializer))
    }

    fn create_variable_declaration_from_name(
        &mut self,
        name: TransformNode,
        initializer: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::VariableDeclaration(tsc_syntax::nodes::VariableDeclarationData {
                name: Some(name.node()),
                exclamation_token: None,
                r#type: None,
                initializer: initializer.map(TransformNode::node),
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
            Some(1),
            Vec::new(),
        );
        let set_default = crate::EmitHelper::with_text(
            "typescript:commonjscreatevalue",
            false,
            SET_MODULE_DEFAULT_HELPER_TEXT,
            Some(1),
            Vec::new(),
        );
        self.context
            .request_emit_helper(crate::EmitHelper::with_text(
                "typescript:commonjsimportstar",
                false,
                IMPORT_STAR_HELPER_TEXT,
                Some(2),
                vec![create_binding, set_default],
            ))
    }

    fn request_import_default_helper(&mut self) -> Result<(), TransformError> {
        self.context
            .request_emit_helper(crate::EmitHelper::with_text(
                "typescript:commonjsimportdefault",
                false,
                IMPORT_DEFAULT_HELPER_TEXT,
                None,
                Vec::new(),
            ))
    }

    fn request_export_star_helper(&mut self) -> Result<(), TransformError> {
        let create_binding = crate::EmitHelper::with_text(
            "typescript:commonjscreatebinding",
            false,
            CREATE_BINDING_HELPER_TEXT,
            Some(1),
            Vec::new(),
        );
        self.context
            .request_emit_helper(crate::EmitHelper::with_text(
                "typescript:export-star",
                false,
                EXPORT_STAR_HELPER_TEXT,
                Some(2),
                vec![create_binding],
            ))
    }

    fn request_rewrite_relative_import_extensions_helper(&mut self) -> Result<(), TransformError> {
        self.context
            .request_emit_helper(crate::EmitHelper::with_text(
                "typescript:rewriteRelativeImportExtensions",
                false,
                REWRITE_RELATIVE_IMPORT_EXTENSIONS_HELPER_TEXT,
                None,
                Vec::new(),
            ))
    }

    fn set_original_and_range(
        &mut self,
        node: TransformNode,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.set_text_range(node, original)?;
        self.context
            .arena_mut()?
            .set_original_node(node, Some(original))?;
        Ok(node)
    }

    fn resolver_node(&self, node: TransformNode) -> Result<EmitResolverNode, TransformError> {
        self.context.arena().require_parse_tree_resolver_node(node)
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
    legacy_decorators: bool,
    preserve_const_enums: bool,
    project_parameter_properties_for_class_fields: bool,
    downlevel_iteration: bool,
    nodes: BTreeMap<NodeId, Option<NodeId>>,
    arrays: BTreeMap<NodeArrayId, Option<NodeArrayId>>,
    class_member_arrays: BTreeMap<NodeArrayId, ClassMemberArrayVisit>,
    expanded_enums: BTreeMap<NodeId, Vec<NodeId>>,
    expanded_modules: BTreeMap<NodeId, Vec<NodeId>>,
    emitted_declarations: BTreeSet<(NodeId, String)>,
    namespace_stack: Vec<NamespaceContext>,
    namespace_container_names: BTreeMap<NodeId, Box<str>>,
    enum_container_names: BTreeMap<NodeId, Box<str>>,
    generated_declaration_names: BTreeMap<NodeId, Box<str>>,
    source_identifier_names: BTreeSet<String>,
    generated_namespace_names: BTreeSet<String>,
    temp_ordinal: usize,
}

/// Semantic decorator mode consumed by `transformTypeScript` class facts.
///
/// Legacy decorators admit parameter decorators and reject private identifiers
/// (`#name`); standard decorators do the inverse. Keeping that distinction
/// typed avoids treating the parser's shared `Decorator` syntax as one uniform
/// feature.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TypeScriptDecoratorMode {
    Legacy,
    Standard,
}

impl TypeScriptDecoratorMode {
    const fn from_legacy(legacy: bool) -> Self {
        if legacy {
            Self::Legacy
        } else {
            Self::Standard
        }
    }
}

/// The subset of `getClassFacts` that establishes declaration identity before
/// later decorator and class-field passes split an anonymous class into
/// generated statements.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct TypeScriptClassFacts {
    has_static_initialized_properties: bool,
    has_member_decorators: bool,
}

impl TypeScriptClassFacts {
    const fn needs_declaration_name(self) -> bool {
        self.has_static_initialized_properties || self.has_member_decorators
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClassMemberArrayVisit {
    Visiting { parent: NodeId },
    Visited { parent: NodeId },
}

const TYPESCRIPT_CLASS_MEMBERS_CONTEXT: &str = "transformTypeScript class members";

impl ClassMemberArrayVisit {
    const fn parent(self) -> NodeId {
        match self {
            Self::Visiting { parent } | Self::Visited { parent } => parent,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NamespaceContext {
    declaration: NodeId,
    container_name: String,
    /// Declaration-list kind owned by the surrounding lexical transform
    /// scope. A dotted namespace body is another ModuleDeclaration, but tsc
    /// deliberately keeps it in the outer declaration's lexical scope.
    variable_flags: NodeFlags,
}

/// `transformTypeScript` changes `currentLexicalScope` only at these parsed
/// containers. In particular, a label is not a scope: an enum directly below
/// a source-level label still belongs to the SourceFile and must use `var`
/// even though its lowered statements are subsequently lifted into a block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TypeScriptLexicalScopeOwner {
    SourceFile(NodeId),
    Nested(NodeId),
}

impl TypeScriptLexicalScopeOwner {
    const fn container(self) -> NodeId {
        match self {
            Self::SourceFile(container) | Self::Nested(container) => container,
        }
    }

    const fn variable_flags(self) -> NodeFlags {
        match self {
            Self::SourceFile(_) => NodeFlags::NONE,
            Self::Nested(_) => NodeFlags::LET,
        }
    }
}

impl<'context, 'resolver> TypeScriptVisitor<'context, 'resolver> {
    fn new(
        context: &'context mut TransformationContext,
        source: TransformSourceId,
        resolver: &'resolver dyn EmitResolver,
        legacy_decorators: bool,
        preserve_const_enums: bool,
        project_parameter_properties_for_class_fields: bool,
        downlevel_iteration: bool,
    ) -> Self {
        let source_identifier_names = system::collect_identifier_texts(context.arena(), source);
        Self {
            context,
            source,
            resolver,
            legacy_decorators,
            preserve_const_enums,
            project_parameter_properties_for_class_fields,
            downlevel_iteration,
            nodes: BTreeMap::new(),
            arrays: BTreeMap::new(),
            class_member_arrays: BTreeMap::new(),
            expanded_enums: BTreeMap::new(),
            expanded_modules: BTreeMap::new(),
            emitted_declarations: BTreeSet::new(),
            namespace_stack: Vec::new(),
            namespace_container_names: BTreeMap::new(),
            enum_container_names: BTreeMap::new(),
            generated_declaration_names: BTreeMap::new(),
            source_identifier_names,
            generated_namespace_names: BTreeSet::new(),
            temp_ordinal: 0,
        }
    }

    fn visit(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        self.visit_with_typescript_gate(id, false)
    }

    fn lexical_scope_owner(
        &self,
        declaration: TransformNode,
    ) -> Result<TypeScriptLexicalScopeOwner, TransformError> {
        let mut ancestor = self.context.arena().node(declaration)?.parent;
        while let Some(id) = ancestor {
            let node = self
                .context
                .arena()
                .node_ref(self.source, id)
                .ok_or_else(|| TransformError::UnknownNode(self.node(id)))?;
            let record = self.context.arena().node(node)?;
            match record.kind {
                SyntaxKind::SourceFile => {
                    return Ok(TypeScriptLexicalScopeOwner::SourceFile(id));
                }
                SyntaxKind::Block | SyntaxKind::CaseBlock | SyntaxKind::ModuleBlock => {
                    return Ok(TypeScriptLexicalScopeOwner::Nested(id));
                }
                _ => ancestor = record.parent,
            }
        }
        Err(TransformError::RequiredChildRemoved {
            parent: self.context.arena().node(declaration)?.kind,
            field: "lexical scope owner",
        })
    }

    /// tsc-port: transformTypeScript/onBeforeVisitNode @6.0.3
    /// tsc-hash: dfcb528feed3d3aaab6331e92ce0f978875db4a69ee5e781ddfbcca5e92b9153
    /// tsc-span: _tsc.js:94097-94116
    /// tsc-port: transformTypeScript/recordEmittedDeclarationInScope @6.0.3
    /// tsc-hash: a839c63b309942bf187a3bd0ab55a3beb30f1f4f84ab53ac53665f6ddae347b5
    /// tsc-span: _tsc.js:95332-95340
    ///
    /// A runtime class or function declaration participates in the same
    /// declaration-merge slot as a later namespace or enum. Record that slot
    /// even when the declaration carries no TypeScript transform flag: tsc's
    /// `onBeforeVisitNode` runs before `visitorWorker` applies its flag gate.
    fn record_class_or_function_declaration(
        &mut self,
        declaration: TransformNode,
        name: Option<NodeId>,
        modifiers: Option<NodeArrayId>,
    ) -> Result<(), TransformError> {
        if NodeFlags::from_bits(self.context.arena().node(declaration)?.flags)
            .contains(NodeFlags::AMBIENT)
            || self.has_modifier(modifiers, SyntaxKind::DeclareKeyword)?
        {
            return Ok(());
        }
        let Some(name) = name else {
            return Ok(());
        };
        let name = self.identifier_text(name)?.to_owned();
        let scope = self.lexical_scope_owner(declaration)?;
        self.emitted_declarations.insert((scope.container(), name));
        Ok(())
    }

    /// tsc's `visitorWorker` normally enters `visitTypeScript` only for a
    /// subtree carrying `ContainsTypeScript`. Source-level elidable statements
    /// and exported namespace declarations have separate visitor entry points
    /// and deliberately bypass that gate.
    fn visit_typescript(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        self.visit_with_typescript_gate(id, true)
    }

    fn visit_with_typescript_gate(
        &mut self,
        id: NodeId,
        force: bool,
    ) -> Result<Option<NodeId>, TransformError> {
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
        match &record.data {
            NodeData::ClassDeclaration(data) => {
                self.record_class_or_function_declaration(original, data.name, data.modifiers)?
            }
            NodeData::FunctionDeclaration(data) => {
                self.record_class_or_function_declaration(original, data.name, data.modifiers)?
            }
            _ => {}
        }
        let retain_namespace_function_default = !self.namespace_stack.is_empty()
            && kind == SyntaxKind::DefaultKeyword
            && record
                .parent
                .and_then(|parent| self.context.arena().node_ref(self.source, parent))
                .and_then(|parent| self.context.arena().node(parent).ok())
                .is_some_and(|parent| parent.kind == SyntaxKind::FunctionDeclaration);
        let namespace_modifier = !self.namespace_stack.is_empty()
            && matches!(kind, SyntaxKind::ExportKeyword | SyntaxKind::DefaultKeyword);
        if !force
            && !namespace_modifier
            && !self
                .context
                .arena()
                .transform_flags(original)
                .contains(TransformFlags::CONTAINS_TYPE_SCRIPT)
        {
            self.nodes.insert(id, Some(id));
            return Ok(Some(id));
        }

        let transformed = if kind == SyntaxKind::SourceFile {
            let NodeData::SourceFile(data) = record.data else {
                unreachable!("source-file kind owns source-file data")
            };
            Some(self.visit_source_file(original, data)?)
        } else if kind == SyntaxKind::EnumDeclaration {
            let statements = self.visit_enum_declaration(id)?;
            Some(self.lift_statement_expansion(statements)?.node())
        } else if kind == SyntaxKind::ModuleDeclaration {
            let statements = self.visit_module_declaration(id)?;
            Some(self.lift_statement_expansion(statements)?.node())
        } else if matches!(
            kind,
            SyntaxKind::InterfaceDeclaration | SyntaxKind::TypeAliasDeclaration
        ) {
            Some(
                self.context
                    .factory()?
                    .create_not_emitted_statement(original)?
                    .node(),
            )
        } else if is_type_node(kind)
            || is_typescript_modifier(kind)
            || (!self.namespace_stack.is_empty()
                && (kind == SyntaxKind::ExportKeyword
                    || kind == SyntaxKind::DefaultKeyword && !retain_namespace_function_default))
            || matches!(
                kind,
                SyntaxKind::IndexSignature
                    | SyntaxKind::NamespaceExportDeclaration
                    | SyntaxKind::PropertySignature
                    | SyntaxKind::MethodSignature
                    | SyntaxKind::CallSignature
                    | SyntaxKind::ConstructSignature
            )
        {
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
                NodeData::ParenthesizedExpression(data) => {
                    self.visit_parenthesized_expression(original, data)?
                }
                NodeData::PropertyAccessExpression(data) => {
                    Some(self.update_generic(original, NodeData::PropertyAccessExpression(data))?)
                }
                NodeData::ElementAccessExpression(data) => {
                    Some(self.update_generic(original, NodeData::ElementAccessExpression(data))?)
                }
                NodeData::FunctionDeclaration(mut data) => {
                    if data.body.is_none()
                        || self.has_modifier(data.modifiers, SyntaxKind::DeclareKeyword)?
                    {
                        Some(
                            self.context
                                .factory()?
                                .create_not_emitted_statement(original)?
                                .node(),
                        )
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
                    if parameter_emit_role(self.context.arena(), self.source, &data)?
                        == ParameterEmitRole::ExplicitThis
                    {
                        None
                    } else {
                        // visitParameter 95029-95047 retains only
                        // decorators from a parameter's modifier-like
                        // list. Invalid JS modifiers such as `static`
                        // are diagnosed by the checker but still erased
                        // from JavaScript output.
                        data.modifiers = self.parameter_runtime_modifiers(data.modifiers)?;
                        data.question_token = None;
                        data.r#type = None;
                        Some(self.update_generic(original, NodeData::Parameter(data))?)
                    }
                }
                NodeData::VariableStatement(data) => {
                    if self.has_modifier(data.modifiers, SyntaxKind::DeclareKeyword)? {
                        Some(
                            self.context
                                .factory()?
                                .create_not_emitted_statement(original)?
                                .node(),
                        )
                    } else {
                        Some(self.update_generic(original, NodeData::VariableStatement(data))?)
                    }
                }
                NodeData::VariableDeclaration(mut data) => {
                    let erased_type = data.r#type.map(|r#type| self.node(r#type));
                    data.exclamation_token = None;
                    data.r#type = None;
                    let updated =
                        self.update_generic(original, NodeData::VariableDeclaration(data))?;
                    if let Some(erased_type) = erased_type {
                        let name = match &self.context.arena().node(self.node(updated))?.data {
                            NodeData::VariableDeclaration(data) => data.name,
                            _ => None,
                        };
                        if let Some(name) =
                            name.and_then(|name| self.context.arena().node_ref(self.source, name))
                        {
                            // tsc's setTypeNode keeps the erased annotation as
                            // an emit-only comment boundary on the transformed
                            // binding name.
                            self.context
                                .arena_mut()?
                                .metadata_mut(name)
                                .set_type_node(erased_type);
                        }
                    }
                    Some(updated)
                }
                NodeData::ClassDeclaration(mut data) => {
                    if self.has_modifier(data.modifiers, SyntaxKind::DeclareKeyword)? {
                        Some(
                            self.context
                                .factory()?
                                .create_not_emitted_statement(original)?
                                .node(),
                        )
                    } else {
                        if data.name.is_none() {
                            let mut generated_name = self
                                .generated_declaration_names
                                .get(&id)
                                .map(ToString::to_string);
                            // transformTypeScript's class facts give an
                            // anonymous declaration a stable identity before
                            // later decorator, class-field, and module passes
                            // split it into statements.
                            let facts = self.typescript_class_facts(data.members)?;
                            if generated_name.is_none() && facts.needs_declaration_name() {
                                generated_name =
                                    Some(self.ensure_generated_declaration_name(id, "default"));
                            }
                            if let Some(name) = generated_name {
                                data.name = Some(self.create_identifier(&name)?.node());
                            }
                        }
                        data.type_parameters = None;
                        Some(self.update_class_declaration(original, id, data)?)
                    }
                }
                NodeData::ClassExpression(mut data) => {
                    data.type_parameters = None;
                    Some(self.update_class_expression(original, id, data)?)
                }
                NodeData::PropertyDeclaration(mut data) => {
                    let is_ambient =
                        NodeFlags::from_bits(self.context.arena().node(original)?.flags)
                            .contains(NodeFlags::AMBIENT)
                            || self.has_modifier(data.modifiers, SyntaxKind::DeclareKeyword)?
                            || self.has_modifier(data.modifiers, SyntaxKind::AbstractKeyword)?;
                    if is_ambient {
                        if self.legacy_decorators
                            && self.has_modifier(data.modifiers, SyntaxKind::Decorator)?
                        {
                            Some(self.update_ambient_property_declaration(original, data)?)
                        } else {
                            None
                        }
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
                        let parameter_properties = self.parameter_properties(data.parameters)?;
                        data.body = Some(self.transform_constructor_body(
                            data.body.expect("constructor body checked"),
                            &parameter_properties,
                        )?);
                        data.type_parameters = None;
                        data.r#type = None;
                        data.modifiers = None;
                        Some(self.update_constructor_declaration(original, data)?)
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
                    if data.body.is_none()
                        && self.has_modifier(data.modifiers, SyntaxKind::AbstractKeyword)?
                    {
                        None
                    } else {
                        if data.body.is_none() {
                            data.body = Some(
                                self.create_block_from_array(Vec::new(), None, false)?
                                    .node(),
                            );
                        }
                        data.type_parameters = None;
                        data.r#type = None;
                        Some(self.update_get_accessor_declaration(original, data)?)
                    }
                }
                NodeData::SetAccessor(mut data) => {
                    if data.body.is_none()
                        && self.has_modifier(data.modifiers, SyntaxKind::AbstractKeyword)?
                    {
                        None
                    } else {
                        if data.body.is_none() {
                            data.body = Some(
                                self.create_block_from_array(Vec::new(), None, false)?
                                    .node(),
                            );
                        }
                        data.type_parameters = None;
                        data.r#type = None;
                        Some(self.update_set_accessor_declaration(original, data)?)
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
                NodeData::JsxSelfClosingElement(mut data) => {
                    // transformTypeScript visitJsxSelfClosingElement passes
                    // `void 0` to updateJsxSelfClosingElement. JSX preserve
                    // keeps the runtime JSX node, but its type arguments are
                    // still compile-time syntax and never reach the printer.
                    //
                    // tsc-port: visitJsxSelfClosingElement @6.0.3
                    // tsc-span: _tsc.js:95159-95167
                    data.type_arguments = None;
                    Some(self.update_generic(original, NodeData::JsxSelfClosingElement(data))?)
                }
                NodeData::JsxOpeningElement(mut data) => {
                    // tsc-port: visitJsxJsxOpeningElement @6.0.3
                    // tsc-span: _tsc.js:95168-95176
                    data.type_arguments = None;
                    Some(self.update_generic(original, NodeData::JsxOpeningElement(data))?)
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
                NodeData::ImportEqualsDeclaration(data) => {
                    self.visit_import_equals_declaration(original, data)?
                }
                NodeData::ExportDeclaration(data) => {
                    self.visit_export_declaration(original, data)?
                }
                NodeData::ExportAssignment(mut data) => {
                    if self
                        .resolver
                        .is_value_alias_declaration(self.resolver_node(original)?)?
                    {
                        data.modifiers = None;
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

    /// tsc's `sourceElementVisitor` owns import/export elision independently
    /// from the ordinary transform-flag gate. Keep that ownership visible
    /// instead of teaching every generic child visit about source context.
    fn visit_source_file(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::SourceFileData,
    ) -> Result<NodeId, TransformError> {
        if let Some(statements) = data.statements {
            let original_statements = self.array(statements);
            let input = self
                .context
                .arena()
                .node_array(original_statements)?
                .nodes
                .clone();
            let mut output = Vec::with_capacity(input.len());
            for statement in input {
                let kind = self.context.arena().node(self.node(statement))?.kind;
                match kind {
                    SyntaxKind::EnumDeclaration => {
                        output.extend(self.visit_enum_declaration(statement)?);
                    }
                    SyntaxKind::ModuleDeclaration => {
                        output.extend(self.visit_module_declaration(statement)?);
                    }
                    SyntaxKind::ImportDeclaration
                    | SyntaxKind::ImportEqualsDeclaration
                    | SyntaxKind::ExportAssignment
                    | SyntaxKind::ExportDeclaration => {
                        if let Some(statement) = self.visit_typescript(statement)? {
                            output.push(self.node(statement));
                        }
                    }
                    _ => {
                        if let Some(statement) = self.visit(statement)? {
                            output.push(self.node(statement));
                        }
                    }
                }
            }
            data.statements = Some(
                self.context
                    .factory()?
                    .update_node_array(original_statements, output)?
                    .array(),
            );
        }
        let data = NodeData::SourceFile(data);
        let flags = flags_after_update(self.context.arena(), original, &data)?;
        Ok(self
            .context
            .factory()?
            .update_node(original, data, flags)?
            .node())
    }

    /// tsc-port: transformClassMembers @6.0.3
    /// tsc-hash: 306e5388a9a5c510a3594d97b7fbe7bf945415e4f4601770e266d55ce28765f8
    /// tsc-span: _tsc.js:94564-94598
    fn prepend_parameter_property_members(
        &mut self,
        original: TransformNodeArray,
        members: Vec<TransformNode>,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let member_ids = self.context.arena().node_array(original)?.nodes.clone();
        let mut parameters = Vec::new();
        for member in &member_ids {
            let record = self.context.arena().node(self.node(*member))?;
            let NodeData::Constructor(data) = &record.data else {
                continue;
            };
            if data.body.is_some() {
                parameters = self.parameter_properties(data.parameters)?;
                break;
            }
        }
        if parameters.is_empty() {
            return Ok(members);
        }
        let mut projected = Vec::with_capacity(parameters.len() + members.len());
        for parameter in parameters {
            if let Some(property) = self.create_parameter_property_declaration(parameter)? {
                projected.push(property);
            }
        }
        projected.extend(members);
        Ok(projected)
    }

    fn parameter_properties(
        &self,
        parameters: Option<NodeArrayId>,
    ) -> Result<Vec<NodeId>, TransformError> {
        let Some(parameters) = parameters else {
            return Ok(Vec::new());
        };
        let parameters = self.array(parameters);
        let mut properties = Vec::new();
        for parameter in &self.context.arena().node_array(parameters)?.nodes {
            let node = self.node(*parameter);
            let NodeData::Parameter(data) = &self.context.arena().node(node)?.data else {
                continue;
            };
            if self.has_parameter_property_modifier(data.modifiers)? {
                properties.push(*parameter);
            }
        }
        Ok(properties)
    }

    fn has_parameter_property_modifier(
        &self,
        modifiers: Option<NodeArrayId>,
    ) -> Result<bool, TransformError> {
        for kind in [
            SyntaxKind::PublicKeyword,
            SyntaxKind::PrivateKeyword,
            SyntaxKind::ProtectedKeyword,
            SyntaxKind::ReadonlyKeyword,
            SyntaxKind::OverrideKeyword,
        ] {
            if self.has_modifier(modifiers, kind)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn create_parameter_property_declaration(
        &mut self,
        parameter: NodeId,
    ) -> Result<Option<TransformNode>, TransformError> {
        let parameter_node = self.node(parameter);
        let NodeData::Parameter(data) = self.context.arena().node(parameter_node)?.data.clone()
        else {
            return Ok(None);
        };
        let Some(name) = data.name else {
            return Ok(None);
        };
        if self.context.arena().node(self.node(name))?.kind != SyntaxKind::Identifier {
            return Ok(None);
        }
        let property = self.context.factory()?.create_node(
            self.source,
            NodeData::PropertyDeclaration(tsc_syntax::nodes::PropertyDeclarationData {
                name: Some(name),
                modifiers: None,
                question_token: None,
                exclamation_token: None,
                r#type: None,
                initializer: None,
            }),
            TransformFlags::CONTAINS_CLASS_FIELDS,
        )?;
        self.context
            .arena_mut()?
            .set_original_node(property, Some(parameter_node))?;
        Ok(Some(property))
    }

    /// tsc-port: visitConstructor/transformConstructorBody/transformParameterWithPropertyAssignment @6.0.3
    /// tsc-hash: 15f80bca2c25edda0aa73938af0a680178e485e0628ab3143280bf481b2cb3ad
    /// tsc-span: _tsc.js:94793-94910
    fn transform_constructor_body(
        &mut self,
        body: NodeId,
        parameters: &[NodeId],
    ) -> Result<NodeId, TransformError> {
        if parameters.is_empty() {
            return Ok(body);
        }
        let mut assignments = Vec::with_capacity(parameters.len());
        for parameter in parameters {
            if let Some(assignment) = self.create_parameter_property_assignment(*parameter)? {
                assignments.push(assignment);
            }
        }
        if assignments.is_empty() {
            return Ok(body);
        }
        let body_node = self.node(body);
        let NodeData::Block(data) = self.context.arena().node(body_node)?.data.clone() else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::Constructor,
                field: "body block",
            });
        };
        let statements = data
            .statements
            .and_then(|statements| self.context.arena().node_array_ref(self.source, statements));
        let statement_ids = statements
            .map(|statements| {
                self.context
                    .arena()
                    .node_array(statements)
                    .map(|array| array.nodes.clone())
            })
            .transpose()?
            .unwrap_or_default();
        let mut prologue_count = 0usize;
        while prologue_count < statement_ids.len()
            && is_prologue_statement(
                self.context.arena(),
                self.node(statement_ids[prologue_count]),
            )?
        {
            prologue_count += 1;
        }
        let super_path = self.find_super_statement_path(&statement_ids, prologue_count)?;
        let updated = if super_path.is_empty() {
            let mut output = Vec::with_capacity(statement_ids.len() + assignments.len());
            output.extend(
                statement_ids[..prologue_count]
                    .iter()
                    .copied()
                    .map(|statement| self.node(statement)),
            );
            output.extend(assignments);
            output.extend(
                statement_ids[prologue_count..]
                    .iter()
                    .copied()
                    .map(|statement| self.node(statement)),
            );
            self.update_block_statements(body_node, data.statements, output)?
        } else {
            self.inject_after_super(body_node, &super_path, &assignments)?
        };
        self.context.factory()?.set_multi_line(updated, true)?;
        Ok(updated.node())
    }

    fn create_parameter_property_assignment(
        &mut self,
        parameter: NodeId,
    ) -> Result<Option<TransformNode>, TransformError> {
        let parameter_node = self.node(parameter);
        let NodeData::Parameter(data) = self.context.arena().node(parameter_node)?.data.clone()
        else {
            return Ok(None);
        };
        let Some(name) = data.name else {
            return Ok(None);
        };
        let name_node = self.node(name);
        let NodeData::Identifier(identifier) = self.context.arena().node(name_node)?.data.clone()
        else {
            return Ok(None);
        };
        let property_name = self.context.factory()?.clone_node(name_node)?;
        self.context
            .factory()?
            .set_text_range(property_name, name_node)?;
        self.context
            .arena_mut()?
            .metadata_mut(property_name)
            .add_flags(EmitFlags::NO_COMMENTS | EmitFlags::NO_SOURCE_MAP);
        let this = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::ThisKeyword,
            TransformFlags::CONTAINS_LEXICAL_THIS,
        )?;
        let access = self.context.factory()?.create_node(
            self.source,
            NodeData::PropertyAccessExpression(tsc_syntax::nodes::PropertyAccessExpressionData {
                name: Some(property_name.node()),
                expression: Some(this.node()),
                question_dot_token: None,
            }),
            TransformFlags::CONTAINS_LEXICAL_THIS,
        )?;
        self.context.factory()?.set_text_range(access, name_node)?;
        let local_name = self.create_identifier_with_original(&identifier.text, name_node)?;
        self.context
            .factory()?
            .set_text_range(local_name, name_node)?;
        self.context
            .arena_mut()?
            .metadata_mut(local_name)
            .add_flags(EmitFlags::NO_COMMENTS);
        let assignment = self.create_assignment(access, local_name)?;
        let statement = self.create_expression_statement(assignment)?;
        self.context
            .arena_mut()?
            .set_original_node(statement, Some(parameter_node))?;
        self.context
            .factory()?
            .set_text_range(statement, parameter_node)?;
        let metadata = self.context.arena_mut()?.metadata_mut(statement);
        metadata.add_flags(EmitFlags::NO_COMMENTS);
        metadata.set_starts_on_new_line(true);
        Ok(Some(statement))
    }

    fn find_super_statement_path(
        &self,
        statements: &[NodeId],
        start: usize,
    ) -> Result<Vec<usize>, TransformError> {
        for (index, statement) in statements.iter().enumerate().skip(start) {
            let statement_node = self.node(*statement);
            if self.statement_is_super_call(statement_node)? {
                return Ok(vec![index]);
            }
            let NodeData::TryStatement(data) =
                self.context.arena().node(statement_node)?.data.clone()
            else {
                continue;
            };
            let Some(try_block) = data.try_block else {
                continue;
            };
            let NodeData::Block(block) = self
                .context
                .arena()
                .node(self.node(try_block))?
                .data
                .clone()
            else {
                continue;
            };
            let nested = block
                .statements
                .and_then(|statements| self.context.arena().node_array_ref(self.source, statements))
                .map(|statements| {
                    self.context
                        .arena()
                        .node_array(statements)
                        .map(|array| array.nodes.clone())
                })
                .transpose()?
                .unwrap_or_default();
            let mut path = self.find_super_statement_path(&nested, 0)?;
            if !path.is_empty() {
                path.insert(0, index);
                return Ok(path);
            }
        }
        Ok(Vec::new())
    }

    fn statement_is_super_call(&self, statement: TransformNode) -> Result<bool, TransformError> {
        let NodeData::ExpressionStatement(data) = &self.context.arena().node(statement)?.data
        else {
            return Ok(false);
        };
        let Some(mut expression) = data
            .expression
            .and_then(|expression| self.context.arena().node_ref(self.source, expression))
        else {
            return Ok(false);
        };
        loop {
            let NodeData::ParenthesizedExpression(data) =
                self.context.arena().node(expression)?.data.clone()
            else {
                break;
            };
            let Some(inner) = data
                .expression
                .and_then(|inner| self.context.arena().node_ref(self.source, inner))
            else {
                return Ok(false);
            };
            expression = inner;
        }
        let NodeData::CallExpression(data) = &self.context.arena().node(expression)?.data else {
            return Ok(false);
        };
        Ok(data
            .expression
            .and_then(|callee| self.context.arena().node_ref(self.source, callee))
            .and_then(|callee| self.context.arena().node(callee).ok())
            .is_some_and(|callee| callee.kind == SyntaxKind::SuperKeyword))
    }

    fn inject_after_super(
        &mut self,
        block: TransformNode,
        path: &[usize],
        assignments: &[TransformNode],
    ) -> Result<TransformNode, TransformError> {
        let NodeData::Block(data) = self.context.arena().node(block)?.data.clone() else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::TryStatement,
                field: "try block",
            });
        };
        let statements = data
            .statements
            .and_then(|statements| self.context.arena().node_array_ref(self.source, statements));
        let mut statement_ids = statements
            .map(|statements| {
                self.context
                    .arena()
                    .node_array(statements)
                    .map(|array| array.nodes.clone())
            })
            .transpose()?
            .unwrap_or_default();
        let index = path[0];
        if path.len() == 1 {
            let mut output = Vec::with_capacity(statement_ids.len() + assignments.len());
            output.extend(
                statement_ids[..=index]
                    .iter()
                    .copied()
                    .map(|statement| self.node(statement)),
            );
            output.extend(assignments.iter().copied());
            output.extend(
                statement_ids[index + 1..]
                    .iter()
                    .copied()
                    .map(|statement| self.node(statement)),
            );
            let updated = self.update_block_statements(block, data.statements, output)?;
            return self.context.factory()?.set_multi_line(updated, true);
        }
        let statement = self.node(statement_ids[index]);
        let NodeData::TryStatement(mut try_data) =
            self.context.arena().node(statement)?.data.clone()
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::TryStatement,
                field: "super path",
            });
        };
        let try_block = try_data
            .try_block
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::TryStatement,
                field: "try block",
            })?;
        let updated_try_block =
            self.inject_after_super(self.node(try_block), &path[1..], assignments)?;
        try_data.try_block = Some(updated_try_block.node());
        let try_node_data = NodeData::TryStatement(try_data);
        let flags = flags_after_update(self.context.arena(), statement, &try_node_data)?;
        let updated_try = self
            .context
            .factory()?
            .update_node(statement, try_node_data, flags)?;
        statement_ids[index] = updated_try.node();
        let output = statement_ids
            .into_iter()
            .map(|statement| self.node(statement))
            .collect();
        self.update_block_statements(block, data.statements, output)
    }

    fn update_block_statements(
        &mut self,
        block: TransformNode,
        original_statements: Option<NodeArrayId>,
        statements: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let statements = if let Some(original) = original_statements
            .and_then(|statements| self.context.arena().node_array_ref(self.source, statements))
        {
            self.context
                .factory()?
                .update_node_array(original, statements)?
        } else {
            self.context
                .factory()?
                .create_node_array(self.source, statements)?
        };
        let data = NodeData::Block(tsc_syntax::nodes::BlockData {
            statements: Some(statements.array()),
        });
        let flags = flags_after_update(self.context.arena(), block, &data)?;
        self.context.factory()?.update_node(block, data, flags)
    }

    /// tsc-port: visitModuleDeclaration/transformModuleBody @6.0.3
    /// tsc-hash: 4e04028a26c04bcd79fb483be4250bb3e90295dcc3ba6f721f917c3a69196d6d
    /// tsc-span: _tsc.js:95325-95517
    fn visit_module_declaration(
        &mut self,
        id: NodeId,
    ) -> Result<Vec<TransformNode>, TransformError> {
        if let Some(expanded) = self.expanded_modules.get(&id) {
            return Ok(expanded.iter().copied().map(|id| self.node(id)).collect());
        }
        let original = self.node(id);
        let record = self.context.arena().node(original)?.clone();
        let NodeData::ModuleDeclaration(data) = record.data else {
            return Err(TransformError::RequiredChildRemoved {
                parent: record.kind,
                field: "module declaration",
            });
        };
        let ambient = NodeFlags::from_bits(record.flags).contains(NodeFlags::AMBIENT)
            || self.has_modifier(data.modifiers, SyntaxKind::DeclareKeyword)?;
        if ambient
            || !self
                .resolver
                .is_instantiated_module(self.resolver_node(original)?)?
        {
            let anchor = self
                .context
                .factory()?
                .create_not_emitted_statement(original)?;
            self.nodes.insert(id, Some(anchor.node()));
            self.expanded_modules.insert(id, vec![anchor.node()]);
            return Ok(vec![anchor]);
        }

        let name_id = data.name.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::ModuleDeclaration,
            field: "identifier name",
        })?;
        let name = self.identifier_text(name_id)?.to_owned();
        let original_name = self.node(name_id);
        let node_flags = NodeFlags::from_bits(record.flags);
        // NestedNamespace is the parser's structural representation of the
        // implicit `export` in `namespace A.B`. TypeScript includes it in the
        // declaration's syntactic modifier flags even though there is no
        // ExportKeyword node in the modifier array.
        let nested_namespace = node_flags.contains(NodeFlags::NESTED_NAMESPACE);
        let exported_from_namespace = !self.namespace_stack.is_empty()
            && (nested_namespace
                || self.has_modifier(data.modifiers, SyntaxKind::ExportKeyword)?);
        let lexical_scope = self.lexical_scope_owner(original)?;
        let declaration_scope = if nested_namespace {
            self.namespace_stack
                .last()
                .map_or(lexical_scope.container(), |namespace| namespace.declaration)
        } else {
            lexical_scope.container()
        };
        let first_in_scope = self
            .emitted_declarations
            .insert((declaration_scope, name.clone()));
        let variable_flags = if nested_namespace {
            self.namespace_stack
                .last()
                .map_or(NodeFlags::NONE, |namespace| namespace.variable_flags)
        } else {
            lexical_scope.variable_flags()
        };
        let mut statements = Vec::with_capacity(if first_in_scope { 2 } else { 1 });

        if first_in_scope {
            let declaration_name = self.create_identifier_with_original(&name, original_name)?;
            let declaration = self.context.factory()?.create_node(
                self.source,
                NodeData::VariableDeclaration(tsc_syntax::nodes::VariableDeclarationData {
                    name: Some(declaration_name.node()),
                    exclamation_token: None,
                    r#type: None,
                    initializer: None,
                }),
                TransformFlags::NONE,
            )?;
            self.context
                .arena_mut()?
                .set_original_node(declaration, Some(original))?;
            let modifiers = self.module_runtime_modifiers(data.modifiers)?;
            let statement =
                self.create_variable_statement(vec![declaration], modifiers, variable_flags)?;
            self.set_original_and_range(statement, original)?;
            self.context
                .arena_mut()?
                .metadata_mut(statement)
                .add_flags(EmitFlags::NO_TRAILING_COMMENTS);
            statements.push(statement);
        }

        let parameter_name = self.namespace_parameter_name(id, data.body, &name)?;
        let body = self.transform_module_body(id, data.body, &parameter_name, variable_flags)?;
        let parameter = self.create_parameter(&parameter_name)?;
        let function = self.create_function_expression(vec![parameter], body)?;
        let function = self.create_parenthesized(function)?;
        let module_arg = if exported_from_namespace {
            let container_name = self
                .namespace_stack
                .last()
                .expect("exported namespace declaration has an outer namespace")
                .container_name
                .clone();
            let container = self.create_identifier(&container_name)?;
            let export_name = self.create_property_access(container, &name)?;
            let container = self.create_identifier(&container_name)?;
            let assignment_name = self.create_property_access(container, &name)?;
            let object = self.create_object_literal()?;
            let assignment = self.create_assignment(assignment_name, object)?;
            let assignment = self.create_parenthesized(assignment)?;
            let exported = self.create_binary(export_name, SyntaxKind::BarBarToken, assignment)?;
            let local_name = self.create_identifier_with_original(&name, original_name)?;
            self.create_assignment(local_name, exported)?
        } else {
            let export_name =
                self.create_declaration_name_reference_with_original(&name, original_name)?;
            let assignment_name =
                self.create_declaration_name_reference_with_original(&name, original_name)?;
            let object = self.create_object_literal()?;
            let assignment = self.create_assignment(assignment_name, object)?;
            let assignment = self.create_parenthesized(assignment)?;
            self.create_binary(export_name, SyntaxKind::BarBarToken, assignment)?
        };
        let call = self.create_call(function, vec![module_arg])?;
        let module_statement = self.create_expression_statement(call)?;
        self.set_original_and_range(module_statement, original)?;
        let mut emit_flags = EmitFlags::ADVISE_ON_EMIT_NODE;
        if first_in_scope {
            emit_flags |= EmitFlags::NO_LEADING_COMMENTS;
        }
        self.context
            .arena_mut()?
            .metadata_mut(module_statement)
            .add_flags(emit_flags);
        statements.push(module_statement);

        self.nodes.insert(id, None);
        self.expanded_modules.insert(
            id,
            statements
                .iter()
                .map(|statement| statement.node())
                .collect(),
        );
        Ok(statements)
    }

    fn transform_module_body(
        &mut self,
        declaration: NodeId,
        body: Option<NodeId>,
        container_name: &str,
        variable_flags: NodeFlags,
    ) -> Result<TransformNode, TransformError> {
        self.namespace_container_names
            .insert(declaration, container_name.into());
        self.namespace_stack.push(NamespaceContext {
            declaration,
            container_name: container_name.to_owned(),
            variable_flags,
        });
        self.context.start_lexical_environment()?;
        let result = (|| {
            let Some(body) = body else {
                return self.create_block_from_array(Vec::new(), None, true);
            };
            let body_node = self.node(body);
            match self.context.arena().node(body_node)?.data.clone() {
                NodeData::ModuleBlock(data) => {
                    let input = data
                        .statements
                        .and_then(|statements| {
                            self.context.arena().node_array_ref(self.source, statements)
                        })
                        .map(|statements| self.context.arena().node_array(statements))
                        .transpose()?
                        .map(|statements| statements.nodes.clone())
                        .unwrap_or_default();
                    let mut output = Vec::new();
                    for statement in input {
                        output.extend(self.visit_namespace_statement(statement)?);
                    }
                    let block = self.create_block_from_array(output, data.statements, true)?;
                    // transformModuleBody transfers the ModuleBlock's text
                    // range, but not its original-node identity. The latter
                    // would make the synthetic block re-own trailing comments
                    // from statements that the TypeScript transform erased.
                    self.context.factory()?.set_text_range(block, body_node)
                }
                NodeData::ModuleDeclaration(_) => {
                    let statements = self.visit_module_declaration(body)?;
                    self.create_block_from_array(statements, None, true)
                }
                _ => Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ModuleDeclaration,
                    field: "module body",
                }),
            }
        })();
        let lexical_environment = self.context.end_lexical_environment();
        self.namespace_stack.pop();
        let block = result?;
        self.merge_namespace_lexical_environment(block, lexical_environment?)
    }

    /// `transformModuleBody` owns a lexical environment in tsc. Destructuring
    /// temps are hoisted into that environment and merged after directive
    /// prologues, before the namespace's ordinary statements.
    fn merge_namespace_lexical_environment(
        &mut self,
        block: TransformNode,
        lexical_environment: LexicalEnvironment,
    ) -> Result<TransformNode, TransformError> {
        if lexical_environment.is_empty() {
            return Ok(block);
        }
        let NodeData::Block(mut data) = self.context.arena().node(block)?.data.clone() else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ModuleDeclaration,
                field: "namespace lexical block",
            });
        };
        let original_statements = data.statements;
        let mut statements =
            node_array_nodes(self.context.arena(), self.source, original_statements)?;
        let mut prologue_end = 0usize;
        while let Some(statement) = statements.get(prologue_end) {
            if !is_prologue_statement(self.context.arena(), *statement)? {
                break;
            }
            prologue_end += 1;
        }

        let mut insertion = prologue_end;
        if !lexical_environment.function_declarations().is_empty() {
            let functions = lexical_environment.function_declarations().to_vec();
            insertion += functions.len();
            statements.splice(prologue_end..prologue_end, functions);
        }
        if !lexical_environment.variable_declarations().is_empty() {
            let declarations = lexical_environment
                .variable_declarations()
                .iter()
                .copied()
                .map(|name| self.create_uninitialized_variable_declaration(name))
                .collect::<Result<Vec<_>, _>>()?;
            let statement = self.create_variable_statement(declarations, None, NodeFlags::NONE)?;
            self.context
                .arena_mut()?
                .metadata_mut(statement)
                .add_flags(EmitFlags::CUSTOM_PROLOGUE);
            statements.insert(insertion, statement);
            insertion += 1;
        }
        if !lexical_environment.initialization_statements().is_empty() {
            statements.splice(
                insertion..insertion,
                lexical_environment
                    .initialization_statements()
                    .iter()
                    .copied(),
            );
        }

        let statements = if let Some(original) = original_statements
            .and_then(|array| self.context.arena().node_array_ref(self.source, array))
        {
            self.context
                .factory()?
                .update_node_array(original, statements)?
        } else {
            self.context
                .factory()?
                .create_node_array(self.source, statements)?
        };
        data.statements = Some(statements.array());
        let flags =
            flags_after_update(self.context.arena(), block, &NodeData::Block(data.clone()))?;
        self.context
            .factory()?
            .update_node(block, NodeData::Block(data), flags)
    }

    fn visit_namespace_statement(
        &mut self,
        id: NodeId,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let original = self.node(id);
        let record = self.context.arena().node(original)?.clone();
        // tsc's namespaceElementVisitor erases every ES import/export and
        // also an external `import = require(...)` before the ordinary
        // TypeScript visitor gets a chance to retain a referenced alias.
        // The latter syntax is diagnosed inside a namespace (TS1147), so it
        // must not leak into the generated namespace IIFE even when uses of
        // the alias made the checker mark it as referenced.
        let external_import_equals = match &record.data {
            NodeData::ImportEqualsDeclaration(data) => data
                .module_reference
                .and_then(|reference| self.context.arena().node_ref(self.source, reference))
                .and_then(|reference| self.context.arena().node(reference).ok())
                .is_some_and(|reference| reference.kind == SyntaxKind::ExternalModuleReference),
            _ => false,
        };
        if matches!(
            record.kind,
            SyntaxKind::ImportDeclaration | SyntaxKind::ExportDeclaration
        ) || external_import_equals
        {
            return Ok(Vec::new());
        }
        match record.data {
            NodeData::ModuleDeclaration(_) => self.visit_module_declaration(id),
            NodeData::EnumDeclaration(_) => self.visit_enum_declaration(id),
            NodeData::VariableStatement(data)
                if self.has_modifier(data.modifiers, SyntaxKind::ExportKeyword)? =>
            {
                self.transform_namespace_exported_variables(original, data)
            }
            NodeData::FunctionDeclaration(ref data)
                if self.has_modifier(data.modifiers, SyntaxKind::ExportKeyword)? =>
            {
                self.visit_namespace_exported_declaration(id, data.name)
            }
            NodeData::ClassDeclaration(ref data)
                if self.has_modifier(data.modifiers, SyntaxKind::ExportKeyword)? =>
            {
                self.visit_namespace_exported_declaration(id, data.name)
            }
            _ => Ok(self
                .visit(id)?
                .map(|id| vec![self.node(id)])
                .unwrap_or_default()),
        }
    }

    fn visit_namespace_exported_declaration(
        &mut self,
        id: NodeId,
        name: Option<NodeId>,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let export_name = if let Some(name) = name {
            Some(self.identifier_text(name)?.to_owned())
        } else if matches!(
            self.context.arena().node(self.node(id))?.kind,
            SyntaxKind::ClassDeclaration | SyntaxKind::FunctionDeclaration
        ) {
            Some(self.ensure_generated_declaration_name(id, "default"))
        } else {
            None
        };
        let Some(updated) = self.visit_typescript(id)? else {
            return Ok(Vec::new());
        };
        let updated = self.node(updated);
        // Ambient exported declarations retain a NotEmittedStatement as their
        // comment/source anchor, but they do not create a runtime binding for
        // the namespace export assignment to read.
        if self.context.arena().node(updated)?.kind == SyntaxKind::NotEmittedStatement {
            return Ok(vec![updated]);
        }
        let mut statements = vec![updated];
        if let Some(name) = export_name {
            statements.push(self.create_namespace_export_assignment(self.node(id), &name)?);
        }
        Ok(statements)
    }

    fn ensure_generated_declaration_name(&mut self, declaration: NodeId, base: &str) -> String {
        if let Some(name) = self.generated_declaration_names.get(&declaration) {
            return name.to_string();
        }
        for ordinal in 1usize.. {
            let candidate = format!("{base}_{ordinal}");
            if !self.source_identifier_names.contains(&candidate)
                && self.generated_namespace_names.insert(candidate.clone())
            {
                self.generated_declaration_names
                    .insert(declaration, candidate.clone().into_boxed_str());
                return candidate;
            }
        }
        unreachable!("the generated declaration-name ordinal space is unbounded")
    }

    fn transform_namespace_exported_variables(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::VariableStatementData,
    ) -> Result<Vec<TransformNode>, TransformError> {
        if self.has_modifier(data.modifiers, SyntaxKind::DeclareKeyword)? {
            return Ok(Vec::new());
        }
        let Some(list) = data.declaration_list else {
            return Ok(Vec::new());
        };
        let list = self.node(list);
        let NodeData::VariableDeclarationList(list_data) =
            self.context.arena().node(list)?.data.clone()
        else {
            return Ok(Vec::new());
        };
        let declarations = list_data
            .declarations
            .and_then(|declarations| {
                self.context
                    .arena()
                    .node_array_ref(self.source, declarations)
            })
            .map(|declarations| self.context.arena().node_array(declarations))
            .transpose()?
            .map(|declarations| declarations.nodes.clone())
            .unwrap_or_default();
        let mut expressions = Vec::new();
        for declaration in declarations {
            let declaration_node = self.node(declaration);
            let NodeData::VariableDeclaration(declaration_data) =
                self.context.arena().node(declaration_node)?.data.clone()
            else {
                continue;
            };
            let (Some(name), Some(initializer)) =
                (declaration_data.name, declaration_data.initializer)
            else {
                continue;
            };
            let name = self.node(name);
            let initializer = self.node(initializer);
            let expression = match self.context.arena().node(name)?.kind {
                SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern => self
                    .flatten_namespace_destructuring_declaration(
                        declaration_node,
                        name,
                        initializer,
                    )?,
                SyntaxKind::Identifier => {
                    let name_text = self.identifier_text(name.node())?.to_owned();
                    let value = self.visit_required_namespace_expression(
                        initializer,
                        SyntaxKind::VariableDeclaration,
                        "initializer",
                    )?;
                    let target = self.create_namespace_export_target(&name_text)?;
                    let assignment = self.create_assignment(target, value)?;
                    self.set_original_and_range(assignment, declaration_node)?
                }
                _ => {
                    return Err(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::VariableDeclaration,
                        field: "binding name",
                    })
                }
            };
            expressions.push(expression);
        }
        if expressions.is_empty() {
            return Ok(Vec::new());
        }
        let expression = self.inline_namespace_destructuring_expressions(expressions)?;
        let statement = self.create_expression_statement(expression)?;
        let statement = self.set_original_and_range(statement, original)?;
        Ok(vec![statement])
    }

    /// tsc `transformInitializedVariable` + shared
    /// `flattenDestructuringAssignment(All)`, specialized only at the leaf
    /// publication boundary (`M.name = value`).
    fn flatten_namespace_destructuring_declaration(
        &mut self,
        original: TransformNode,
        pattern: TransformNode,
        initializer: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let value = self.visit_required_namespace_expression(
            initializer,
            SyntaxKind::VariableDeclaration,
            "initializer",
        )?;
        let force_fresh_value = match &self.context.arena().node(value)?.data {
            NodeData::Identifier(identifier) => {
                self.namespace_pattern_assigns_to_identifier(pattern, &identifier.text)?
            }
            _ => false,
        } || self
            .namespace_pattern_contains_nonliteral_computed_name(pattern)?;

        let mut plan = NamespaceDestructuringPlan::default();
        let value = if force_fresh_value {
            self.ensure_namespace_destructuring_identifier(&mut plan, value, false, Some(original))?
        } else {
            value
        };
        self.flatten_namespace_destructuring_target(&mut plan, pattern, value, Some(original))?;
        let expressions = self.materialize_namespace_destructuring_plan(plan)?;
        self.inline_namespace_destructuring_expressions(expressions)
    }

    fn flatten_namespace_destructuring_target(
        &mut self,
        plan: &mut NamespaceDestructuringPlan,
        target: TransformNode,
        value: TransformNode,
        original: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        match self.context.arena().node(target)?.data.clone() {
            NodeData::ObjectBindingPattern(data) => {
                self.flatten_namespace_object_pattern(plan, data.elements, value, original)
            }
            NodeData::ArrayBindingPattern(data) => {
                self.flatten_namespace_array_pattern(plan, data.elements, value, original)
            }
            NodeData::Identifier(identifier) => {
                plan.push(
                    NamespaceDestructuringTarget::Export(identifier.text.into()),
                    value,
                    original,
                );
                Ok(())
            }
            _ => Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::VariableDeclaration,
                field: "destructuring binding leaf",
            }),
        }
    }

    fn flatten_namespace_destructuring_element(
        &mut self,
        plan: &mut NamespaceDestructuringPlan,
        element: ModuleDestructuringElement,
        mut value: TransformNode,
    ) -> Result<(), TransformError> {
        if let Some(initializer) = element.initializer {
            let initializer = self.visit_required_namespace_expression(
                initializer,
                SyntaxKind::BindingElement,
                "initializer",
            )?;
            let initializer_is_simple =
                self.namespace_initializer_is_simple_inlineable(initializer)?;
            value = self.create_namespace_destructuring_default(
                plan,
                value,
                initializer,
                element.original,
            )?;
            if self.namespace_is_destructuring_pattern(element.target)? && !initializer_is_simple {
                value = self.ensure_namespace_destructuring_identifier(
                    plan,
                    value,
                    true,
                    Some(element.original),
                )?;
            }
        }
        self.flatten_namespace_destructuring_target(
            plan,
            element.target,
            value,
            Some(element.original),
        )
    }

    fn flatten_namespace_object_pattern(
        &mut self,
        plan: &mut NamespaceDestructuringPlan,
        elements: Option<NodeArrayId>,
        mut value: TransformNode,
        original: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        let elements = node_array_nodes(self.context.arena(), self.source, elements)?;
        if elements.len() != 1 {
            // Empty declaration patterns do not reuse an identifier value in
            // tsc; the generated write is what preserves initializer
            // evaluation in the otherwise leafless plan.
            value = self.ensure_namespace_destructuring_identifier(
                plan,
                value,
                !elements.is_empty(),
                original,
            )?;
        }
        let mut excluded = Vec::new();
        for (index, node) in elements.iter().copied().enumerate() {
            let element = self.namespace_destructuring_element(node)?;
            if element.rest {
                if index + 1 == elements.len() {
                    let rest =
                        self.create_namespace_object_rest(value, &excluded, element.original)?;
                    self.flatten_namespace_destructuring_element(plan, element, rest)?;
                }
                continue;
            }
            let (property_value, exclusion) =
                self.create_namespace_destructuring_property_access(plan, value, element)?;
            excluded.push(exclusion);
            self.flatten_namespace_destructuring_element(plan, element, property_value)?;
        }
        Ok(())
    }

    fn flatten_namespace_array_pattern(
        &mut self,
        plan: &mut NamespaceDestructuringPlan,
        elements: Option<NodeArrayId>,
        mut value: TransformNode,
        original: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        let elements = node_array_nodes(self.context.arena(), self.source, elements)?;
        let all_omitted = !elements.is_empty()
            && elements.iter().all(|element| {
                self.context
                    .arena()
                    .node(*element)
                    .is_ok_and(|node| node.kind == SyntaxKind::OmittedExpression)
            });
        if self.downlevel_iteration {
            self.context.request_emit_helper(helpers::read())?;
            let helper = self
                .context
                .factory()?
                .create_unscoped_helper_identifier(self.source, EmitHelperName::Read)?;
            let value_argument = self.context.factory()?.clone_node(value)?;
            let last_is_rest = elements.last().is_some_and(|element| {
                self.context.arena().node(*element).is_ok_and(|node| {
                    matches!(
                        &node.data,
                        NodeData::BindingElement(data) if data.dot_dot_dot_token.is_some()
                    )
                })
            });
            let mut arguments = vec![value_argument];
            if !last_is_rest {
                arguments.push(self.create_numeric_literal(&elements.len().to_string())?);
            }
            let read = self.create_call(helper, arguments)?;
            value = self.ensure_namespace_destructuring_identifier(plan, read, false, original)?;
        } else if elements.len() != 1 || all_omitted {
            value = self.ensure_namespace_destructuring_identifier(
                plan,
                value,
                !elements.is_empty(),
                original,
            )?;
        }
        for (index, node) in elements.into_iter().enumerate() {
            if self.context.arena().node(node)?.kind == SyntaxKind::OmittedExpression {
                continue;
            }
            let element = self.namespace_destructuring_element(node)?;
            let base = self.context.factory()?.clone_node(value)?;
            let element_value = if element.rest {
                let slice = self.create_property_access(base, "slice")?;
                let index = self.create_numeric_literal(&index.to_string())?;
                self.create_call(slice, vec![index])?
            } else {
                let index = self.create_numeric_literal(&index.to_string())?;
                self.create_element_access(base, index)?
            };
            self.flatten_namespace_destructuring_element(plan, element, element_value)?;
        }
        Ok(())
    }

    fn create_namespace_destructuring_property_access(
        &mut self,
        plan: &mut NamespaceDestructuringPlan,
        value: TransformNode,
        element: ModuleDestructuringElement,
    ) -> Result<(TransformNode, ModuleDestructuringExcludedProperty), TransformError> {
        let property_name = element
            .property_name
            .ok_or(TransformError::RequiredChildRemoved {
                parent: self.context.arena().node(element.original)?.kind,
                field: "property name",
            })?;
        let base = self.context.factory()?.clone_node(value)?;
        if let NodeData::ComputedPropertyName(data) =
            self.context.arena().node(property_name)?.data.clone()
        {
            let expression = data
                .expression
                .map(|expression| self.node(expression))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ComputedPropertyName,
                    field: "expression",
                })?;
            let argument = self.visit_required_namespace_expression(
                expression,
                SyntaxKind::ComputedPropertyName,
                "expression",
            )?;
            let argument = self.ensure_namespace_destructuring_identifier(
                plan,
                argument,
                false,
                Some(property_name),
            )?;
            let access_argument = self.context.factory()?.clone_node(argument)?;
            return Ok((
                self.create_element_access(base, access_argument)?,
                ModuleDestructuringExcludedProperty::Computed(argument),
            ));
        }
        let kind = self.context.arena().node(property_name)?.kind;
        if matches!(
            kind,
            SyntaxKind::StringLiteral | SyntaxKind::NumericLiteral | SyntaxKind::BigIntLiteral
        ) {
            let argument = self.context.factory()?.clone_node(property_name)?;
            return Ok((
                self.create_element_access(base, argument)?,
                ModuleDestructuringExcludedProperty::Named(property_name),
            ));
        }
        let property = identifier_or_literal_text(self.context.arena(), property_name)?;
        Ok((
            self.create_property_access(base, &property)?,
            ModuleDestructuringExcludedProperty::Named(property_name),
        ))
    }

    fn create_namespace_destructuring_default(
        &mut self,
        plan: &mut NamespaceDestructuringPlan,
        value: TransformNode,
        initializer: TransformNode,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let value =
            self.ensure_namespace_destructuring_identifier(plan, value, true, Some(original))?;
        let condition_value = self.context.factory()?.clone_node(value)?;
        let undefined = self.create_void_zero()?;
        let condition = self.create_binary(
            condition_value,
            SyntaxKind::EqualsEqualsEqualsToken,
            undefined,
        )?;
        let fallback = self.context.factory()?.clone_node(value)?;
        self.create_conditional(condition, initializer, fallback)
    }

    fn create_namespace_object_rest(
        &mut self,
        value: TransformNode,
        excluded: &[ModuleDestructuringExcludedProperty],
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.request_emit_helper(helpers::object_rest())?;
        let mut properties = Vec::with_capacity(excluded.len());
        for property in excluded {
            properties.push(match *property {
                ModuleDestructuringExcludedProperty::Named(name) => {
                    let text = identifier_or_literal_text(self.context.arena(), name)?;
                    self.create_string_literal(&text)?
                }
                ModuleDestructuringExcludedProperty::Computed(temp) => {
                    let type_value = self.context.factory()?.clone_node(temp)?;
                    let type_of = self.create_typeof(type_value)?;
                    let symbol = self.create_string_literal("symbol")?;
                    let condition =
                        self.create_binary(type_of, SyntaxKind::EqualsEqualsEqualsToken, symbol)?;
                    let symbol_value = self.context.factory()?.clone_node(temp)?;
                    let string_value = self.context.factory()?.clone_node(temp)?;
                    let empty = self.create_string_literal("")?;
                    let as_string =
                        self.create_binary(string_value, SyntaxKind::PlusToken, empty)?;
                    self.create_conditional(condition, symbol_value, as_string)?
                }
            });
        }
        let excluded = self.create_array_literal(properties)?;
        self.context.factory()?.set_text_range(excluded, original)?;
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(self.source, EmitHelperName::Rest)?;
        let value = self.context.factory()?.clone_node(value)?;
        self.create_call(helper, vec![value, excluded])
    }

    fn ensure_namespace_destructuring_identifier(
        &mut self,
        plan: &mut NamespaceDestructuringPlan,
        value: TransformNode,
        reuse_identifier: bool,
        original: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        if reuse_identifier && self.context.arena().node(value)?.kind == SyntaxKind::Identifier {
            return Ok(value);
        }
        let binding = NamespaceTempBinding(self.next_typescript_temp_name().into());
        plan.push(
            NamespaceDestructuringTarget::Temp(binding.clone()),
            value,
            original,
        );
        self.create_identifier(&binding.0)
    }

    fn materialize_namespace_destructuring_plan(
        &mut self,
        plan: NamespaceDestructuringPlan,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let mut expressions = Vec::with_capacity(plan.steps.len());
        for step in plan.steps {
            let target = match step.target {
                NamespaceDestructuringTarget::Temp(binding) => {
                    let declaration = self.create_identifier(&binding.0)?;
                    self.context.hoist_variable_declaration(declaration)?;
                    self.create_identifier(&binding.0)?
                }
                NamespaceDestructuringTarget::Export(name) => {
                    self.create_namespace_export_target(&name)?
                }
            };
            let assignment = self.create_assignment(target, step.value)?;
            expressions.push(if let Some(original) = step.original {
                self.set_original_and_range(assignment, original)?
            } else {
                assignment
            });
        }
        Ok(expressions)
    }

    fn inline_namespace_destructuring_expressions(
        &mut self,
        expressions: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let mut expressions = expressions.into_iter();
        let Some(mut expression) = expressions.next() else {
            return self.create_omitted_expression();
        };
        for next in expressions {
            expression = self.create_binary(expression, SyntaxKind::CommaToken, next)?;
        }
        Ok(expression)
    }

    fn create_namespace_export_target(
        &mut self,
        name: &str,
    ) -> Result<TransformNode, TransformError> {
        let container_name = self
            .namespace_stack
            .last()
            .expect("namespace export has a namespace container")
            .container_name
            .clone();
        let container = self.create_identifier(&container_name)?;
        self.create_property_access(container, name)
    }

    fn visit_required_namespace_expression(
        &mut self,
        expression: TransformNode,
        parent: SyntaxKind,
        field: &'static str,
    ) -> Result<TransformNode, TransformError> {
        self.visit(expression.node())?
            .map(|visited| self.node(visited))
            .ok_or(TransformError::RequiredChildRemoved { parent, field })
    }

    fn namespace_destructuring_element(
        &self,
        element: TransformNode,
    ) -> Result<ModuleDestructuringElement, TransformError> {
        let NodeData::BindingElement(data) = &self.context.arena().node(element)?.data else {
            return Err(TransformError::RequiredChildRemoved {
                parent: self.context.arena().node(element)?.kind,
                field: "binding element",
            });
        };
        let target = data
            .name
            .and_then(|name| self.context.arena().node_ref(self.source, name))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::BindingElement,
                field: "name",
            })?;
        Ok(ModuleDestructuringElement {
            original: element,
            target,
            property_name: data
                .property_name
                .and_then(|name| self.context.arena().node_ref(self.source, name))
                .or_else(|| data.dot_dot_dot_token.is_none().then_some(target)),
            initializer: data
                .initializer
                .and_then(|initializer| self.context.arena().node_ref(self.source, initializer)),
            rest: data.dot_dot_dot_token.is_some(),
        })
    }

    fn namespace_pattern_assigns_to_identifier(
        &self,
        pattern: TransformNode,
        identifier: &str,
    ) -> Result<bool, TransformError> {
        if let NodeData::Identifier(data) = &self.context.arena().node(pattern)?.data {
            return Ok(data.text == identifier);
        }
        let elements = match &self.context.arena().node(pattern)?.data {
            NodeData::ObjectBindingPattern(data) => {
                node_array_nodes(self.context.arena(), self.source, data.elements)?
            }
            NodeData::ArrayBindingPattern(data) => {
                node_array_nodes(self.context.arena(), self.source, data.elements)?
            }
            _ => return Ok(false),
        };
        for element in elements {
            if self.context.arena().node(element)?.kind == SyntaxKind::OmittedExpression {
                continue;
            }
            if self.namespace_pattern_assigns_to_identifier(
                self.namespace_destructuring_element(element)?.target,
                identifier,
            )? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn namespace_pattern_contains_nonliteral_computed_name(
        &self,
        pattern: TransformNode,
    ) -> Result<bool, TransformError> {
        let elements = match &self.context.arena().node(pattern)?.data {
            NodeData::ObjectBindingPattern(data) => {
                node_array_nodes(self.context.arena(), self.source, data.elements)?
            }
            NodeData::ArrayBindingPattern(data) => {
                node_array_nodes(self.context.arena(), self.source, data.elements)?
            }
            _ => return Ok(false),
        };
        for element in elements {
            if self.context.arena().node(element)?.kind == SyntaxKind::OmittedExpression {
                continue;
            }
            let element = self.namespace_destructuring_element(element)?;
            if let Some(property_name) = element.property_name {
                if let NodeData::ComputedPropertyName(data) =
                    &self.context.arena().node(property_name)?.data
                {
                    let literal = data
                        .expression
                        .and_then(|expression| {
                            self.context.arena().node_ref(self.source, expression)
                        })
                        .is_some_and(|expression| {
                            self.context.arena().node(expression).is_ok_and(|node| {
                                matches!(
                                    node.kind,
                                    SyntaxKind::StringLiteral
                                        | SyntaxKind::NumericLiteral
                                        | SyntaxKind::BigIntLiteral
                                        | SyntaxKind::NoSubstitutionTemplateLiteral
                                )
                            })
                        });
                    if !literal {
                        return Ok(true);
                    }
                }
            }
            if self.namespace_pattern_contains_nonliteral_computed_name(element.target)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn namespace_is_destructuring_pattern(
        &self,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        Ok(matches!(
            self.context.arena().node(node)?.kind,
            SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern
        ))
    }

    fn namespace_initializer_is_simple_inlineable(
        &self,
        expression: TransformNode,
    ) -> Result<bool, TransformError> {
        let kind = self.context.arena().node(expression)?.kind;
        let simple_copiable = matches!(
            kind,
            SyntaxKind::StringLiteral
                | SyntaxKind::NoSubstitutionTemplateLiteral
                | SyntaxKind::NumericLiteral
                | SyntaxKind::Identifier
        ) || kind.value() >= SyntaxKind::FirstKeyword.value()
            && kind.value() <= SyntaxKind::LastKeyword.value();
        Ok(kind != SyntaxKind::Identifier && simple_copiable)
    }

    fn next_typescript_temp_name(&mut self) -> String {
        loop {
            let ordinal = self.temp_ordinal;
            self.temp_ordinal += 1;
            let candidate = if ordinal < 26 {
                format!("_{}", (b'a' + ordinal as u8) as char)
            } else {
                format!("_{}", ordinal - 26)
            };
            if !self.source_identifier_names.contains(&candidate)
                && self.generated_namespace_names.insert(candidate.clone())
            {
                return candidate;
            }
        }
    }

    fn create_namespace_export_assignment(
        &mut self,
        original: TransformNode,
        name: &str,
    ) -> Result<TransformNode, TransformError> {
        let container_name = self
            .namespace_stack
            .last()
            .expect("namespace export has a namespace container")
            .container_name
            .clone();
        let container = self.create_identifier(&container_name)?;
        let target = self.create_property_access(container, name)?;
        let value = self.create_identifier(name)?;
        let assignment = self.create_assignment(target, value)?;
        // createExportMemberAssignmentStatement gives the assignment a
        // source-map range from the declaration name through the declaration
        // end, but deliberately leaves its text/comment range synthesized.
        // Keeping those ownership channels distinct prevents the declaration's
        // trailing comment from being emitted a second time on `N.member =
        // member`.
        let (assignment_range, statement_range) = {
            let arena = self.context.arena();
            let source = arena.source(original.source())?.syntax();
            let record = arena.node(original)?;
            let name_node = match &record.data {
                NodeData::FunctionDeclaration(data) => data.name,
                NodeData::ClassDeclaration(data) => data.name,
                _ => None,
            };
            let start = name_node
                .and_then(|name| arena.node_ref(original.source(), name))
                .and_then(|name| arena.node(name).ok())
                .map_or(record.pos, |name| name.pos);
            let assignment_range = SourceRange::from_raw(start, record.end, source.positions())
                .expect("parsed declaration source range is valid");
            // tsc represents the statement range as [-1, node.end]. The
            // typed Rust equivalent is an end-point range whose leading hook
            // is disabled below.
            let statement_range = SourceRange::from_raw(record.end, record.end, source.positions())
                .expect("parsed declaration end is a valid source position");
            (assignment_range, statement_range)
        };
        self.context
            .arena_mut()?
            .metadata_mut(assignment)
            .set_source_map_range(SourceMapRange::new(original.source(), assignment_range));
        let statement = self.create_expression_statement(assignment)?;
        let metadata = self.context.arena_mut()?.metadata_mut(statement);
        metadata.set_source_map_range(SourceMapRange::new(original.source(), statement_range));
        metadata.add_flags(EmitFlags::NO_LEADING_SOURCE_MAP);
        Ok(statement)
    }

    /// tsc-port: visitEnumDeclaration @6.0.3
    /// tsc-hash: 932cc1ff33658d02275ad8c936dadbfeb77f89f6645b194ba826e38c5e1e676a
    /// tsc-span: _tsc.js:95177-95311
    fn visit_enum_declaration(&mut self, id: NodeId) -> Result<Vec<TransformNode>, TransformError> {
        if let Some(expanded) = self.expanded_enums.get(&id) {
            return Ok(expanded.iter().copied().map(|id| self.node(id)).collect());
        }
        let original = self.node(id);
        let record = self.context.arena().node(original)?.clone();
        let NodeData::EnumDeclaration(data) = record.data else {
            return Err(TransformError::RequiredChildRemoved {
                parent: record.kind,
                field: "enum declaration",
            });
        };
        let ambient = NodeFlags::from_bits(record.flags).contains(NodeFlags::AMBIENT)
            || self.has_modifier(data.modifiers, SyntaxKind::DeclareKeyword)?;
        let is_const = self.has_modifier(data.modifiers, SyntaxKind::ConstKeyword)?;
        if ambient || is_const && !self.preserve_const_enums {
            let anchor = self
                .context
                .factory()?
                .create_not_emitted_statement(original)?;
            self.nodes.insert(id, Some(anchor.node()));
            self.expanded_enums.insert(id, vec![anchor.node()]);
            return Ok(vec![anchor]);
        }

        let name_id = data.name.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::EnumDeclaration,
            field: "name",
        })?;
        let name = self.identifier_text(name_id)?.to_owned();
        let original_name = self.node(name_id);
        let lexical_scope = self.lexical_scope_owner(original)?;
        let first_in_scope = self
            .emitted_declarations
            .insert((lexical_scope.container(), name.clone()));
        let mut statements = Vec::with_capacity(if first_in_scope { 2 } else { 1 });

        if first_in_scope {
            let declaration_name = self.create_identifier_with_original(&name, original_name)?;
            let declaration = self.context.factory()?.create_node(
                self.source,
                NodeData::VariableDeclaration(tsc_syntax::nodes::VariableDeclarationData {
                    name: Some(declaration_name.node()),
                    exclamation_token: None,
                    r#type: None,
                    initializer: None,
                }),
                TransformFlags::NONE,
            )?;
            self.context
                .arena_mut()?
                .set_original_node(declaration, Some(original))?;
            let modifiers = self.enum_runtime_modifiers(data.modifiers)?;
            let flags = lexical_scope.variable_flags();
            let statement = self.create_variable_statement(vec![declaration], modifiers, flags)?;
            self.set_original_and_range(statement, original)?;
            self.context
                .arena_mut()?
                .metadata_mut(statement)
                .add_flags(EmitFlags::NO_TRAILING_COMMENTS);
            statements.push(statement);
        }

        let members = data
            .members
            .and_then(|members| self.context.arena().node_array_ref(self.source, members))
            .map(|members| self.context.arena().node_array(members))
            .transpose()?
            .map(|members| members.nodes.clone())
            .unwrap_or_default();
        self.enum_container_names
            .insert(id, name.clone().into_boxed_str());
        let mut member_statements = Vec::with_capacity(members.len());
        for member in members {
            member_statements.push(self.transform_enum_member(member, &name)?);
        }
        let body = self.create_block_from_array(member_statements, data.members, true)?;
        let parameter = self.create_parameter(&name)?;
        let function = self.create_function_expression(vec![parameter], body)?;
        let function = self.create_parenthesized(function)?;

        let exported_from_namespace = !self.namespace_stack.is_empty()
            && self.has_modifier(data.modifiers, SyntaxKind::ExportKeyword)?;
        let module_arg = if exported_from_namespace {
            let container_name = self
                .namespace_stack
                .last()
                .expect("exported enum has an outer namespace")
                .container_name
                .clone();
            let container = self.create_identifier(&container_name)?;
            let export_name = self.create_property_access(container, &name)?;
            let container = self.create_identifier(&container_name)?;
            let assignment_left = self.create_property_access(container, &name)?;
            let object = self.create_object_literal()?;
            let assignment = self.create_assignment(assignment_left, object)?;
            let assignment = self.create_parenthesized(assignment)?;
            let exported = self.create_binary(export_name, SyntaxKind::BarBarToken, assignment)?;
            let local_name = self.create_identifier_with_original(&name, original_name)?;
            self.create_assignment(local_name, exported)?
        } else {
            let export_name =
                self.create_declaration_name_reference_with_original(&name, original_name)?;
            let assignment_left =
                self.create_declaration_name_reference_with_original(&name, original_name)?;
            let object = self.create_object_literal()?;
            let assignment = self.create_assignment(assignment_left, object)?;
            let assignment = self.create_parenthesized(assignment)?;
            self.create_binary(export_name, SyntaxKind::BarBarToken, assignment)?
        };
        let call = self.create_call(function, vec![module_arg])?;
        let enum_statement = self.create_expression_statement(call)?;
        self.set_original_and_range(enum_statement, original)?;
        let mut emit_flags = EmitFlags::ADVISE_ON_EMIT_NODE;
        if first_in_scope {
            emit_flags |= EmitFlags::NO_LEADING_COMMENTS;
        }
        self.context
            .arena_mut()?
            .metadata_mut(enum_statement)
            .add_flags(emit_flags);
        statements.push(enum_statement);

        self.nodes.insert(id, None);
        self.expanded_enums.insert(
            id,
            statements
                .iter()
                .map(|statement| statement.node())
                .collect(),
        );
        Ok(statements)
    }

    fn transform_enum_member(
        &mut self,
        id: NodeId,
        container_name: &str,
    ) -> Result<TransformNode, TransformError> {
        let original = self.node(id);
        let NodeData::EnumMember(data) = self.context.arena().node(original)?.data.clone() else {
            return Err(TransformError::RequiredChildRemoved {
                parent: self.context.arena().node(original)?.kind,
                field: "enum member",
            });
        };
        let evaluated = self
            .resolver
            .get_enum_member_value(self.resolver_node(original)?)?;
        let value_expression = if let Some(value) = evaluated
            .as_ref()
            .and_then(crate::EmitEnumMemberValue::value)
        {
            self.create_constant_expression(value)?
        } else if let Some(initializer) = data.initializer {
            self.visit(initializer)?
                .map(|initializer| self.node(initializer))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::EnumMember,
                    field: "initializer",
                })?
        } else {
            self.create_void_zero()?
        };
        let name = data.name.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::EnumMember,
            field: "name",
        })?;
        let name_expression = self.enum_member_name_expression(name)?;
        let reverse_name = self.context.factory()?.clone_node(name_expression)?;
        let container = self.create_identifier(container_name)?;
        let target = self.create_element_access(container, name_expression)?;
        let inner_assignment = self.create_assignment(target, value_expression)?;
        let is_string = evaluated.as_ref().is_some_and(|evaluated| {
            matches!(evaluated.value(), Some(EmitConstantValue::String(_)))
                || evaluated.is_syntactically_string()
        });
        let expression = if is_string {
            inner_assignment
        } else {
            let container = self.create_identifier(container_name)?;
            let target = self.create_element_access(container, inner_assignment)?;
            self.create_assignment(target, reverse_name)?
        };
        self.set_original_and_range(expression, original)?;
        let statement = self.create_expression_statement(expression)?;
        self.set_original_and_range(statement, original)
    }

    fn enum_member_name_expression(&mut self, id: NodeId) -> Result<TransformNode, TransformError> {
        let original = self.node(id);
        match self.context.arena().node(original)?.data.clone() {
            NodeData::Identifier(data) => self.create_string_literal(&data.text),
            NodeData::StringLiteral(data) => self.create_string_literal(&data.text),
            NodeData::NumericLiteral(data) => self.create_numeric_literal(&data.text),
            NodeData::BigIntLiteral(_) => self.context.factory()?.clone_node(original),
            NodeData::ComputedPropertyName(data) => {
                let expression = data
                    .expression
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ComputedPropertyName,
                        field: "expression",
                    })?;
                self.visit(expression)?
                    .map(|expression| self.node(expression))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ComputedPropertyName,
                        field: "expression",
                    })
            }
            // Invalid enum recovery follows getExpressionForPropertyName:
            // a private name has no runtime property spelling, so retain an
            // empty expression Identifier rather than inventing the string
            // key/value "".
            NodeData::PrivateIdentifier(_) => self.create_identifier(""),
            _ => Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::EnumMember,
                field: "property name",
            }),
        }
    }

    fn enum_runtime_modifiers(
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
                self.context.arena().node(*modifier).is_ok_and(|modifier| {
                    modifier.kind != SyntaxKind::ConstKeyword
                        && !is_typescript_modifier(modifier.kind)
                        && (self.namespace_stack.is_empty()
                            || modifier.kind != SyntaxKind::ExportKeyword)
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

    fn parameter_runtime_modifiers(
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
                self.context
                    .arena()
                    .node(*modifier)
                    .is_ok_and(|modifier| modifier.kind == SyntaxKind::Decorator)
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

    fn module_runtime_modifiers(
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
                self.context.arena().node(*modifier).is_ok_and(|modifier| {
                    !is_typescript_modifier(modifier.kind)
                        && (self.namespace_stack.is_empty()
                            || modifier.kind != SyntaxKind::ExportKeyword)
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

    fn identifier_text(&self, id: NodeId) -> Result<&str, TransformError> {
        match &self.context.arena().node(self.node(id))?.data {
            NodeData::Identifier(data) => Ok(&data.text),
            _ => Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::EnumDeclaration,
                field: "identifier name",
            }),
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

    fn create_identifier_with_original(
        &mut self,
        text: &str,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let identifier = self.create_declaration_identifier_with_original(text, original)?;
        self.context
            .arena_mut()?
            .metadata_mut(identifier)
            .add_flags(EmitFlags::LOCAL_NAME);
        Ok(identifier)
    }

    /// Rust ownership projection of `NodeFactory.getDeclarationName`.
    ///
    /// A declaration name retains parse-tree identity so later module
    /// transforms can resolve its binding. It must not carry `LOCAL_NAME`:
    /// that flag represents `getLocalName` and deliberately suppresses export
    /// substitution.
    fn create_declaration_identifier_with_original(
        &mut self,
        text: &str,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let identifier = self.create_identifier(text)?;
        self.context
            .arena_mut()?
            .set_original_node(identifier, Some(original))?;
        Ok(identifier)
    }

    /// Expression-owned projection of `NodeFactory.getDeclarationName`.
    ///
    /// The original identifier remains the resolver identity, while the
    /// internal flag records the transformed parent role that TypeScript gets
    /// from mutable parent pointers. This lets CommonJS ask the checker for
    /// the ordinary (`prefixLocals = false`) export container without treating
    /// the expression as the declaration name itself.
    fn create_declaration_name_reference_with_original(
        &mut self,
        text: &str,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let identifier = self.create_declaration_identifier_with_original(text, original)?;
        self.context
            .arena_mut()?
            .metadata_mut(identifier)
            .set_internal_flags(InternalEmitFlags::DECLARATION_NAME_REFERENCE);
        Ok(identifier)
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

    fn create_constant_expression(
        &mut self,
        value: &EmitConstantValue,
    ) -> Result<TransformNode, TransformError> {
        match value {
            EmitConstantValue::String(value) => {
                let text = String::from_utf16_lossy(value.code_units());
                let literal = self.create_string_literal(&text)?;
                self.context
                    .arena_mut()?
                    .metadata_mut(literal)
                    .set_javascript_string_value(value.clone());
                Ok(literal)
            }
            EmitConstantValue::Number(value) => {
                let value = value.as_f64();
                if value.is_nan() {
                    return self.create_identifier("NaN");
                }
                if value == f64::INFINITY {
                    return self.create_identifier("Infinity");
                }
                if value == f64::NEG_INFINITY {
                    let infinity = self.create_identifier("Infinity")?;
                    return self.create_prefix_unary(SyntaxKind::MinusToken, infinity);
                }
                if value < 0.0 {
                    let literal =
                        self.create_numeric_literal(&tsc_types::js_number_to_string(-value))?;
                    self.create_prefix_unary(SyntaxKind::MinusToken, literal)
                } else {
                    self.create_numeric_literal(&tsc_types::js_number_to_string(value))
                }
            }
            EmitConstantValue::Boolean(value) => {
                self.create_identifier(if *value { "true" } else { "false" })
            }
        }
    }

    fn create_prefix_unary(
        &mut self,
        operator: SyntaxKind,
        operand: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::PrefixUnaryExpression(tsc_syntax::nodes::PrefixUnaryExpressionData {
                operator,
                operand: Some(operand.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_object_literal(&mut self) -> Result<TransformNode, TransformError> {
        let properties = self
            .context
            .factory()?
            .create_node_array(self.source, Vec::new())?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ObjectLiteralExpression(tsc_syntax::nodes::ObjectLiteralExpressionData {
                properties: Some(properties.array()),
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

    fn create_element_access(
        &mut self,
        expression: TransformNode,
        argument: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::ElementAccessExpression(tsc_syntax::nodes::ElementAccessExpressionData {
                expression: Some(expression.node()),
                question_dot_token: None,
                argument_expression: Some(argument.node()),
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

    fn create_assignment(
        &mut self,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_binary(left, SyntaxKind::EqualsToken, right)
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

    fn create_omitted_expression(&mut self) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::OmittedExpression(tsc_syntax::nodes::OmittedExpressionData {}),
            TransformFlags::NONE,
        )
    }

    fn create_uninitialized_variable_declaration(
        &mut self,
        name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::VariableDeclaration(tsc_syntax::nodes::VariableDeclarationData {
                name: Some(name.node()),
                exclamation_token: None,
                r#type: None,
                initializer: None,
            }),
            TransformFlags::NONE,
        )
    }

    fn create_variable_statement(
        &mut self,
        declarations: Vec<TransformNode>,
        modifiers: Option<NodeArrayId>,
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
                modifiers,
                declaration_list: Some(list.node()),
            }),
            TransformFlags::NONE,
        )
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

    /// A single statement field can receive a statement array from the
    /// TypeScript transform (notably runtime enums and namespaces). Preserve
    /// one statement directly and give larger expansions a structural owner.
    /// The NodeFactory separately converts a lone NotEmittedStatement anchor
    /// into an EmptyStatement when it is installed in a control-flow node.
    ///
    /// tsc-port: NodeFactory.liftToBlock @6.0.3
    /// tsc-hash: c96ac6375abe99aeb4b2779fc5d1a4b28d835df33d5198647cd888d1abd36a48
    /// tsc-span: _tsc.js:24878-24881
    fn lift_statement_expansion(
        &mut self,
        mut statements: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        if statements.len() == 1 {
            return Ok(statements.remove(0));
        }
        self.create_block_from_array(statements, None, true)
    }

    fn set_original_and_range(
        &mut self,
        node: TransformNode,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.set_text_range(node, original)?;
        self.context
            .arena_mut()?
            .set_original_node(node, Some(original))?;
        Ok(node)
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

    /// Preserve a decorated ambient property as the handoff record consumed
    /// by `transformLegacyDecorators`. The record has no runtime field shape:
    /// only visited decorators, the visited property name, and a synthesized
    /// `declare` marker survive. The later legacy pass uses that marker to
    /// keep a computed name at the decoration call instead of evaluating it
    /// in the class body.
    ///
    /// tsc-port: transformTypeScript/visitPropertyDeclaration @6.0.3
    /// tsc-hash: 599b3e2dee89e237fde844c97c77a0e4ea82b11aaaa5bacdccafc4d76db0508f
    /// tsc-span: _tsc.js:94763-94792
    fn update_ambient_property_declaration(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::PropertyDeclarationData,
    ) -> Result<NodeId, TransformError> {
        data.modifiers = Some(self.visit_ambient_property_modifiers(data.modifiers)?);
        data.name = self.visit_optional_child(data.name)?;
        data.question_token = None;
        data.exclamation_token = None;
        data.r#type = None;
        data.initializer = None;
        let data = NodeData::PropertyDeclaration(data);
        let flags = flags_after_update(self.context.arena(), original, &data)?;
        Ok(self
            .context
            .factory()?
            .update_node(original, data, flags)?
            .node())
    }

    fn visit_ambient_property_modifiers(
        &mut self,
        modifiers: Option<NodeArrayId>,
    ) -> Result<NodeArrayId, TransformError> {
        // This is a property-owned contextual projection, not the ordinary
        // modifier-array visit: all source modifiers are erased while
        // decorators are visited and one synthetic `declare` is appended.
        // Do not publish that contextual result through `self.arrays`, where
        // a separately owned use of a synthetic shared array could observe it.
        let mut retained = Vec::new();
        let original = modifiers
            .and_then(|modifiers| self.context.arena().node_array_ref(self.source, modifiers));
        if let Some(original) = original {
            let input = self.context.arena().node_array(original)?.nodes.clone();
            for modifier in input {
                let modifier_node = self.node(modifier);
                if self.context.arena().node(modifier_node)?.kind == SyntaxKind::Decorator {
                    if let Some(modifier) = self.visit(modifier)? {
                        retained.push(self.node(modifier));
                    }
                }
            }
        }
        retained.push(self.context.factory()?.create_token(
            self.source,
            SyntaxKind::DeclareKeyword,
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )?);
        let modifiers = if let Some(original) = original {
            self.context
                .factory()?
                .update_node_array(original, retained)?
        } else {
            self.context
                .factory()?
                .create_node_array(self.source, retained)?
        };
        Ok(modifiers.array())
    }

    /// Setter return types and type parameters are parser-recovery fields,
    /// not part of the runtime setter factory. Visit and erase only the
    /// runtime shape here, then let NodeFactory restore those original fields
    /// exactly as tsc's `finishUpdateSetAccessorDeclaration` does.
    fn update_set_accessor_declaration(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::SetAccessorData,
    ) -> Result<NodeId, TransformError> {
        let mut runtime_data = NodeData::SetAccessor(data);
        try_visit_each_child(&mut runtime_data, self)?;
        let flags = flags_after_update(self.context.arena(), original, &runtime_data)?;
        let NodeData::SetAccessor(runtime_data) = runtime_data else {
            unreachable!("setter update retains its syntax kind")
        };
        let updated = self.context.factory()?.update_set_accessor_declaration(
            original,
            runtime_data.modifiers,
            runtime_data.name,
            runtime_data.parameters,
            runtime_data.body,
            flags,
        )?;
        Ok(updated.node())
    }

    /// Constructor type parameters and return types are attached by parser
    /// recovery after `createConstructorDeclaration` has established the
    /// runtime node shape. The public factory updater therefore accepts only
    /// modifiers, parameters, and body, then restores those recovery fields
    /// from the original when a runtime field changes.
    ///
    /// tsc-port: updateConstructorDeclaration/finishUpdateConstructorDeclaration @6.0.3
    /// tsc-hash: 458f5a752c894ba21fc18800fe4a10be5fd7f9e837fd38e4c0f20ba1e054072e
    /// tsc-span: _tsc.js:21982-22010
    fn update_constructor_declaration(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::ConstructorData,
    ) -> Result<NodeId, TransformError> {
        let mut runtime_data = NodeData::Constructor(data);
        try_visit_each_child(&mut runtime_data, self)?;
        let flags = flags_after_update(self.context.arena(), original, &runtime_data)?;
        let NodeData::Constructor(runtime_data) = runtime_data else {
            unreachable!("constructor update retains its syntax kind")
        };
        let updated = self.context.factory()?.update_constructor_declaration(
            original,
            runtime_data.modifiers,
            runtime_data.parameters,
            runtime_data.body,
            flags,
        )?;
        Ok(updated.node())
    }

    /// Getter type parameters are likewise a parser-recovery extension of
    /// the factory-owned getter shape. Its return type is a real factory
    /// field and remains erased by transformTypeScript; only type parameters
    /// are restored by the updater when the runtime getter changes.
    ///
    /// tsc-port: updateGetAccessorDeclaration/finishUpdateGetAccessorDeclaration @6.0.3
    /// tsc-hash: c2cee5560b6c2d55d7fc907e6cef6821f93e3bfa32f5bc1e1d0c4c264dfa4ac6
    /// tsc-span: _tsc.js:22012-22043
    fn update_get_accessor_declaration(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::GetAccessorData,
    ) -> Result<NodeId, TransformError> {
        let mut runtime_data = NodeData::GetAccessor(data);
        try_visit_each_child(&mut runtime_data, self)?;
        let flags = flags_after_update(self.context.arena(), original, &runtime_data)?;
        let NodeData::GetAccessor(runtime_data) = runtime_data else {
            unreachable!("getter update retains its syntax kind")
        };
        let updated = self.context.factory()?.update_get_accessor_declaration(
            original,
            runtime_data.modifiers,
            runtime_data.name,
            runtime_data.parameters,
            runtime_data.r#type,
            runtime_data.body,
            flags,
        )?;
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

    fn visit_parenthesized_expression(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::ParenthesizedExpressionData,
    ) -> Result<Option<NodeId>, TransformError> {
        if self.parentheses_wrap_type_assertion(data.expression)? {
            return self.visit_partially_emitted(original, data.expression);
        }
        Ok(Some(self.update_generic(
            original,
            NodeData::ParenthesizedExpression(data),
        )?))
    }

    fn parentheses_wrap_type_assertion(
        &self,
        expression: Option<NodeId>,
    ) -> Result<bool, TransformError> {
        let Some(mut expression) = expression else {
            return Ok(false);
        };
        loop {
            let record = self.context.arena().node(self.node(expression))?;
            expression = match &record.data {
                NodeData::ParenthesizedExpression(data) => match data.expression {
                    Some(expression) => expression,
                    None => return Ok(false),
                },
                NodeData::PartiallyEmittedExpression(data) => match data.expression {
                    Some(expression) => expression,
                    None => return Ok(false),
                },
                NodeData::AsExpression(_)
                | NodeData::TypeAssertionExpression(_)
                | NodeData::SatisfiesExpression(_) => return Ok(true),
                _ => return Ok(false),
            };
        }
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

    /// tsc-port: visitImportEqualsDeclaration @6.0.3
    /// tsc-hash: 5ef8a385c17d4f71d34bdb72046973d6ddc5012c9e4c705883c5263d5629c703
    /// tsc-span: _tsc.js:95600-95644
    fn visit_import_equals_declaration(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ImportEqualsDeclarationData,
    ) -> Result<Option<NodeId>, TransformError> {
        if data.is_type_only {
            return Ok(None);
        }
        let module_reference = data
            .module_reference
            .and_then(|id| self.context.arena().node_ref(self.source, id))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ImportEqualsDeclaration,
                field: "module_reference",
            })?;
        let referenced = self
            .resolver
            .is_referenced_alias_declaration(self.resolver_node(original)?)?;
        if self.context.arena().node(module_reference)?.kind == SyntaxKind::ExternalModuleReference
        {
            if !referenced {
                return Ok(None);
            }
            data.modifiers = self.module_runtime_modifiers(data.modifiers)?;
            return Ok(Some(self.update_generic(
                original,
                NodeData::ImportEqualsDeclaration(data),
            )?));
        }

        let source_is_external = self
            .context
            .arena()
            .source(self.source)?
            .syntax()
            .external_module_indicator
            .is_some();
        if !referenced
            && (source_is_external
                || !self
                    .resolver
                    .is_top_level_value_import_equals_with_entity_name(
                        self.resolver_node(original)?,
                    )?)
        {
            return Ok(None);
        }

        let name = data.name.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::ImportEqualsDeclaration,
            field: "name",
        })?;
        let name = self
            .visit(name)?
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ImportEqualsDeclaration,
                field: "name",
            })?;
        let module_reference = self.create_expression_from_entity_name(module_reference)?;
        self.context
            .arena_mut()?
            .metadata_mut(module_reference)
            .add_flags(EmitFlags::NO_COMMENTS | EmitFlags::NO_NESTED_COMMENTS);
        if !self.namespace_stack.is_empty()
            && self.has_modifier(data.modifiers, SyntaxKind::ExportKeyword)?
        {
            let name_node = self.node(name);
            let name = self.identifier_text(name)?.to_owned();
            let container_name = self
                .namespace_stack
                .last()
                .expect("namespace export has a namespace container")
                .container_name
                .clone();
            let container = self.create_identifier(&container_name)?;
            let target = self.create_property_access(container, &name)?;
            self.context.factory()?.set_text_range(target, name_node)?;
            let assignment = self.create_assignment(target, module_reference)?;
            let statement = self.create_expression_statement(assignment)?;
            self.set_original_and_range(statement, original)?;
            return Ok(Some(statement.node()));
        }
        let declaration = self.context.factory()?.create_node(
            self.source,
            NodeData::VariableDeclaration(tsc_syntax::nodes::VariableDeclarationData {
                name: Some(name),
                exclamation_token: None,
                r#type: None,
                initializer: Some(module_reference.node()),
            }),
            TransformFlags::NONE,
        )?;
        self.context
            .arena_mut()?
            .set_original_node(declaration, Some(original))?;
        let modifiers = self.module_runtime_modifiers(data.modifiers)?;
        let statement =
            self.create_variable_statement(vec![declaration], modifiers, NodeFlags::NONE)?;
        self.set_original_and_range(statement, original)?;
        Ok(Some(statement.node()))
    }

    fn create_expression_from_entity_name(
        &mut self,
        name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        match self.context.arena().node(name)?.data.clone() {
            NodeData::Identifier(_) => Ok(name),
            NodeData::QualifiedName(data) => {
                let left = data
                    .left
                    .and_then(|id| self.context.arena().node_ref(self.source, id))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::QualifiedName,
                        field: "left",
                    })?;
                let right = data
                    .right
                    .and_then(|id| self.context.arena().node_ref(self.source, id))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::QualifiedName,
                        field: "right",
                    })?;
                let right = identifier_or_literal_text(self.context.arena(), right)?;
                let left = self.create_expression_from_entity_name(left)?;
                let access = self.create_property_access(left, &right)?;
                self.set_original_and_range(access, name)
            }
            _ => Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ImportEqualsDeclaration,
                field: "entity-name module_reference",
            }),
        }
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

    fn namespace_parameter_name(
        &mut self,
        declaration: NodeId,
        body: Option<NodeId>,
        base: &str,
    ) -> Result<String, TransformError> {
        let declaration_node = self.resolver_node(self.node(declaration))?;
        let unique = match self.resolver.is_unique_local_name(declaration_node, base) {
            Ok(unique) => unique,
            Err(EmitResolverError::Unavailable {
                method: EmitResolverMethod::IsUniqueLocalName,
                ..
            }) => match body {
                Some(body) => !self.module_body_reserves_value_name(body, base)?,
                None => true,
            },
            Err(error) => return Err(error.into()),
        };
        if unique {
            return Ok(base.to_owned());
        }
        let mut ordinal = 1usize;
        loop {
            let candidate = format!("{base}_{ordinal}");
            if !self.source_identifier_names.contains(&candidate)
                && self.generated_namespace_names.insert(candidate.clone())
            {
                return Ok(candidate);
            }
            ordinal += 1;
        }
    }

    /// Rust-side equivalent of the binder-backed `isUniqueLocalName` used by
    /// tsc's generated module name. Namespace substitution reaches through
    /// descendant containers, so a parameter or variable in an accessor also
    /// reserves the IIFE parameter name. Member property names do not create
    /// locals and therefore deliberately stay out of this walk.
    fn module_body_reserves_value_name(
        &self,
        body: NodeId,
        expected: &str,
    ) -> Result<bool, TransformError> {
        let syntax = self.context.arena().source(self.source)?.syntax();
        let mut pending = vec![body];
        let mut seen = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !seen.insert(id) {
                continue;
            }
            let record = self.context.arena().node(self.node(id))?;
            let binding_name = match &record.data {
                NodeData::VariableDeclaration(data) => data.name,
                NodeData::Parameter(data) => data.name,
                NodeData::BindingElement(data) => data.name,
                NodeData::FunctionDeclaration(data) => data.name,
                NodeData::FunctionExpression(data) => data.name,
                NodeData::ClassDeclaration(data) => data.name,
                NodeData::ClassExpression(data) => data.name,
                NodeData::EnumDeclaration(data) => data.name,
                NodeData::ModuleDeclaration(data) => data.name,
                NodeData::ImportClause(data) => data.name,
                NodeData::ImportEqualsDeclaration(data) => data.name,
                NodeData::ImportSpecifier(data) => data.name,
                NodeData::NamespaceImport(data) => data.name,
                _ => None,
            };
            if binding_name
                .map(|name| self.binding_name_contains(name, expected))
                .transpose()?
                .unwrap_or(false)
            {
                return Ok(true);
            }
            for_each_child(&syntax.arena, record, |child| {
                pending.push(child);
                false
            });
        }
        Ok(false)
    }

    fn binding_name_contains(&self, name: NodeId, expected: &str) -> Result<bool, TransformError> {
        let name = self.node(name);
        match self.context.arena().node(name)?.data.clone() {
            NodeData::Identifier(data) => Ok(data.text == expected),
            NodeData::BindingElement(data) => data
                .name
                .map(|name| self.binding_name_contains(name, expected))
                .unwrap_or(Ok(false)),
            NodeData::ObjectBindingPattern(data) => {
                self.binding_array_contains(data.elements, expected)
            }
            NodeData::ArrayBindingPattern(data) => {
                self.binding_array_contains(data.elements, expected)
            }
            _ => Ok(false),
        }
    }

    fn binding_array_contains(
        &self,
        elements: Option<NodeArrayId>,
        expected: &str,
    ) -> Result<bool, TransformError> {
        let Some(elements) = elements
            .and_then(|elements| self.context.arena().node_array_ref(self.source, elements))
        else {
            return Ok(false);
        };
        for element in &self.context.arena().node_array(elements)?.nodes {
            if self.binding_name_contains(*element, expected)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn resolver_node(&self, node: TransformNode) -> Result<EmitResolverNode, TransformError> {
        self.context.arena().require_parse_tree_resolver_node(node)
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

    /// tsc-port: getClassFacts @6.0.3
    /// tsc-hash: 96527ff84ba078ef8ea3d635bffc120f5ccf13c9441ff83640e65831989f4261
    /// tsc-span: _tsc.js:94410-94426
    fn typescript_class_facts(
        &self,
        members: Option<NodeArrayId>,
    ) -> Result<TypeScriptClassFacts, TransformError> {
        let mut facts = TypeScriptClassFacts::default();
        let Some(members) = members else {
            return Ok(facts);
        };
        for member in &self.context.arena().node_array(self.array(members))?.nodes {
            let Some(member) = self.context.arena().node_ref(self.source, *member) else {
                continue;
            };
            if let NodeData::PropertyDeclaration(data) = &self.context.arena().node(member)?.data {
                if data.initializer.is_some()
                    && self.has_modifier(data.modifiers, SyntaxKind::StaticKeyword)?
                {
                    facts.has_static_initialized_properties = true;
                }
            }
            if !facts.has_member_decorators
                && self.class_member_or_child_is_decorated(
                    member,
                    TypeScriptDecoratorMode::from_legacy(self.legacy_decorators),
                )?
            {
                facts.has_member_decorators = true;
            }
        }
        Ok(facts)
    }

    /// tsc-port: nodeCanBeDecorated/nodeIsDecorated/childIsDecorated @6.0.3
    /// tsc-hash: c4ea3855faefd80ff6734156bfadc82b2a3a8a9d106c3c585fe9c9b66beaff35
    /// tsc-span: _tsc.js:14651-14695
    fn class_member_or_child_is_decorated(
        &self,
        member: TransformNode,
        mode: TypeScriptDecoratorMode,
    ) -> Result<bool, TransformError> {
        if self.class_member_is_decorated(member, mode)? {
            return Ok(true);
        }
        if mode == TypeScriptDecoratorMode::Standard {
            return Ok(false);
        }
        let parameters = match &self.context.arena().node(member)?.data {
            NodeData::MethodDeclaration(data) if data.body.is_some() => data.parameters,
            NodeData::SetAccessor(data) if data.body.is_some() => data.parameters,
            NodeData::Constructor(data) if data.body.is_some() => data.parameters,
            _ => None,
        };
        self.legacy_parameter_list_is_decorated(parameters)
    }

    fn class_member_is_decorated(
        &self,
        member: TransformNode,
        mode: TypeScriptDecoratorMode,
    ) -> Result<bool, TransformError> {
        let record = self.context.arena().node(member)?;
        let (modifiers, name, eligible) = match &record.data {
            NodeData::PropertyDeclaration(data) => {
                let standard_ambient = mode == TypeScriptDecoratorMode::Standard
                    && (self.has_modifier(data.modifiers, SyntaxKind::DeclareKeyword)?
                        || self.has_modifier(data.modifiers, SyntaxKind::AbstractKeyword)?);
                (data.modifiers, data.name, !standard_ambient)
            }
            NodeData::MethodDeclaration(data) => (data.modifiers, data.name, data.body.is_some()),
            NodeData::GetAccessor(data) => (data.modifiers, data.name, data.body.is_some()),
            NodeData::SetAccessor(data) => (data.modifiers, data.name, data.body.is_some()),
            _ => (None, None, false),
        };
        if !eligible || !self.has_modifier(modifiers, SyntaxKind::Decorator)? {
            return Ok(false);
        }
        Ok(mode == TypeScriptDecoratorMode::Standard || !self.name_is_private(name)?)
    }

    fn legacy_parameter_list_is_decorated(
        &self,
        parameters: Option<NodeArrayId>,
    ) -> Result<bool, TransformError> {
        let Some(parameters) = parameters else {
            return Ok(false);
        };
        for (index, parameter) in self
            .context
            .arena()
            .node_array(self.array(parameters))?
            .nodes
            .iter()
            .enumerate()
        {
            let Some(parameter) = self.context.arena().node_ref(self.source, *parameter) else {
                continue;
            };
            let NodeData::Parameter(data) = &self.context.arena().node(parameter)?.data else {
                continue;
            };
            let is_this_parameter = index == 0
                && parameter_emit_role(self.context.arena(), self.source, data)?
                    == ParameterEmitRole::ExplicitThis;
            if !is_this_parameter
                && !self.name_is_private(data.name)?
                && self.has_modifier(data.modifiers, SyntaxKind::Decorator)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn name_is_private(&self, name: Option<NodeId>) -> Result<bool, TransformError> {
        let Some(name) = name else {
            return Ok(false);
        };
        Ok(self
            .context
            .arena()
            .node_ref(self.source, name)
            .and_then(|name| self.context.arena().node(name).ok())
            .is_some_and(|name| name.kind == SyntaxKind::PrivateIdentifier))
    }

    /// Once a class itself is admitted to `transformTypeScript`, tsc uses a
    /// closed class-element visitor rather than applying one uniform
    /// `ContainsTypeScript` gate to every immediate member. Keep that
    /// contextual route in a class-specific child traversal so the type
    /// system, rather than an ambient array registry, owns the distinction.
    ///
    /// This distinction is observable for parser-recovery syntax. An
    /// `accessor constructor` does not itself carry `ContainsTypeScript`, but
    /// its modifiers are erased when a TypeScript-bearing parent class is
    /// transformed. The same member remains untouched when its parent class
    /// is never admitted.
    ///
    /// tsc-port: transformTypeScript/getClassElementVisitor/classElementVisitorWorker @6.0.3
    /// tsc-hash: f5a88121af2c5fa8bf55dfef44ac548a1801ca8ea2a4b2227a33958193f15bc2
    /// tsc-span: _tsc.js:94215-94239
    fn update_class_declaration(
        &mut self,
        original: TransformNode,
        parent: NodeId,
        mut data: tsc_syntax::nodes::ClassDeclarationData,
    ) -> Result<NodeId, TransformError> {
        data.modifiers = self.visit_optional_child_array(data.modifiers)?;
        data.name = self.visit_optional_child(data.name)?;
        data.heritage_clauses = self.visit_optional_child_array(data.heritage_clauses)?;
        data.members = self.visit_optional_class_members(parent, data.members)?;
        let data = NodeData::ClassDeclaration(data);
        let flags = flags_after_update(self.context.arena(), original, &data)?;
        Ok(self
            .context
            .factory()?
            .update_node(original, data, flags)?
            .node())
    }

    fn update_class_expression(
        &mut self,
        original: TransformNode,
        parent: NodeId,
        mut data: tsc_syntax::nodes::ClassExpressionData,
    ) -> Result<NodeId, TransformError> {
        data.modifiers = self.visit_optional_child_array(data.modifiers)?;
        data.name = self.visit_optional_child(data.name)?;
        data.heritage_clauses = self.visit_optional_child_array(data.heritage_clauses)?;
        data.members = self.visit_optional_class_members(parent, data.members)?;
        let data = NodeData::ClassExpression(data);
        let flags = flags_after_update(self.context.arena(), original, &data)?;
        Ok(self
            .context
            .factory()?
            .update_node(original, data, flags)?
            .node())
    }

    fn visit_optional_child(
        &mut self,
        child: Option<NodeId>,
    ) -> Result<Option<NodeId>, TransformError> {
        child
            .map(|child| self.visit(child))
            .transpose()
            .map(Option::flatten)
    }

    fn visit_optional_child_array(
        &mut self,
        children: Option<NodeArrayId>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        children
            .map(|children| self.visit_nodes(children))
            .transpose()
            .map(Option::flatten)
    }

    fn visit_optional_class_members(
        &mut self,
        parent: NodeId,
        members: Option<NodeArrayId>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        members
            .map(|members| self.visit_class_members(parent, members))
            .transpose()
            .map(Option::flatten)
    }

    fn visit_class_members(
        &mut self,
        parent: NodeId,
        members: NodeArrayId,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let original = self.array(members);
        let parent_node = self.node(parent);
        if let Some(state) = self.class_member_arrays.get(&members).copied() {
            let existing_parent = state.parent();
            if existing_parent != parent {
                return Err(TransformError::ContextualNodeArrayOwnerConflict {
                    context: TYPESCRIPT_CLASS_MEMBERS_CONTEXT,
                    array: original,
                    existing_parent: self.node(existing_parent),
                    attempted_parent: parent_node,
                });
            }
            return match state {
                ClassMemberArrayVisit::Visiting { .. } => {
                    Err(TransformError::ReentrantContextualNodeArrayVisit {
                        context: TYPESCRIPT_CLASS_MEMBERS_CONTEXT,
                        array: original,
                        parent: parent_node,
                    })
                }
                ClassMemberArrayVisit::Visited { .. } => {
                    Err(TransformError::ContextualNodeArrayAlreadyVisited {
                        context: TYPESCRIPT_CLASS_MEMBERS_CONTEXT,
                        array: original,
                        parent: parent_node,
                    })
                }
            };
        }
        if self.arrays.contains_key(&members) {
            return Err(TransformError::ContextualNodeArrayAlreadyVisited {
                context: TYPESCRIPT_CLASS_MEMBERS_CONTEXT,
                array: original,
                parent: parent_node,
            });
        }
        self.class_member_arrays
            .insert(members, ClassMemberArrayVisit::Visiting { parent });
        let input = self.context.arena().node_array(original)?.nodes.clone();
        let mut visited = Vec::with_capacity(input.len());
        for member in input {
            if let Some(member) = self.visit_class_element(parent, member)? {
                visited.push(self.node(member));
            }
        }
        if self.project_parameter_properties_for_class_fields {
            visited = self.prepend_parameter_property_members(original, visited)?;
        }
        let updated = self
            .context
            .factory()?
            .update_node_array(original, visited)?;
        let mapped = Some(updated.array());
        self.arrays.insert(members, mapped);
        self.class_member_arrays
            .insert(members, ClassMemberArrayVisit::Visited { parent });
        Ok(mapped)
    }

    fn visit_class_element(
        &mut self,
        parent: NodeId,
        member: NodeId,
    ) -> Result<Option<NodeId>, TransformError> {
        let parent_kind = self.context.arena().node(self.node(parent))?.kind;
        let kind = self.context.arena().node(self.node(member))?.kind;
        match kind {
            SyntaxKind::Constructor | SyntaxKind::PropertyDeclaration => {
                self.visit_typescript(member)
            }
            SyntaxKind::GetAccessor | SyntaxKind::SetAccessor | SyntaxKind::MethodDeclaration => {
                self.visit(member)
            }
            SyntaxKind::ClassStaticBlockDeclaration => {
                self.visit_class_static_block_children(member)
            }
            SyntaxKind::SemicolonClassElement => {
                self.nodes.entry(member).or_insert(Some(member));
                Ok(Some(member))
            }
            SyntaxKind::IndexSignature => {
                self.nodes.insert(member, None);
                Ok(None)
            }
            actual => Err(TransformError::UnexpectedChildKind {
                parent: parent_kind,
                field: "members",
                actual,
            }),
        }
    }

    /// tsc's class-element dispatcher applies `visitEachChild` directly to a
    /// static block: the block node bypasses admission, while each child still
    /// uses the ordinary TypeScript visitor and its normal gate.
    fn visit_class_static_block_children(
        &mut self,
        id: NodeId,
    ) -> Result<Option<NodeId>, TransformError> {
        if let Some(mapped) = self.nodes.get(&id) {
            return Ok(*mapped);
        }
        let original = self
            .context
            .arena()
            .node_ref(self.source, id)
            .ok_or_else(|| TransformError::UnknownNode(self.node(id)))?;
        let record = self.context.arena().node(original)?.clone();
        let NodeData::ClassStaticBlockDeclaration(data) = record.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::ClassStaticBlockDeclaration,
                actual: record.kind,
            });
        };
        let transformed =
            Some(self.update_generic(original, NodeData::ClassStaticBlockDeclaration(data))?);
        self.nodes.insert(id, transformed);
        Ok(transformed)
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
        if let Some(state) = self.class_member_arrays.get(&id).copied() {
            return Err(TransformError::ContextualNodeArrayWrongVisitor {
                context: TYPESCRIPT_CLASS_MEMBERS_CONTEXT,
                array: self.array(id),
                parent: self.node(state.parent()),
            });
        }
        if let Some(mapped) = self.arrays.get(&id) {
            return Ok(*mapped);
        }
        let original = self.array(id);
        let nodes = self.context.arena().node_array(original)?.nodes.clone();
        // An unchanged empty NodeArray is itself observable: it owns the
        // trivia between delimiters. Retain its exact identity and boundary
        // positions instead of routing it through an unnecessary update.
        if nodes.is_empty() {
            self.arrays.insert(id, Some(id));
            return Ok(Some(id));
        }
        let mut visited = Vec::with_capacity(nodes.len());
        for node in nodes {
            match self.context.arena().node(self.node(node))?.kind {
                SyntaxKind::EnumDeclaration => {
                    visited.extend(self.visit_enum_declaration(node)?);
                }
                SyntaxKind::ModuleDeclaration => {
                    visited.extend(self.visit_module_declaration(node)?);
                }
                _ => {
                    if let Some(node) = self.visit(node)? {
                        visited.push(self.node(node));
                    }
                }
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
    _allow_ambient_module_erasure: bool,
    allow_jsx: bool,
    allow_legacy_decorators: bool,
    downlevels_es2018: bool,
) -> Result<(), TransformError> {
    let syntax = arena.source(source)?.syntax();
    if !syntax.parse_diagnostics.is_empty() {
        return Err(TransformError::ParseDiagnosticsDeferred {
            count: syntax.parse_diagnostics.len(),
            owner_slice: "H2.9",
        });
    }
    if has_advanced_comment_placement(syntax.text(), downlevels_es2018) {
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
        let feature = match node.kind {
            SyntaxKind::Decorator if !allow_legacy_decorators => {
                Some(UnsupportedTransformFeature::Decorators)
            }
            kind if is_jsx_kind(kind) && !allow_jsx => Some(UnsupportedTransformFeature::Jsx),
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

fn has_advanced_comment_placement(text: &str, downlevels_es2018: bool) -> bool {
    (!downlevels_es2018 && has_comment_after_ellipsis(text))
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
    matches!(
        kind,
        SyntaxKind::JsxText | SyntaxKind::JsxTextAllWhiteSpaces
    ) || (kind as u16 >= SyntaxKind::JsxElement as u16
        && kind as u16 <= SyntaxKind::JsxNamespacedName as u16)
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
    let recomputed = TransformFlags::CONTAINS_TYPE_SCRIPT
        | TransformFlags::CONTAINS_ES_2021
        | TransformFlags::CONTAINS_ES_2020
        | TransformFlags::CONTAINS_ES_2019
        | TransformFlags::CONTAINS_ES_2018
        | TransformFlags::CONTAINS_ES_2016
        | TransformFlags::CONTAINS_PRIVATE_IDENTIFIER_IN_EXPRESSION;
    let mut flags = old & !recomputed;
    flags |= local_transform_flags(&probe)
        | local_contextual_target_flags(arena, original.source(), &probe)?;
    flags |= factory_child_transform_flags(arena, original.source(), &probe)? & recomputed;
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

    let mut flags =
        local_transform_flags(&record) | local_contextual_target_flags(arena, source, &record)?;
    flags |= factory_child_transform_flags(arena, source, &record)?;
    arena.set_transform_flags(node, flags);
    visiting.remove(&id);
    complete.insert(id);
    Ok(flags)
}

/// Child fields that participate in a node factory's transform flags.
///
/// Most nodes propagate every public syntax child. Constructor and accessor
/// factories are deliberately narrower: the parser attaches a few invalid
/// signature fields only after creation so diagnostics and recovery printing
/// can observe them without admitting transformTypeScript. Encoding those
/// shapes explicitly keeps the flag gate aligned with the factory API rather
/// than guessing from a fully populated recovery tree.
#[derive(Clone, Copy, Debug)]
enum FactoryTransformChildren<'a> {
    All,
    None,
    Constructor(&'a tsc_syntax::nodes::ConstructorData),
    GetAccessor(&'a tsc_syntax::nodes::GetAccessorData),
    SetAccessor(&'a tsc_syntax::nodes::SetAccessorData),
}

/// Runtime ownership of a parsed parameter declaration.
///
/// `this` is represented by tsc as an Identifier in a Parameter name slot,
/// even though the scanner token is `ThisKeyword`. Projecting that structural
/// AST shape into a typed role keeps both transform-flag admission and
/// parameter erasure on one rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ParameterEmitRole {
    ExplicitThis,
    Runtime,
}

/// tsc-port: identifierToKeywordKind @6.0.3
/// tsc-hash: 5b8e8a44db1923acc5abf70a8b0ae71e5599660f5dc3b14ef8f670b87eb52566
/// tsc-span: _tsc.js:11448-11451
/// tsc-port: parameterIsThisKeyword/isThisIdentifier @6.0.3
/// tsc-hash: 4a18704aa76199d221e5cf3256c3646b079a79c2d95f6efe82c870aad7ac9f7a
/// tsc-span: _tsc.js:16695-16700
fn parameter_emit_role(
    arena: &TransformArena,
    source: TransformSourceId,
    parameter: &tsc_syntax::nodes::ParameterData,
) -> Result<ParameterEmitRole, TransformError> {
    let Some(name) = parameter.name else {
        return Ok(ParameterEmitRole::Runtime);
    };
    let name = arena
        .node_ref(source, name)
        .ok_or_else(|| TransformError::UnknownNode(TransformNode::new(source, name)))?;
    let name = arena.node(name)?;
    let is_this = name.kind == SyntaxKind::ThisKeyword
        || matches!(
            &name.data,
            NodeData::Identifier(identifier)
                if identifier_to_keyword_kind(&identifier.escaped_text)
                    == Some(SyntaxKind::ThisKeyword)
        );
    Ok(if is_this {
        ParameterEmitRole::ExplicitThis
    } else {
        ParameterEmitRole::Runtime
    })
}

impl<'a> FactoryTransformChildren<'a> {
    fn of(node: &'a Node) -> Self {
        if !propagates_transform_child_flags(node.kind) {
            return Self::None;
        }
        match &node.data {
            NodeData::Constructor(data) => Self::Constructor(data),
            NodeData::GetAccessor(data) => Self::GetAccessor(data),
            NodeData::SetAccessor(data) => Self::SetAccessor(data),
            _ => Self::All,
        }
    }
}

/// tsc-port: createConstructorDeclaration/createGetAccessorDeclaration/createSetAccessorDeclaration @6.0.3 (transformFlags)
/// tsc-hash: 9fbde42cfc09830f4a0c63b6d9424f350f0f5a3b5938aa838cce11657c801e09
/// tsc-span: _tsc.js:21982-22073
fn factory_child_transform_flags(
    arena: &TransformArena,
    source: TransformSourceId,
    node: &Node,
) -> Result<TransformFlags, TransformError> {
    let array_flags = |array: Option<NodeArrayId>| {
        array
            .and_then(|array| arena.node_array_ref(source, array))
            .map(|array| arena.array_transform_flags(array))
            .unwrap_or(TransformFlags::NONE)
    };
    let child_flags = |child: Option<NodeId>| -> Result<TransformFlags, TransformError> {
        let Some(child) = child else {
            return Ok(TransformFlags::NONE);
        };
        let child = arena
            .node_ref(source, child)
            .ok_or_else(|| TransformError::UnknownNode(TransformNode::new(source, child)))?;
        arena.propagate_child_flags(child)
    };
    let body_flags = |body: Option<NodeId>| {
        child_flags(body).map(|flags| flags & !TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT)
    };
    let name_flags = |name: Option<NodeId>| -> Result<TransformFlags, TransformError> {
        let flags = child_flags(name)?;
        let is_identifier = name
            .and_then(|name| arena.node_ref(source, name))
            .is_some_and(|name| {
                arena
                    .node(name)
                    .is_ok_and(|node| node.kind == SyntaxKind::Identifier)
            });
        Ok(if is_identifier {
            flags & !TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT
        } else {
            flags
        })
    };

    match FactoryTransformChildren::of(node) {
        FactoryTransformChildren::None => Ok(TransformFlags::NONE),
        FactoryTransformChildren::All => {
            let syntax = arena.source(source)?.syntax();
            let mut children = Vec::new();
            for_each_child(&syntax.arena, node, |child| {
                children.push(child);
                false
            });
            let mut flags = TransformFlags::NONE;
            for child in children {
                flags |= child_flags(Some(child))?;
            }
            Ok(flags)
        }
        FactoryTransformChildren::Constructor(data) => {
            if data.body.is_none() {
                return Ok(TransformFlags::NONE);
            }
            Ok(array_flags(data.modifiers) | array_flags(data.parameters) | body_flags(data.body)?)
        }
        FactoryTransformChildren::GetAccessor(data) => {
            if data.body.is_none() {
                return Ok(TransformFlags::NONE);
            }
            Ok(array_flags(data.modifiers)
                | name_flags(data.name)?
                | array_flags(data.parameters)
                | child_flags(data.r#type)?
                | body_flags(data.body)?)
        }
        FactoryTransformChildren::SetAccessor(data) => {
            if data.body.is_none() {
                return Ok(TransformFlags::NONE);
            }
            Ok(array_flags(data.modifiers)
                | name_flags(data.name)?
                | array_flags(data.parameters)
                | body_flags(data.body)?)
        }
    }
}

fn local_transform_flags(node: &Node) -> TransformFlags {
    let kind = node.kind;
    let mut flags = TransformFlags::NONE;
    if NodeFlags::from_bits(node.flags).contains(NodeFlags::OPTIONAL_CHAIN)
        && matches!(
            kind,
            SyntaxKind::PropertyAccessExpression
                | SyntaxKind::ElementAccessExpression
                | SyntaxKind::CallExpression
                | SyntaxKind::NonNullExpression
        )
    {
        flags |= TransformFlags::CONTAINS_ES_2020;
    }
    if is_jsx_kind(kind) {
        flags |= TransformFlags::CONTAINS_JSX;
    }
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
        )
    {
        flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
    }
    match &node.data {
        NodeData::Token => match kind {
            SyntaxKind::AsyncKeyword => {
                flags |= TransformFlags::CONTAINS_ES_2017;
                flags |= TransformFlags::CONTAINS_ES_2018;
            }
            SyntaxKind::PublicKeyword
            | SyntaxKind::PrivateKeyword
            | SyntaxKind::ProtectedKeyword
            | SyntaxKind::AbstractKeyword
            | SyntaxKind::DeclareKeyword
            | SyntaxKind::ConstKeyword
            | SyntaxKind::AnyKeyword
            | SyntaxKind::NumberKeyword
            | SyntaxKind::BigIntKeyword
            | SyntaxKind::NeverKeyword
            | SyntaxKind::ObjectKeyword
            | SyntaxKind::InKeyword
            | SyntaxKind::OutKeyword
            | SyntaxKind::OverrideKeyword
            | SyntaxKind::StringKeyword
            | SyntaxKind::BooleanKeyword
            | SyntaxKind::SymbolKeyword
            | SyntaxKind::VoidKeyword
            | SyntaxKind::UnknownKeyword
            | SyntaxKind::UndefinedKeyword
            | SyntaxKind::ReadonlyKeyword => {
                flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
            }
            SyntaxKind::ThisKeyword => flags |= TransformFlags::CONTAINS_LEXICAL_THIS,
            SyntaxKind::SuperKeyword => {
                flags |= TransformFlags::CONTAINS_ES_2015;
                flags |= TransformFlags::CONTAINS_LEXICAL_SUPER;
            }
            // createToken StaticKeyword row (_tsc.js:21751-21753) — the
            // ES2015 class-member modifier facet (B-4 completion; zero
            // active readers of CONTAINS_ES_2015, ratchet-enforced).
            SyntaxKind::StaticKeyword => flags |= TransformFlags::CONTAINS_ES_2015,
            _ => {}
        },
        // --- B-4 parsed-tree ES2015 facet completion (packet §12.4).
        // Each arm mirrors its vendored factory row; corpus-inert: zero
        // active readers of CONTAINS_ES_2015 (the sole consult site is the
        // dormant ES2015 gate) and the full-corpus ratchet enforces byte
        // identity.
        NodeData::NumericLiteral(_) => {
            // createNumericLiteral BinaryOrOctalSpecifier row
            // (_tsc.js:21508-21514) over the parse record's
            // numeric_literal_flags word (the scanner's TokenFlags
            // carrier; 384 = BinaryOrOctalSpecifier).
            if node.numeric_literal_flags & 384 != 0 {
                flags |= TransformFlags::CONTAINS_ES_2015;
            }
        }
        NodeData::StringLiteral(data) => {
            // createStringLiteral hasExtendedUnicodeEscape row
            // (_tsc.js:21529-21534).
            if data.has_extended_unicode_escape == Some(true) {
                flags |= TransformFlags::CONTAINS_ES_2015;
            }
        }
        NodeData::TemplateExpression(_) => {
            // createTemplateExpression row (_tsc.js:22833-22837).
            flags |= TransformFlags::CONTAINS_ES_2015;
        }
        NodeData::NoSubstitutionTemplateLiteral(_)
        | NodeData::TemplateHead(_)
        | NodeData::TemplateMiddle(_)
        | NodeData::TemplateTail(_) => {
            // getTransformFlagsOfTemplateLiteralLike (_tsc.js:22862-22868);
            // the parse records carry no templateFlags word, so the
            // invalid-escape ES2018 half is unrepresentable here — it is
            // consulted only by the B-5 tagged-template module through
            // rawText.
            flags |= TransformFlags::CONTAINS_ES_2015;
        }
        NodeData::ComputedPropertyName(_) => {
            // createComputedPropertyName row (_tsc.js:21815-21819).
            flags |= TransformFlags::CONTAINS_ES_2015;
            flags |= TransformFlags::CONTAINS_COMPUTED_PROPERTY_NAME;
        }
        NodeData::ShorthandPropertyAssignment(_) => {
            // createShorthandPropertyAssignment row (_tsc.js:24160-24166).
            flags |= TransformFlags::CONTAINS_ES_2015;
        }
        NodeData::MetaProperty(data) => {
            // createMetaProperty keyword rows (_tsc.js:23009-23026).
            match data.keyword_token {
                SyntaxKind::NewKeyword => flags |= TransformFlags::CONTAINS_ES_2015,
                SyntaxKind::ImportKeyword => flags |= TransformFlags::CONTAINS_ES_2020,
                _ => {}
            }
        }
        NodeData::Parameter(data) => {
            if data.question_token.is_some() || data.r#type.is_some() {
                flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
            }
            if data.dot_dot_dot_token.is_some() || data.initializer.is_some() {
                flags |= TransformFlags::CONTAINS_ES_2015;
            }
            if data.dot_dot_dot_token.is_some() {
                flags |= TransformFlags::CONTAINS_REST_OR_SPREAD;
            }
        }
        NodeData::PropertyDeclaration(data) => {
            flags |= TransformFlags::CONTAINS_CLASS_FIELDS;
            if NodeFlags::from_bits(node.flags).contains(NodeFlags::AMBIENT)
                || data.question_token.is_some()
                || data.exclamation_token.is_some()
                || data.r#type.is_some()
            {
                flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
            }
        }
        NodeData::MethodDeclaration(data) => {
            flags |= TransformFlags::CONTAINS_ES_2015;
            if data.body.is_none()
                || data.question_token.is_some()
                || data.type_parameters.is_some()
                || data.r#type.is_some()
            {
                flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
            }
        }
        NodeData::Constructor(data) => {
            // createConstructorDeclaration owns neither `typeParameters` nor
            // `type`; the parser attaches both later for error recovery
            // without recomputing transform flags. A bodyless constructor is
            // TypeScript-only, while a runtime constructor contributes ES2015.
            if data.body.is_none() {
                flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
            } else {
                flags |= TransformFlags::CONTAINS_ES_2015;
            }
        }
        NodeData::GetAccessor(data) => {
            // Getter return type is factory-owned TypeScript syntax. Generic
            // type parameters are parser recovery and intentionally do not
            // affect this gate.
            if data.body.is_none() || data.r#type.is_some() {
                flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
            }
        }
        NodeData::SetAccessor(data) => {
            // Setter type parameters and return type are both attached only
            // after factory creation, so neither participates in transform
            // flag aggregation.
            if data.body.is_none() {
                flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
            }
        }
        NodeData::FunctionDeclaration(data) => {
            flags |= TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION;
            if data.body.is_none()
                || NodeFlags::from_bits(node.flags).contains(NodeFlags::AMBIENT)
                || data.type_parameters.is_some()
                || data.r#type.is_some()
            {
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
            if NodeFlags::from_bits(node.flags).contains(NodeFlags::AMBIENT)
                || data.type_parameters.is_some()
            {
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
            if matches!(
                node_flags & NodeFlags::BLOCK_SCOPED,
                NodeFlags::USING | NodeFlags::AWAIT_USING
            ) {
                flags |= TransformFlags::CONTAINS_ES_NEXT;
            }
            if node_flags.intersects(NodeFlags::BLOCK_SCOPED) {
                flags |= TransformFlags::CONTAINS_ES_2015;
                flags |= TransformFlags::CONTAINS_BLOCK_SCOPED_BINDING;
            }
        }
        NodeData::ReturnStatement(_) => {
            flags |= TransformFlags::CONTAINS_ES_2018;
            flags |= TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION;
        }
        // `createContinueStatement`/`createBreakStatement`
        // (`_tsc.js:23177`/`:23188`): completion statements carry the
        // hoisted-declaration-or-completion facet so completion-routing
        // passes (the Generators machine) descend to them.
        NodeData::ContinueStatement(_) | NodeData::BreakStatement(_) => {
            flags |= TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION;
        }
        NodeData::AwaitExpression(_) => {
            flags |= TransformFlags::CONTAINS_ES_2017;
            flags |= TransformFlags::CONTAINS_ES_2018;
            flags |= TransformFlags::CONTAINS_AWAIT;
        }
        NodeData::YieldExpression(_) => {
            flags |= TransformFlags::CONTAINS_ES_2015;
            flags |= TransformFlags::CONTAINS_ES_2018;
            flags |= TransformFlags::CONTAINS_YIELD;
        }
        NodeData::BindingElement(data) => {
            flags |= TransformFlags::CONTAINS_ES_2015;
            if data.dot_dot_dot_token.is_some() {
                flags |= TransformFlags::CONTAINS_REST_OR_SPREAD;
            }
        }
        NodeData::ObjectBindingPattern(_) | NodeData::ArrayBindingPattern(_) => {
            flags |= TransformFlags::CONTAINS_ES_2015;
            flags |= TransformFlags::CONTAINS_BINDING_PATTERN;
        }
        NodeData::SpreadAssignment(_) => {
            flags |= TransformFlags::CONTAINS_ES_2018;
            flags |= TransformFlags::CONTAINS_OBJECT_REST_OR_SPREAD;
        }
        NodeData::SpreadElement(_) => {
            flags |= TransformFlags::CONTAINS_ES_2015;
            flags |= TransformFlags::CONTAINS_REST_OR_SPREAD;
        }
        NodeData::ForOfStatement(data) => {
            flags |= TransformFlags::CONTAINS_ES_2015;
            if data.await_modifier.is_some() {
                flags |= TransformFlags::CONTAINS_ES_2018;
            }
        }
        NodeData::CatchClause(data) => {
            if data.variable_declaration.is_none() {
                flags |= TransformFlags::CONTAINS_ES_2019;
            }
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
            // createTaggedTemplateExpression row (_tsc.js:22635-22646):
            // ES2015 always (the invalid-escape ES2018 half rides
            // templateFlags, unrepresentable on parse records — B-5's
            // tagged-template module reads rawText for it).
            flags |= TransformFlags::CONTAINS_ES_2015;
            if data.type_arguments.is_some() {
                flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
            }
        }
        NodeData::JsxSelfClosingElement(data) => {
            // NodeFactory.createJsxSelfClosingElement marks the node itself,
            // independently of child type flags. This is required for empty
            // and JSDoc-recovery type-argument arrays as well as ordinary
            // TypeNodes.
            //
            // tsc-port: createJsxSelfClosingElement @6.0.3
            // tsc-span: _tsc.js:23970-23980
            if data.type_arguments.is_some() {
                flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
            }
        }
        NodeData::JsxOpeningElement(data) => {
            // tsc-port: createJsxOpeningElement @6.0.3
            // tsc-span: _tsc.js:23984-23994
            if data.type_arguments.is_some() {
                flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
            }
        }
        NodeData::ExpressionWithTypeArguments(_) => {
            // Unlike calls/new/tagged templates, tsc does not mark this node
            // locally as TypeScript. Valid type arguments propagate the bit;
            // JSDoc recovery types intentionally do not.
            flags |= TransformFlags::CONTAINS_ES_2015;
        }
        NodeData::EnumDeclaration(_) | NodeData::ModuleDeclaration(_) => {
            flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
        }
        NodeData::HeritageClause(data) if data.token == SyntaxKind::ImplementsKeyword => {
            flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
        }
        NodeData::Decorator(_) => {
            flags |= TransformFlags::CONTAINS_TYPE_SCRIPT
                | TransformFlags::CONTAINS_TYPE_SCRIPT_CLASS_SYNTAX
                | TransformFlags::CONTAINS_DECORATORS;
        }
        NodeData::PartiallyEmittedExpression(_) => {
            flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
        }
        _ => {}
    }
    flags
}

/// JSDoc unary types are parser-recovery syntax as well as comment syntax.
/// Their NodeFactory constructors do not propagate child transform flags, so
/// a nested `string` keyword must not make `foo<string?>` look like ordinary
/// TypeScript syntax to `transformTypeScript`.
const fn propagates_transform_child_flags(kind: SyntaxKind) -> bool {
    !matches!(
        kind,
        SyntaxKind::JSDocNullableType
            | SyntaxKind::JSDocNonNullableType
            | SyntaxKind::JSDocOptionalType
            | SyntaxKind::JSDocVariadicType
            | SyntaxKind::JSDocNamepathType
    )
}

fn local_contextual_target_flags(
    arena: &TransformArena,
    source: TransformSourceId,
    node: &Node,
) -> Result<TransformFlags, TransformError> {
    match &node.data {
        NodeData::CallExpression(data) => {
            // createBaseCallExpression super-property row
            // (_tsc.js:22574-22576): a call whose callee is a super
            // property carries ContainsLexicalThis (B-4 completion;
            // zero active readers of the bit — every other hit is a
            // write site — and the corpus ratchet enforces).
            let is_super_property_callee = data.expression.is_some_and(|callee| {
                let Some(callee) = arena.node_ref(source, callee) else {
                    return false;
                };
                let Ok(record) = arena.node(callee) else {
                    return false;
                };
                let receiver = match &record.data {
                    NodeData::PropertyAccessExpression(access) => access.expression,
                    NodeData::ElementAccessExpression(access) => access.expression,
                    _ => None,
                };
                receiver
                    .and_then(|receiver| arena.node_ref(source, receiver))
                    .and_then(|receiver| arena.node(receiver).ok())
                    .is_some_and(|receiver| receiver.kind == SyntaxKind::SuperKeyword)
            });
            if is_super_property_callee {
                return Ok(TransformFlags::CONTAINS_LEXICAL_THIS);
            }
            Ok(TransformFlags::NONE)
        }
        NodeData::Identifier(_) => {
            // createIdentifier extended-unicode row (_tsc.js:21621-21623;
            // NodeFlags 256). The parse records keep the COOKED text and
            // stamp no flag, so the facet derives from the token's SOURCE
            // spelling (the printer's own spelling channel): an `\u{`
            // escape in the identifier slice is exactly the scanner's
            // hasExtendedUnicodeEscape carrier.
            let syntax = arena.source(source)?.syntax();
            let text = syntax.text();
            let start = node.pos as usize;
            let end = (node.end as usize).min(text.len());
            if start < end && text[start..end].contains("\\u{") {
                return Ok(TransformFlags::CONTAINS_ES_2015);
            }
            Ok(TransformFlags::NONE)
        }
        NodeData::Parameter(data) => Ok(
            if parameter_emit_role(arena, source, data)? == ParameterEmitRole::ExplicitThis {
                // createParameterDeclaration marks an explicit `this`
                // parameter as TypeScript syntax even without an annotation.
                // The bit must reach the containing parameter list so
                // transformTypeScript visits and removes the declaration.
                TransformFlags::CONTAINS_TYPE_SCRIPT
            } else {
                TransformFlags::NONE
            },
        ),
        NodeData::ImportEqualsDeclaration(data) => {
            let Some(module_reference) = data.module_reference else {
                return Ok(TransformFlags::CONTAINS_TYPE_SCRIPT);
            };
            let module_reference = arena.node_ref(source, module_reference).ok_or_else(|| {
                TransformError::UnknownNode(TransformNode::new(source, module_reference))
            })?;
            Ok(
                if arena.node(module_reference)?.kind == SyntaxKind::ExternalModuleReference {
                    // `import x = require("x")` is JavaScript-module syntax
                    // at a source-element boundary. tsc leaves it unmarked so
                    // invalid nested placements survive recovery emit; the
                    // source-file and namespace visitors still own their
                    // explicit elision rules.
                    TransformFlags::NONE
                } else {
                    TransformFlags::CONTAINS_TYPE_SCRIPT
                },
            )
        }
        NodeData::BinaryExpression(data) => {
            let Some(operator) = data.operator_token else {
                return Ok(TransformFlags::NONE);
            };
            let operator = arena
                .node_ref(source, operator)
                .ok_or_else(|| TransformError::UnknownNode(TransformNode::new(source, operator)))?;
            let operator = arena.node(operator)?.kind;
            let mut flags = match operator {
                SyntaxKind::BarBarEqualsToken
                | SyntaxKind::AmpersandAmpersandEqualsToken
                | SyntaxKind::QuestionQuestionEqualsToken => TransformFlags::CONTAINS_ES_2021,
                SyntaxKind::QuestionQuestionToken => TransformFlags::CONTAINS_ES_2020,
                SyntaxKind::AsteriskAsteriskToken | SyntaxKind::AsteriskAsteriskEqualsToken => {
                    TransformFlags::CONTAINS_ES_2016
                }
                _ => TransformFlags::NONE,
            };
            if operator == SyntaxKind::EqualsToken {
                if let Some(left) = data.left.and_then(|left| arena.node_ref(source, left)) {
                    // createBinaryExpression destructuring rows
                    // (_tsc.js:22785-22812): `=` over an object-literal
                    // target is ES2015|ES2018|DestructuringAssignment;
                    // over an array-literal target ES2015|
                    // DestructuringAssignment; both propagate the
                    // assignment-pattern ObjectRestOrSpread facet (the
                    // ES2015 bits are the B-4 parsed-tree completion —
                    // zero active readers, ratchet-enforced).
                    if arena.node(left)?.kind == SyntaxKind::ObjectLiteralExpression {
                        flags |= TransformFlags::CONTAINS_ES_2015;
                        flags |= TransformFlags::CONTAINS_ES_2018;
                        flags |= TransformFlags::CONTAINS_DESTRUCTURING_ASSIGNMENT;
                        if arena
                            .transform_flags(left)
                            .contains(TransformFlags::CONTAINS_OBJECT_REST_OR_SPREAD)
                        {
                            flags |= TransformFlags::CONTAINS_OBJECT_REST_OR_SPREAD;
                        }
                    } else if arena.node(left)?.kind == SyntaxKind::ArrayLiteralExpression {
                        flags |= TransformFlags::CONTAINS_ES_2015;
                        flags |= TransformFlags::CONTAINS_DESTRUCTURING_ASSIGNMENT;
                        if arena
                            .transform_flags(left)
                            .contains(TransformFlags::CONTAINS_OBJECT_REST_OR_SPREAD)
                        {
                            flags |= TransformFlags::CONTAINS_OBJECT_REST_OR_SPREAD;
                        }
                    }
                }
            }
            flags |= private_identifier_expression_flags(arena, source, &node.data)?;
            Ok(flags)
        }
        NodeData::ObjectBindingPattern(data) => {
            let contains_rest = data
                .elements
                .and_then(|elements| arena.node_array_ref(source, elements))
                .is_some_and(|elements| {
                    arena.node_array(elements).is_ok_and(|elements| {
                        elements.nodes.iter().any(|element| {
                            arena
                                .node_ref(source, *element)
                                .and_then(|element| arena.node(element).ok())
                                .is_some_and(|element| {
                                    matches!(
                                        &element.data,
                                        NodeData::BindingElement(element)
                                            if element.dot_dot_dot_token.is_some()
                                    )
                                })
                        })
                    })
                });
            Ok(if contains_rest {
                TransformFlags::CONTAINS_ES_2018 | TransformFlags::CONTAINS_OBJECT_REST_OR_SPREAD
            } else {
                TransformFlags::NONE
            })
        }
        NodeData::PropertyAccessExpression(data) => {
            Ok(super_access_target_flags(arena, source, data.expression)?
                | private_identifier_expression_flags(arena, source, &node.data)?)
        }
        NodeData::ElementAccessExpression(data) => {
            Ok(super_access_target_flags(arena, source, data.expression)?
                | private_identifier_expression_flags(arena, source, &node.data)?)
        }
        // The factory's per-function facet conditional
        // (`createFunctionExpression`/`createFunctionDeclaration`/
        // `createMethodDeclaration`, `_tsc.js:22685-22688`; ported for
        // synthesized nodes as `function_facets` in
        // `TransformArena::propagate_child_flags`): async generators mark
        // ES2018, async functions ES2017, plain generators
        // `ContainsGenerator`. The async halves are bit-idempotent with
        // the AsyncKeyword modifier token's own facets; the generator
        // facet is what the parsed tree was missing
        // (`docs/design/greenfield/slices/h2-5h-b-b-3.md` §12.2).
        NodeData::FunctionDeclaration(data) => Ok(function_like_facet_flags(
            arena,
            source,
            data.asterisk_token,
            data.modifiers,
        )?),
        NodeData::FunctionExpression(data) => Ok(function_like_facet_flags(
            arena,
            source,
            data.asterisk_token,
            data.modifiers,
        )?),
        NodeData::MethodDeclaration(data) => Ok(function_like_facet_flags(
            arena,
            source,
            data.asterisk_token,
            data.modifiers,
        )?),
        _ => Ok(TransformFlags::NONE),
    }
}

fn function_like_facet_flags(
    arena: &TransformArena,
    source: TransformSourceId,
    asterisk_token: Option<tsc_syntax::NodeId>,
    modifiers: Option<tsc_syntax::NodeArrayId>,
) -> Result<TransformFlags, TransformError> {
    let is_generator = asterisk_token.is_some();
    let mut is_async = false;
    if let Some(modifiers) = modifiers.and_then(|array| arena.node_array_ref(source, array)) {
        for modifier in &arena.node_array(modifiers)?.nodes {
            if let Some(modifier) = arena.node_ref(source, *modifier) {
                if arena.node(modifier)?.kind == SyntaxKind::AsyncKeyword {
                    is_async = true;
                    break;
                }
            }
        }
    }
    Ok(if is_async && is_generator {
        TransformFlags::CONTAINS_ES_2018
    } else if is_async {
        TransformFlags::CONTAINS_ES_2017
    } else if is_generator {
        TransformFlags::CONTAINS_GENERATOR
    } else {
        TransformFlags::NONE
    })
}

fn super_access_target_flags(
    arena: &TransformArena,
    source: TransformSourceId,
    expression: Option<NodeId>,
) -> Result<TransformFlags, TransformError> {
    let Some(expression) = expression.and_then(|expression| arena.node_ref(source, expression))
    else {
        return Ok(TransformFlags::NONE);
    };
    Ok(
        if arena.node(expression)?.kind == SyntaxKind::SuperKeyword {
            TransformFlags::CONTAINS_ES_2017 | TransformFlags::CONTAINS_ES_2018
        } else {
            TransformFlags::NONE
        },
    )
}

#[cfg(test)]
#[path = "../tests/unit/builtins/tests.rs"]
mod tests;

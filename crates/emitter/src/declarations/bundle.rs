//! Declaration Bundle visitor. Shared references belong to the output bundle;
//! visibility, scope markers, and late-painted declarations belong to a source.
use std::path::Path;

use tsc_syntax::{NodeData, SyntaxKind};
use tsc_types::NodeFlags;

use crate::{TransformBundle, TransformError, TransformationContext};

use super::root::{
    is_javascript_source, normalize_slashes, source_statement_array, source_statements,
    transform_declarations_for_js,
};
use super::state::{RawFileReferences, TransformState, VisitResult};
use super::DeclarationTransformer;

/// tsc-port: transformRoot bundle arm @6.0.3 (_tsc.js:114446-114513).
pub(super) fn transform_bundle(
    transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    bundle: TransformBundle,
) -> Result<TransformBundle, TransformError> {
    let declaration_path = transformer
        .paths
        .bundle_declaration_file_path()
        .ok_or_else(|| {
            DeclarationTransformer::contract("bundle declaration output path is required")
        })?;
    let declaration_path = normalize_slashes(declaration_path);
    let output_directory = declaration_path.parent().unwrap_or_else(|| Path::new(""));
    let mut raw_references = RawFileReferences::default();
    let mut sources = Vec::with_capacity(bundle.sources().len());
    let mut last_source = None;
    for &source in bundle.sources() {
        let syntax = context.arena().source(source)?.syntax().clone();
        if syntax.is_declaration_file {
            continue;
        }
        let root = context.arena().root(source)?;
        let program_source = context
            .arena()
            .source(source)?
            .program_source()
            .ok_or(TransformError::MissingProgramSource(root))?;
        let is_javascript = is_javascript_source(&syntax, context.arena().node(root)?.flags);
        let is_json = syntax.file_name.to_ascii_lowercase().ends_with(".json");
        let resolver_node = transformer.required_resolver_node(context, root)?;
        let wrapped = transformer
            .resolver
            .is_external_or_common_js_module(resolver_node)?
            || is_json;

        transformer.state = Some(TransformState::for_source(source, root));
        transformer.state_mut()?.is_bundled_emit = true;
        transformer.state_mut()?.needs_declare = !wrapped;
        transformer
            .tracker
            .reset_for_file(Some(program_source), source, is_javascript);
        let references = RawFileReferences::collect(context.arena(), source)?;
        raw_references.referenced.extend(references.referenced);
        raw_references
            .type_directives
            .extend(references.type_directives);
        raw_references
            .lib_directives
            .extend(references.lib_directives);

        let mut statements = if is_javascript {
            transform_declarations_for_js(transformer, context, source, root)?
        } else {
            let mut statements = Vec::new();
            for statement in source_statements(context.arena(), root)? {
                match super::statements::visit_declaration_statement(
                    transformer,
                    context,
                    statement,
                )? {
                    VisitResult::None => {}
                    VisitResult::Node(statement) => statements.push(statement),
                    VisitResult::Nodes(result) => statements.extend(result),
                }
            }
            statements
        };
        statements = super::statements::transform_and_replace_late_painted_statements(
            transformer,
            context,
            statements,
        )?;
        let original_range = source_statement_array(context.arena(), root)?
            .map(|array| {
                context
                    .arena()
                    .node_array(array)
                    .map(|array| (array.pos, array.end))
            })
            .transpose()?;
        let module_name = wrapped.then(|| {
            crate::external_module_names::get_resolved_external_module_name(
                transformer.host,
                &syntax,
                None,
            )
        });
        let updated = {
            let mut factory = context.factory()?;
            let mut statements = factory.create_node_array(source, statements)?;
            if wrapped || !is_javascript {
                if let Some((pos, end)) = original_range {
                    factory.set_node_array_text_range(statements, pos, end)?;
                }
            }
            if let Some(module_name) = module_name {
                let block = factory.create_module_block(source, statements)?;
                let name = factory.create_string_literal(source, module_name, false)?;
                let declare = factory.create_modifier(source, SyntaxKind::DeclareKeyword)?;
                let modifiers = factory.create_node_array(source, vec![declare])?;
                let module = factory.create_module_declaration(
                    source,
                    Some(modifiers),
                    name,
                    Some(block),
                    NodeFlags::NONE,
                )?;
                statements = factory.create_node_array(source, vec![module])?;
            }
            factory.update_source_file(
                root,
                statements,
                true,
                Vec::new(),
                Vec::new(),
                false,
                Vec::new(),
            )?
        };
        context.arena_mut()?.replace_root(source, updated)?;
        sources.push(source);
        last_source = Some((source, program_source));
    }
    let type_references = raw_references.type_references();
    let lib_references = raw_references.lib_references();
    let file_references = if let Some((source, program_source)) = last_source {
        transformer.state_mut()?.references = raw_references;
        super::root::referenced_files(
            transformer,
            context.arena(),
            source,
            output_directory,
            program_source,
            Some(bundle.sources()),
        )?
    } else {
        Vec::new()
    };
    Ok(TransformBundle::new(sources).with_synthetic_references(
        file_references,
        type_references,
        lib_references,
    ))
}

/// Declaration-import policy of getExternalModuleNameFromDeclaration
/// (_tsc.js:16541-16551), distinct from JavaScript dependency names.
pub(super) fn external_module_name_from_declaration(
    transformer: &DeclarationTransformer<'_>,
    context: &TransformationContext,
    declaration: crate::TransformNode,
    specifier: &str,
) -> Result<Option<String>, TransformError> {
    let declaration = transformer.required_resolver_node(context, declaration)?;
    let Some(target) = transformer
        .resolver
        .get_external_module_file_from_declaration(declaration)?
    else {
        return Ok(None);
    };
    let file = transformer.host.source_file(target.source()).ok_or(
        crate::EmitResolverError::UnknownSource {
            method: crate::EmitResolverMethod::GetExternalModuleFileFromDeclaration,
            node: target,
        },
    )?;
    let syntax = file
        .syntax()
        .ok_or(crate::EmitResolverError::UnknownSource {
            method: crate::EmitResolverMethod::GetExternalModuleFileFromDeclaration,
            node: target,
        })?;
    if syntax.is_declaration_file {
        return Ok(None);
    }
    let relative = specifier == "."
        || specifier == ".."
        || specifier.starts_with("./")
        || specifier.starts_with("../")
        || specifier.starts_with(".\\")
        || specifier.starts_with("..\\");
    if !relative {
        use crate::source_map::paths;
        let cwd = paths::normalize_slashes(&transformer.host.current_directory().to_string_lossy());
        let canonical = |path: &str| {
            let normalized = paths::get_normalized_absolute_path(path, &cwd);
            if transformer.host.use_case_sensitive_file_names() {
                normalized
            } else {
                paths::to_file_name_lower_case(&normalized)
            }
        };
        let common =
            paths::normalize_slashes(&transformer.host.common_source_directory().to_string_lossy());
        let common = if common.ends_with('/') {
            common
        } else {
            format!("{common}/")
        };
        // Upstream uses includes, not a path-prefix test.
        if !canonical(&file.canonical_path().to_string_lossy()).contains(&canonical(&common)) {
            return Ok(None);
        }
    }
    Ok(Some(
        crate::external_module_names::get_resolved_external_module_name(
            transformer.host,
            syntax,
            None,
        ),
    ))
}

/// tsc-port: isExternalModuleAugmentation/isModuleAugmentationExternal
/// @6.0.3 (_tsc.js:13737-13749).
pub(super) fn is_external_module_augmentation(
    transformer: &DeclarationTransformer<'_>,
    context: &TransformationContext,
    node: crate::TransformNode,
) -> Result<bool, TransformError> {
    fn ambient(
        context: &TransformationContext,
        node: crate::TransformNode,
    ) -> Result<bool, TransformError> {
        let record = context.arena().node(node)?;
        let NodeData::ModuleDeclaration(data) = &record.data else {
            return Ok(false);
        };
        Ok(
            NodeFlags::from_bits(record.flags).contains(NodeFlags::GLOBAL_AUGMENTATION)
                || data.name.is_some_and(|name| {
                    context
                        .arena()
                        .node(crate::TransformNode::new(node.source(), name))
                        .is_ok_and(|name| name.kind == SyntaxKind::StringLiteral)
                }),
        )
    }
    if !ambient(context, node)? {
        return Ok(false);
    }
    let Some(parent) = transformer.parent(context, node)? else {
        return Ok(false);
    };
    let source_is_external = context
        .arena()
        .source(node.source())?
        .syntax()
        .external_module_indicator
        .is_some();
    match context.arena().node(parent)?.kind {
        SyntaxKind::SourceFile => Ok(source_is_external),
        SyntaxKind::ModuleBlock => {
            let Some(module) = transformer.parent(context, parent)? else {
                return Ok(false);
            };
            let Some(source) = transformer.parent(context, module)? else {
                return Ok(false);
            };
            Ok(ambient(context, module)?
                && context.arena().node(source)?.kind == SyntaxKind::SourceFile
                && !source_is_external)
        }
        _ => Ok(false),
    }
}

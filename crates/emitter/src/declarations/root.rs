use std::path::{Path, PathBuf};

use tsc_program::SourceFileId;
use tsc_syntax::{FileReference, NodeData, SourceFile, TypeReferenceDirective};

use crate::{
    source_map::paths::get_relative_path_to_directory_or_url, EmitHost, TransformArena,
    TransformError, TransformNode, TransformRoot, TransformSourceId, TransformationContext,
};

use super::diagnostics::DiagnosticContext;
use super::state::{RawFileReferences, TransformState, VisitResult};
use super::tracker::materialize_effects;
use super::DeclarationTransformer;

impl RawFileReferences {
    /// tsc-port: collectFileReferences @6.0.3
    /// tsc-hash: 891b95bebb0275d95502640fb6425e51de6ecfb9bc535f6cab79aeda319a466c
    /// tsc-span: _tsc.js:114557-114561
    pub(crate) fn collect(
        arena: &TransformArena,
        source: TransformSourceId,
    ) -> Result<Self, TransformError> {
        let syntax = arena.source(source)?.syntax();
        Ok(Self {
            referenced: syntax
                .referenced_files
                .iter()
                .cloned()
                .map(|reference| (source, reference))
                .collect(),
            type_directives: syntax.type_reference_directives.clone(),
            lib_directives: syntax.lib_reference_directives.clone(),
        })
    }

    /// tsc-port: copyFileReferenceAsSynthetic @6.0.3
    /// tsc-hash: 6f780877bffd7cbd6214f11b4a2a1b2eb229de6bbdb371c597a4d2206acdfb0e
    /// tsc-span: _tsc.js:114562-114567
    pub(crate) fn synthetic_copy(reference: &FileReference) -> FileReference {
        let mut copy = reference.clone();
        copy.pos = u32::MAX;
        copy.end = u32::MAX;
        copy
    }

    fn synthetic_type_copy(reference: &TypeReferenceDirective) -> TypeReferenceDirective {
        let mut copy = reference.clone();
        copy.pos = u32::MAX;
        copy.end = u32::MAX;
        copy
    }

    /// tsc-port: getTypeReferences @6.0.3
    /// tsc-hash: 05a76e61906844205892e973e47651cd1b753d710d579db6751d1de18860f502
    /// tsc-span: _tsc.js:114568-114573
    pub(crate) fn type_references(&self) -> Vec<TypeReferenceDirective> {
        self.type_directives
            .iter()
            .filter(|reference| reference.preserve)
            .map(Self::synthetic_type_copy)
            .collect()
    }

    /// tsc-port: getLibReferences @6.0.3
    /// tsc-hash: 55208a018ce342b9abb9f693b4f5fe59a32d3c97a8b199367d14202ab99a23a7
    /// tsc-span: _tsc.js:114574-114579
    pub(crate) fn lib_references(&self) -> Vec<FileReference> {
        self.lib_directives
            .iter()
            .filter(|reference| reference.preserve)
            .map(Self::synthetic_copy)
            .collect()
    }
}

/// tsc-port: transformRoot @6.0.3
/// tsc-hash: 43626308f2d953c11c6170dba54982dd788297319f7ccd3d58c722f4930fdd73
/// tsc-span: _tsc.js:114441-114614
pub(crate) fn transform_root(
    transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    root: TransformRoot,
) -> Result<TransformRoot, TransformError> {
    let TransformRoot::SourceFile(source) = root else {
        // H2.7d owns bundle declaration composition.
        return Err(TransformError::Unsupported(
            crate::UnsupportedEmitFeature::BundleRoot,
        ));
    };

    let root_node = context.arena().root(source)?;
    let source_syntax = context.arena().source(source)?.syntax().clone();
    if source_syntax.is_declaration_file {
        return Ok(TransformRoot::SourceFile(source));
    }
    if transformer.options.isolated_declarations == Some(true) {
        return Err(TransformError::Unsupported(
            crate::UnsupportedEmitFeature::IsolatedDeclarations,
        ));
    }

    let program_source = context
        .arena()
        .source(source)?
        .program_source()
        .ok_or(TransformError::MissingProgramSource(root_node))?;
    let is_javascript =
        is_javascript_source(&source_syntax, context.arena().node(root_node)?.flags);
    transformer.state = Some(TransformState::for_source(source, root_node));
    transformer
        .tracker
        .reset_for_file(Some(program_source), source, is_javascript);
    transformer.state_mut()?.references = RawFileReferences::collect(context.arena(), source)?;

    let declaration_path = transformer
        .paths
        .declaration_file_path(program_source)
        .unwrap_or_else(|| PathBuf::from(&source_syntax.file_name));
    let declaration_path = normalize_slashes(declaration_path);
    let output_directory = declaration_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();

    let original_statements = source_statements(context.arena(), root_node)?;
    let combined = if is_javascript {
        transform_declarations_for_js(transformer, context, source, root_node)?
    } else {
        let mut statements = Vec::new();
        for statement in original_statements {
            match super::statements::visit_declaration_statement(transformer, context, statement)? {
                VisitResult::None => {}
                VisitResult::Node(statement) => statements.push(statement),
                VisitResult::Nodes(result) => statements.extend(result),
            }
        }
        let statements = super::statements::transform_and_replace_late_painted_statements(
            transformer,
            context,
            statements,
        )?;
        let is_external_module = source_syntax.external_module_indicator.is_some();
        let state = transformer.state()?;
        if is_external_module
            && (!state.result_has_external_module_indicator
                || state.needs_scope_fix_marker && !state.result_has_scope_marker)
        {
            let mut factory = context.factory()?;
            let empty_exports = super::statements::create_empty_exports(&mut factory, source)?;
            statements_with_empty_exports(&mut factory, source, statements, empty_exports)?
        } else {
            statements
        }
    };

    let original_statement_range = source_statement_array(context.arena(), root_node)?
        .map(|array| {
            context
                .arena()
                .node_array(array)
                .map(|array| (array.pos, array.end))
        })
        .transpose()?;
    let referenced_files = referenced_files(
        transformer,
        context.arena(),
        source,
        &output_directory,
        program_source,
    )?;
    let type_references = transformer.state()?.references.type_references();
    let lib_references = transformer.state()?.references.lib_references();
    let updated = {
        let mut factory = context.factory()?;
        let statements = factory.create_node_array(source, combined)?;
        if let Some((pos, end)) = original_statement_range {
            factory.set_node_array_text_range(statements, pos, end)?;
        }
        factory.update_source_file(
            root_node,
            statements,
            true,
            referenced_files,
            type_references,
            false,
            lib_references,
        )?
    };
    context.arena_mut()?.replace_root(source, updated)?;
    Ok(TransformRoot::SourceFile(source))
}

/// tsc-port: transformDeclarationsForJS @6.0.3
/// tsc-hash: fe83d798e4e7ba53668936902ed8d8c0c15191b6138123cc72989e365c67168d
/// tsc-span: _tsc.js:114431-114440
pub(crate) fn transform_declarations_for_js(
    transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    target: TransformSourceId,
    source: TransformNode,
) -> Result<Vec<TransformNode>, TransformError> {
    let resolver_node = transformer.required_resolver_node(context, source)?;
    let saved = transformer
        .tracker
        .replace_diagnostic_context(context.arena(), DiagnosticContext::JsFile(target))?;
    let result = transformer
        .resolver
        .get_declaration_statements_for_source_file(
            context.arena_mut()?,
            target,
            resolver_node,
            crate::EmitNodeBuilderFlags::DECLARATION_EMIT,
            crate::EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
            &mut transformer.tracker,
        )
        .map_err(TransformError::from);
    let effects = transformer.tracker.take_pending_effects();
    let materialized = materialize_effects(context, transformer.host, effects);
    transformer.tracker.restore_diagnostic_context(saved);
    materialized?;
    Ok(result?.unwrap_or_default())
}

/// tsc-port: getReferencedFiles @6.0.3
/// tsc-hash: 42db67eb2183e97330dedfb4a879f7859fc6893797e4b64b616934449f53781a
/// tsc-span: _tsc.js:114580-114613
pub(crate) fn referenced_files(
    transformer: &DeclarationTransformer<'_>,
    arena: &TransformArena,
    _source: TransformSourceId,
    output_directory: &Path,
    program_source: SourceFileId,
) -> Result<Vec<FileReference>, TransformError> {
    let mut result = Vec::new();
    for (referencing_source, reference) in &transformer.state()?.references.referenced {
        if !reference.preserve {
            continue;
        }
        let Some(referencing_program_source) = arena
            .source(*referencing_source)?
            .program_source()
            .or(Some(program_source))
        else {
            continue;
        };
        let Some(file) = transformer
            .host
            .source_file_from_reference(referencing_program_source, reference)
        else {
            continue;
        };
        let declaration_file_name = if is_declaration_file(file.path())
            || file
                .syntax()
                .is_some_and(|syntax| syntax.is_declaration_file)
        {
            file.path().to_path_buf()
        } else {
            transformer
                .paths
                .reference_target_path(file.id())
                .unwrap_or_else(|| file.path().to_path_buf())
        };
        let file_name =
            get_relative_path(output_directory, &declaration_file_name, transformer.host);
        let mut copied = RawFileReferences::synthetic_copy(reference);
        copied.file_name = file_name;
        result.push(copied);
    }
    Ok(result)
}

fn source_statements(
    arena: &TransformArena,
    root: TransformNode,
) -> Result<Vec<TransformNode>, TransformError> {
    let Some(array) = source_statement_array(arena, root)? else {
        return Ok(Vec::new());
    };
    Ok(arena
        .node_array(array)?
        .nodes
        .iter()
        .copied()
        .map(|node| TransformNode::new(root.source(), node))
        .collect())
}

fn source_statement_array(
    arena: &TransformArena,
    root: TransformNode,
) -> Result<Option<crate::TransformNodeArray>, TransformError> {
    let NodeData::SourceFile(data) = &arena.node(root)?.data else {
        return Err(TransformError::RootKindExpected {
            actual: arena.node(root)?.kind,
        });
    };
    Ok(data
        .statements
        .and_then(|array| arena.node_array_ref(root.source(), array)))
}

fn statements_with_empty_exports(
    factory: &mut crate::NodeFactory<'_>,
    source: TransformSourceId,
    mut statements: Vec<TransformNode>,
    empty_exports: TransformNode,
) -> Result<Vec<TransformNode>, TransformError> {
    statements.push(empty_exports);
    let array = factory.create_node_array(source, statements)?;
    let nodes = factory.arena().node_array(array)?.nodes.clone();
    Ok(nodes
        .into_iter()
        .map(|node| TransformNode::new(source, node))
        .collect())
}

fn normalize_slashes(path: PathBuf) -> PathBuf {
    PathBuf::from(path.to_string_lossy().replace('\\', "/"))
}

fn is_javascript_source(source: &SourceFile, flags: i32) -> bool {
    let name = source.file_name.to_ascii_lowercase();
    name.ends_with(".js")
        || name.ends_with(".jsx")
        || name.ends_with(".mjs")
        || name.ends_with(".cjs")
        || tsc_types::NodeFlags::from_bits(flags).contains(tsc_types::NodeFlags::JAVA_SCRIPT_FILE)
}

fn is_declaration_file(path: &Path) -> bool {
    let name = path.to_string_lossy().to_ascii_lowercase();
    name.ends_with(".d.ts") || name.ends_with(".d.mts") || name.ends_with(".d.cts")
}

fn get_relative_path(
    output_directory: &Path,
    declaration_file_name: &Path,
    host: &dyn EmitHost,
) -> String {
    get_relative_path_to_directory_or_url(
        &output_directory.to_string_lossy(),
        &declaration_file_name.to_string_lossy(),
        &host.current_directory().to_string_lossy(),
        host.use_case_sensitive_file_names(),
        false,
    )
}

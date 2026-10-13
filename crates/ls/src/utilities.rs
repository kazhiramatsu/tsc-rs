//! tsgo `ls/utilities.go`: what the language service's features share.

use tsc_checker::state::CheckerState;
use tsc_diagnostics::JsStr;
use tsc_program::{
    CanonicalPath, PreparedProgram, ResolutionKey, ResolutionMode, ResolutionOutcome,
    TypeReferenceResolutionKey,
};
use tsc_syntax::{FileReference, NodeData, NodeId, SourceFile, SyntaxKind};

use crate::astnav::{Found, Navigator};
use crate::references::{collect_external_module_references, is_external_module_name_relative};

/// tsgo `refInfo`: a file a reference at a position names, and whether the
/// program has it.
#[derive(Clone, Debug)]
pub(crate) struct ReferenceInfo {
    /// The program's index of the file, when the program has it.
    pub file: Option<usize>,
    pub file_name: String,
}

/// tsgo `getReferenceAtPosition`: the file a triple-slash reference, or a
/// relative module specifier, at `position` names.
pub(crate) fn reference_at_position(
    state: &CheckerState<'_>,
    prepared: &PreparedProgram,
    file: usize,
    position: u32,
) -> Option<ReferenceInfo> {
    let source = state.binder.source(file);
    if let Some(reference) = find_reference_in_position(&source.referenced_files, position) {
        let target = source_file_from_reference(state, prepared, source, reference)?;
        return Some(file_reference(state, target));
    }

    if let Some(directive) = source
        .type_reference_directives
        .iter()
        .find(|directive| directive.pos <= position && position <= directive.end)
    {
        let mode = match directive.resolution_mode {
            Some(tsc_syntax::TypeReferenceDirectiveResolutionMode::Import) => {
                ResolutionMode::EsNext
            }
            Some(tsc_syntax::TypeReferenceDirectiveResolutionMode::Require) => {
                ResolutionMode::CommonJs
            }
            None => resolution_mode(state.api_default_resolution_mode_for_file(source.root))?,
        };
        let key = TypeReferenceResolutionKey::source(
            canonical_path(prepared, &source.file_name.to_string_lossy())?,
            directive.file_name.clone(),
            mode,
        );
        let resolution = prepared.resolutions().require_type_reference(&key).ok()?;
        let ResolutionOutcome::Resolved(resolved) = resolution.outcome() else {
            return None;
        };
        let file_name = resolved.target().display().to_string_lossy().into_owned();
        let target = source_file(state, prepared, &file_name)?;
        return Some(file_reference(state, target));
    }

    if let Some(reference) = find_reference_in_position(&source.lib_reference_directives, position)
    {
        let target = lib_file_from_reference(state, prepared, reference)?;
        return Some(file_reference(state, target));
    }

    let references = collect_external_module_references(source);
    if references.imports.is_empty() && references.module_augmentations.is_empty() {
        return None;
    }
    let node = match Navigator::new(source).touching_token(position) {
        Found::Node(view) => view.node()?,
        Found::Token(_) => return None,
    };
    let text = string_literal_like_text(source, node)?;
    if !is_module_specifier_like(source, node)
        || !is_external_module_name_relative(&tsc_types::JsString::from(text.as_str()))
    {
        return None;
    }
    let resolution = resolved_module_from_module_specifier(state, prepared, source, node, &text)?;
    let file_name = match resolution {
        Some(file_name) => file_name,
        None => tsc_program::get_normalized_absolute_path(
            JsStr::from_str(&text),
            tsc_program::get_directory_path(source.file_name.as_js()).as_js(),
        )
        .to_string_lossy()
        .into_owned(),
    };
    Some(ReferenceInfo {
        file: source_file(state, prepared, &file_name),
        file_name,
    })
}

fn file_reference(state: &CheckerState<'_>, file: usize) -> ReferenceInfo {
    ReferenceInfo {
        file: Some(file),
        file_name: state
            .binder
            .source(file)
            .file_name
            .to_string_lossy()
            .into_owned(),
    }
}

/// tsgo `findReferenceInPosition`.
fn find_reference_in_position(
    references: &[FileReference],
    position: u32,
) -> Option<&FileReference> {
    references
        .iter()
        .find(|reference| reference.pos <= position && position <= reference.end)
}

/// tsgo `Program.GetSourceFileFromReference`: the file a `/// <reference
/// path>` names, a supported extension added when it has none.
fn source_file_from_reference(
    state: &CheckerState<'_>,
    prepared: &PreparedProgram,
    origin: &SourceFile,
    reference: &FileReference,
) -> Option<usize> {
    let file_name = tsc_program::get_normalized_absolute_path(
        reference.file_name.as_js(),
        tsc_program::get_directory_path(origin.file_name.as_js()).as_js(),
    )
    .to_string_lossy()
    .into_owned();
    let options = prepared.compiler_options();
    let extensions = supported_extensions(options);
    let allow_non_ts_extensions = options.allow_non_ts_extensions == Some(true);
    if has_extension(&file_name) {
        if !allow_non_ts_extensions {
            let canonical = path_key(prepared, &file_name);
            if !extensions
                .iter()
                .flatten()
                .any(|extension| canonical.ends_with(extension))
            {
                return None;
            }
        }
        return source_file(state, prepared, &file_name);
    }
    if allow_non_ts_extensions {
        if let Some(file) = source_file(state, prepared, &file_name) {
            return Some(file);
        }
    }
    // Only the first group (.ts, .tsx, .d.ts, and the JavaScript ones with
    // allowJs) is tried.
    extensions[0]
        .iter()
        .find_map(|extension| source_file(state, prepared, &format!("{file_name}{extension}")))
}

/// tsgo `GetSupportedExtensionsWithJsonIfResolveJsonModule(options,
/// GetSupportedExtensions(options, nil))` (a content mapper's extensions
/// come with the content mapper's language service).
fn supported_extensions(options: &tsc_types::CompilerOptions) -> Vec<Vec<&'static str>> {
    let mut extensions = if options.allow_js {
        vec![
            vec![".ts", ".tsx", ".d.ts", ".js", ".jsx"],
            vec![".cts", ".d.cts", ".cjs"],
            vec![".mts", ".d.mts", ".mjs"],
        ]
    } else {
        vec![
            vec![".ts", ".tsx", ".d.ts"],
            vec![".cts", ".d.cts"],
            vec![".mts", ".d.mts"],
        ]
    };
    if options.resolve_json_module_effective() {
        extensions.push(vec![".json"]);
    }
    extensions
}

/// tsgo `tspath.HasExtension`: whether the base name has a dot.
fn has_extension(file_name: &str) -> bool {
    file_name
        .rsplit('/')
        .next()
        .is_some_and(|name| name.contains('.'))
}

/// tsgo `Program.GetLibFileFromReference`: the program's library file a
/// `/// <reference lib>` names.
fn lib_file_from_reference(
    state: &CheckerState<'_>,
    prepared: &PreparedProgram,
    reference: &FileReference,
) -> Option<usize> {
    let catalog = tsc_program::LibraryCatalog::typescript_7_1("");
    let lower = tsc_program::to_file_name_lower_case_js(reference.file_name.as_js())
        .to_string_lossy()
        .into_owned();
    let name = if catalog.contains_file_name(&lower) {
        lower
    } else {
        catalog.reference_file_name(lower.as_str())?.to_owned()
    };
    prepared.library_files().iter().find_map(|&id| {
        let path = prepared
            .source_file(id)?
            .path()
            .display()
            .to_string_lossy()
            .into_owned();
        if tsc_program::base_file_name(JsStr::from_str(&path)).to_string_lossy() != name {
            return None;
        }
        source_file(state, prepared, &path)
    })
}

/// tsgo `Program.GetResolvedModuleFromModuleSpecifier`: the program's
/// resolution of a module specifier (`Some(None)` when it did not resolve),
/// none when the program did not resolve it.
fn resolved_module_from_module_specifier(
    state: &CheckerState<'_>,
    prepared: &PreparedProgram,
    source: &SourceFile,
    specifier: NodeId,
    text: &str,
) -> Option<Option<String>> {
    let parent = source.arena.node(specifier).parent?;
    if tsc_syntax::utilities::is_source_phase_import(&source.arena, parent) {
        return None;
    }
    let mode = resolution_mode(state.api_mode_for_usage_location(specifier))?;
    let key = ResolutionKey::new(
        canonical_path(prepared, &source.file_name.to_string_lossy())?,
        JsStr::from_str(text).to_owned(),
        mode,
    );
    let resolution = prepared.resolutions().require_module(&key).ok()?;
    Some(match resolution.outcome() {
        ResolutionOutcome::Resolved(module) => Some(
            module
                .target()
                .resolved_file()
                .display()
                .to_string_lossy()
                .into_owned(),
        ),
        _ => None,
    })
}

/// tsgo's `core.ResolutionMode` number as the resolution tables key it.
fn resolution_mode(mode: u32) -> Option<ResolutionMode> {
    match mode {
        0 => Some(ResolutionMode::Unspecified),
        1 => Some(ResolutionMode::CommonJs),
        99 => Some(ResolutionMode::EsNext),
        _ => None,
    }
}

/// tsgo `isModuleSpecifierLike`.
fn is_module_specifier_like(source: &SourceFile, node: NodeId) -> bool {
    let Some(parent) = source.arena.node(node).parent else {
        return false;
    };
    let first_argument = |arguments: Option<tsc_syntax::NodeArrayId>| {
        arguments.and_then(|arguments| source.arena.node_array(arguments).nodes.first().copied())
    };
    if let NodeData::CallExpression(call) = &source.arena.node(parent).data {
        if is_require_call(source, parent)
            || tsc_syntax::utilities::is_import_call(&source.arena, parent)
        {
            return first_argument(call.arguments) == Some(node);
        }
    }
    matches!(
        source.arena.node(parent).kind,
        SyntaxKind::ExternalModuleReference | SyntaxKind::ImportDeclaration
    )
}

/// tsgo `ast.IsRequireCall(node, false)`.
fn is_require_call(source: &SourceFile, node: NodeId) -> bool {
    let NodeData::CallExpression(call) = &source.arena.node(node).data else {
        return false;
    };
    let is_require = call.expression.is_some_and(|expression| {
        tsc_binder::node_util::id_text(source, expression) == Some("require")
            && source.arena.node(expression).kind == SyntaxKind::Identifier
    });
    is_require
        && call
            .arguments
            .is_some_and(|arguments| source.arena.node_array(arguments).nodes.len() == 1)
}

/// The text of a string literal or no-substitution template literal.
fn string_literal_like_text(source: &SourceFile, node: NodeId) -> Option<String> {
    match &source.arena.node(node).data {
        NodeData::StringLiteral(literal) => Some(literal.text.to_string_lossy().into_owned()),
        NodeData::NoSubstitutionTemplateLiteral(literal) => {
            Some(literal.text.to_string_lossy().into_owned())
        }
        _ => None,
    }
}

/// tsgo `Program.GetSourceFile`: the program's file named `file_name`, by
/// path.
pub(crate) fn source_file(
    state: &CheckerState<'_>,
    prepared: &PreparedProgram,
    file_name: &str,
) -> Option<usize> {
    let path = path_key(prepared, file_name);
    (0..state.binder.file_count()).find(|&index| {
        path_key(
            prepared,
            &state.binder.source(index).file_name.to_string_lossy(),
        ) == path
    })
}

/// tsgo `ToPath`: a file name's path, case-folded unless the program's
/// file names are case sensitive.
fn path_key(prepared: &PreparedProgram, file_name: &str) -> String {
    let absolute = tsc_program::get_normalized_absolute_path(
        JsStr::from_str(file_name),
        prepared.current_directory().display(),
    );
    if prepared.path_context().use_case_sensitive_file_names() {
        absolute.to_string_lossy().into_owned()
    } else {
        tsc_program::to_file_name_lower_case_js(absolute.as_js())
            .to_string_lossy()
            .into_owned()
    }
}

fn canonical_path(prepared: &PreparedProgram, file_name: &str) -> Option<CanonicalPath> {
    CanonicalPath::from_js_normalized(JsStr::from_str(&path_key(prepared, file_name))).ok()
}

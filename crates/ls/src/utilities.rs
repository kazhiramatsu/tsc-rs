//! tsgo `ls/utilities.go`: what the language service's features share.

use tsc_checker::state::CheckerState;
use tsc_diagnostics::JsStr;
use tsc_program::{
    CanonicalPath, PreparedProgram, ResolutionKey, ResolutionMode, ResolutionOutcome,
    TypeReferenceResolutionKey,
};
use tsc_syntax::{FileReference, NodeData, NodeId, SourceFile, SyntaxKind};

use std::ops::Range;

use tsc_binder::node_util;

use crate::astnav::{Found, Navigator};
use crate::lsconv::TextScript;
use crate::references::{collect_external_module_references, is_external_module_name_relative};

/// tsgo `refInfo`: a file a reference at a position names, and whether the
/// program has it.
#[derive(Clone, Debug)]
pub(crate) struct ReferenceInfo {
    /// The program's index of the file, when the program has it.
    pub file: Option<usize>,
    pub file_name: String,
    /// The range of the triple-slash directive's file name (tsgo
    /// `reference`; none for a module specifier).
    pub reference: Option<Range<usize>>,
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
        return Some(file_reference(state, target, reference.pos..reference.end));
    }

    if let Some(directive) = source
        .type_reference_directives
        .iter()
        .find(|directive| directive.pos <= position && position <= directive.end)
    {
        let target = type_reference_directive_target(state, prepared, source, directive)?;
        return Some(file_reference(state, target, directive.pos..directive.end));
    }

    if let Some(reference) = find_reference_in_position(&source.lib_reference_directives, position)
    {
        let target = lib_file_from_reference(state, prepared, reference)?;
        return Some(file_reference(state, target, reference.pos..reference.end));
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
        reference: None,
    })
}

/// tsgo `Program.GetResolvedTypeReferenceDirectiveFromTypeReferenceDirective`:
/// the file a `/// <reference types>` resolves to.
fn type_reference_directive_target(
    state: &CheckerState<'_>,
    prepared: &PreparedProgram,
    source: &SourceFile,
    directive: &tsc_syntax::TypeReferenceDirective,
) -> Option<usize> {
    let mode = match directive.resolution_mode {
        Some(tsc_syntax::TypeReferenceDirectiveResolutionMode::Import) => ResolutionMode::EsNext,
        Some(tsc_syntax::TypeReferenceDirectiveResolutionMode::Require) => ResolutionMode::CommonJs,
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
    source_file(state, prepared, &file_name)
}

/// The `/// <reference path>` and `/// <reference types>` directives of a
/// file, with the files they name (tsgo `findModuleReferences`' reads of
/// `ReferencedFiles` and `TypeReferenceDirectives`).
pub(crate) fn file_reference_targets(
    state: &CheckerState<'_>,
    prepared: &PreparedProgram,
    file: usize,
) -> Vec<(Range<usize>, Option<usize>)> {
    let source = state.binder.source(file);
    let mut targets: Vec<(Range<usize>, Option<usize>)> = source
        .referenced_files
        .iter()
        .map(|reference| {
            (
                reference.pos as usize..reference.end as usize,
                source_file_from_reference(state, prepared, source, reference),
            )
        })
        .collect();
    targets.extend(source.type_reference_directives.iter().map(|directive| {
        (
            directive.pos as usize..directive.end as usize,
            type_reference_directive_target(state, prepared, source, directive),
        )
    }));
    targets
}

/// tsgo `ast.IsVariableDeclarationInitializedToBareOrAccessedRequire`: a
/// variable initialized to `require("m")` or an access of one.
pub(crate) fn is_variable_declaration_initialized_to_bare_or_accessed_require(
    source: &SourceFile,
    node: NodeId,
) -> bool {
    let NodeData::VariableDeclaration(data) = &source.arena.node(node).data else {
        return false;
    };
    let Some(mut initializer) = data.initializer else {
        return false;
    };
    // tsgo `GetLeftmostAccessExpression`.
    loop {
        match &source.arena.node(initializer).data {
            NodeData::PropertyAccessExpression(access) => match access.expression {
                Some(expression) => initializer = expression,
                None => break,
            },
            NodeData::ElementAccessExpression(access) => match access.expression {
                Some(expression) => initializer = expression,
                None => break,
            },
            _ => break,
        }
    }
    is_require_call(source, initializer, true)
}

fn file_reference(state: &CheckerState<'_>, file: usize, reference: Range<u32>) -> ReferenceInfo {
    ReferenceInfo {
        file: Some(file),
        file_name: state
            .binder
            .source(file)
            .file_name
            .to_string_lossy()
            .into_owned(),
        reference: Some(reference.start as usize..reference.end as usize),
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
fn supported_extensions(options: &tsc_types::CompilerOptions) -> Vec<Vec<String>> {
    let mut extensions = supported_extensions_with(options, &[]);
    if options.resolve_json_module_effective() {
        extensions.push(vec![".json".to_owned()]);
    }
    extensions
}

/// tsgo `tsoptions.GetSupportedExtensions`: the TypeScript (and with
/// `allowJs` the JavaScript) extensions, then each extra one that is no
/// built-in.
pub(crate) fn supported_extensions_with(
    options: &tsc_types::CompilerOptions,
    extra_extensions: &[String],
) -> Vec<Vec<String>> {
    let builtins: &[&[&str]] = if options.allow_js {
        &[
            &[".ts", ".tsx", ".d.ts", ".js", ".jsx"],
            &[".cts", ".d.cts", ".cjs"],
            &[".mts", ".d.mts", ".mjs"],
        ]
    } else {
        &[
            &[".ts", ".tsx", ".d.ts"],
            &[".cts", ".d.cts"],
            &[".mts", ".d.mts"],
        ]
    };
    let mut extensions = builtins
        .iter()
        .map(|group| {
            group
                .iter()
                .map(|&extension| extension.to_owned())
                .collect()
        })
        .collect::<Vec<Vec<String>>>();
    for extension in extra_extensions {
        if !builtins
            .iter()
            .any(|group| group.contains(&extension.as_str()))
        {
            extensions.push(vec![extension.clone()]);
        }
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
pub(crate) fn resolved_module_from_module_specifier(
    state: &CheckerState<'_>,
    prepared: &PreparedProgram,
    source: &SourceFile,
    specifier: NodeId,
    text: &str,
) -> Option<Option<String>> {
    let resolution = module_resolution_of_specifier(state, prepared, source, specifier, text)?;
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

/// The program's resolution of the module specifier `specifier` (its text
/// `text`) of `source` under its usage mode (tsgo `resolvedModules[file]`
/// with `GetModeForUsageLocation`), none when the program did not resolve it
/// (a source phase import is not resolved).
pub(crate) fn module_resolution_of_specifier<'p>(
    state: &CheckerState<'_>,
    prepared: &'p PreparedProgram,
    source: &SourceFile,
    specifier: NodeId,
    text: &str,
) -> Option<&'p tsc_program::ModuleResolution> {
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
    prepared.resolutions().require_module(&key).ok()
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
        if is_require_call(source, parent, false)
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

/// tsgo `ast.IsRequireCall`: a `require` call with one argument, a string
/// literal when `require_string_literal_like_argument`.
pub(crate) fn is_require_call(
    source: &SourceFile,
    node: NodeId,
    require_string_literal_like_argument: bool,
) -> bool {
    let NodeData::CallExpression(call) = &source.arena.node(node).data else {
        return false;
    };
    let is_require = call.expression.is_some_and(|expression| {
        tsc_binder::node_util::id_text(source, expression) == Some("require")
            && source.arena.node(expression).kind == SyntaxKind::Identifier
    });
    let Some(arguments) = call
        .arguments
        .map(|arguments| source.arena.node_array(arguments).nodes)
    else {
        return false;
    };
    is_require
        && arguments.len() == 1
        && (!require_string_literal_like_argument
            || matches!(
                source.arena.node(arguments[0]).kind,
                SyntaxKind::StringLiteral | SyntaxKind::NoSubstitutionTemplateLiteral
            ))
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
pub(crate) fn path_key(prepared: &PreparedProgram, file_name: &str) -> String {
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

pub(crate) fn canonical_path(prepared: &PreparedProgram, file_name: &str) -> Option<CanonicalPath> {
    CanonicalPath::from_js_normalized(JsStr::from_str(&path_key(prepared, file_name))).ok()
}

/// What tsgo's `GetTouchingPropertyName` finds, as the definition reads it:
/// a node, or a token tsc-rs's tree does not keep (tsgo's source file
/// creates one for the scanner's token).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Touching {
    Node(NodeId),
    Token {
        kind: SyntaxKind,
        pos: u32,
        end: u32,
        parent: NodeId,
    },
}

/// What astnav found, as a node or token of tsc-rs's tree; none for a view
/// without a node (a list or a JSDoc comment's text).
pub(crate) fn touching(found: Found) -> Option<Touching> {
    match found {
        Found::Node(view) => view.node().map(Touching::Node),
        Found::Token(token) => token.parent.node().map(|parent| Touching::Token {
            kind: token.kind,
            pos: token.pos,
            end: token.end,
            parent,
        }),
    }
}

/// A file of the program as a script: its name and text.
pub(crate) fn script_of(source: &SourceFile) -> TextScript<'_> {
    TextScript {
        file_name: source.file_name.to_string_lossy().into_owned(),
        text: source.text(),
    }
}

/// tsgo `isInComment`: `getRangeOfEnclosingComment` with the token before
/// the position.
pub(crate) fn is_in_comment(
    navigator: &Navigator<'_>,
    position: u32,
    token_at_position: Found,
) -> bool {
    range_of_enclosing_comment(
        navigator,
        position,
        navigator.preceding_token(position),
        token_at_position,
    )
    .is_some()
}

/// tsgo `getRangeOfEnclosingComment`: the comment that contains the
/// position, among the trailing comments of the token before it and the
/// leading comments of the token at it (the host's, in a JSDoc comment).
pub(crate) fn range_of_enclosing_comment(
    navigator: &Navigator<'_>,
    position: u32,
    preceding_token: Option<Found>,
    token_at_position: Found,
) -> Option<(u32, u32)> {
    let mut token = token_at_position;
    let mut ancestor = Some(token_at_position);
    while let Some(node) = ancestor {
        if navigator.kind(node) == SyntaxKind::JSDoc {
            token = navigator.parent(node).expect("a JSDoc comment's host");
            break;
        }
        ancestor = navigator.parent(node);
    }
    let start = navigator.start_of_node(token, false);
    if start <= position && position < navigator.end(token) {
        return None;
    }

    let text = navigator.file().text();
    // Between two consecutive tokens, all comments are either trailing on the former
    // or leading on the latter (and none are in both lists).
    let mut comment_ranges = Vec::new();
    if let Some(preceding) = preceding_token {
        comment_ranges.extend(tsc_syntax::comment_ranges(
            text,
            navigator.end(preceding) as usize,
            true,
        ));
    }
    if navigator.kind(token) != SyntaxKind::JsxText {
        comment_ranges.extend(tsc_syntax::comment_ranges(
            text,
            navigator.pos(token) as usize,
            false,
        ));
    }
    let position = position as usize;
    comment_ranges
        .into_iter()
        .find(|&(pos, end)| {
            // The end marker of a single-line comment does not include the newline character.
            // In the following case where the cursor is at `^`, we are inside a comment:
            //
            //    // asdf   ^\n
            //
            // But for closed multi-line comments, we don't want to be inside the comment in the following case:
            //
            //    /* asdf */^
            //
            // Internally, we represent the end of the comment prior to the newline and at the '/', respectively.
            //
            // However, unterminated multi-line comments lack a `/`, end at the end of the file, and *do* contain their end.
            //
            let single_line = text[pos..].starts_with("//");
            (pos < position && position < end)
                || position == end && (single_line || position == text.len())
        })
        .map(|(pos, end)| (pos as u32, end as u32))
}

/// tsgo `GetTokenPosOfNode(node, file, false)` of a node of the tree.
pub(crate) fn token_start(source: &SourceFile, node: NodeId) -> usize {
    Navigator::new(source).start_of_node(Found::Node(crate::tree::View::Node(node)), false) as usize
}

/// tsgo `getRangeOfNode` (without an end node): a string literal's range
/// without its quotes.
pub(crate) fn range_of_node(state: &CheckerState<'_>, node: NodeId) -> Range<usize> {
    range_of_node_in(state.binder.source_of_node(node), node)
}

/// [`range_of_node`] of a node in any parse of a file.
pub(crate) fn range_of_node_in(source: &SourceFile, node: NodeId) -> Range<usize> {
    let record = source.arena.node(node);
    let mut start = token_start(source, node);
    let mut end = record.end as usize;
    if matches!(
        record.kind,
        SyntaxKind::StringLiteral | SyntaxKind::NoSubstitutionTemplateLiteral
    ) && end - start > 2
    {
        start += 1;
        end -= 1;
    }
    start..end
}

/// tsgo `getContainingObjectLiteralElement`: the object literal (or JSX
/// attributes) element a name names.
pub(crate) fn containing_object_literal_element(
    state: &CheckerState<'_>,
    node: NodeId,
) -> Option<NodeId> {
    let element = containing_object_literal_element_worker(state, node)?;
    let parent = state.binder.node_record(element).parent?;
    matches!(
        state.binder.node_record(parent).kind,
        SyntaxKind::ObjectLiteralExpression | SyntaxKind::JsxAttributes
    )
    .then_some(element)
}

pub(crate) fn containing_object_literal_element_worker(
    state: &CheckerState<'_>,
    node: NodeId,
) -> Option<NodeId> {
    let record = state.binder.node_record(node);
    let parent = record.parent?;
    let parent_record = state.binder.node_record(parent);
    match record.kind {
        SyntaxKind::StringLiteral
        | SyntaxKind::NoSubstitutionTemplateLiteral
        | SyntaxKind::NumericLiteral
            if parent_record.kind == SyntaxKind::ComputedPropertyName =>
        {
            let element = parent_record.parent?;
            is_object_literal_or_jsx_element(state.binder.node_record(element).kind)
                .then_some(element)
        }
        SyntaxKind::StringLiteral
        | SyntaxKind::NoSubstitutionTemplateLiteral
        | SyntaxKind::NumericLiteral
        | SyntaxKind::Identifier
        | SyntaxKind::JsxNamespacedName => {
            let grandparent = parent_record.parent?;
            let source = state.binder.source_of_node(parent);
            (is_object_literal_or_jsx_element(parent_record.kind)
                && matches!(
                    state.binder.node_record(grandparent).kind,
                    SyntaxKind::ObjectLiteralExpression | SyntaxKind::JsxAttributes
                )
                && node_util::name_field_of(source, parent) == Some(node))
            .then_some(parent)
        }
        _ => None,
    }
}

/// tsgo `isObjectLiteralOrJsxElement`.
pub(crate) fn is_object_literal_or_jsx_element(kind: SyntaxKind) -> bool {
    is_object_literal_element(kind)
        || matches!(
            kind,
            SyntaxKind::JsxAttribute | SyntaxKind::JsxSpreadAttribute
        )
}

/// tsgo `ast.IsObjectLiteralElement`.
pub(crate) fn is_object_literal_element(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PropertyAssignment
            | SyntaxKind::ShorthandPropertyAssignment
            | SyntaxKind::SpreadAssignment
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
    )
}

/// tsgo `ast.IsPropertyName`.
pub(crate) fn is_property_name(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Identifier
            | SyntaxKind::PrivateIdentifier
            | SyntaxKind::StringLiteral
            | SyntaxKind::NumericLiteral
            | SyntaxKind::ComputedPropertyName
    )
}

/// tsgo `ast.IsClassElement`.
pub(crate) fn is_class_element(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Constructor
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::IndexSignature
            | SyntaxKind::ClassStaticBlockDeclaration
            | SyntaxKind::SemicolonClassElement
    )
}

/// tsgo `ast.IsFunctionLikeKind`.
pub(crate) fn is_function_like_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::MethodSignature
            | SyntaxKind::CallSignature
            | SyntaxKind::JSDocSignature
            | SyntaxKind::ConstructSignature
            | SyntaxKind::IndexSignature
            | SyntaxKind::FunctionType
            | SyntaxKind::ConstructorType
    ) || node_util::is_function_like_declaration_kind(kind)
}

/// tsgo `scanner.ComputeLineOfPosition`: the line of a position among a
/// file's ECMAScript line starts.
pub(crate) fn line_of_position(line_starts: &[usize], position: usize) -> usize {
    match line_starts.binary_search(&position) {
        Ok(line) => line,
        Err(insertion) => insertion.saturating_sub(1),
    }
}

/// tsgo `getLineEndOfPosition`: the end of a position's line, before its
/// line break (both characters of a CRLF).
pub(crate) fn line_end_of_position(
    source: &SourceFile,
    line_starts: &[usize],
    position: usize,
) -> usize {
    let line = line_of_position(line_starts, position);
    let last = match line_starts.get(line + 1) {
        Some(&next) => next - 1,
        None => source.arena.node(source.root).end as usize,
    };
    let bytes = source.text().as_bytes();
    if last > 0 && last < bytes.len() && bytes[last] == b'\n' && bytes[last - 1] == b'\r' {
        return last - 1;
    }
    last
}

/// tsgo `ast.CanHaveSymbol`.
pub(crate) fn can_have_symbol(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ArrowFunction
            | SyntaxKind::BinaryExpression
            | SyntaxKind::BindingElement
            | SyntaxKind::CallExpression
            | SyntaxKind::CallSignature
            | SyntaxKind::ClassDeclaration
            | SyntaxKind::ClassExpression
            | SyntaxKind::ClassStaticBlockDeclaration
            | SyntaxKind::Constructor
            | SyntaxKind::ConstructorType
            | SyntaxKind::ConstructSignature
            | SyntaxKind::ElementAccessExpression
            | SyntaxKind::EnumDeclaration
            | SyntaxKind::EnumMember
            | SyntaxKind::ExportAssignment
            | SyntaxKind::ExportDeclaration
            | SyntaxKind::ExportSpecifier
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::FunctionExpression
            | SyntaxKind::FunctionType
            | SyntaxKind::GetAccessor
            | SyntaxKind::ImportClause
            | SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::ImportSpecifier
            | SyntaxKind::IndexSignature
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::JsxAttribute
            | SyntaxKind::JsxAttributes
            | SyntaxKind::JsxSpreadAttribute
            | SyntaxKind::MappedType
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::MethodSignature
            | SyntaxKind::ModuleDeclaration
            | SyntaxKind::NamedTupleMember
            | SyntaxKind::NamespaceExport
            | SyntaxKind::NamespaceExportDeclaration
            | SyntaxKind::NamespaceImport
            | SyntaxKind::NewExpression
            | SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::NumericLiteral
            | SyntaxKind::ObjectLiteralExpression
            | SyntaxKind::Parameter
            | SyntaxKind::PropertyAccessExpression
            | SyntaxKind::PropertyAssignment
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::PropertySignature
            | SyntaxKind::SetAccessor
            | SyntaxKind::ShorthandPropertyAssignment
            | SyntaxKind::SourceFile
            | SyntaxKind::SpreadAssignment
            | SyntaxKind::StringLiteral
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::TypeLiteral
            | SyntaxKind::TypeParameter
            | SyntaxKind::VariableDeclaration
    )
}

/// tsgo's nodes with `DeclarationData` (the node types that embed
/// `DeclarationBase`).
pub(crate) fn has_declaration_data(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ArrowFunction
            | SyntaxKind::BinaryExpression
            | SyntaxKind::BindingElement
            | SyntaxKind::CallExpression
            | SyntaxKind::CallSignature
            | SyntaxKind::ClassDeclaration
            | SyntaxKind::ClassExpression
            | SyntaxKind::ClassStaticBlockDeclaration
            | SyntaxKind::ConstructSignature
            | SyntaxKind::Constructor
            | SyntaxKind::ConstructorType
            | SyntaxKind::EnumDeclaration
            | SyntaxKind::EnumMember
            | SyntaxKind::ExportAssignment
            | SyntaxKind::ExportDeclaration
            | SyntaxKind::ExportSpecifier
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::FunctionExpression
            | SyntaxKind::FunctionType
            | SyntaxKind::GetAccessor
            | SyntaxKind::ImportClause
            | SyntaxKind::ImportDeclaration
            | SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::ImportSpecifier
            | SyntaxKind::IndexSignature
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::JSDocSignature
            | SyntaxKind::JSDocTypeLiteral
            | SyntaxKind::JsxAttribute
            | SyntaxKind::JsxAttributes
            | SyntaxKind::JsxSpreadAttribute
            | SyntaxKind::MappedType
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::MethodSignature
            | SyntaxKind::MissingDeclaration
            | SyntaxKind::ModuleDeclaration
            | SyntaxKind::NamedTupleMember
            | SyntaxKind::NamespaceExport
            | SyntaxKind::NamespaceExportDeclaration
            | SyntaxKind::NamespaceImport
            | SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::ObjectLiteralExpression
            | SyntaxKind::Parameter
            | SyntaxKind::PropertyAssignment
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::PropertySignature
            | SyntaxKind::SemicolonClassElement
            | SyntaxKind::SetAccessor
            | SyntaxKind::ShorthandPropertyAssignment
            | SyntaxKind::SourceFile
            | SyntaxKind::SpreadAssignment
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::TypeLiteral
            | SyntaxKind::TypeParameter
            | SyntaxKind::VariableDeclaration
    )
}

/// A function-like node's parameters (tsgo `Node.Parameters()`).
pub(crate) fn parameters_of(state: &CheckerState<'_>, node: NodeId) -> Vec<NodeId> {
    parameters_in(state.binder.source_of_node(node), node)
}

/// A function-like node's parameters in a parse of its file.
pub(crate) fn parameters_in(source: &SourceFile, node: NodeId) -> Vec<NodeId> {
    let list = match &source.arena.node(node).data {
        NodeData::FunctionDeclaration(data) => data.parameters,
        NodeData::FunctionExpression(data) => data.parameters,
        NodeData::ArrowFunction(data) => data.parameters,
        NodeData::MethodDeclaration(data) => data.parameters,
        NodeData::MethodSignature(data) => data.parameters,
        NodeData::Constructor(data) => data.parameters,
        NodeData::GetAccessor(data) => data.parameters,
        NodeData::SetAccessor(data) => data.parameters,
        NodeData::CallSignature(data) => data.parameters,
        NodeData::ConstructSignature(data) => data.parameters,
        NodeData::IndexSignature(data) => data.parameters,
        NodeData::FunctionType(data) => data.parameters,
        NodeData::ConstructorType(data) => data.parameters,
        NodeData::JSDocFunctionType(data) => data.parameters,
        _ => None,
    };
    list.map(|list| source.arena.node_array(list).nodes.to_vec())
        .unwrap_or_default()
}

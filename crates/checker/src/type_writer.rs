//! The type and symbol writer of TypeScript's native compiler runner
//! (`tsc/internal/testutil/tsbaseline/type_symbol_baseline.go:263-500`):
//! the walk over one file's tree that lists, for every expression,
//! identifier and declaration name, the type at the node (the `.types`
//! baseline) or the symbol at the node with its declarations (the
//! `.symbols` baseline). The layout of the lines is the runner's.
//!
//! The walk queries the checker after it has reported every diagnostic
//! (`GetTypeAtLocation`, `GetSymbolAtLocation`) and prints the types the
//! way the runner does: the node builder with `NoTruncation`,
//! `AllowUniqueESSymbolType` and `GenerateNamesForShadowedTypeParams`,
//! ignoring errors and allowing unresolved names, printed without
//! comments.
//! tsrs-native: harness writer; no tsc counterpart.

use tsc_binder::node_util;
use tsc_emitter::{
    create_printer, EmitInternalNodeBuilderFlags, EmitNodeBuilderFlags, EmitSymbolMeaning,
    NewLineKind, PrintRequest, PrinterOptions, StandaloneWriter,
};
use tsc_syntax::{for_each_child, NodeData, NodeId, SyntaxKind};
use tsc_types::{JsString, TypeData, TypeId};

use crate::declaration_emit::SymbolFormatFlags;
use crate::state::CheckerState;

/// One line of a baseline: the 0-based line of the node's first token, the
/// node's source text and the type or symbol text (`typeWriterResult`).
/// A type line whose type is any-flagged at a node outside the writer's
/// exceptions also carries the intrinsic name (`any`, `error`,
/// `unresolved`, `intrinsic`), which the runner prints instead of the
/// built type when the configuration has no error baseline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeWriterLine {
    pub line: usize,
    pub source_text: String,
    pub text: String,
    pub any_name: Option<String>,
}

/// `TypeFormatFlagsNoTruncation | AllowUniqueESSymbolType |
/// GenerateNamesForShadowedTypeParams` masked to the node builder's flags,
/// plus `FlagsIgnoreErrors` (type_symbol_baseline.go:390-391).
const TYPE_NODE_FLAGS: u32 = 1 | 1_048_576 | 4 | 70_221_824;
/// `TypeFormatFlagsInTypeAlias`.
const IN_TYPE_ALIAS: u32 = 8_388_608;
/// `InternalFlagsAllowUnresolvedNames`.
const ALLOW_UNRESOLVED_NAMES: u32 = 8;

/// The `.types` lines of one file (`getTypes`, :293-297).
pub fn write_types(
    state: &mut CheckerState<'_>,
    file_index: usize,
) -> Result<Vec<TypeWriterLine>, String> {
    walk(state, file_index, false)
}

/// The `.symbols` lines of one file (`getSymbols`, :299-303).
pub fn write_symbols(
    state: &mut CheckerState<'_>,
    file_index: usize,
) -> Result<Vec<TypeWriterLine>, String> {
    walk(state, file_index, true)
}

/// `visitNode` (:304-317): every candidate node in walk order, with the
/// lines the type or symbol query produced.
fn walk(
    state: &mut CheckerState<'_>,
    file_index: usize,
    is_symbol_walk: bool,
) -> Result<Vec<TypeWriterLine>, String> {
    // The display arena must know every file a type or symbol node may
    // reach (a reused annotation, a declaration's name): mount them all.
    for index in 0..state.binder.file_count() {
        state.emit_display_target(index);
    }
    let root = state.binder.source(file_index).root;
    let nodes = for_each_ast_node(state, root);
    let mut results = Vec::new();
    for node in nodes {
        let candidate = state.is_expression_node_like_tsgo(node)
            || state.kind_of(node) == SyntaxKind::Identifier
            || state.is_declaration_name(node)
            || (state.kind_of(node) == SyntaxKind::QualifiedName
                && state.is_name_of_heritage_clause_type_reference(node)
                && (is_symbol_walk
                    || state
                        .parent_of(node)
                        .is_some_and(|parent| state.kind_of(parent) == SyntaxKind::QualifiedName)));
        if !candidate {
            continue;
        }
        if let Some(result) = write_type_or_symbol(state, node, is_symbol_walk)? {
            results.push(result);
        }
    }
    Ok(results)
}

/// `forEachASTNode` (:319-344): an iterative pre-order traversal; the
/// children of a node are pushed in reverse so the first child is visited
/// first. The reparsed JSDoc nodes tsgo skips are not tree nodes here.
fn for_each_ast_node(state: &CheckerState<'_>, root: NodeId) -> Vec<NodeId> {
    let source = state.binder.source_of_node(root);
    let mut result = Vec::new();
    let mut work = vec![root];
    let mut children = Vec::new();
    while let Some(node) = work.pop() {
        result.push(node);
        children.clear();
        for_each_child(&source.arena, source.arena.node(node), |child| {
            children.push(child);
            false
        });
        children.reverse();
        work.extend(children.iter().copied());
    }
    result
}

/// `writeTypeOrSymbol` (:356-456).
fn write_type_or_symbol(
    state: &mut CheckerState<'_>,
    node: NodeId,
    is_symbol_walk: bool,
) -> Result<Option<TypeWriterLine>, String> {
    let source = state.binder.source_of_node(node);
    let raw = source.arena.node(node);
    let text = source.text();
    let actual_pos = tsc_syntax::skip_trivia(text, raw.pos as usize);
    let line = source
        .positions()
        .line_of_byte(u32::try_from(actual_pos).map_err(|_| "position exceeds u32")?)
        .unwrap_or(0) as usize;
    // GetSourceTextOfNodeFromSourceFile: the text from the first token to
    // the end; a missing node has none.
    let source_text = if node_util::node_is_missing(source, Some(node)) {
        String::new()
    } else {
        text[actual_pos.min(raw.end as usize)..raw.end as usize].to_owned()
    };
    let parent = state.parent_of(node);

    if !is_symbol_walk {
        // Don't try to get the type of something that's already a type,
        // except `T` in `type T = something`.
        if state.is_part_of_type_node(node)
            || (state.kind_of(node) == SyntaxKind::Identifier
                && parent.is_some_and(|parent| {
                    get_meaning_from_declaration(state, parent) & MEANING_VALUE == 0
                        && !(state.kind_of(parent) == SyntaxKind::TypeAliasDeclaration
                            && node_util::name_field_of(source, parent) == Some(node))
                }))
            || state.kind_of(node) == SyntaxKind::OmittedExpression
        {
            return Ok(None);
        }

        let mut ty: Option<TypeId> = None;
        // Workaround to ensure we output 'C' instead of 'typeof C' for base
        // class expressions.
        if let Some(parent) = parent {
            if state.is_expression_with_type_arguments_in_class_extends_clause(parent) {
                ty = Some(check(state.get_type_at_location(parent))?);
            }
        }
        let ty = match ty {
            Some(ty) if !state.is_type_any_flagged(ty) => ty,
            _ => check(state.get_type_at_location(node))?,
        };
        // Without an error baseline the runner prints an any-flagged type's
        // intrinsic name at these nodes; the runner decides, so both texts
        // are produced.
        let parent_kind = parent.map(|parent| state.kind_of(parent));
        let any_name = if state.is_type_any_flagged(ty)
            && parent_kind != Some(SyntaxKind::BindingElement)
            && !matches!(
                parent_kind,
                Some(SyntaxKind::PropertyAccessExpression | SyntaxKind::QualifiedName)
            )
            && !is_label_name(state, node)
            && parent.is_none_or(|parent| !node_util::is_global_scope_augmentation(source, parent))
            && parent_kind != Some(SyntaxKind::MetaProperty)
            && !is_import_statement_name(state, node)
            && !is_export_statement_name(state, node)
            && !is_intrinsic_jsx_tag(state, node)
        {
            Some(intrinsic_name(state, ty).to_owned())
        } else {
            None
        };
        let type_string = {
            let (mut printed, built_kind) = type_to_string(state, ty, parent, TYPE_NODE_FLAGS)?;
            // For a type alias `type T = ...` whose type builds as the bare
            // identifier `T` (an alias reference is a type reference, not an
            // identifier), print the type without reusing the alias name.
            if state.kind_of(node) == SyntaxKind::Identifier
                && parent.is_some_and(|parent| {
                    state.kind_of(parent) == SyntaxKind::TypeAliasDeclaration
                        && node_util::name_field_of(source, parent) == Some(node)
                })
                && built_kind == SyntaxKind::Identifier
                && state.identifier_text_of(node) == Some(printed.as_str())
            {
                printed = type_to_string(state, ty, parent, TYPE_NODE_FLAGS | IN_TYPE_ALIAS)?.0;
            }
            printed
        };
        return Ok(Some(TypeWriterLine {
            line,
            source_text,
            text: type_string,
            any_name,
        }));
    }

    let Some(symbol) = check(state.get_symbol_at_location(node))? else {
        return Ok(None);
    };
    let mut symbol_string = String::from("Symbol(");
    let name = check(state.symbol_to_string_via_node_builder(
        symbol,
        parent,
        EmitSymbolMeaning(0),
        SymbolFormatFlags::ALLOW_ANY_NODE_KIND,
    ))?;
    symbol_string.push_str(&name.to_string_lossy());
    let declarations = state.binder.symbol(symbol).declarations.clone();
    for (count, &declaration) in declarations.iter().enumerate() {
        if count >= 5 {
            symbol_string.push_str(&format!(" ... and {} more", declarations.len() - count));
            break;
        }
        symbol_string.push_str(", ");
        let declaration_source = state.binder.source_of_node(declaration);
        let file_name = declaration_source.file_name.to_string_lossy();
        let base_name = file_name
            .rsplit('/')
            .next()
            .unwrap_or(&file_name)
            .to_owned();
        symbol_string.push_str("Decl(");
        symbol_string.push_str(&base_name);
        symbol_string.push_str(", ");
        if is_default_library_file(&base_name) {
            symbol_string.push_str("--, --)");
        } else {
            let position = declaration_source.arena.node(declaration).pos;
            let location = declaration_source
                .positions()
                .line_and_character_byte(position)
                .ok_or_else(|| format!("declaration position {position} outside {base_name}"))?;
            symbol_string.push_str(&format!("{}, {})", location.line, location.character));
        }
    }
    symbol_string.push(')');
    Ok(Some(TypeWriterLine {
        line,
        source_text,
        text: symbol_string,
        any_name: None,
    }))
}

fn check<T>(result: crate::state::CheckResult<T>) -> Result<T, String> {
    result.map_err(|abort| format!("checker abort: {}", abort.description()))
}

/// `TypeToTypeNode` with the writer's flags, printed with comments removed
/// into a single-line writer; with the kind of the built node.
fn type_to_string(
    state: &mut CheckerState<'_>,
    ty: TypeId,
    enclosing: Option<NodeId>,
    flags: u32,
) -> Result<(String, SyntaxKind), String> {
    let file_index = enclosing.map_or(0, |node| state.binder.file_index_of_node(node));
    let target = state.emit_display_target(file_index);
    let mut display = state.take_emit_display();
    let built = crate::node_builder::type_to_type_node(
        state,
        display
            .arena_mut()
            .expect("checker display result remains live"),
        target,
        ty,
        enclosing,
        Some(EmitNodeBuilderFlags(flags)),
        Some(EmitInternalNodeBuilderFlags(ALLOW_UNRESOLVED_NAMES)),
        None,
        None,
        None,
        None,
    );
    let node = match built {
        Ok(Some(node)) => node,
        Ok(None) => {
            state.restore_emit_display(display);
            return Err("type node builder produced no node".to_owned());
        }
        Err(error) => {
            state.restore_emit_display(display);
            return Err(format!("type node builder: {error}"));
        }
    };
    let built_kind = display
        .arena_mut()
        .ok()
        .and_then(|arena| arena.node(node).ok().map(|node| node.kind))
        .unwrap_or(SyntaxKind::Unknown);
    state.restore_emit_display(display);
    // tsgo prints with RemoveComments only; the port's printer prints type
    // nodes under its declaration-syntax gate.
    let options = PrinterOptions::new(NewLineKind::LineFeed)
        .with_remove_comments(true)
        .with_declaration_syntax(true);
    let printed = state
        .with_emit_display(|display| {
            create_printer(options).print(
                display,
                PrintRequest::StandaloneNode {
                    node,
                    writer: StandaloneWriter::SingleLine,
                },
                None,
            )
        })
        .map_err(|error| format!("type node printer: {error:?}"))?;
    Ok((
        JsString::from_code_units(printed.text_utf16().as_ref())
            .to_string_lossy()
            .into_owned(),
        built_kind,
    ))
}

/// `IntrinsicName()` of an any-flagged type: any, error, unresolved or
/// intrinsic.
fn intrinsic_name<'a>(state: &'a CheckerState<'_>, ty: TypeId) -> &'a str {
    match &state.tables.type_of(ty).data {
        TypeData::Intrinsic { name, .. } => name,
        _ => "any",
    }
}

const MEANING_VALUE: u32 = 1;
const MEANING_TYPE: u32 = 2;
const MEANING_NAMESPACE: u32 = 4;
const MEANING_ALL: u32 = MEANING_VALUE | MEANING_TYPE | MEANING_NAMESPACE;

/// `GetMeaningFromDeclaration` (ast/utilities.go:2250-2305).
fn get_meaning_from_declaration(state: &CheckerState<'_>, node: NodeId) -> u32 {
    let source = state.binder.source_of_node(node);
    match state.kind_of(node) {
        SyntaxKind::VariableDeclaration
        | SyntaxKind::Parameter
        | SyntaxKind::BindingElement
        | SyntaxKind::PropertyDeclaration
        | SyntaxKind::PropertySignature
        | SyntaxKind::PropertyAssignment
        | SyntaxKind::ShorthandPropertyAssignment
        | SyntaxKind::MethodDeclaration
        | SyntaxKind::MethodSignature
        | SyntaxKind::Constructor
        | SyntaxKind::GetAccessor
        | SyntaxKind::SetAccessor
        | SyntaxKind::FunctionDeclaration
        | SyntaxKind::FunctionExpression
        | SyntaxKind::ArrowFunction
        | SyntaxKind::CatchClause
        | SyntaxKind::JsxAttribute => MEANING_VALUE,
        SyntaxKind::TypeParameter
        | SyntaxKind::InterfaceDeclaration
        | SyntaxKind::TypeAliasDeclaration
        | SyntaxKind::TypeLiteral => MEANING_TYPE,
        SyntaxKind::EnumMember | SyntaxKind::ClassDeclaration => MEANING_VALUE | MEANING_TYPE,
        SyntaxKind::ModuleDeclaration => {
            if node_util::is_ambient_module(source, node)
                || tsc_binder::containers::get_module_instance_state(
                    source,
                    node,
                    &mut Default::default(),
                ) == tsc_binder::containers::ModuleInstanceState::Instantiated
            {
                MEANING_NAMESPACE | MEANING_VALUE
            } else {
                MEANING_NAMESPACE
            }
        }
        SyntaxKind::EnumDeclaration
        | SyntaxKind::NamedImports
        | SyntaxKind::ImportSpecifier
        | SyntaxKind::ImportEqualsDeclaration
        | SyntaxKind::ImportDeclaration
        | SyntaxKind::ExportAssignment
        | SyntaxKind::ExportDeclaration => MEANING_ALL,
        // An external module can be a Value.
        SyntaxKind::SourceFile => MEANING_NAMESPACE | MEANING_VALUE,
        _ => MEANING_ALL,
    }
}

/// `IsLabelName`: the label of a labeled statement or a jump statement's
/// target.
fn is_label_name(state: &CheckerState<'_>, node: NodeId) -> bool {
    if state.kind_of(node) != SyntaxKind::Identifier {
        return false;
    }
    state
        .parent_of(node)
        .is_some_and(|parent| match state.data_of(parent) {
            NodeData::LabeledStatement(data) => data.label == Some(node),
            NodeData::BreakStatement(data) => data.label == Some(node),
            NodeData::ContinueStatement(data) => data.label == Some(node),
            _ => false,
        })
}

/// `isImportStatementName` (:458-469).
fn is_import_statement_name(state: &CheckerState<'_>, node: NodeId) -> bool {
    state
        .parent_of(node)
        .is_some_and(|parent| match state.data_of(parent) {
            NodeData::ImportSpecifier(data) => {
                data.name == Some(node) || data.property_name == Some(node)
            }
            NodeData::ImportClause(data) => data.name == Some(node),
            NodeData::ImportEqualsDeclaration(data) => data.name == Some(node),
            _ => false,
        })
}

/// `isExportStatementName` (:471-479).
fn is_export_statement_name(state: &CheckerState<'_>, node: NodeId) -> bool {
    state
        .parent_of(node)
        .is_some_and(|parent| match state.data_of(parent) {
            NodeData::ExportAssignment(data) => data.expression == Some(node),
            NodeData::ExportSpecifier(data) => {
                data.name == Some(node) || data.property_name == Some(node)
            }
            _ => false,
        })
}

/// `isIntrinsicJsxTag` (:481-490): the tag name of a JSX element whose
/// text is an intrinsic name (lower-case start or a dash).
fn is_intrinsic_jsx_tag(state: &CheckerState<'_>, node: NodeId) -> bool {
    if !state.is_jsx_tag_name(node) {
        return false;
    }
    let source = state.binder.source_of_node(node);
    let raw = source.arena.node(node);
    let start = tsc_syntax::skip_trivia(source.text(), raw.pos as usize);
    let text = &source.text()[start.min(raw.end as usize)..raw.end as usize];
    text.as_bytes()
        .first()
        .is_some_and(|first| first.is_ascii_lowercase())
        || text.contains('-')
}

/// `isDefaultLibraryFile` (tsbaseline/util.go:51-54), by base name.
fn is_default_library_file(base_name: &str) -> bool {
    base_name.starts_with("lib.") && base_name.ends_with(".d.ts")
}

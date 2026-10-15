//! tsgo `ls/codeactions_importfixes.go`: the import code fix provider — a
//! missing name, a UMD global used in a module, a type-only import used as
//! a value — and its fix-all, "Add all missing imports".

use tsc_checker::state::{CheckResult, CheckerState};
use tsc_diagnostics::gen;
use tsc_lsproto::{AutoImportFix, AutoImportFixKind};
use tsc_syntax::{NodeData, NodeId, SourceFile, SyntaxKind};
use tsc_types::{EscapedName, SymbolFlags, SymbolId};

use super::{CodeAction, CodeFixContext, CodeFixProvider, CombinedCodeActions};
use crate::astnav::{Found, Navigator};
use crate::autoimport::{fix_edits, symbol_to_export, Fix, ImportAdder, QueryKind};
use crate::language_service::{LanguageService, LsError};
use crate::request::Request;
use crate::tree::View;
use crate::utilities::path_key;

/// tsgo `importFixErrorCodes`.
fn import_fix_error_codes() -> Vec<u32> {
    [
        &gen::Cannot_find_name_0,
        &gen::Cannot_find_name_0_Did_you_mean_1,
        &gen::Cannot_find_name_0_Did_you_mean_the_instance_member_this_0,
        &gen::Cannot_find_name_0_Did_you_mean_the_static_member_1_0,
        &gen::Cannot_find_namespace_0,
        &gen::_0_refers_to_a_UMD_global_but_the_current_file_is_a_module_Consider_adding_an_import_instead,
        &gen::_0_only_refers_to_a_type_but_is_being_used_as_a_value_here,
        &gen::No_value_exists_in_scope_for_the_shorthand_property_0_Either_declare_one_or_provide_an_initializer,
        &gen::_0_cannot_be_used_as_a_value_because_it_was_imported_using_import_type,
        &gen::Cannot_find_name_0_Do_you_need_to_install_type_definitions_for_jQuery_Try_npm_i_save_dev_types_jquery,
        &gen::Cannot_find_name_0_Do_you_need_to_change_your_target_library_Try_changing_the_lib_compiler_option_to_1_or_later,
        &gen::Cannot_find_name_0_Do_you_need_to_change_your_target_library_Try_changing_the_lib_compiler_option_to_include_dom,
        &gen::Cannot_find_name_0_Do_you_need_to_install_type_definitions_for_a_test_runner_Try_npm_i_save_dev_types_jest_or_npm_i_save_dev_types_mocha_and_then_add_jest_or_mocha_to_the_types_field_in_your_tsconfig,
        &gen::Cannot_find_name_0_Did_you_mean_to_write_this_in_an_async_function,
        &gen::Cannot_find_name_0_Do_you_need_to_install_type_definitions_for_jQuery_Try_npm_i_save_dev_types_jquery_and_then_add_jquery_to_the_types_field_in_your_tsconfig,
        &gen::Cannot_find_name_0_Do_you_need_to_install_type_definitions_for_a_test_runner_Try_npm_i_save_dev_types_jest_or_npm_i_save_dev_types_mocha,
        &gen::Cannot_find_name_0_Do_you_need_to_install_type_definitions_for_node_Try_npm_i_save_dev_types_node,
        &gen::Cannot_find_name_0_Do_you_need_to_install_type_definitions_for_node_Try_npm_i_save_dev_types_node_and_then_add_node_to_the_types_field_in_your_tsconfig,
        &gen::Cannot_find_namespace_0_Did_you_mean_1,
        &gen::Cannot_extend_an_interface_0_Did_you_mean_implements,
        &gen::This_JSX_tag_requires_0_to_be_in_scope_but_it_could_not_be_found,
    ]
    .iter()
    .map(|message| message.code)
    .collect()
}

const IMPORT_FIX_ID: &str = "fixMissingImport";

/// tsgo `ImportFixProvider`.
pub(super) const IMPORT_FIX_PROVIDER: CodeFixProvider = CodeFixProvider {
    error_codes: import_fix_error_codes,
    get_code_actions: get_import_code_actions,
    get_all_code_actions: Some(get_all_import_code_actions),
};

/// tsgo `fixInfo`.
struct FixInfo {
    fix: Fix,
    symbol_name: String,
    is_jsx_namespace_fix: bool,
}

/// tsgo `symbolNameInfo`.
struct SymbolNameInfo {
    name: String,
    escaped: EscapedName,
    /// whether the symbol currently resolves to a type-only import
    is_type_only: bool,
}

/// tsgo `getImportCodeActions`.
fn get_import_code_actions(
    service: &mut LanguageService<'_>,
    context: &CodeFixContext<'_>,
) -> Result<Vec<CodeAction>, LsError> {
    let format_settings = service.format_options().clone();
    let diagnostic_message = context
        .diagnostic
        .map(|diagnostic| match &diagnostic.message {
            tsc_lsproto::StringOrMarkupContent::String(text) => text.clone(),
            tsc_lsproto::StringOrMarkupContent::MarkupContent(content) => content.value.clone(),
        });
    let actions =
        service.with_request_at(context.file, context.span.0 as u32, false, |request, _| {
            let infos = match get_fix_infos(
                request,
                context.file,
                context.error_code,
                context.span.0,
                diagnostic_message.as_deref(),
            )? {
                Ok(infos) => infos,
                Err(error) => return Ok(Err(error)),
            };
            let source = request.state.binder.source(context.file);
            let mut actions = Vec::new();
            for info in infos {
                let (changes, description) = fix_edits(
                    &info.fix,
                    source,
                    request.state.options,
                    format_settings.clone(),
                    request.converters,
                    request.preferences,
                    request.catalog,
                )
                .unwrap_or_else(|error| panic!("{error}"));
                actions.push(CodeAction {
                    description,
                    changes,
                    fix_id: IMPORT_FIX_ID.to_owned(),
                });
            }
            Ok(Ok(actions))
        })?;
    actions.unwrap_or(Ok(Vec::new()))
}

/// tsgo `getAllImportCodeActions`: the first fix of each of the file's
/// import diagnostics, made at once.
fn get_all_import_code_actions(
    service: &mut LanguageService<'_>,
    context: &CodeFixContext<'_>,
) -> Result<Option<CombinedCodeActions>, LsError> {
    if service
        .program
        .document(context.file)
        .is_some_and(|document| {
            tsc_program::is_encoded_dynamic_file_name(
                &document.source().file_name.to_string_lossy(),
            )
        })
    {
        return Ok(None);
    }
    let error_codes = import_fix_error_codes();
    let import_diagnostics: Vec<(u32, usize, usize)> = service
        .program
        .semantic_diagnostics(context.file)
        .map_err(|error| LsError(error.to_string()))?
        .into_iter()
        .filter(|diagnostic| super::is_fixable_diagnostic(diagnostic, &error_codes))
        .map(|diagnostic| {
            let start = diagnostic.start.unwrap_or_default() as usize;
            let end = start + diagnostic.length.unwrap_or_default() as usize;
            (diagnostic.code(), start, end)
        })
        .collect();
    if import_diagnostics.is_empty() {
        return Ok(None);
    }
    let format_settings = service.format_options().clone();
    let description = service.localize(&gen::Add_all_missing_imports, &[]);
    let edits = service.with_request_at(context.file, 0, false, |request, _| {
        let source = request.state.binder.source(context.file);
        let options = request.state.options;
        let mut fixes = Vec::new();
        for &(code, start, _) in &import_diagnostics {
            // tsgo `addImportFromDiagnostic`: the diagnostic's best fix.
            let infos = match get_fix_infos(request, context.file, code, start, None)? {
                Ok(infos) => infos,
                Err(error) => return Ok(Err(error)),
            };
            if let Some(info) = infos.into_iter().next() {
                fixes.push(info.fix.auto_import);
            }
        }
        let mut adder = ImportAdder::new(
            source,
            options,
            format_settings,
            request.converters,
            request.preferences,
        );
        for fix in &fixes {
            adder.add_import_fix(fix);
        }
        if !adder.has_fixes() {
            return Ok(Ok(None));
        }
        Ok(Ok(Some(
            adder.edits().unwrap_or_else(|error| panic!("{error}")),
        )))
    })?;
    match edits {
        Some(Ok(Some(changes))) => Ok(Some(CombinedCodeActions {
            description,
            changes,
        })),
        Some(Err(error)) => Err(error),
        _ => Ok(None),
    }
}

/// tsgo `getPreparedAutoImportView`: the view of the file's project, none
/// without a registry, an error when the registry is not prepared for the
/// file (tsgo `ErrNeedsAutoImports`).
pub(crate) fn prepared_view<'r>(
    request: &Request<'r, '_>,
    file: usize,
) -> Result<Option<crate::autoimport::View<'r>>, LsError> {
    let (Some(registry), Some(project_id)) =
        (request.host.auto_import_registry(), request.project_id)
    else {
        return Ok(None);
    };
    let file_name = request
        .state
        .binder
        .source(file)
        .file_name
        .to_string_lossy()
        .into_owned();
    if !registry.is_prepared_for_importing_file(
        &path_key(request.prepared, &file_name),
        project_id,
        request.preferences,
    ) {
        return Err(LsError::needs_auto_imports());
    }
    Ok(Some(current_view(request, file).expect("a registry")))
}

/// tsgo `getCurrentAutoImportView`: the view of the registry as it is.
fn current_view<'r>(request: &Request<'r, '_>, file: usize) -> Option<crate::autoimport::View<'r>> {
    let (Some(registry), Some(project_id)) =
        (request.host.auto_import_registry(), request.project_id)
    else {
        return None;
    };
    Some(crate::autoimport::View::new(
        registry,
        request.state,
        file,
        project_id,
        request.prepared,
        request.preferences.module_specifier_preferences(),
    ))
}

/// tsgo `getFixInfos`.
fn get_fix_infos(
    request: &mut Request<'_, '_>,
    file: usize,
    error_code: u32,
    pos: usize,
    diagnostic_message: Option<&str>,
) -> CheckResult<Result<Vec<FixInfo>, LsError>> {
    let source = request.state.binder.source(file);
    // Can't compute import fixes for dynamic/untitled files since they don't have real file paths
    if tsc_program::is_encoded_dynamic_file_name(&source.file_name.to_string_lossy()) {
        return Ok(Ok(Vec::new()));
    }
    let navigator = Navigator::new(source);
    let symbol_token = navigator.token_at(pos as u32);
    let identifier = match symbol_token {
        Found::Node(View::Node(node)) if source.arena.node(node).kind == SyntaxKind::Identifier => {
            Some(node)
        }
        _ => None,
    };
    let umd_code = gen::_0_refers_to_a_UMD_global_but_the_current_file_is_a_module_Consider_adding_an_import_instead.code;
    if error_code != umd_code && identifier.is_none() {
        return Ok(Ok(Vec::new()));
    }
    let (info, view) = if error_code == umd_code {
        let mut current = current_view(request, file);
        let info = match current.as_mut() {
            Some(current) => {
                get_fixes_info_for_umd_import(request.state, source, symbol_token, current)?
            }
            None => Vec::new(),
        };
        (info, current)
    } else if error_code
        == gen::_0_cannot_be_used_as_a_value_because_it_was_imported_using_import_type.code
    {
        let token = identifier.expect("checked above");
        let symbol_names = get_symbol_names_to_import(request.state, source, token)?;
        let mut all_type_only_fixes = Vec::new();
        for symbol_name in symbol_names {
            if !symbol_name.is_type_only {
                continue;
            }
            if let Some(fix) =
                get_type_only_promotion_fix(request.state, file, token, &symbol_name.escaped)?
            {
                all_type_only_fixes.push(FixInfo {
                    fix,
                    symbol_name: symbol_name.name,
                    is_jsx_namespace_fix: false,
                });
            }
        }
        // For JSX opening tags, there can be separate type-only errors for both the tag name
        // identifier and the JSX namespace identifier. When both produce valid fixes, we
        // disambiguate using the diagnostic message, which quotes the symbol name in single
        // quotes (e.g., "'React' cannot be used as a value..."). If filtering yields nothing
        // (e.g., due to localization), fall back to returning all candidates.
        let mut info = Vec::new();
        if all_type_only_fixes.len() > 1 {
            if let Some(message) = diagnostic_message.filter(|message| !message.is_empty()) {
                let quoted = |fix: &FixInfo| message.contains(&format!("'{}'", fix.symbol_name));
                if all_type_only_fixes.iter().any(quoted) {
                    info = all_type_only_fixes.into_iter().filter(quoted).collect();
                    return Ok(Ok(info));
                }
            }
        }
        info.extend(all_type_only_fixes);
        return Ok(Ok(info));
    } else {
        let mut prepared = match prepared_view(request, file) {
            Ok(prepared) => prepared,
            Err(error) => return Ok(Err(error)),
        };
        let token = identifier.expect("checked above");
        let info = match prepared.as_mut() {
            Some(prepared) => get_fixes_info_for_non_umd_import(request, file, token, prepared)?,
            None => Vec::new(),
        };
        (info, prepared)
    };
    // Sort fixes by preference
    let Some(view) = view.or_else(|| current_view(request, file)) else {
        return Ok(Ok(info));
    };
    Ok(Ok(sort_fix_info(info, &view)))
}

/// tsgo `getFixesInfoForUMDImport`.
fn get_fixes_info_for_umd_import(
    checker: &mut CheckerState<'_>,
    source: &SourceFile,
    token: Found,
    view: &mut crate::autoimport::View<'_>,
) -> CheckResult<Vec<FixInfo>> {
    let Some(umd_symbol) = get_umd_symbol(checker, source, token)? else {
        return Ok(Vec::new());
    };
    let path_of = |file_name: &str| view.path_of(file_name);
    let Some(export) = symbol_to_export(checker, &path_of, umd_symbol)? else {
        return Ok(Vec::new());
    };
    let token_node = match token {
        Found::Node(View::Node(node)) => Some(node),
        _ => None,
    };
    let is_valid_type_only_use_site =
        token_node.is_some_and(|node| checker.is_valid_type_only_alias_use_site(node));
    let symbol_name = checker
        .binder
        .symbol(umd_symbol)
        .escaped_name
        .unescape()
        .to_string_lossy()
        .into_owned();
    Ok(view
        .get_fixes(checker, &export, false, is_valid_type_only_use_site, None)?
        .into_iter()
        .map(|fix| FixInfo {
            fix,
            symbol_name: symbol_name.clone(),
            is_jsx_namespace_fix: false,
        })
        .collect())
}

/// tsgo `getUmdSymbol`.
fn get_umd_symbol(
    checker: &mut CheckerState<'_>,
    source: &SourceFile,
    token: Found,
) -> CheckResult<Option<SymbolId>> {
    // try the identifier to see if it is the umd symbol
    let token_node = match token {
        Found::Node(View::Node(node)) => Some(node),
        _ => None,
    };
    if let Some(identifier) =
        token_node.filter(|&node| source.arena.node(node).kind == SyntaxKind::Identifier)
    {
        let umd_symbol = checker.get_resolved_symbol(identifier)?;
        if umd_symbol.is_some_and(|symbol| is_umd_export_symbol(checker, symbol)) {
            return Ok(umd_symbol);
        }
    }
    // The error wasn't for the symbolAtLocation, it was for the JSX tag itself, which needs access to e.g. `React`.
    let navigator = Navigator::new(source);
    let Some(Found::Node(View::Node(parent))) = navigator.parent(token) else {
        return Ok(None);
    };
    let opening_like_tag = jsx_opening_like_tag_name(source, parent);
    let location = if opening_like_tag.is_some() && opening_like_tag == token_node {
        token_node.expect("a tag name")
    } else if source.arena.node(parent).kind == SyntaxKind::JsxOpeningFragment {
        parent
    } else {
        return Ok(None);
    };
    let jsx_namespace = checker.get_jsx_namespace(parent);
    let parent_symbol = checker.resolve_name(
        Some(location),
        jsx_namespace,
        SymbolFlags::VALUE,
        None,
        false,
        false,
    )?;
    Ok(parent_symbol.filter(|&symbol| is_umd_export_symbol(checker, symbol)))
}

/// tsgo `isUMDExportSymbol`: a symbol declared first by `export as
/// namespace`.
fn is_umd_export_symbol(checker: &CheckerState<'_>, symbol: SymbolId) -> bool {
    checker
        .binder
        .symbol(symbol)
        .declarations
        .first()
        .is_some_and(|&declaration| {
            checker.binder.node_record(declaration).kind == SyntaxKind::NamespaceExportDeclaration
        })
}

/// tsgo `getFixesInfoForNonUMDImport`.
fn get_fixes_info_for_non_umd_import(
    request: &mut Request<'_, '_>,
    file: usize,
    symbol_token: NodeId,
    view: &mut crate::autoimport::View<'_>,
) -> CheckResult<Vec<FixInfo>> {
    let checker = &mut *request.state;
    let source = checker.binder.source(file);
    let is_valid_type_only_use_site = checker.is_valid_type_only_alias_use_site(symbol_token);
    let symbol_names = get_symbol_names_to_import(checker, source, symbol_token)?;
    let token_text = identifier_text(source, symbol_token);
    // Compute usage position for JSDoc import type fixes
    let usage_position = request.converters.to_lsp_position(
        &crate::utilities::script_of(source),
        crate::utilities::token_start(source, symbol_token),
    );
    let mut all_info = Vec::new();
    for symbol_name in symbol_names {
        // Type-only imports are handled by the promotion code path, not the auto-import path.
        if symbol_name.is_type_only {
            continue;
        }
        let name = symbol_name.name;
        // "default" is a keyword and not a legal identifier for the import
        if name == "default" {
            continue;
        }
        let is_jsx_tag_name = name == token_text && is_jsx_tag_name(source, symbol_token);
        let query_kind = if is_jsx_tag_name {
            QueryKind::CaseInsensitiveMatch
        } else {
            QueryKind::ExactMatch
        };
        for export in view.search(&name, query_kind) {
            if is_jsx_tag_name && !(export.name() == name || export.is_renameable()) {
                continue;
            }
            let fixes = view.get_fixes(
                request.state,
                &export,
                is_jsx_tag_name,
                is_valid_type_only_use_site,
                Some(usage_position.clone()),
            )?;
            for fix in fixes {
                all_info.push(FixInfo {
                    fix,
                    symbol_name: name.clone(),
                    is_jsx_namespace_fix: name != token_text,
                });
            }
        }
    }
    Ok(all_info)
}

/// tsgo `getTypeOnlyPromotionFix`.
fn get_type_only_promotion_fix(
    checker: &mut CheckerState<'_>,
    file: usize,
    symbol_token: NodeId,
    symbol_name: &EscapedName,
) -> CheckResult<Option<Fix>> {
    // Get the symbol at the token location
    let Some(symbol) = checker.resolve_name(
        Some(symbol_token),
        symbol_name,
        SymbolFlags::VALUE,
        None,
        false,
        true,
    )?
    else {
        return Ok(None);
    };
    // Get the type-only alias declaration
    let Some(type_only_alias_declaration) = checker.get_type_only_alias_declaration(symbol)? else {
        return Ok(None);
    };
    if checker
        .binder
        .file_index_of_node(type_only_alias_declaration)
        != file
    {
        return Ok(None);
    }
    Ok(Some(Fix {
        type_only_alias_declaration: Some(type_only_alias_declaration),
        ..Fix::of(AutoImportFix {
            kind: AutoImportFixKind::PROMOTE_TYPE_ONLY,
            ..AutoImportFix::default()
        })
    }))
}

/// tsgo `getSymbolNamesToImport`.
fn get_symbol_names_to_import(
    checker: &mut CheckerState<'_>,
    source: &SourceFile,
    symbol_token: NodeId,
) -> CheckResult<Vec<SymbolNameInfo>> {
    let token_escaped = identifier_escaped_text(source, symbol_token);
    let token_text = identifier_text(source, symbol_token);
    let parent = source.arena.node(symbol_token).parent;
    let is_tag_name = parent.is_some_and(|parent| {
        jsx_opening_like_tag_name(source, parent).or_else(|| {
            match &source.arena.node(parent).data {
                NodeData::JsxClosingElement(data) => data.tag_name,
                _ => None,
            }
        }) == Some(symbol_token)
    });
    if is_tag_name && jsx_mode_needs_explicit_import(checker.options.jsx) {
        let jsx_namespace = checker.get_jsx_namespace(source.root);
        if needs_jsx_namespace_fix(checker, &jsx_namespace, symbol_token, &token_text)? {
            let mut result = Vec::new();
            if !tsc_syntax::is_intrinsic_jsx_name(&token_text) {
                let component = checker.resolve_name(
                    Some(symbol_token),
                    token_escaped,
                    SymbolFlags::VALUE,
                    None,
                    false,
                    false,
                )?;
                match component {
                    None => result.push(SymbolNameInfo {
                        name: token_text.clone(),
                        escaped: token_escaped,
                        is_type_only: false,
                    }),
                    Some(component) => {
                        if checker
                            .get_type_only_alias_declaration(component)?
                            .is_some()
                        {
                            result.push(SymbolNameInfo {
                                name: token_text.clone(),
                                escaped: token_escaped,
                                is_type_only: true,
                            });
                        }
                    }
                }
            }
            let namespace_symbol = checker.resolve_name(
                Some(symbol_token),
                jsx_namespace,
                SymbolFlags::VALUE,
                None,
                false,
                true,
            )?;
            let namespace_is_type_only = match namespace_symbol {
                Some(symbol) => checker.get_type_only_alias_declaration(symbol)?.is_some(),
                None => false,
            };
            result.push(SymbolNameInfo {
                name: jsx_namespace.unescape().to_string_lossy().into_owned(),
                escaped: jsx_namespace,
                is_type_only: namespace_is_type_only,
            });
            return Ok(result);
        }
    }
    let token_is_type_only = match checker.resolve_name(
        Some(symbol_token),
        token_escaped,
        SymbolFlags::VALUE,
        None,
        false,
        true,
    )? {
        Some(symbol) => checker.get_type_only_alias_declaration(symbol)?.is_some(),
        None => false,
    };
    Ok(vec![SymbolNameInfo {
        name: token_text,
        escaped: token_escaped,
        is_type_only: token_is_type_only,
    }])
}

/// tsgo `needsJsxNamespaceFix`.
fn needs_jsx_namespace_fix(
    checker: &mut CheckerState<'_>,
    jsx_namespace: &EscapedName,
    symbol_token: NodeId,
    token_text: &str,
) -> CheckResult<bool> {
    if tsc_syntax::is_intrinsic_jsx_name(token_text) {
        return Ok(true);
    }
    let Some(namespace_symbol) = checker.resolve_name(
        Some(symbol_token),
        jsx_namespace,
        SymbolFlags::VALUE,
        None,
        false,
        true,
    )?
    else {
        return Ok(true);
    };
    let declarations = checker.binder.symbol(namespace_symbol).declarations.clone();
    if declarations
        .iter()
        .any(|&declaration| checker.is_type_only_import_or_export_declaration(declaration))
    {
        return Ok(!checker
            .binder
            .symbol(namespace_symbol)
            .flags
            .intersects(SymbolFlags::VALUE));
    }
    Ok(false)
}

/// tsgo `jsxModeNeedsExplicitImport`: `react` or `react-native`.
fn jsx_mode_needs_explicit_import(jsx: Option<i32>) -> bool {
    matches!(jsx, Some(2 | 3))
}

/// tsgo `sortFixInfo`: JSX namespace fixes last, then the view's order.
fn sort_fix_info(mut fixes: Vec<FixInfo>, view: &crate::autoimport::View<'_>) -> Vec<FixInfo> {
    fixes.sort_by(|a, b| {
        a.is_jsx_namespace_fix
            .cmp(&b.is_jsx_namespace_fix)
            .then_with(|| view.compare_fixes_for_sorting(&a.fix, &b.fix))
    });
    fixes
}

/// The tag name of a JSX opening or self-closing element.
fn jsx_opening_like_tag_name(source: &SourceFile, node: NodeId) -> Option<NodeId> {
    match &source.arena.node(node).data {
        NodeData::JsxOpeningElement(data) => data.tag_name,
        NodeData::JsxSelfClosingElement(data) => data.tag_name,
        _ => None,
    }
}

/// tsgo `ast.IsJsxTagName`.
fn is_jsx_tag_name(source: &SourceFile, node: NodeId) -> bool {
    let Some(parent) = source.arena.node(node).parent else {
        return false;
    };
    let tag_name = match &source.arena.node(parent).data {
        NodeData::JsxOpeningElement(data) => data.tag_name,
        NodeData::JsxClosingElement(data) => data.tag_name,
        NodeData::JsxSelfClosingElement(data) => data.tag_name,
        _ => None,
    };
    tag_name == Some(node)
}

/// An identifier's text (tsgo `Text()`).
fn identifier_text(source: &SourceFile, node: NodeId) -> String {
    match &source.arena.node(node).data {
        NodeData::Identifier(data) => data.text().to_owned(),
        _ => String::new(),
    }
}

/// An identifier's name as the binder keeps it.
fn identifier_escaped_text(source: &SourceFile, node: NodeId) -> EscapedName {
    match &source.arena.node(node).data {
        NodeData::Identifier(data) => data.escaped_text,
        _ => EscapedName::from_escaped_text(tsc_types::JsStr::from_str("")),
    }
}

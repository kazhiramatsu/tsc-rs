use std::collections::BTreeSet;

use tsc_syntax::{
    for_each_child, parse_source_file, LanguageVariant, NodeData, ParseOptions, SyntaxKind,
};
use tsc_types::CompilerOptions;

use crate::prepared::PreparedSourceFile;
use crate::resolution::{ResolutionError, ResolutionKey, ResolutionMode};

const FEATURE: &str = "static-module-request-plan";

/// Plan the exact authoritative resolution keys for the static-import-only
/// program slice retained by H0.2b.
///
/// The returned order is the first reachable source occurrence of each exact
/// key. Syntax which can produce another kind of module request fails closed
/// instead of publishing an incomplete plan.
pub fn plan_static_module_requests(
    source: &PreparedSourceFile,
    options: &CompilerOptions,
) -> Result<Vec<ResolutionKey>, ResolutionError> {
    plan_module_requests_worker(source, options, false)
}

/// Plan the exact authoritative module keys for the H0 package-map program
/// slice, including static imports, export-from declarations, external
/// import-equals declarations, and literal dynamic imports.
///
/// Other request-bearing syntax remains a typed failure; this function never
/// publishes a partially discovered source plan.
pub fn plan_module_requests(
    source: &PreparedSourceFile,
    options: &CompilerOptions,
) -> Result<Vec<ResolutionKey>, ResolutionError> {
    plan_module_requests_worker(source, options, true)
}

fn plan_module_requests_worker(
    source: &PreparedSourceFile,
    options: &CompilerOptions,
    expanded: bool,
) -> Result<Vec<ResolutionKey>, ResolutionError> {
    let module_kind = options.emit_module_kind();
    if (!expanded && !(100..=199).contains(&module_kind))
        || (expanded && !matches!(module_kind, 1 | 99 | 100..=200))
    {
        return Err(unsupported(
            source,
            format!(
                "module kind {module_kind} is outside the owned CommonJS/ESNext/Node/Preserve range"
            ),
        ));
    }

    let resolution_kind = options.emit_module_resolution_kind();
    if (!expanded && !matches!(resolution_kind, 3 | 99))
        || (expanded && !matches!(resolution_kind, 3 | 99 | 100))
    {
        return Err(unsupported(
            source,
            format!("module resolution kind {resolution_kind} is not Node16, NodeNext, or Bundler"),
        ));
    }

    let file_name = source
        .path()
        .display()
        .to_str()
        .ok_or_else(|| unsupported(source, "the source display path is not valid Unicode"))?;
    let file_emit_kind = file_emit_module_kind(source, file_name, module_kind)?;
    let static_mode = static_request_mode(source, file_emit_kind)?;
    let dynamic_mode = dynamic_import_mode(source, module_kind, file_emit_kind)?;
    let javascript_file = is_javascript_file_name(file_name);
    let language_variant = if file_name.ends_with(".tsx") || javascript_file {
        LanguageVariant::Jsx
    } else {
        LanguageVariant::Standard
    };
    let parsed = parse_source_file(
        file_name.to_owned(),
        source.text().to_owned(),
        ParseOptions {
            script_target: options.emit_script_target(),
            language_variant,
            javascript_file,
            ..ParseOptions::default()
        },
        None,
    );
    if !parsed.parse_diagnostics.is_empty() {
        return Err(unsupported(
            source,
            format!(
                "the source has {} parse diagnostic(s)",
                parsed.parse_diagnostics.len()
            ),
        ));
    }

    let mut requests = Vec::new();
    let mut seen = BTreeSet::new();
    let mut stack = vec![parsed.root];
    while let Some(node_id) = stack.pop() {
        let node = parsed.arena.node(node_id);
        match &node.data {
            NodeData::ImportDeclaration(import) => {
                if import.attributes.is_some() {
                    return Err(unsupported_at(
                        source,
                        node.pos,
                        "an import declaration has attributes",
                    ));
                }
                let module_specifier = import.module_specifier.ok_or_else(|| {
                    unsupported_at(
                        source,
                        node.pos,
                        "an import declaration has no module specifier",
                    )
                })?;
                let NodeData::StringLiteral(literal) = &parsed.arena.node(module_specifier).data
                else {
                    return Err(unsupported_at(
                        source,
                        node.pos,
                        "an import declaration has a non-string module specifier",
                    ));
                };
                let key = ResolutionKey::new(
                    source.path().canonical().clone(),
                    literal.text.clone(),
                    static_mode,
                );
                if seen.insert(key.clone()) {
                    requests.push(key);
                }
            }
            NodeData::ExportDeclaration(export) if export.module_specifier.is_some() => {
                if !expanded {
                    return Err(unsupported_at(
                        source,
                        node.pos,
                        "an export declaration has a module specifier",
                    ));
                }
                if export.attributes.is_some() {
                    return Err(unsupported_at(
                        source,
                        node.pos,
                        "an export declaration has attributes",
                    ));
                }
                let module_specifier = export
                    .module_specifier
                    .expect("guarded export module specifier");
                let NodeData::StringLiteral(literal) = &parsed.arena.node(module_specifier).data
                else {
                    return Err(unsupported_at(
                        source,
                        node.pos,
                        "an export declaration has a non-string module specifier",
                    ));
                };
                let key = ResolutionKey::new(
                    source.path().canonical().clone(),
                    literal.text.clone(),
                    static_mode,
                );
                if seen.insert(key.clone()) {
                    requests.push(key);
                }
            }
            NodeData::ImportEqualsDeclaration(import_equals) => {
                if !expanded {
                    return Err(unsupported_at(
                        source,
                        node.pos,
                        "an import-equals declaration is outside the static-import slice",
                    ));
                }
                let module_reference = import_equals.module_reference.ok_or_else(|| {
                    unsupported_at(
                        source,
                        node.pos,
                        "an import-equals declaration has no module reference",
                    )
                })?;
                if let NodeData::ExternalModuleReference(reference) =
                    &parsed.arena.node(module_reference).data
                {
                    let expression = reference.expression.ok_or_else(|| {
                        unsupported_at(
                            source,
                            node.pos,
                            "an external import-equals declaration has no expression",
                        )
                    })?;
                    let NodeData::StringLiteral(literal) = &parsed.arena.node(expression).data
                    else {
                        return Err(unsupported_at(
                            source,
                            node.pos,
                            "an external import-equals declaration has a non-string specifier",
                        ));
                    };
                    let key = ResolutionKey::new(
                        source.path().canonical().clone(),
                        literal.text.clone(),
                        ResolutionMode::CommonJs,
                    );
                    if seen.insert(key.clone()) {
                        requests.push(key);
                    }
                }
                // An internal `import alias = namespace.member` declaration does
                // not issue a module-resolution request.
            }
            NodeData::ExternalModuleReference(_) => {
                // The parser only produces these as the module-reference child of
                // an import-equals declaration, which the parent arm owns above.
            }
            NodeData::ImportType(_) => {
                return Err(unsupported_at(
                    source,
                    node.pos,
                    "an import type is outside the static-import slice",
                ));
            }
            NodeData::JSDocImportTag(_) => {
                return Err(unsupported_at(
                    source,
                    node.pos,
                    "a JSDoc import is outside the static-import slice",
                ));
            }
            NodeData::ModuleDeclaration(module)
                if module.name.is_some_and(|name| {
                    parsed.arena.node(name).kind == SyntaxKind::StringLiteral
                }) =>
            {
                return Err(unsupported_at(
                    source,
                    node.pos,
                    "a string-named module declaration may require module resolution",
                ));
            }
            NodeData::CallExpression(call) => {
                let callee = call.expression.map(|id| parsed.arena.node(id));
                if callee.is_some_and(|callee| callee.kind == SyntaxKind::ImportKeyword) {
                    if !expanded {
                        return Err(unsupported_at(
                            source,
                            node.pos,
                            "a dynamic import call is outside the static-import slice",
                        ));
                    }
                    let arguments = call
                        .arguments
                        .map(|arguments| parsed.arena.node_array(arguments).nodes.as_slice())
                        .unwrap_or_default();
                    if arguments.len() != 1 {
                        return Err(unsupported_at(
                            source,
                            node.pos,
                            "a dynamic import call does not have exactly one argument",
                        ));
                    }
                    let NodeData::StringLiteral(literal) = &parsed.arena.node(arguments[0]).data
                    else {
                        return Err(unsupported_at(
                            source,
                            node.pos,
                            "a dynamic import call has a non-string argument",
                        ));
                    };
                    let key = ResolutionKey::new(
                        source.path().canonical().clone(),
                        literal.text.clone(),
                        dynamic_mode,
                    );
                    if seen.insert(key.clone()) {
                        requests.push(key);
                    }
                }
                let is_require = callee.is_some_and(|callee| {
                    matches!(
                        &callee.data,
                        NodeData::Identifier(identifier) if identifier.escaped_text == "require"
                    )
                });
                let arguments = call
                    .arguments
                    .map(|arguments| parsed.arena.node_array(arguments).nodes.as_slice())
                    .unwrap_or_default();
                if is_require
                    && arguments.len() == 1
                    && matches!(
                        parsed.arena.node(arguments[0]).kind,
                        SyntaxKind::StringLiteral | SyntaxKind::NoSubstitutionTemplateLiteral
                    )
                {
                    return Err(unsupported_at(
                        source,
                        node.pos,
                        "a require call is outside the static-import slice",
                    ));
                }
            }
            _ => {}
        }

        // JSDoc is an internal attachment rather than a for_each_child edge.
        // It is nevertheless reachable from its syntax host and can contain
        // import tags, so include those attached roots without scanning the
        // arena's speculative or abandoned nodes.
        let mut children = Vec::new();
        if let Some(js_doc) = node.js_doc {
            children.extend(parsed.arena.node_array(js_doc).nodes.iter().copied());
        }
        for_each_child(&parsed.arena, node, |child| {
            children.push(child);
            false
        });
        stack.extend(children.into_iter().rev());
    }

    Ok(requests)
}

fn is_javascript_file_name(file_name: &str) -> bool {
    [".js", ".jsx", ".mjs", ".cjs"]
        .iter()
        .any(|extension| file_name.ends_with(extension))
}

/// tsc `getEmitModuleFormatOfFileWorker` for the representation exposed by
/// `PreparedSourceFile`: an authoritative effective implied format wins, then
/// the computed `module` kind is used.
fn file_emit_module_kind(
    source: &PreparedSourceFile,
    file_name: &str,
    module_kind: i32,
) -> Result<i32, ResolutionError> {
    if let Some(mode) = source.implied_node_format_for_emit() {
        return Ok(match mode {
            ResolutionMode::CommonJs => 1,
            ResolutionMode::EsNext => 99,
            ResolutionMode::Unspecified => {
                return Err(unsupported(
                    source,
                    "the source publishes an unspecified implied Node format",
                ));
            }
        });
    }

    if (100..=199).contains(&module_kind) {
        return Err(unsupported(
            source,
            format!(
                "{file_name} has no authoritative implied Node format for module kind {module_kind}"
            ),
        ));
    }

    Ok(module_kind)
}

/// tsc `getEmitSyntaxForUsageLocationWorker` for an ordinary static
/// import/export usage after `getEmitModuleFormatOfFileWorker`.
fn static_request_mode(
    source: &PreparedSourceFile,
    file_emit_kind: i32,
) -> Result<ResolutionMode, ResolutionError> {
    match file_emit_kind {
        1 => Ok(ResolutionMode::CommonJs),
        5..=99 | 200 => Ok(ResolutionMode::EsNext),
        other => Err(unsupported(
            source,
            format!("file emit module kind {other} has no owned static resolution mode"),
        )),
    }
}

/// tsc `shouldTransformImportCallWorker`: Node and Preserve retain dynamic
/// import syntax, while other module kinds use the effective per-file emit
/// format to decide whether the call becomes CommonJS `require`.
fn dynamic_import_mode(
    source: &PreparedSourceFile,
    module_kind: i32,
    file_emit_kind: i32,
) -> Result<ResolutionMode, ResolutionError> {
    if (100..=199).contains(&module_kind) || module_kind == 200 {
        return Ok(ResolutionMode::EsNext);
    }
    match file_emit_kind {
        0..=4 => Ok(ResolutionMode::CommonJs),
        5..=200 => Ok(ResolutionMode::EsNext),
        other => Err(unsupported(
            source,
            format!("file emit module kind {other} has no owned dynamic-import resolution mode"),
        )),
    }
}

fn unsupported(source: &PreparedSourceFile, detail: impl Into<String>) -> ResolutionError {
    ResolutionError::unsupported(
        FEATURE,
        format!("{}: {}", source.path().display().display(), detail.into()),
    )
}

fn unsupported_at(
    source: &PreparedSourceFile,
    position: u32,
    detail: impl Into<String>,
) -> ResolutionError {
    unsupported(
        source,
        format!("{} at source offset {position}", detail.into()),
    )
}

use std::collections::BTreeSet;
use std::ops::Range;

use tsc_syntax::{
    for_each_child, parse_source_file, LanguageVariant, NodeData, NodeId, ParseOptions, SourceFile,
    SyntaxKind, TypeReferenceDirectiveResolutionMode,
};
use tsc_types::{CompilerOptions, NodeFlags};

use crate::prepared::PreparedSourceFile;
use crate::resolution::{
    ResolutionError, ResolutionKey, ResolutionMode, TypeReferenceResolutionKey,
};

const FEATURE: &str = "static-module-request-plan";

/// A source-owned triple-slash type-reference request and its diagnostic span.
///
/// The span is the UTF-16 range covering only the `types` attribute value,
/// matching TypeScript's `FileReference` and TS2688 contracts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedTypeReferenceDirective {
    key: TypeReferenceResolutionKey,
    pos: u32,
    end: u32,
    preserve: bool,
}

impl PlannedTypeReferenceDirective {
    pub fn key(&self) -> &TypeReferenceResolutionKey {
        &self.key
    }

    pub const fn pos(&self) -> u32 {
        self.pos
    }

    pub const fn end(&self) -> u32 {
        self.end
    }

    pub const fn length(&self) -> u32 {
        self.end - self.pos
    }

    pub fn span(&self) -> Range<u32> {
        self.pos..self.end
    }

    pub const fn preserve(&self) -> bool {
        self.preserve
    }
}

/// A source-owned triple-slash path reference and its diagnostic span.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedPathReference {
    file_name: String,
    pos: u32,
    end: u32,
    preserve: bool,
}

impl PlannedPathReference {
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    pub const fn pos(&self) -> u32 {
        self.pos
    }

    pub const fn end(&self) -> u32 {
        self.end
    }

    pub const fn length(&self) -> u32 {
        self.end - self.pos
    }

    pub fn span(&self) -> Range<u32> {
        self.pos..self.end
    }

    pub const fn preserve(&self) -> bool {
        self.preserve
    }
}

/// A source-owned triple-slash lib reference and its diagnostic span.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedLibReferenceDirective {
    file_name: String,
    pos: u32,
    end: u32,
    preserve: bool,
}

impl PlannedLibReferenceDirective {
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    pub const fn pos(&self) -> u32 {
        self.pos
    }

    pub const fn end(&self) -> u32 {
        self.end
    }

    pub const fn length(&self) -> u32 {
        self.end - self.pos
    }

    pub fn span(&self) -> Range<u32> {
        self.pos..self.end
    }

    pub const fn preserve(&self) -> bool {
        self.preserve
    }
}

/// Exact source-owned resolution requests discovered by one syntax parse.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceRequestPlan {
    path_references: Vec<PlannedPathReference>,
    module_requests: Vec<ResolutionKey>,
    loadable_module_requests: BTreeSet<ResolutionKey>,
    type_reference_directives: Vec<PlannedTypeReferenceDirective>,
    lib_reference_directives: Vec<PlannedLibReferenceDirective>,
    observed_request_occurrence_count: usize,
}

impl SourceRequestPlan {
    pub fn path_references(&self) -> &[PlannedPathReference] {
        &self.path_references
    }

    pub fn module_requests(&self) -> &[ResolutionKey] {
        &self.module_requests
    }

    pub fn module_requests_with_loadability(
        &self,
    ) -> impl ExactSizeIterator<Item = (&ResolutionKey, bool)> {
        self.module_requests
            .iter()
            .map(|key| (key, self.loadable_module_requests.contains(key)))
    }

    /// Whether a resolved target participates in program source discovery.
    /// External-module augmentations still require a resolution row but do
    /// not load their target unless another request for the same exact key
    /// does so. A key outside this plan returns `None` rather than being
    /// confused with a resolution-only request.
    pub fn module_request_loads_source(&self, key: &ResolutionKey) -> Option<bool> {
        self.module_requests
            .contains(key)
            .then(|| self.loadable_module_requests.contains(key))
    }

    pub fn type_reference_directives(&self) -> &[PlannedTypeReferenceDirective] {
        &self.type_reference_directives
    }

    pub fn lib_reference_directives(&self) -> &[PlannedLibReferenceDirective] {
        &self.lib_reference_directives
    }

    /// Count every observed path/type/lib/module occurrence before exact-key
    /// deduplication. Program loaders use this input-sensitive count for
    /// bounded edge admission; repeated imports cannot evade the ceiling.
    pub const fn observed_request_occurrence_count(&self) -> usize {
        self.observed_request_occurrence_count
    }

    fn into_module_requests(self) -> Vec<ResolutionKey> {
        self.module_requests
    }
}

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
    Ok(plan_module_requests_worker(source, options, false)?.into_module_requests())
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
    Ok(plan_source_requests(source, options)?.into_module_requests())
}

/// Plan module requests and leading triple-slash path/type/lib directives
/// from the same parse of a prepared source file.
///
/// Synthetic helpers precede top-level static imports/re-exports in statement
/// order, followed by dynamic/type/JSDoc/require requests in text order and
/// then module augmentations in source order. This matches upstream
/// `collectExternalModuleReferences`/`getModuleNames`; repeated exact keys are
/// emitted once and any loadable occurrence wins. Every type-reference
/// occurrence is retained so a cached resolution can still produce a
/// diagnostic at each directive span. Automatic JSX runtime requests remain
/// a typed unsupported boundary until their synthetic source order is owned.
pub fn plan_source_requests(
    source: &PreparedSourceFile,
    options: &CompilerOptions,
) -> Result<SourceRequestPlan, ResolutionError> {
    plan_module_requests_worker(source, options, true)
}

fn plan_module_requests_worker(
    source: &PreparedSourceFile,
    options: &CompilerOptions,
    expanded: bool,
) -> Result<SourceRequestPlan, ResolutionError> {
    let module_kind = options.emit_module_kind();
    if (!expanded && !(100..=199).contains(&module_kind))
        || (expanded && !matches!(module_kind, 1 | 5..=7 | 99 | 100..=200))
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
        || (expanded && !matches!(resolution_kind, 1 | 2 | 3 | 99 | 100))
    {
        return Err(unsupported(
            source,
            format!(
                "module resolution kind {resolution_kind} is not Classic, Node10, Node16, NodeNext, or Bundler"
            ),
        ));
    }

    let file_name = source
        .path()
        .display()
        .to_str()
        .ok_or_else(|| unsupported(source, "the source display path is not valid Unicode"))?;
    let import_syntax_affects_resolution = import_syntax_affects_module_resolution(options);
    let (static_mode, dynamic_mode) = if import_syntax_affects_resolution {
        let file_emit_kind = file_emit_module_kind(source, file_name, module_kind)?;
        (
            static_request_mode(source, file_emit_kind)?,
            dynamic_import_mode(source, module_kind, file_emit_kind)?,
        )
    } else {
        (ResolutionMode::Unspecified, ResolutionMode::Unspecified)
    };
    let javascript_file = is_javascript_file_name(file_name);
    let has_automatic_jsx_option = matches!(options.jsx, Some(4 | 5))
        || options
            .jsx_import_source
            .as_deref()
            .is_some_and(|source| !source.is_empty());
    if has_automatic_jsx_option {
        return Err(unsupported(
            source,
            "automatic JSX runtime requests are not yet owned",
        ));
    }
    let language_variant = if file_name.ends_with(".tsx") || javascript_file {
        LanguageVariant::Jsx
    } else {
        LanguageVariant::Standard
    };
    let is_declaration_file = is_declaration_file_name(file_name);
    let module_detection = options.emit_module_detection_kind();
    let force_external_module = !is_declaration_file
        && match module_detection {
            3 => true,
            2 => {
                [".cjs", ".cts", ".mjs", ".mts"]
                    .iter()
                    .any(|extension| file_name.ends_with(extension))
                    || source.implied_node_format() == Some(ResolutionMode::EsNext)
            }
            _ => false,
        };
    let detect_external_module_from_jsx =
        !is_declaration_file && module_detection == 2 && matches!(options.jsx, Some(4 | 5));
    let parsed = parse_source_file(
        file_name.to_owned(),
        source.text().to_owned(),
        ParseOptions {
            script_target: options.emit_script_target(),
            language_variant,
            javascript_file,
            force_external_module,
            detect_external_module_from_jsx,
            ..ParseOptions::default()
        },
        None,
    );
    if parsed.has_jsx_import_source_pragma || parsed.has_jsx_runtime_pragma {
        return Err(unsupported(
            source,
            "source-level JSX runtime pragmas are not yet owned",
        ));
    }
    let path_references: Vec<PlannedPathReference> = parsed
        .referenced_files
        .iter()
        .map(|reference| PlannedPathReference {
            file_name: reference.file_name.clone(),
            pos: reference.pos,
            end: reference.end,
            preserve: reference.preserve,
        })
        .collect();
    let lib_reference_directives: Vec<PlannedLibReferenceDirective> = parsed
        .lib_reference_directives
        .iter()
        .map(|reference| PlannedLibReferenceDirective {
            file_name: reference.file_name.clone(),
            pos: reference.pos,
            end: reference.end,
            preserve: reference.preserve,
        })
        .collect();

    let mut type_reference_directives = Vec::new();
    for directive in &parsed.type_reference_directives {
        let mode = match directive.resolution_mode {
            Some(TypeReferenceDirectiveResolutionMode::Import) => ResolutionMode::EsNext,
            Some(TypeReferenceDirectiveResolutionMode::Require) => ResolutionMode::CommonJs,
            None if import_syntax_affects_module_resolution(options) => source
                .implied_node_format_for_emit()
                .unwrap_or(ResolutionMode::Unspecified),
            None => ResolutionMode::Unspecified,
        };
        let key = TypeReferenceResolutionKey::source(
            source.path().canonical().clone(),
            directive.file_name.clone(),
            mode,
        );
        type_reference_directives.push(PlannedTypeReferenceDirective {
            key,
            pos: directive.pos,
            end: directive.end,
            preserve: directive.preserve,
        });
    }

    // collectExternalModuleReferences publishes ordinary imports first and
    // appends module augmentations afterwards, independently of source
    // position. createProgram uses that boundary to resolve augmentations
    // without loading their target source.
    let mut static_occurrences = Vec::new();
    let mut dynamic_occurrences = Vec::new();
    let mut augmentation_occurrences = Vec::new();
    let mut stack = vec![parsed.root];
    while let Some(node_id) = stack.pop() {
        let node = parsed.arena.node(node_id);
        match &node.data {
            NodeData::ImportDeclaration(import) => {
                let mode = match import.attributes {
                    None => static_mode,
                    Some(attributes) if expanded => {
                        let is_type_only = import.import_clause.is_some_and(|clause| {
                            matches!(
                                &parsed.arena.node(clause).data,
                                NodeData::ImportClause(clause) if clause.is_type_only
                            )
                        });
                        if !is_type_only {
                            return Err(unsupported_at(
                                source,
                                node.pos,
                                "a non-type-only import declaration has attributes",
                            ));
                        }
                        resolution_mode_override(&parsed, attributes).ok_or_else(|| {
                            unsupported_at(
                                source,
                                node.pos,
                                "a type-only import declaration has unsupported attributes",
                            )
                        })?
                    }
                    Some(_) => {
                        return Err(unsupported_at(
                            source,
                            node.pos,
                            "an import declaration has attributes",
                        ));
                    }
                };
                let module_specifier = import.module_specifier.ok_or_else(|| {
                    unsupported_at(
                        source,
                        node.pos,
                        "an import declaration has no module specifier",
                    )
                })?;
                let module_specifier = parsed.arena.node(module_specifier);
                let NodeData::StringLiteral(literal) = &module_specifier.data else {
                    return Err(unsupported_at(
                        source,
                        node.pos,
                        "an import declaration has a non-string module specifier",
                    ));
                };
                if !literal.text.is_empty() {
                    static_occurrences.push(ModuleRequestOccurrence {
                        pos: module_specifier.pos,
                        loads_source: true,
                        key: ResolutionKey::new(
                            source.path().canonical().clone(),
                            literal.text.clone(),
                            mode,
                        ),
                    });
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
                let module_specifier = parsed.arena.node(module_specifier);
                let NodeData::StringLiteral(literal) = &module_specifier.data else {
                    return Err(unsupported_at(
                        source,
                        node.pos,
                        "an export declaration has a non-string module specifier",
                    ));
                };
                if !literal.text.is_empty() {
                    static_occurrences.push(ModuleRequestOccurrence {
                        pos: module_specifier.pos,
                        loads_source: true,
                        key: ResolutionKey::new(
                            source.path().canonical().clone(),
                            literal.text.clone(),
                            static_mode,
                        ),
                    });
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
                    let expression = parsed.arena.node(expression);
                    let NodeData::StringLiteral(literal) = &expression.data else {
                        return Err(unsupported_at(
                            source,
                            node.pos,
                            "an external import-equals declaration has a non-string specifier",
                        ));
                    };
                    if !literal.text.is_empty() {
                        static_occurrences.push(ModuleRequestOccurrence {
                            pos: expression.pos,
                            loads_source: true,
                            key: ResolutionKey::new(
                                source.path().canonical().clone(),
                                literal.text.clone(),
                                if import_syntax_affects_resolution {
                                    ResolutionMode::CommonJs
                                } else {
                                    ResolutionMode::Unspecified
                                },
                            ),
                        });
                    }
                }
                // An internal `import alias = namespace.member` declaration does
                // not issue a module-resolution request.
            }
            NodeData::ExternalModuleReference(_) => {
                // The parser only produces these as the module-reference child of
                // an import-equals declaration, which the parent arm owns above.
            }
            NodeData::ImportType(import_type) => {
                if !expanded {
                    return Err(unsupported_at(
                        source,
                        node.pos,
                        "an import type is outside the static-import slice",
                    ));
                }
                let argument = import_type.argument.ok_or_else(|| {
                    unsupported_at(source, node.pos, "an import type has no argument")
                })?;
                let NodeData::LiteralType(argument) = &parsed.arena.node(argument).data else {
                    return Err(unsupported_at(
                        source,
                        node.pos,
                        "an import type has a non-literal argument",
                    ));
                };
                let literal = argument.literal.ok_or_else(|| {
                    unsupported_at(source, node.pos, "an import type literal has no value")
                })?;
                let literal = parsed.arena.node(literal);
                let NodeData::StringLiteral(literal_data) = &literal.data else {
                    return Err(unsupported_at(
                        source,
                        node.pos,
                        "an import type has a non-string argument",
                    ));
                };
                let mode = match import_type.attributes {
                    None => static_mode,
                    Some(attributes) => {
                        resolution_mode_override(&parsed, attributes).ok_or_else(|| {
                            unsupported_at(
                                source,
                                node.pos,
                                "an import type has unsupported attributes",
                            )
                        })?
                    }
                };
                dynamic_occurrences.push(ModuleRequestOccurrence {
                    pos: literal.pos,
                    loads_source: javascript_file
                        || !NodeFlags::from_bits(node.flags).contains(NodeFlags::JS_DOC),
                    key: ResolutionKey::new(
                        source.path().canonical().clone(),
                        literal_data.text.clone(),
                        mode,
                    ),
                });
            }
            NodeData::JSDocImportTag(import) => {
                if !expanded {
                    return Err(unsupported_at(
                        source,
                        node.pos,
                        "a JSDoc import is outside the static-import slice",
                    ));
                }
                let module_specifier = import.module_specifier.ok_or_else(|| {
                    unsupported_at(source, node.pos, "a JSDoc import has no module specifier")
                })?;
                let module_specifier = parsed.arena.node(module_specifier);
                let NodeData::StringLiteral(literal) = &module_specifier.data else {
                    return Err(unsupported_at(
                        source,
                        node.pos,
                        "a JSDoc import has a non-string module specifier",
                    ));
                };
                let mode = import
                    .attributes
                    .and_then(|attributes| resolution_mode_override(&parsed, attributes))
                    .unwrap_or(static_mode);
                if !literal.text.is_empty() {
                    dynamic_occurrences.push(ModuleRequestOccurrence {
                        pos: module_specifier.pos,
                        loads_source: javascript_file,
                        key: ResolutionKey::new(
                            source.path().canonical().clone(),
                            literal.text.clone(),
                            mode,
                        ),
                    });
                }
            }
            NodeData::ModuleDeclaration(module)
                if module.name.is_some_and(|name| {
                    parsed.arena.node(name).kind == SyntaxKind::StringLiteral
                }) =>
            {
                let name = module
                    .name
                    .expect("guarded string-named module declaration");
                let NodeData::StringLiteral(literal) = &parsed.arena.node(name).data else {
                    unreachable!("guarded string-named module declaration")
                };
                let top_level = node.parent == Some(parsed.root);
                if !top_level {
                    return Err(unsupported_at(
                        source,
                        node.pos,
                        "a nested string-named module declaration is outside the owned augmentation surface",
                    ));
                }
                let has_declare_modifier = module.modifiers.is_some_and(|modifiers| {
                    parsed
                        .arena
                        .node_array(modifiers)
                        .nodes
                        .iter()
                        .any(|&modifier| {
                            parsed.arena.node(modifier).kind == SyntaxKind::DeclareKeyword
                        })
                });
                let ambient_syntax = has_declare_modifier || parsed.is_declaration_file;
                if let Some((position, detail)) =
                    module_body_request_syntax(&parsed, node_id, module.body, javascript_file)
                {
                    return Err(unsupported_at(source, position, detail));
                }
                if ambient_syntax && parsed.external_module_indicator.is_some() {
                    if !expanded {
                        return Err(unsupported_at(
                            source,
                            node.pos,
                            "a module augmentation is outside the static-import slice",
                        ));
                    }
                    augmentation_occurrences.push(ModuleRequestOccurrence {
                        pos: parsed.arena.node(name).pos,
                        loads_source: false,
                        key: ResolutionKey::new(
                            source.path().canonical().clone(),
                            literal.text.clone(),
                            static_mode,
                        ),
                    });
                }
                // A script ambient declaration introduces an external module,
                // while a bare `module "name"` is not ambient syntax. Neither
                // asks the host to resolve that name.
                // collectModuleReferences owns the declaration as a boundary:
                // its body is not part of the source-level traversal.
                continue;
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
                    let argument = parsed.arena.node(arguments[0]);
                    let NodeData::StringLiteral(literal) = &argument.data else {
                        return Err(unsupported_at(
                            source,
                            node.pos,
                            "a dynamic import call has a non-string argument",
                        ));
                    };
                    dynamic_occurrences.push(ModuleRequestOccurrence {
                        pos: argument.pos,
                        loads_source: true,
                        key: ResolutionKey::new(
                            source.path().canonical().clone(),
                            literal.text.clone(),
                            dynamic_mode,
                        ),
                    });
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
                    if !expanded {
                        return Err(unsupported_at(
                            source,
                            node.pos,
                            "a require call is outside the static-import slice",
                        ));
                    }
                    if !javascript_file {
                        return Err(unsupported_at(
                            source,
                            node.pos,
                            "a TypeScript require call is outside the owned JavaScript request surface",
                        ));
                    }
                    // collectExternalModuleReferences only treats require()
                    // as a module request in JavaScript files. Node/Bundler
                    // records CommonJS even when ordinary static imports emit
                    // as ESM; Classic/Node10 retain upstream's undefined mode.
                    let argument = arguments[0];
                    let specifier = string_literal_like_text(&parsed, argument)
                        .expect("guarded string-literal-like require argument");
                    dynamic_occurrences.push(ModuleRequestOccurrence {
                        pos: parsed.arena.node(argument).pos,
                        loads_source: true,
                        key: ResolutionKey::new(
                            source.path().canonical().clone(),
                            specifier.to_owned(),
                            if import_syntax_affects_resolution {
                                ResolutionMode::CommonJs
                            } else {
                                ResolutionMode::Unspecified
                            },
                        ),
                    });
                }
            }
            _ => {}
        }

        // JSDoc is an internal attachment rather than a for_each_child edge.
        // collectExternalModuleReferences descends into those attachments
        // only for JavaScript sources.
        let mut children = Vec::new();
        if javascript_file {
            if let Some(js_doc) = node.js_doc {
                children.extend(parsed.arena.node_array(js_doc).nodes.iter().copied());
            }
        }
        for_each_child(&parsed.arena, node, |child| {
            children.push(child);
            false
        });
        stack.extend(children.into_iter().rev());
    }

    static_occurrences.sort_by_key(|occurrence| occurrence.pos);
    dynamic_occurrences.sort_by_key(|occurrence| occurrence.pos);
    augmentation_occurrences.sort_by_key(|occurrence| occurrence.pos);
    let mut module_requests = Vec::new();
    let mut loadable_module_requests = BTreeSet::new();
    let mut seen_module_requests = BTreeSet::new();
    // tsc collectExternalModuleReferences prepends a synthesized `tslib`
    // import when importHelpers can participate in this source. The checker
    // later resolves that exact synthetic specifier while validating the
    // requested helper shape, so the authoritative table must contain the
    // row even though no source-text module literal exists.
    let computed_isolated_modules =
        options.isolated_modules == Some(true) || options.verbatim_module_syntax == Some(true);
    let has_synthetic_tslib = options.import_helpers == Some(true)
        && (javascript_file
            || (!parsed.is_declaration_file
                && (computed_isolated_modules || parsed.external_module_indicator.is_some())));
    let observed_request_occurrence_count = path_references
        .len()
        .saturating_add(type_reference_directives.len())
        .saturating_add(lib_reference_directives.len())
        .saturating_add(static_occurrences.len())
        .saturating_add(dynamic_occurrences.len())
        .saturating_add(augmentation_occurrences.len())
        .saturating_add(usize::from(has_synthetic_tslib));
    if has_synthetic_tslib {
        let key = ResolutionKey::new(source.path().canonical().clone(), "tslib", static_mode);
        seen_module_requests.insert(key.clone());
        loadable_module_requests.insert(key.clone());
        module_requests.push(key);
    }
    for occurrence in static_occurrences
        .into_iter()
        .chain(dynamic_occurrences)
        .chain(augmentation_occurrences)
    {
        if occurrence.loads_source {
            loadable_module_requests.insert(occurrence.key.clone());
        }
        if seen_module_requests.insert(occurrence.key.clone()) {
            module_requests.push(occurrence.key);
        }
    }

    Ok(SourceRequestPlan {
        path_references,
        module_requests,
        loadable_module_requests,
        type_reference_directives,
        lib_reference_directives,
        observed_request_occurrence_count,
    })
}

/// `isDeclarationFileName` includes arbitrary-extension declaration twins
/// such as `style.d.css.ts`, in addition to the three standard spellings.
pub(crate) fn is_declaration_file_name(file_name: &str) -> bool {
    if file_name.ends_with(".d.ts")
        || file_name.ends_with(".d.cts")
        || file_name.ends_with(".d.mts")
    {
        return true;
    }
    let base_name = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    base_name.ends_with(".ts") && base_name.contains(".d.")
}

struct ModuleRequestOccurrence {
    pos: u32,
    loads_source: bool,
    key: ResolutionKey,
}

/// A string-named module declaration is a collectModuleReferences boundary.
/// This slice does not model the upstream `inAmbientModule` traversal rules,
/// so request-bearing syntax under that boundary must fail closed rather than
/// leaking into the source-level request list or being silently omitted.
fn module_body_request_syntax(
    parsed: &SourceFile,
    declaration: NodeId,
    body: Option<NodeId>,
    javascript_file: bool,
) -> Option<(u32, &'static str)> {
    let mut stack = body.into_iter().collect::<Vec<_>>();
    if javascript_file {
        if let Some(js_doc) = parsed.arena.node(declaration).js_doc {
            stack.extend(parsed.arena.node_array(js_doc).nodes.iter().copied());
        }
    }
    while let Some(node_id) = stack.pop() {
        let node = parsed.arena.node(node_id);
        let detail = match &node.data {
            NodeData::ImportDeclaration(_) => {
                Some("an import declaration appears inside a string-named module")
            }
            NodeData::ExportDeclaration(export) if export.module_specifier.is_some() => {
                Some("an export-from declaration appears inside a string-named module")
            }
            NodeData::ImportEqualsDeclaration(_) => {
                Some("an import-equals declaration appears inside a string-named module")
            }
            NodeData::ImportType(_) => Some("an import type appears inside a string-named module"),
            NodeData::JSDocImportTag(_) => {
                Some("a JSDoc import appears inside a string-named module")
            }
            NodeData::ModuleDeclaration(module)
                if module.name.is_some_and(|name| {
                    parsed.arena.node(name).kind == SyntaxKind::StringLiteral
                }) =>
            {
                Some("a nested string-named module requires ambient-context tracking")
            }
            NodeData::CallExpression(call) => {
                let callee = call.expression.map(|id| parsed.arena.node(id));
                let dynamic_import =
                    callee.is_some_and(|callee| callee.kind == SyntaxKind::ImportKeyword);
                let literal_require = javascript_file
                    && callee.is_some_and(|callee| {
                        matches!(
                            &callee.data,
                            NodeData::Identifier(identifier)
                                if identifier.escaped_text == "require"
                        )
                    })
                    && call.arguments.is_some_and(|arguments| {
                        let arguments = &parsed.arena.node_array(arguments).nodes;
                        arguments.len() == 1
                            && matches!(
                                parsed.arena.node(arguments[0]).kind,
                                SyntaxKind::StringLiteral
                                    | SyntaxKind::NoSubstitutionTemplateLiteral
                            )
                    });
                (dynamic_import || literal_require)
                    .then_some("a dynamic module request appears inside a string-named module")
            }
            _ => None,
        };
        if let Some(detail) = detail {
            return Some((node.pos, detail));
        }

        if javascript_file {
            if let Some(js_doc) = node.js_doc {
                stack.extend(parsed.arena.node_array(js_doc).nodes.iter().copied());
            }
        }
        let mut children = Vec::new();
        for_each_child(&parsed.arena, node, |child| {
            children.push(child);
            false
        });
        stack.extend(children.into_iter().rev());
    }
    None
}

/// tsc `getResolutionModeOverride`: only the exact one-element
/// `"resolution-mode": "import" | "require"` shape overrides the fallback.
fn resolution_mode_override(parsed: &SourceFile, attributes: NodeId) -> Option<ResolutionMode> {
    let NodeData::ImportAttributes(attributes) = &parsed.arena.node(attributes).data else {
        return None;
    };
    let elements = attributes.elements?;
    let elements = &parsed.arena.node_array(elements).nodes;
    if elements.len() != 1 {
        return None;
    }
    let NodeData::ImportAttribute(attribute) = &parsed.arena.node(elements[0]).data else {
        return None;
    };
    if string_literal_like_text(parsed, attribute.name?)? != "resolution-mode" {
        return None;
    }
    match string_literal_like_text(parsed, attribute.value?)? {
        "import" => Some(ResolutionMode::EsNext),
        "require" => Some(ResolutionMode::CommonJs),
        _ => None,
    }
}

fn string_literal_like_text(parsed: &SourceFile, node: NodeId) -> Option<&str> {
    match &parsed.arena.node(node).data {
        NodeData::StringLiteral(literal) => Some(&literal.text),
        NodeData::NoSubstitutionTemplateLiteral(literal) => Some(&literal.text),
        _ => None,
    }
}

/// tsc `importSyntaxAffectsModuleResolution`, including the computed package
/// map defaults for Node16, NodeNext, and Bundler resolution.
fn import_syntax_affects_module_resolution(options: &CompilerOptions) -> bool {
    let module_resolution = options.emit_module_resolution_kind();
    (3..=99).contains(&module_resolution)
        || (matches!(module_resolution, 3 | 99 | 100)
            && (options.resolve_package_json_exports != Some(false)
                || options.resolve_package_json_imports != Some(false)))
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

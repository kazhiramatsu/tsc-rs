use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::sync::Arc;

use tsc_syntax::{
    for_each_child, parse_source_file_from_snapshot, skip_trivia, LanguageVariant, NodeData,
    NodeId, ParseOptions, SourceFile, SyntaxKind, TypeReferenceDirectiveResolutionMode,
};
use tsc_types::{CompilerOptions, NodeFlags};

use crate::module_resolution::is_external_module_name_relative;
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
    unpreprocessed_module_requests: BTreeSet<ResolutionKey>,
    loadable_module_requests: BTreeSet<ResolutionKey>,
    module_request_spans: BTreeMap<ResolutionKey, (u32, u32)>,
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

    /// Static module references which remain reachable from checker syntax,
    /// but are deliberately excluded from TypeScript's source-file import
    /// preprocessing. The prepared-program provider returns authoritative
    /// `NotFound` for these keys without asking the module-resolution host.
    /// This is observable for imports and re-exports in a module augmentation
    /// body.
    pub fn unpreprocessed_module_requests(&self) -> impl ExactSizeIterator<Item = &ResolutionKey> {
        self.unpreprocessed_module_requests.iter()
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

    /// The first source span for a module request, including its string
    /// literal delimiters.  Program construction uses this only for
    /// file-preprocessing inclusion diagnostics; synthetic requests have no
    /// source span.
    pub fn module_request_span(&self, key: &ResolutionKey) -> Option<(u32, u32)> {
        self.module_request_spans.get(key).copied()
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
/// diagnostic at each directive span. Synthetic `tslib` and JSX runtime
/// imports use the same leading order as TypeScript.
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
        || (expanded && !matches!(module_kind, 0..=7 | 99 | 100..=200))
    {
        return Err(unsupported(
            source,
            format!(
                "module kind {module_kind} is outside the owned None/CommonJS/AMD/UMD/System/ESNext/Node/Preserve range"
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
    let parsed = parse_source_file_from_snapshot(
        file_name.to_owned(),
        Arc::clone(source.snapshot()),
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
    let mut unpreprocessed_module_requests = BTreeSet::new();
    collect_static_module_references(
        &parsed,
        source,
        expanded,
        static_mode,
        import_syntax_affects_resolution,
        &mut static_occurrences,
        &mut augmentation_occurrences,
        &mut unpreprocessed_module_requests,
    )?;

    // `forEachDynamicImportOrRequireCall` is a separate whole-file walk in
    // tsc. In particular, module-declaration boundaries affect the static
    // collector above but do not hide import types or dynamic import calls.
    let mut contains_jsx = false;
    let mut stack = vec![parsed.root];
    while let Some(node_id) = stack.pop() {
        let node = parsed.arena.node(node_id);
        contains_jsx |= matches!(
            &node.data,
            NodeData::JsxElement(_)
                | NodeData::JsxFragment(_)
                | NodeData::JsxOpeningElement(_)
                | NodeData::JsxSelfClosingElement(_)
        );
        match &node.data {
            NodeData::ImportType(import_type) => {
                if !expanded {
                    return Err(unsupported_at(
                        source,
                        node.pos,
                        "an import type is outside the static-import slice",
                    ));
                }
                if let Some(argument) = import_type.argument {
                    if let NodeData::LiteralType(argument) = &parsed.arena.node(argument).data {
                        if let Some(literal) = argument.literal {
                            let literal = parsed.arena.node(literal);
                            if let NodeData::StringLiteral(literal_data) = &literal.data {
                                let mode_override = import_type.attributes.and_then(|attributes| {
                                    resolution_mode_override(&parsed, attributes)
                                });
                                let mode = mode_override.unwrap_or(static_mode);
                                dynamic_occurrences.push(ModuleRequestOccurrence {
                                    pos: literal.pos,
                                    end: literal.end,
                                    loads_source: javascript_file
                                        || !NodeFlags::from_bits(node.flags)
                                            .contains(NodeFlags::JS_DOC),
                                    key: ResolutionKey::new(
                                        source.path().canonical().clone(),
                                        literal_data.text.clone(),
                                        mode,
                                    ),
                                });
                                // Invalid type-import attributes still reach
                                // checker resolution during recovery. The
                                // checker may probe either Node16/NodeNext
                                // branch while reporting the grammar error,
                                // so retain both mode rows when the explicit
                                // override was not valid. This keeps the
                                // authoritative table complete without
                                // changing valid overrides or legacy modes.
                                if import_type.attributes.is_some()
                                    && mode_override.is_none()
                                    && import_syntax_affects_resolution
                                {
                                    for alternate in
                                        [ResolutionMode::CommonJs, ResolutionMode::EsNext]
                                    {
                                        if alternate != mode {
                                            dynamic_occurrences.push(ModuleRequestOccurrence {
                                                pos: literal.pos,
                                                end: literal.end,
                                                loads_source: javascript_file
                                                    || !NodeFlags::from_bits(node.flags)
                                                        .contains(NodeFlags::JS_DOC),
                                                key: ResolutionKey::new(
                                                    source.path().canonical().clone(),
                                                    literal_data.text.clone(),
                                                    alternate,
                                                ),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            NodeData::JSDocImportTag(import) => {
                if !expanded {
                    return Err(unsupported_at(
                        source,
                        node.pos,
                        "a JSDoc import is outside the static-import slice",
                    ));
                }
                if let Some(module_specifier) = import.module_specifier {
                    let module_specifier = parsed.arena.node(module_specifier);
                    if let NodeData::StringLiteral(literal) = &module_specifier.data {
                        let mode = import
                            .attributes
                            .and_then(|attributes| resolution_mode_override(&parsed, attributes))
                            .unwrap_or(static_mode);
                        if !literal.text.is_empty() {
                            dynamic_occurrences.push(ModuleRequestOccurrence {
                                pos: module_specifier.pos,
                                end: module_specifier.end,
                                loads_source: javascript_file,
                                key: ResolutionKey::new(
                                    source.path().canonical().clone(),
                                    literal.text.clone(),
                                    mode,
                                ),
                            });
                        }
                    }
                }
            }
            NodeData::CallExpression(call) => {
                let callee = call.expression;
                if callee.is_some_and(|callee| is_import_call_callee(&parsed, callee)) {
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
                    // TypeScript's collector records the first string-literal
                    // argument whenever at least one argument exists. The
                    // optional second argument carries import attributes and
                    // does not suppress the host resolution request.
                    if !arguments.is_empty() {
                        let argument = parsed.arena.node(arguments[0]);
                        if let NodeData::StringLiteral(literal) = &argument.data {
                            dynamic_occurrences.push(ModuleRequestOccurrence {
                                pos: argument.pos,
                                end: argument.end,
                                loads_source: true,
                                key: ResolutionKey::new(
                                    source.path().canonical().clone(),
                                    literal.text.clone(),
                                    dynamic_mode,
                                ),
                            });
                        }
                    }
                }
                let is_require = callee.is_some_and(|callee| {
                    matches!(
                        &parsed.arena.node(callee).data,
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
                    if javascript_file {
                        // collectExternalModuleReferences only treats
                        // require() as a module request in JavaScript files.
                        // Node/Bundler records CommonJS even when ordinary
                        // static imports emit as ESM; Classic/Node10 retain
                        // upstream's undefined mode.
                        let argument = arguments[0];
                        let specifier = string_literal_like_text(&parsed, argument)
                            .expect("guarded string-literal-like require argument");
                        dynamic_occurrences.push(ModuleRequestOccurrence {
                            pos: parsed.arena.node(argument).pos,
                            end: parsed.arena.node(argument).end,
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
    let mut module_request_spans = BTreeMap::new();
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
    // `collectExternalModuleReferences` inserts the JSX runtime import next to
    // the synthetic `tslib` import. The checker addresses it by this stable
    // source-file import index, so it must participate in the authoritative
    // resolution table even though no source-text literal exists.
    let jsx_runtime_import = jsx_runtime_import_specifier(&parsed, options);
    let has_synthetic_jsx_runtime = jsx_runtime_import.is_some()
        && !parsed.is_declaration_file
        && (javascript_file
            || contains_jsx
            || computed_isolated_modules
            || parsed.external_module_indicator.is_some());
    let observed_request_occurrence_count = path_references
        .len()
        .saturating_add(type_reference_directives.len())
        .saturating_add(lib_reference_directives.len())
        .saturating_add(static_occurrences.len())
        .saturating_add(dynamic_occurrences.len())
        .saturating_add(augmentation_occurrences.len())
        .saturating_add(usize::from(has_synthetic_tslib))
        .saturating_add(usize::from(has_synthetic_jsx_runtime));
    if has_synthetic_tslib {
        let key = ResolutionKey::new(source.path().canonical().clone(), "tslib", static_mode);
        seen_module_requests.insert(key.clone());
        loadable_module_requests.insert(key.clone());
        module_requests.push(key);
    }
    if has_synthetic_jsx_runtime {
        let key = ResolutionKey::new(
            source.path().canonical().clone(),
            jsx_runtime_import.expect("guarded JSX runtime synthetic import"),
            static_mode,
        );
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
        let source_text = source.text();
        let span_start =
            skip_trivia(source_text, occurrence.pos as usize).min(occurrence.end as usize);
        let span = (
            byte_to_utf16_offset(source_text, span_start),
            byte_to_utf16_offset(source_text, occurrence.end as usize),
        );
        module_request_spans
            .entry(occurrence.key.clone())
            .or_insert(span);
        if seen_module_requests.insert(occurrence.key.clone()) {
            module_requests.push(occurrence.key);
        }
    }
    // The source-file resolution cache is keyed by exact specifier and mode.
    // If another preprocessed occurrence owns the same key, checker lookups in
    // an augmentation body observe that ordinary authoritative row.
    unpreprocessed_module_requests.retain(|key| !seen_module_requests.contains(key));

    Ok(SourceRequestPlan {
        path_references,
        module_requests,
        unpreprocessed_module_requests,
        loadable_module_requests,
        module_request_spans,
        type_reference_directives,
        lib_reference_directives,
        observed_request_occurrence_count,
    })
}

fn byte_to_utf16_offset(text: &str, byte_offset: usize) -> u32 {
    text.get(..byte_offset.min(text.len()))
        .unwrap_or(text)
        .encode_utf16()
        .count() as u32
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
    end: u32,
    loads_source: bool,
    key: ResolutionKey,
}

/// `collectExternalModuleReferences`' statement-only static collector.
///
/// Unlike a generic syntax walk, this enters only a top-level ambient module
/// in a global script. External augmentations and nested non-relative ambient
/// declarations are terminal module-augmentation entries. Their bodies are
/// retained separately as checker-visible, host-unpreprocessed requests.
fn collect_static_module_references(
    parsed: &SourceFile,
    source: &PreparedSourceFile,
    expanded: bool,
    static_mode: ResolutionMode,
    import_syntax_affects_resolution: bool,
    static_occurrences: &mut Vec<ModuleRequestOccurrence>,
    augmentation_occurrences: &mut Vec<ModuleRequestOccurrence>,
    unpreprocessed_module_requests: &mut BTreeSet<ResolutionKey>,
) -> Result<(), ResolutionError> {
    let NodeData::SourceFile(root) = &parsed.arena.node(parsed.root).data else {
        return Err(unsupported(source, "the parsed root is not a source file"));
    };
    let statements = root
        .statements
        .map(|statements| parsed.arena.node_array(statements).nodes.as_slice())
        .unwrap_or_default();
    collect_static_module_reference_statements(
        parsed,
        statements,
        source,
        expanded,
        static_mode,
        import_syntax_affects_resolution,
        /*in_ambient_module*/ false,
        static_occurrences,
        augmentation_occurrences,
        unpreprocessed_module_requests,
    )
}

#[allow(clippy::too_many_arguments)]
fn collect_static_module_reference_statements(
    parsed: &SourceFile,
    statements: &[NodeId],
    source: &PreparedSourceFile,
    expanded: bool,
    static_mode: ResolutionMode,
    import_syntax_affects_resolution: bool,
    in_ambient_module: bool,
    static_occurrences: &mut Vec<ModuleRequestOccurrence>,
    augmentation_occurrences: &mut Vec<ModuleRequestOccurrence>,
    unpreprocessed_module_requests: &mut BTreeSet<ResolutionKey>,
) -> Result<(), ResolutionError> {
    for &node_id in statements {
        let node = parsed.arena.node(node_id);
        match &node.data {
            NodeData::ImportDeclaration(import) => {
                if let Some(module_specifier) = import.module_specifier {
                    let module_specifier = parsed.arena.node(module_specifier);
                    if let NodeData::StringLiteral(literal) = &module_specifier.data {
                        if !literal.text.is_empty()
                            && (!in_ambient_module
                                || !is_external_module_name_relative(&literal.text))
                        {
                            let mode = import
                                .attributes
                                .filter(|_| expanded)
                                .and_then(|attributes| {
                                    import.import_clause.filter(|&clause| {
                                        matches!(
                                            &parsed.arena.node(clause).data,
                                            NodeData::ImportClause(clause) if clause.is_type_only
                                        )
                                    })?;
                                    resolution_mode_override(parsed, attributes)
                                })
                                .unwrap_or(static_mode);
                            static_occurrences.push(ModuleRequestOccurrence {
                                pos: module_specifier.pos,
                                end: module_specifier.end,
                                loads_source: true,
                                key: ResolutionKey::new(
                                    source.path().canonical().clone(),
                                    literal.text.clone(),
                                    mode,
                                ),
                            });
                        }
                    }
                }
                continue;
            }
            NodeData::ExportDeclaration(export) if export.module_specifier.is_some() => {
                if !expanded && !in_ambient_module {
                    return Err(unsupported_at(
                        source,
                        node.pos,
                        "an export declaration has a module specifier",
                    ));
                }
                let module_specifier = export
                    .module_specifier
                    .expect("guarded ambient export module specifier");
                let module_specifier = parsed.arena.node(module_specifier);
                if let NodeData::StringLiteral(literal) = &module_specifier.data {
                    if !literal.text.is_empty()
                        && (!in_ambient_module || !is_external_module_name_relative(&literal.text))
                    {
                        static_occurrences.push(ModuleRequestOccurrence {
                            pos: module_specifier.pos,
                            end: module_specifier.end,
                            loads_source: true,
                            key: ResolutionKey::new(
                                source.path().canonical().clone(),
                                literal.text.clone(),
                                static_mode,
                            ),
                        });
                    }
                }
                continue;
            }
            NodeData::ImportEqualsDeclaration(import_equals) => {
                if !expanded && !in_ambient_module {
                    return Err(unsupported_at(
                        source,
                        node.pos,
                        "an import-equals declaration is outside the static-import slice",
                    ));
                }
                if let Some(module_reference) = import_equals.module_reference {
                    if let NodeData::ExternalModuleReference(reference) =
                        &parsed.arena.node(module_reference).data
                    {
                        if let Some(expression) = reference.expression {
                            let expression = parsed.arena.node(expression);
                            if let NodeData::StringLiteral(literal) = &expression.data {
                                if !literal.text.is_empty() {
                                    static_occurrences.push(ModuleRequestOccurrence {
                                        pos: expression.pos,
                                        end: expression.end,
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
                        }
                    }
                }
            }
            NodeData::ModuleDeclaration(module) => {
                let Some(name) = module.name else {
                    continue;
                };
                let name_node = parsed.arena.node(name);
                let is_ambient_module = name_node.kind == SyntaxKind::StringLiteral
                    || NodeFlags::from_bits(node.flags).contains(NodeFlags::GLOBAL_AUGMENTATION);
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
                if !is_ambient_module
                    || !(in_ambient_module || has_declare_modifier || parsed.is_declaration_file)
                {
                    // `collectExternalModuleReferences` treats an ordinary
                    // namespace (and invalid, non-ambient string-named
                    // module syntax) as a hard preprocessing boundary. The
                    // checker still visits external import-equals
                    // declarations in that subtree, so retain their exact
                    // keys as authoritative unpreprocessed misses instead of
                    // asking the host to resolve syntax which tsc never
                    // publishes in `SourceFile.imports`.
                    collect_unpreprocessed_module_requests(
                        parsed,
                        module.body,
                        source,
                        expanded,
                        static_mode,
                        import_syntax_affects_resolution,
                        unpreprocessed_module_requests,
                    );
                    continue;
                }
                let name_text = match &name_node.data {
                    NodeData::StringLiteral(literal) => literal.text.as_str(),
                    NodeData::Identifier(identifier) => identifier.text.as_str(),
                    _ => "",
                };
                let is_augmentation = parsed.external_module_indicator.is_some()
                    || (in_ambient_module && !is_external_module_name_relative(name_text));
                if is_augmentation {
                    if let NodeData::StringLiteral(literal) = &name_node.data {
                        if !expanded {
                            return Err(unsupported_at(
                                source,
                                node.pos,
                                "a module augmentation is outside the static-import slice",
                            ));
                        }
                        augmentation_occurrences.push(ModuleRequestOccurrence {
                            pos: name_node.pos,
                            end: name_node.end,
                            loads_source: false,
                            key: ResolutionKey::new(
                                source.path().canonical().clone(),
                                literal.text.clone(),
                                static_mode,
                            ),
                        });
                    }
                    collect_unpreprocessed_module_requests(
                        parsed,
                        module.body,
                        source,
                        expanded,
                        static_mode,
                        import_syntax_affects_resolution,
                        unpreprocessed_module_requests,
                    );
                } else if !in_ambient_module {
                    if let Some(body_statements) = module_body_statements(parsed, module.body) {
                        collect_static_module_reference_statements(
                            parsed,
                            body_statements,
                            source,
                            expanded,
                            static_mode,
                            import_syntax_affects_resolution,
                            /*in_ambient_module*/ true,
                            static_occurrences,
                            augmentation_occurrences,
                            unpreprocessed_module_requests,
                        )?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn module_body_statements(parsed: &SourceFile, body: Option<NodeId>) -> Option<&[NodeId]> {
    let NodeData::ModuleBlock(block) = &parsed.arena.node(body?).data else {
        return None;
    };
    Some(
        block
            .statements
            .map(|statements| parsed.arena.node_array(statements).nodes.as_slice())
            .unwrap_or_default(),
    )
}

fn collect_unpreprocessed_module_requests(
    parsed: &SourceFile,
    body: Option<NodeId>,
    source: &PreparedSourceFile,
    expanded: bool,
    static_mode: ResolutionMode,
    import_syntax_affects_resolution: bool,
    requests: &mut BTreeSet<ResolutionKey>,
) {
    let Some(body) = body else {
        return;
    };
    let body = parsed.arena.node(body);
    if let NodeData::ModuleDeclaration(module) = &body.data {
        collect_unpreprocessed_module_requests(
            parsed,
            module.body,
            source,
            expanded,
            static_mode,
            import_syntax_affects_resolution,
            requests,
        );
        return;
    }
    let NodeData::ModuleBlock(block) = &body.data else {
        return;
    };
    let statements = block
        .statements
        .map(|statements| parsed.arena.node_array(statements).nodes.as_slice())
        .unwrap_or_default();
    for &statement in statements {
        match &parsed.arena.node(statement).data {
            NodeData::ImportDeclaration(import) => {
                let Some(specifier) = import.module_specifier else {
                    continue;
                };
                let NodeData::StringLiteral(literal) = &parsed.arena.node(specifier).data else {
                    continue;
                };
                if literal.text.is_empty() {
                    continue;
                }
                let mode = import
                    .attributes
                    .filter(|_| expanded)
                    .and_then(|attributes| {
                        import.import_clause.filter(|&clause| {
                            matches!(
                                &parsed.arena.node(clause).data,
                                NodeData::ImportClause(clause) if clause.is_type_only
                            )
                        })?;
                        resolution_mode_override(parsed, attributes)
                    })
                    .unwrap_or(static_mode);
                requests.insert(ResolutionKey::new(
                    source.path().canonical().clone(),
                    literal.text.clone(),
                    mode,
                ));
            }
            NodeData::ExportDeclaration(export) => {
                let Some(specifier) = export.module_specifier else {
                    continue;
                };
                let NodeData::StringLiteral(literal) = &parsed.arena.node(specifier).data else {
                    continue;
                };
                if !literal.text.is_empty() {
                    requests.insert(ResolutionKey::new(
                        source.path().canonical().clone(),
                        literal.text.clone(),
                        static_mode,
                    ));
                }
            }
            NodeData::ImportEqualsDeclaration(import_equals) => {
                let expression = import_equals
                    .module_reference
                    .and_then(|reference| match &parsed.arena.node(reference).data {
                        NodeData::ExternalModuleReference(reference) => reference.expression,
                        _ => None,
                    });
                let Some(expression) = expression else {
                    continue;
                };
                let NodeData::StringLiteral(literal) = &parsed.arena.node(expression).data else {
                    continue;
                };
                if !literal.text.is_empty() {
                    requests.insert(ResolutionKey::new(
                        source.path().canonical().clone(),
                        literal.text.clone(),
                        if import_syntax_affects_resolution {
                            ResolutionMode::CommonJs
                        } else {
                            ResolutionMode::Unspecified
                        },
                    ));
                }
            }
            NodeData::ModuleDeclaration(module) => {
                collect_unpreprocessed_module_requests(
                    parsed,
                    module.body,
                    source,
                    expanded,
                    static_mode,
                    import_syntax_affects_resolution,
                    requests,
                );
            }
            _ => {}
        }
    }
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

/// Port of TypeScript's getJSXImplicitImportBase and getJSXRuntimeImport.
/// The parser keeps the final recognized pragma value, matching the
/// compiler's last-write-wins pragma map.
fn jsx_runtime_import_specifier(parsed: &SourceFile, options: &CompilerOptions) -> Option<String> {
    if parsed
        .jsx_runtime_pragma
        .as_deref()
        .is_some_and(|runtime| runtime.eq_ignore_ascii_case("classic"))
    {
        return None;
    }

    let base = parsed
        .jsx_import_source_pragma
        .as_deref()
        .filter(|source| !source.is_empty())
        .or_else(|| {
            options
                .jsx_import_source
                .as_deref()
                .filter(|source| !source.is_empty())
        })
        .or_else(|| matches!(options.jsx, Some(4 | 5)).then_some("react"))
        .or_else(|| {
            parsed
                .jsx_runtime_pragma
                .as_deref()
                .filter(|runtime| runtime.eq_ignore_ascii_case("automatic"))
                .map(|_| "react")
        })?;
    let runtime = if options.jsx == Some(5) {
        "jsx-dev-runtime"
    } else {
        "jsx-runtime"
    };
    Some(format!("{base}/{runtime}"))
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
        // AMD, UMD, System, and None do not assign an import/require
        // resolution mode. TypeScript's getEmitSyntaxForUsageLocationWorker
        // returns undefined for these legacy emit formats; retain the
        // resolver's ordinary (unspecified) mode instead of inventing a
        // CommonJS/ESNext condition.
        0 | 2..=4 => Ok(ResolutionMode::Unspecified),
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

/// tsc `isImportCall`: both `import(...)` and the TS 6 deferred form
/// `import.defer(...)` publish the first literal argument as a dynamic module
/// request. `keyword_token` keeps this structural, so `new.defer(...)` and an
/// arbitrary `object.defer(...)` cannot be mistaken for an import call.
fn is_import_call_callee(source: &SourceFile, callee: NodeId) -> bool {
    match &source.arena.node(callee).data {
        NodeData::Token if source.arena.node(callee).kind == SyntaxKind::ImportKeyword => true,
        NodeData::MetaProperty(meta) if meta.keyword_token == SyntaxKind::ImportKeyword => {
            meta.name.is_some_and(|name| {
                matches!(&source.arena.node(name).data,
                NodeData::Identifier(identifier) if identifier.escaped_text == "defer")
            })
        }
        _ => false,
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

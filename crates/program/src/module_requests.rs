use std::collections::BTreeSet;
use std::ops::Range;
use std::sync::{Arc, LazyLock};

use memchr::memmem::Finder;
use tsc_diagnostics::{JsStr, JsString, PositionIndex};

use tsc_syntax::utilities::{is_import_call, is_source_phase_import, is_source_phase_import_call};
use tsc_syntax::{
    for_each_child, parse_source_file_from_snapshot, skip_trivia, JSDocParsingMode, NodeData,
    NodeId, ParseOptions, SourceFile, SyntaxKind, TypeReferenceDirectiveResolutionMode,
};
use tsc_types::{CompilerOptions, NodeFlags};

use crate::module_resolution::is_external_module_name_relative;
use crate::parse_cache::{CachedParse, ParseCache};
use crate::prepared::{PreparedSourceFile, PreparsedSourceFile, PreparsedSyntax};
use crate::resolution::{
    ResolutionError, ResolutionKey, ResolutionMode, TypeReferenceResolutionKey,
};

const FEATURE: &str = "static-module-request-plan";

/// tsgo ast.GetNodeAtPosition: stop at the deepest non-token child, and
/// leave a phase meta-property with its containing import call.
fn import_node_at_position(source: &SourceFile, position: u32) -> NodeId {
    let javascript = source.is_source_file_js();
    let contains = |id| {
        let node = source.arena.node(id);
        node.kind.value() > SyntaxKind::LastToken.value()
            && node.pos <= position
            && position < node.end
    };
    let mut current = source.root;
    loop {
        let node = source.arena.node(current);
        let mut child = None;
        if javascript {
            if let Some(docs) = node.js_doc() {
                child = source
                    .arena
                    .node_array(docs)
                    .nodes
                    .iter()
                    .copied()
                    .find(|&id| contains(id));
            }
        }
        // A source file, a block and a module block have one child list, their
        // statements (a source file's end-of-file token is a token: it never
        // holds the position). In a TypeScript file the statements are in
        // source order and do not overlap, so the one that can hold the
        // position is the first that ends after it. A raw-text hit in a
        // declaration file of a few thousand statements walked half of them
        // from the root, once for every `import` and `require` in its text.
        // JavaScript keeps the visitor: its statements include reparsed
        // JSDoc declarations.
        let statements = match node.kind {
            SyntaxKind::SourceFile | SyntaxKind::Block | SyntaxKind::ModuleBlock if !javascript => {
                let list = match node.data() {
                    NodeData::SourceFile(data) => data.statements,
                    NodeData::Block(data) => data.statements,
                    NodeData::ModuleBlock(data) => data.statements,
                    _ => unreachable!("the three statement-list kinds"),
                };
                Some(list.map_or(&[][..], |list| source.arena.node_array(list).nodes))
            }
            _ => None,
        };
        if let Some(statements) = statements {
            let index = statements.partition_point(|&id| source.arena.node(id).end <= position);
            child = statements.get(index).copied().filter(|&id| contains(id));
            debug_assert_eq!(
                child,
                statements.iter().copied().find(|&id| contains(id)),
                "statements in source order that do not overlap"
            );
        } else if child.is_none() {
            for_each_child(&source.arena, node, |id| {
                if contains(id) {
                    child = Some(id);
                } else if javascript && source.arena.node(id).kind == SyntaxKind::EndOfFileToken {
                    // Go reparses these EOF tags as source-file statements
                    // (parser.go:442-445). Ours retains their JSDoc attachment.
                    if let Some(docs) = source.arena.node(id).js_doc() {
                        child = source.arena.node_array(docs).nodes.iter().find_map(|&doc| {
                            let tags = source.arena.node(doc).as_js_doc()?.tags?;
                            source
                                .arena
                                .node_array(tags)
                                .nodes
                                .iter()
                                .copied()
                                .find(|&tag| {
                                    matches!(
                                        source.arena.node(tag).kind,
                                        SyntaxKind::JSDocTypedefTag
                                            | SyntaxKind::JSDocCallbackTag
                                            | SyntaxKind::JSDocImportTag
                                    ) && contains(tag)
                                })
                        });
                    }
                }
                child.is_some()
            });
        }
        let Some(child) = child else {
            return current;
        };
        if source.arena.node(child).kind == SyntaxKind::MetaProperty {
            return current;
        }
        current = child;
    }
}

/// Merge the raw spelling hits in source order. The two spellings occupy
/// disjoint byte ranges, and advancing past each full match keeps the
/// original cursor's handling of `require` overlapping itself.
fn dynamic_import_offsets(text: &[u8]) -> impl Iterator<Item = usize> + '_ {
    static IMPORT: LazyLock<Finder<'static>> = LazyLock::new(|| Finder::new(b"import"));
    static REQUIRE: LazyLock<Finder<'static>> = LazyLock::new(|| Finder::new(b"require"));

    let mut imports = IMPORT.find_iter(text).peekable();
    let mut requires = REQUIRE.find_iter(text).peekable();
    std::iter::from_fn(move || match (imports.peek(), requires.peek()) {
        (Some(import), Some(require)) if require < import => requires.next(),
        (Some(_), _) => imports.next(),
        (None, Some(_)) => requires.next(),
        (None, None) => None,
    })
}

/// tsgo ast.ForEachDynamicImportOrRequireCall scans raw text, then asks for
/// the containing syntax node. Occurrences of "import" or "require" inside
/// a dynamic call's leading argument trivia can count that same call again.
fn for_each_dynamic_import(
    source: &SourceFile,
    mut occurrence: impl FnMut(NodeId) -> Result<(), ResolutionError>,
) -> Result<usize, ResolutionError> {
    let javascript = source.is_source_file_js();
    if !javascript
        && !NodeFlags::from_bits(source.arena.node(source.root).flags)
            .contains(NodeFlags::POSSIBLY_CONTAINS_DYNAMIC_IMPORT)
    {
        return Ok(0);
    }
    let mut count = 0;
    for offset in dynamic_import_offsets(source.text().as_bytes()) {
        let id = import_node_at_position(source, offset as u32);
        let node = source.arena.node(id);
        match node.data() {
            NodeData::CallExpression(call) => {
                let arguments = call
                    .arguments
                    .map(|arguments| source.arena.node_array(arguments).nodes)
                    .unwrap_or_default();
                let require = javascript && call.expression.is_some_and(|callee| {
                    matches!(source.arena.node(callee).data(), NodeData::Identifier(identifier) if identifier.escaped_text == "require")
                }) && arguments.len() == 1;
                if (require || is_import_call(&source.arena, id))
                    && arguments
                        .first()
                        .is_some_and(|&first| string_literal_like_text(source, first).is_some())
                {
                    count += 1;
                    occurrence(id)?;
                }
            }
            NodeData::ImportType(import)
                if import
                    .argument
                    .and_then(|argument| source.arena.node(argument).as_literal_type())
                    .and_then(|argument| argument.literal)
                    .is_some_and(|literal| {
                        source.arena.node(literal).kind == SyntaxKind::StringLiteral
                    }) =>
            {
                count += 1;
                occurrence(id)?;
            }
            _ => {}
        }
    }
    Ok(count)
}

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
    file_name: JsString,
    pos: u32,
    end: u32,
    preserve: bool,
}

impl PlannedPathReference {
    pub fn file_name(&self) -> JsStr<'_> {
        self.file_name.as_js()
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
    file_name: JsString,
    pos: u32,
    end: u32,
    preserve: bool,
}

impl PlannedLibReferenceDirective {
    pub fn file_name(&self) -> JsStr<'_> {
        self.file_name.as_js()
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
pub type SourceRequestPlan = RequestPlan<GroupedOccurrences>;

pub(crate) type LoaderRequestPlan = RequestPlan<LoaderOccurrences>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestPlan<O> {
    is_external_module: bool,
    /// Parser Imports() length; synthesized resolver requests are separate.
    import_count: usize,
    path_references: Vec<PlannedPathReference>,
    module_requests: Vec<ResolutionKey>,
    unpreprocessed_module_requests: BTreeSet<ResolutionKey>,
    /// By request: whether an occurrence of it loads its target.
    module_request_loads: Vec<bool>,
    occurrences: O,
    module_request_order: Vec<ResolutionKey>,
    type_reference_directives: Vec<PlannedTypeReferenceDirective>,
    lib_reference_directives: Vec<PlannedLibReferenceDirective>,
    observed_request_occurrence_count: usize,
    /// The source has the synthesized `tslib` or JSX runtime import.
    synthetic_imports: bool,
}

/// A module request of a plan, moved out by
/// [`SourceRequestPlan::take_module_requests`].
#[derive(Clone, Debug)]
pub struct PlannedModuleRequest {
    pub key: ResolutionKey,
    pub loads_source: bool,
    pub spans: Vec<(u32, u32)>,
}

#[derive(Default)]
pub(crate) struct AdmissionPlanParts {
    pub path_references: Vec<PlannedPathReference>,
    pub type_reference_directives: Vec<PlannedTypeReferenceDirective>,
    pub lib_reference_directives: Vec<PlannedLibReferenceDirective>,
    pub module_request_order: Vec<ResolutionKey>,
}

/// The public planner groups real spans by resolution row and also exposes
/// the loading order across rows.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GroupedOccurrences {
    spans: Vec<Vec<(u32, u32)>>,
    requests: Vec<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ImportOccurrence {
    pub request: u32,
    pub pos: u32,
    pub end: u32,
    pub synthetic: bool,
}

const _: () = assert!(std::mem::size_of::<ImportOccurrence>() == 16);

#[derive(Default)]
pub(crate) struct LoaderOccurrences {
    pub records: Vec<ImportOccurrence>,
    /// Each synthetic insertion creates its own leading row.
    pub synthetic_rows: usize,
}

pub(crate) trait OccurrenceDestination: Default {
    fn with_capacity(_loading: usize) -> Self {
        Self::default()
    }
    fn add_row(&mut self);
    fn push(&mut self, request: u32, span: Option<(u32, u32)>);
    #[cfg(feature = "loader-counters")]
    fn storage(&self) -> (usize, usize);
}

impl OccurrenceDestination for GroupedOccurrences {
    fn add_row(&mut self) {
        self.spans.push(Vec::new());
    }
    fn push(&mut self, request: u32, span: Option<(u32, u32)>) {
        self.requests.push(request);
        if let Some(span) = span {
            self.spans[request as usize].push(span);
        }
    }
    #[cfg(feature = "loader-counters")]
    fn storage(&self) -> (usize, usize) {
        (
            self.requests.len(),
            self.requests.capacity() * size_of::<u32>()
                + self.spans.capacity() * size_of::<Vec<(u32, u32)>>()
                + self
                    .spans
                    .iter()
                    .map(|row| row.capacity() * size_of::<(u32, u32)>())
                    .sum::<usize>(),
        )
    }
}

impl OccurrenceDestination for LoaderOccurrences {
    fn with_capacity(loading: usize) -> Self {
        Self {
            records: Vec::with_capacity(loading),
            synthetic_rows: 0,
        }
    }
    fn add_row(&mut self) {}
    fn push(&mut self, request: u32, span: Option<(u32, u32)>) {
        let synthetic = span.is_none();
        if synthetic {
            debug_assert_eq!(request as usize, self.synthetic_rows);
            self.synthetic_rows += 1;
        }
        let (pos, end) = span.unwrap_or_default();
        self.records.push(ImportOccurrence {
            request,
            pos,
            end,
            synthetic,
        });
    }
    #[cfg(feature = "loader-counters")]
    fn storage(&self) -> (usize, usize) {
        (
            self.records.len(),
            self.records.capacity() * size_of::<ImportOccurrence>(),
        )
    }
}

impl LoaderRequestPlan {
    pub(crate) fn into_loader_parts(
        self,
    ) -> (
        AdmissionPlanParts,
        Vec<ResolutionKey>,
        Vec<bool>,
        LoaderOccurrences,
    ) {
        (
            AdmissionPlanParts {
                path_references: self.path_references,
                type_reference_directives: self.type_reference_directives,
                lib_reference_directives: self.lib_reference_directives,
                module_request_order: self.module_request_order,
            },
            self.module_requests,
            self.module_request_loads,
            self.occurrences,
        )
    }
}

impl<O> RequestPlan<O> {
    pub const fn import_count(&self) -> usize {
        self.import_count
    }

    /// Count every path, directive and module occurrence before deduplication
    /// so repeated imports still count towards the admission limit.
    pub const fn observed_request_occurrence_count(&self) -> usize {
        self.observed_request_occurrence_count
    }

    /// Module detection from the same parse used to collect requests.
    pub const fn is_external_module(&self) -> bool {
        self.is_external_module
    }

    #[cfg(feature = "loader-counters")]
    fn record_high_water(&self, file: JsStr<'_>)
    where
        O: OccurrenceDestination,
    {
        let (occurrences, occurrence_bytes) = self.occurrences.storage();
        use crate::loader_counters as c;
        c::maximum("plan.requests", self.module_requests.len());
        c::maximum("plan.occurrences", occurrences);
        c::maximum("plan.observed", self.observed_request_occurrence_count);
        c::maximum("plan.path_references", self.path_references.len());
        c::maximum("plan.type_directives", self.type_reference_directives.len());
        c::maximum("plan.lib_directives", self.lib_reference_directives.len());
        let bytes = self.path_references.capacity() * std::mem::size_of::<PlannedPathReference>()
            + self.type_reference_directives.capacity()
                * std::mem::size_of::<PlannedTypeReferenceDirective>()
            + self.lib_reference_directives.capacity()
                * std::mem::size_of::<PlannedLibReferenceDirective>()
            + (self.module_requests.capacity() + self.module_request_order.capacity())
                * std::mem::size_of::<ResolutionKey>()
            + self.module_request_loads.capacity()
            + occurrence_bytes;
        c::maximum("plan.vector_capacity_bytes", bytes);
        c::plan(
            file,
            [
                self.module_requests.len(),
                occurrences,
                self.observed_request_occurrence_count,
                bytes,
            ],
        );
    }
}

impl SourceRequestPlan {
    /// tsgo `canReplaceFileInProgram`'s request half: the same module
    /// status and the same requests in the same order (imports and their
    /// modes, module augmentations, path, type and lib references),
    /// wherever they are in the text.
    pub fn has_same_requests(&self, other: &Self) -> bool {
        self.is_external_module == other.is_external_module
            && self.import_count == other.import_count
            && self.module_requests == other.module_requests
            && self.unpreprocessed_module_requests == other.unpreprocessed_module_requests
            && self.module_request_loads == other.module_request_loads
            && self.module_request_order == other.module_request_order
            && self.observed_request_occurrence_count == other.observed_request_occurrence_count
            && self.synthetic_imports == other.synthetic_imports
            && self
                .path_references
                .iter()
                .map(|reference| (reference.file_name(), reference.preserve()))
                .eq(other
                    .path_references
                    .iter()
                    .map(|reference| (reference.file_name(), reference.preserve())))
            && self
                .type_reference_directives
                .iter()
                .map(|directive| (directive.key(), directive.preserve()))
                .eq(other
                    .type_reference_directives
                    .iter()
                    .map(|directive| (directive.key(), directive.preserve())))
            && self
                .lib_reference_directives
                .iter()
                .map(|directive| (directive.file_name(), directive.preserve()))
                .eq(other
                    .lib_reference_directives
                    .iter()
                    .map(|directive| (directive.file_name(), directive.preserve())))
    }

    /// Whether the source has the synthesized `tslib` or JSX runtime import
    /// (tsgo does not reuse a Program in place of such a file).
    pub const fn has_synthetic_imports(&self) -> bool {
        self.synthetic_imports
    }

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
            .zip(self.module_request_loads.iter().copied())
    }

    /// The request of each occurrence that loads its target, in tsgo's
    /// `moduleNames` order: processImportedModules adds a subtask per
    /// occurrence in this order.
    pub fn module_request_occurrences(&self) -> &[u32] {
        &self.occurrences.requests
    }

    /// The module requests in order with whether each loads its target and
    /// the spans of its loading occurrences (see
    /// [`Self::module_request_spans`]), moved out of the plan.
    pub fn take_module_requests(&mut self) -> Vec<PlannedModuleRequest> {
        self.take_module_requests_iter().collect()
    }

    pub(crate) fn take_module_requests_iter(
        &mut self,
    ) -> impl ExactSizeIterator<Item = PlannedModuleRequest> {
        let keys = std::mem::take(&mut self.module_requests);
        let loads = std::mem::take(&mut self.module_request_loads);
        let spans = std::mem::take(&mut self.occurrences.spans);
        keys.into_iter()
            .zip(loads)
            .zip(spans)
            .map(|((key, loads_source), spans)| PlannedModuleRequest {
                key,
                loads_source,
                spans,
            })
    }

    /// Whether a resolved target participates in program source discovery.
    /// External-module augmentations still require a resolution row but do
    /// not load their target unless another request for the same exact key
    /// does so. A key outside this plan returns `None` rather than being
    /// confused with a resolution-only request.
    pub fn module_request_loads_source(&self, key: &ResolutionKey) -> Option<bool> {
        self.module_requests
            .iter()
            .position(|request| request == key)
            .map(|index| self.module_request_loads[index])
    }

    /// Borrowed lookup when the caller already selected this source's plan.
    pub fn module_request_loads_source_by_specifier(
        &self,
        specifier: JsStr<'_>,
        mode: ResolutionMode,
    ) -> Option<bool> {
        self.module_requests
            .iter()
            .position(|request| request.specifier() == specifier && request.mode() == mode)
            .map(|index| self.module_request_loads[index])
    }

    /// The source spans of the occurrences of a module request that load
    /// their target, in source order, each including its string literal
    /// delimiters. Program construction records one inclusion reason per
    /// occurrence (tsgo's processImportedModules adds a file once for every
    /// import of it; fileloader.go:928-940), so a specifier imported twice
    /// explains its target twice. Synthetic requests and occurrences that do
    /// not load a source (augmentation names, JSDoc import types of a
    /// TypeScript file) have no span here.
    pub fn module_request_spans(&self, key: &ResolutionKey) -> &[(u32, u32)] {
        self.module_requests
            .iter()
            .position(|request| request == key)
            .map_or(&[], |index| self.occurrences.spans[index].as_slice())
    }

    /// Every resolved occurrence in tsgo's `moduleNames` order
    /// (fileloader.go:864-871): the imports (static, then dynamic), then the
    /// string-literal module augmentations, each as often as it occurs; the
    /// synthetic requests are not here. Built only under `traceResolution`
    /// (the loader replays the traces in this order).
    pub fn module_request_order(&self) -> &[ResolutionKey] {
        &self.module_request_order
    }

    pub fn type_reference_directives(&self) -> &[PlannedTypeReferenceDirective] {
        &self.type_reference_directives
    }

    pub fn lib_reference_directives(&self) -> &[PlannedLibReferenceDirective] {
        &self.lib_reference_directives
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
    Ok(
        plan_module_requests_worker(source, options, false, false, None)?
            .0
            .into_module_requests(),
    )
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
    plan_source_requests_cached(source, options, None)
}

/// [`plan_source_requests`] over the parse `cache` holds of the source, when
/// it holds one (tsgo's parse cache: the auto-import alias resolver's files,
/// the file a reused Program replaces).
pub fn plan_source_requests_cached(
    source: &PreparedSourceFile,
    options: &CompilerOptions,
    cache: Option<&dyn ParseCache>,
) -> Result<SourceRequestPlan, ResolutionError> {
    Ok(plan_module_requests_worker(source, options, true, true, cache)?.0)
}

/// [`plan_source_requests`] that also retains the parse the plan was computed
/// from, together with the exact [`ParseOptions`] used, in an opaque
/// take-once [`PreparsedSyntax`] slot so a later session can adopt the tree
/// instead of parsing the same snapshot again. This is the only producer of
/// a filled slot; the tree is the planner's own parse of `source`'s snapshot.
/// A parse `cache` holds (tsgo's parse cache) is read instead of parsing,
/// and the slot stays empty: the checker session takes the cache's document.
pub fn plan_source_requests_retaining_syntax(
    source: &PreparedSourceFile,
    options: &CompilerOptions,
    cache: Option<&dyn ParseCache>,
) -> Result<(SourceRequestPlan, PreparsedSyntax), ResolutionError> {
    plan_retaining_syntax(source, options, cache)
}

pub(crate) fn plan_retaining_syntax<O: OccurrenceDestination>(
    source: &PreparedSourceFile,
    options: &CompilerOptions,
    cache: Option<&dyn ParseCache>,
) -> Result<(RequestPlan<O>, PreparsedSyntax), ResolutionError> {
    if cache.is_none()
        && source.path().display().starts_with("bundled:///libs/")
        && source.path().display().ends_with(".d.ts")
        && source.script_kind() == tsc_syntax::ScriptKind::Ts
        && source.content_mapped().is_none()
        && matches!(options.emit_module_kind(), 0..=7 | 99 | 100..=200)
        && matches!(options.emit_module_resolution_kind(), 3 | 99 | 100)
    {
        let parse_options = source_request_parse_options(source, options);
        if let Some(image) = tsc_syntax::library_image::lookup(
            source.path().display(),
            source.snapshot(),
            &parse_options,
        ) {
            let metadata = image
                .metadata(source.path().display(), source.snapshot(), &parse_options)
                .expect("compiler library image metadata must match its source");
            return Ok((
                library_image_plan(metadata.is_external_module, metadata.references),
                PreparsedSyntax::default(),
            ));
        }
        if let Some((external, libs)) = tsc_syntax::placement::library_metadata(source.text()) {
            return Ok((
                library_image_plan(external, libs),
                PreparsedSyntax::default(),
            ));
        }
    }
    // The loader never asks for a request outside `SourceFile.imports`, so
    // it skips the whole-file walk for misplaced imports and exports.
    let started = std::time::Instant::now();
    let mut output = None;
    let parsed = plan_module_requests_with_parser(
        source,
        options,
        true,
        false,
        cache,
        &mut output,
        |parse_options| {
            if source.content_mapped().is_none() {
                if let Some(image) = tsc_syntax::library_image::lookup(
                    source.path().display(),
                    source.snapshot(),
                    &parse_options,
                ) {
                    return PlanningConstruction::Deferred(
                        image
                            .metadata(source.path().display(), source.snapshot(), &parse_options)
                            .expect("compiler library image metadata must match its source"),
                    );
                }
            }
            let parsed = parse_source_file_from_snapshot(
                source.path().display().to_owned(),
                Arc::clone(source.snapshot()),
                parse_options.clone(),
                None,
            );
            PlanningConstruction::parsed(parsed, parse_options)
        },
    )?;
    let syntax = match parsed {
        PlanningParse::Parsed(parsed) => {
            PreparsedSyntax::new(parsed, (started, std::time::Instant::now()))
        }
        PlanningParse::Cached(parse) => PreparsedSyntax::cached(parse),
        PlanningParse::Deferred => PreparsedSyntax::default(),
    };
    Ok((
        output.expect("successful planning publishes a complete plan"),
        syntax,
    ))
}

fn library_image_plan<O: Default>(
    external: bool,
    libs: Vec<tsc_syntax::FileReference>,
) -> RequestPlan<O> {
    let observed_request_occurrence_count = libs.len();
    RequestPlan {
        is_external_module: external,
        import_count: 0,
        path_references: Vec::new(),
        module_requests: Vec::new(),
        unpreprocessed_module_requests: BTreeSet::new(),
        module_request_loads: Vec::new(),
        occurrences: O::default(),
        module_request_order: Vec::new(),
        type_reference_directives: Vec::new(),
        lib_reference_directives: libs
            .into_iter()
            .map(|r| PlannedLibReferenceDirective {
                file_name: r.file_name,
                pos: r.pos,
                end: r.end,
                preserve: r.preserve,
            })
            .collect(),
        observed_request_occurrence_count,
        synthetic_imports: false,
    }
}

// A constructed tree keeps its final take-once box while requests are planned.
// The library box retains its pristine capability; metadata needs no tree.
enum PlanningConstruction {
    Parsed(Box<PreparsedSourceFile>),
    Library(Box<PreparsedSourceFile>),
    Deferred(tsc_syntax::library_image::LibraryMetadata),
}
impl PlanningConstruction {
    fn parsed(source: SourceFile, options: ParseOptions) -> Self {
        Self::Parsed(Box::new(PreparsedSourceFile::new(options, source)))
    }

    fn library(
        source: tsc_syntax::library_image::FreshLibrarySource,
        options: ParseOptions,
    ) -> Self {
        Self::Library(Box::new(PreparsedSourceFile::new_library(options, source)))
    }
}

/// The tree the planner read: its own parse, or a cached one.
enum PlanningParse {
    Parsed(Box<PreparsedSourceFile>),
    Cached(CachedParse),
    Deferred,
}

/// Exact syntax projection used by the module-request planner. Exposed so
/// corpus replay can capture every load-steering parse without duplicating
/// module detection, language-variant, or JSDoc-mode rules.
/// The JSDoc parsing mode of every Program parse in this process:
/// `createSourceFile`'s ParseAll unless the command line selects tsc's
/// `defaultJSDocParsingMode` (ParseForTypeErrors, _tsc.js:132784) with
/// [`set_default_js_doc_parsing_mode`] before loading.
static DEFAULT_JS_DOC_PARSING_MODE: std::sync::atomic::AtomicU8 =
    std::sync::atomic::AtomicU8::new(JSDocParsingMode::ParseAll as u8);

/// Select the process-wide JSDoc parsing mode for Program parses (see
/// [`default_js_doc_parsing_mode`]). A command-line process sets it once
/// before loading a Program; library consumers leave the default.
pub fn set_default_js_doc_parsing_mode(mode: JSDocParsingMode) {
    DEFAULT_JS_DOC_PARSING_MODE.store(mode as u8, std::sync::atomic::Ordering::Relaxed);
}

/// The JSDoc parsing mode Program parses use unless a source-API fact
/// overrides it.
pub fn default_js_doc_parsing_mode() -> JSDocParsingMode {
    match DEFAULT_JS_DOC_PARSING_MODE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => JSDocParsingMode::ParseNone,
        2 => JSDocParsingMode::ParseForTypeErrors,
        3 => JSDocParsingMode::ParseForTypeInfo,
        _ => JSDocParsingMode::ParseAll,
    }
}

pub fn source_request_parse_options(
    source: &PreparedSourceFile,
    options: &CompilerOptions,
) -> ParseOptions {
    let file_name = source.path().display();
    // A content mapper's output is parsed as its virtual file (tsgo
    // ParseResult: the virtual name's script kind), and a module extension
    // makes it a module.
    let parse_file_name = source.parse_file_name();
    let is_declaration_file = is_declaration_file_name(file_name);
    let module_detection = options.emit_module_detection_kind();
    let module_extension = |name: JsStr<'_>| {
        [".cjs", ".cts", ".mjs", ".mts"]
            .iter()
            .any(|extension| name.ends_with(extension))
    };
    let force_external_module = (!is_declaration_file
        && match module_detection {
            3 => true,
            2 => {
                module_extension(file_name)
                    || source.implied_node_format() == Some(ResolutionMode::EsNext)
            }
            _ => false,
        })
        || (source.content_mapped().is_some() && module_extension(parse_file_name));
    let detect_external_module_from_jsx =
        !is_declaration_file && module_detection == 2 && matches!(options.jsx, Some(4 | 5));
    ParseOptions {
        script_kind: source.script_kind(),
        force_external_module,
        detect_external_module_from_jsx,
        js_doc_parsing_mode: default_js_doc_parsing_mode(),
        ..ParseOptions::default()
    }
}

fn plan_module_requests_worker(
    source: &PreparedSourceFile,
    options: &CompilerOptions,
    expanded: bool,
    nested_requests: bool,
    cache: Option<&dyn ParseCache>,
) -> Result<(SourceRequestPlan, PlanningParse), ResolutionError> {
    let mut output = None;
    let parsed = plan_module_requests_with_parser(
        source,
        options,
        expanded,
        nested_requests,
        cache,
        &mut output,
        |parse_options| {
            let parsed = parse_source_file_from_snapshot(
                source.path().display().to_owned(),
                Arc::clone(source.snapshot()),
                parse_options.clone(),
                None,
            );
            PlanningConstruction::parsed(parsed, parse_options)
        },
    )?;
    Ok((
        output.expect("successful planning publishes a complete plan"),
        parsed,
    ))
}

pub fn plan_source_requests_in_stripe(
    source: &PreparedSourceFile,
    options: &CompilerOptions,
    stripe: &mut tsc_syntax::placement::SyntaxStripe,
) -> Result<(SourceRequestPlan, PreparsedSyntax), ResolutionError> {
    plan_in_stripe(source, options, stripe)
}

pub(crate) fn plan_in_stripe<O: OccurrenceDestination>(
    source: &PreparedSourceFile,
    options: &CompilerOptions,
    stripe: &mut tsc_syntax::placement::SyntaxStripe,
) -> Result<(RequestPlan<O>, PreparsedSyntax), ResolutionError> {
    let mut output = None;
    let syntax = plan_source_requests_in_stripe_into(source, options, stripe, &mut output)?;
    Ok((
        output.expect("successful planning publishes a complete plan"),
        syntax,
    ))
}

/// Publish a complete request plan once; an error leaves the slot empty.
/// Library syntax keeps its pristine capability and existing fallback.
pub(crate) fn plan_source_requests_in_stripe_into<O: OccurrenceDestination>(
    source: &PreparedSourceFile,
    options: &CompilerOptions,
    stripe: &mut tsc_syntax::placement::SyntaxStripe,
    output: &mut Option<RequestPlan<O>>,
) -> Result<PreparsedSyntax, ResolutionError> {
    let started = std::time::Instant::now();
    let parsed = plan_module_requests_with_parser(
        source,
        options,
        true,
        false,
        None,
        output,
        |parse_options| {
            if source.content_mapped().is_none() {
                if let Some(image) = tsc_syntax::library_image::lookup(
                    source.path().display(),
                    source.snapshot(),
                    &parse_options,
                ) {
                    let counts = image
                        .counts(source.path().display(), source.snapshot(), &parse_options)
                        .expect("compiler library image counts must match its source");
                    if stripe.can_construct(counts) {
                        let fresh = image.restore_fresh_in_stripe(
                            stripe,
                            source.path().display().to_owned(),
                            Arc::clone(source.snapshot()),
                            &parse_options,
                        );
                        return PlanningConstruction::library(fresh, parse_options);
                    }
                    stripe.defer_image();
                    return PlanningConstruction::Deferred(
                        image
                            .metadata(source.path().display(), source.snapshot(), &parse_options)
                            .expect("compiler library image metadata must match its source"),
                    );
                }
            }
            let parsed = stripe.parse(
                source.path().display().to_owned(),
                Arc::clone(source.snapshot()),
                parse_options.clone(),
            );
            PlanningConstruction::parsed(parsed, parse_options)
        },
    )?;
    let syntax = match parsed {
        PlanningParse::Parsed(parsed) => {
            PreparsedSyntax::new(parsed, (started, std::time::Instant::now()))
        }
        PlanningParse::Deferred => PreparsedSyntax::default(),
        PlanningParse::Cached(_) => unreachable!("direct parser has no cache"),
    };
    Ok(syntax)
}

/// Where a source's requests so far stand in its plan, by key.
///
/// Every key of one source carries the same source path. A source has a
/// handful of requests, and comparing the specifiers and modes of those few
/// is cheaper than hashing the source's path with each look-up and keeping a
/// second copy of each key in a map made anew for every source. A source of
/// many requests (an index file of ten thousand `require` calls) gets the
/// map once it has more than [`RequestIndex::LINEAR`] of them.
#[derive(Default)]
struct RequestIndex {
    map: Option<rustc_hash::FxHashMap<ResolutionKey, usize>>,
}

impl RequestIndex {
    const LINEAR: usize = 16;

    /// The place of `key` among `requests`, the plan's requests so far.
    fn find(&self, requests: &[ResolutionKey], key: &ResolutionKey) -> Option<usize> {
        match &self.map {
            Some(map) => map.get(key).copied(),
            None => {
                debug_assert!(requests
                    .iter()
                    .all(|request| request.source() == key.source()));
                requests.iter().position(|request| {
                    request.mode() == key.mode() && request.specifier() == key.specifier()
                })
            }
        }
    }

    /// `requests` has just gained its last element.
    fn pushed(&mut self, requests: &[ResolutionKey]) {
        match &mut self.map {
            Some(map) => {
                let index = requests.len() - 1;
                map.insert(requests[index].clone(), index);
            }
            None if requests.len() > Self::LINEAR => {
                self.map = Some(
                    requests
                        .iter()
                        .cloned()
                        .enumerate()
                        .map(|(index, key)| (key, index))
                        .collect(),
                );
            }
            None => {}
        }
    }
}

fn plan_module_requests_with_parser<O: OccurrenceDestination>(
    source: &PreparedSourceFile,
    options: &CompilerOptions,
    expanded: bool,
    nested_requests: bool,
    cache: Option<&dyn ParseCache>,
    output: &mut Option<RequestPlan<O>>,
    parse: impl FnOnce(ParseOptions) -> PlanningConstruction,
) -> Result<PlanningParse, ResolutionError> {
    debug_assert!(output.is_none());
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
        || (expanded && !matches!(resolution_kind, 3 | 99 | 100))
    {
        return Err(unsupported(
            source,
            format!("module resolution kind {resolution_kind} is not Node16, NodeNext, or Bundler"),
        ));
    }

    let file_name = source.path().display();
    let import_syntax_affects_resolution = import_syntax_affects_module_resolution(options);
    let (static_mode, dynamic_mode) = if import_syntax_affects_resolution {
        let file_emit_kind = file_emit_module_kind(source, module_kind)?;
        (
            static_request_mode(source, file_emit_kind)?,
            dynamic_import_mode(source, module_kind, file_emit_kind)?,
        )
    } else {
        (ResolutionMode::Unspecified, ResolutionMode::Unspecified)
    };
    let parse_options = source_request_parse_options(source, options);
    let javascript_file = parse_options.javascript_file();
    let cached = cache.and_then(|cache| cache.find(file_name, source.text(), &parse_options));
    let mut fresh = None;
    let parsed: &SourceFile = match &cached {
        Some(cached) => cached.source(),
        None => match parse(parse_options) {
            PlanningConstruction::Library(fresh) => {
                let plan = library_image_plan(
                    fresh.source().external_module_indicator.is_some(),
                    fresh.source().lib_reference_directives.clone(),
                );
                #[cfg(feature = "loader-counters")]
                plan.record_high_water(source.path().display());
                *output = Some(plan);
                return Ok(PlanningParse::Parsed(fresh));
            }
            PlanningConstruction::Parsed(parsed) => fresh.insert(parsed).source(),
            PlanningConstruction::Deferred(metadata) => {
                let plan = library_image_plan(metadata.is_external_module, metadata.references);
                #[cfg(feature = "loader-counters")]
                plan.record_high_water(source.path().display());
                *output = Some(plan);
                return Ok(PlanningParse::Deferred);
            }
        },
    };
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
    let mut hosted_import_count = 0;
    let mut augmentation_occurrences = Vec::new();
    let mut unpreprocessed_module_requests = BTreeSet::new();
    let source_phase_import_count =
        collect_static_module_references(StaticModuleReferenceContext {
            parsed,
            source,
            expanded,
            static_mode,
            import_syntax_affects_resolution,
            static_occurrences: &mut static_occurrences,
            augmentation_occurrences: &mut augmentation_occurrences,
            unpreprocessed_module_requests: &mut unpreprocessed_module_requests,
        })?;

    // The remaining tree walk collects reparsed JSDoc @import declarations
    // and the checker-only EOF JSDoc requests. TS calls/types use only the
    // raw-text scan below. The static slice also walks to reject unsupported
    // syntax, as before.
    let text = source.text();
    let dynamic_walk = javascript_file
        || (!expanded
            && (NodeFlags::from_bits(parsed.arena.node(parsed.root).flags)
                .contains(NodeFlags::POSSIBLY_CONTAINS_DYNAMIC_IMPORT)
                || memchr::memmem::find(text.as_bytes(), b"/**").is_some()
                || memchr::memmem::find(text.as_bytes(), b"require").is_some()));
    let mut stack = if dynamic_walk {
        vec![parsed.root]
    } else {
        Vec::new()
    };
    while let Some(node_id) = stack.pop() {
        let node = parsed.arena.node(node_id);
        match node.data() {
            NodeData::ImportType(_) | NodeData::CallExpression(_) if !expanded => {
                collect_dynamic_module_reference(
                    parsed,
                    source,
                    node_id,
                    expanded,
                    static_mode,
                    dynamic_mode,
                    import_syntax_affects_resolution,
                    &mut dynamic_occurrences,
                )?;
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
                    if let Some(literal) = module_specifier.as_string_literal() {
                        let mode = import
                            .attributes
                            .and_then(|attributes| resolution_mode_override(parsed, attributes))
                            .unwrap_or(static_mode);
                        if !parsed.arena.text(literal.text).is_empty() {
                            hosted_import_count +=
                                usize::from(javascript_file && import.import_clause.is_some());
                            dynamic_occurrences.push(ModuleRequestOccurrence {
                                pos: module_specifier.pos,
                                end: module_specifier.end,
                                loads_source: javascript_file,
                                key: ResolutionKey::new(
                                    source.path().canonical().clone(),
                                    parsed.arena.text_js(literal.text).to_owned(),
                                    mode,
                                ),
                            });
                        }
                    }
                }
            }
            _ => {}
        }

        // JSDoc is an internal attachment rather than a for_each_child edge.
        // collectExternalModuleReferences descends into those attachments
        // only for JavaScript sources.
        // Children go straight onto the walk stack, reversed in place so
        // the first child is visited first (no per-node vector).
        let first_child = stack.len();
        if javascript_file {
            if let Some(js_doc) = node.js_doc() {
                let comments = parsed.arena.node_array(js_doc).nodes.iter().copied();
                if node.kind == SyntaxKind::EndOfFileToken {
                    // tsgo GetNodeAtPosition never enters the end-of-file
                    // token (a token kind) or its JSDoc: only the statements
                    // the reparser makes of its `@typedef`, `@callback` and
                    // `@import` tags are reached, within each tag's range.
                    // The import types of its other tags (an `@template`
                    // beside a typedef) are not collected, and the checker
                    // finds no resolution for them.
                    for comment in comments {
                        let Some(comment) = parsed.arena.node(comment).as_js_doc() else {
                            continue;
                        };
                        let tags = comment
                            .tags
                            .map(|tags| parsed.arena.node_array(tags).nodes)
                            .unwrap_or_default();
                        for &tag in tags {
                            if matches!(
                                parsed.arena.node(tag).kind,
                                SyntaxKind::JSDocTypedefTag
                                    | SyntaxKind::JSDocCallbackTag
                                    | SyntaxKind::JSDocImportTag
                            ) {
                                stack.push(tag);
                            } else {
                                uncollected_import_types(
                                    parsed,
                                    source,
                                    tag,
                                    static_mode,
                                    &mut unpreprocessed_module_requests,
                                );
                            }
                        }
                    }
                } else {
                    stack.extend(comments);
                }
            }
        }
        for_each_child(&parsed.arena, node, |child| {
            stack.push(child);
            false
        });
        stack[first_child..].reverse();
    }

    // tsgo's collectExternalModuleReferences reads only source-file and
    // ambient-module statements (parser/references.go:11-90). An import or
    // export that a grammar error placed in a block is still checked; its
    // lookup finds no resolution and reports the module as not found.
    if nested_requests {
        collect_nested_module_requests(
            parsed,
            source,
            expanded,
            static_mode,
            import_syntax_affects_resolution,
            &mut unpreprocessed_module_requests,
        )?;
    }

    let static_import_count = static_occurrences.len();
    // Reparsed JSDoc @import declarations belong to the static statement
    // inventory. Append scan hits afterwards; do not sort them by argument
    // position (two different hits may select the same call).
    static_occurrences.append(&mut dynamic_occurrences);
    static_occurrences.sort_by_key(|occurrence| occurrence.pos);
    let dynamic_import_count = for_each_dynamic_import(parsed, |node_id| {
        collect_dynamic_module_reference(
            parsed,
            source,
            node_id,
            expanded,
            static_mode,
            dynamic_mode,
            import_syntax_affects_resolution,
            &mut dynamic_occurrences,
        )
    })?;
    augmentation_occurrences.sort_by_key(|occurrence| occurrence.pos);
    // checkerpool.go weighs parser imports, not the resolver's moduleNames.
    // Alternate resolution-mode rows refer to the same syntactic occurrence.
    let import_count = static_import_count
        + hosted_import_count
        + dynamic_import_count
        + source_phase_import_count;
    let mut module_requests = Vec::new();
    let mut module_request_loads = Vec::new();
    let mut request_index = RequestIndex::default();
    // tsc collectExternalModuleReferences prepends a synthesized `tslib`
    // import when importHelpers can participate in this source. The checker
    // later resolves that exact synthetic specifier while validating the
    // requested helper shape, so the authoritative table must contain the
    // row even though no source-text module literal exists. The JSX runtime
    // import follows; the checker addresses it by this stable source-file
    // import index, so it participates in the authoritative resolution table
    // as well.
    let synthetic = synthetic_imports(
        parsed,
        javascript_file,
        source.parse_file_name().ends_with(".tsx"),
        options,
    );
    let has_synthetic_tslib = synthetic.import_helpers;
    let jsx_runtime_import = synthetic.jsx_runtime;
    let has_synthetic_jsx_runtime = jsx_runtime_import.is_some();
    let observed_request_occurrence_count = path_references
        .len()
        .saturating_add(type_reference_directives.len())
        .saturating_add(lib_reference_directives.len())
        .saturating_add(static_occurrences.len())
        .saturating_add(dynamic_occurrences.len())
        .saturating_add(augmentation_occurrences.len())
        .saturating_add(usize::from(has_synthetic_tslib))
        .saturating_add(usize::from(has_synthetic_jsx_runtime));
    // All JS imports load. For TS we know the static count without
    // walking the dynamic rows to exclude checker-only occurrences.
    let known_loading = if javascript_file {
        static_occurrences.len() + dynamic_occurrences.len()
    } else {
        static_import_count
    };
    let mut occurrences = O::with_capacity(
        known_loading + usize::from(has_synthetic_tslib) + usize::from(has_synthetic_jsx_runtime),
    );
    if has_synthetic_tslib {
        let key = ResolutionKey::new(source.path().canonical().clone(), "tslib", static_mode);
        occurrences.add_row();
        occurrences.push(module_requests.len() as u32, None);
        module_requests.push(key);
        request_index.pushed(&module_requests);
        module_request_loads.push(true);
    }
    if has_synthetic_jsx_runtime {
        let key = ResolutionKey::new(
            source.path().canonical().clone(),
            jsx_runtime_import.expect("guarded JSX runtime synthetic import"),
            static_mode,
        );
        occurrences.add_row();
        occurrences.push(module_requests.len() as u32, None);
        module_requests.push(key);
        request_index.pushed(&module_requests);
        module_request_loads.push(true);
    }
    // The spans are UTF-16 offsets from the snapshot's position index: one
    // lookup each. Counting the UTF-16 units of the text prefix for every
    // occurrence made the plan quadratic in the file; a CommonJS index of
    // 10,000 `require` calls (material-ui's `@mui/icons-material/lib/index.js`,
    // 2.4 MB) took 13 seconds here where tsgo takes milliseconds.
    let module_request_order: Vec<ResolutionKey> = if options.trace_resolution == Some(true) {
        static_occurrences
            .iter()
            .chain(dynamic_occurrences.iter())
            .filter(|occurrence| occurrence.loads_source)
            .chain(augmentation_occurrences.iter())
            .map(|occurrence| occurrence.key.clone())
            .collect()
    } else {
        Vec::new()
    };
    // The tree's snapshot: a cached parse's has its index built already.
    let positions = parsed.snapshot().positions();
    let source_text = parsed.text();
    for occurrence in static_occurrences
        .into_iter()
        .chain(dynamic_occurrences)
        .chain(augmentation_occurrences)
    {
        let index = match request_index.find(&module_requests, &occurrence.key) {
            Some(index) => index,
            None => {
                let index = module_requests.len();
                module_requests.push(occurrence.key);
                request_index.pushed(&module_requests);
                module_request_loads.push(false);
                occurrences.add_row();
                index
            }
        };
        if occurrence.loads_source {
            module_request_loads[index] = true;
            let span_start =
                skip_trivia(source_text, occurrence.pos as usize).min(occurrence.end as usize);
            occurrences.push(
                index as u32,
                Some((
                    byte_to_utf16_offset(positions, source_text, span_start),
                    byte_to_utf16_offset(positions, source_text, occurrence.end as usize),
                )),
            );
        }
    }
    // The source-file resolution cache is keyed by exact specifier and mode.
    // If another preprocessed occurrence owns the same key, checker lookups in
    // an augmentation body observe that ordinary authoritative row.
    unpreprocessed_module_requests
        .retain(|key| request_index.find(&module_requests, key).is_none());

    let plan = RequestPlan {
        is_external_module: parsed.external_module_indicator.is_some(),
        import_count,
        path_references,
        module_requests,
        unpreprocessed_module_requests,
        module_request_loads,
        occurrences,
        module_request_order,
        type_reference_directives,
        lib_reference_directives,
        observed_request_occurrence_count,
        synthetic_imports: has_synthetic_tslib || has_synthetic_jsx_runtime,
    };
    #[cfg(feature = "loader-counters")]
    plan.record_high_water(source.path().display());
    let parsed = match fresh {
        Some(fresh) => PlanningParse::Parsed(fresh),
        None => PlanningParse::Cached(cached.expect("the planner read the cache's parse")),
    };
    *output = Some(plan);
    Ok(parsed)
}

// tsc-port: getErrorSpanForNode (external module indicators) @6.0.3
// tsc-hash: dcb14b8979d1c3871354787b49af4e5fa54040952f7b6d256ab508d1bbc355b7
// tsc-span: _tsc.js:14023-14118

/// The UTF-16 offset of a byte offset: the position index answers a byte
/// on a character boundary; a byte inside a character (or past the end)
/// falls back to counting the prefix.
fn byte_to_utf16_offset(positions: &PositionIndex, text: &str, byte_offset: usize) -> u32 {
    let clamped = byte_offset.min(text.len());
    if let Some(offset) = u32::try_from(clamped)
        .ok()
        .and_then(|byte| positions.byte_to_utf16(byte))
    {
        return offset;
    }
    text.get(..clamped).unwrap_or(text).encode_utf16().count() as u32
}

/// `isDeclarationFileName` includes arbitrary-extension declaration twins
/// such as `style.d.css.ts`, in addition to the three standard spellings.
///
/// Every declaration spelling ends in `ts`, and all but the twins are told
/// by their last bytes: a name that is none of them (`a.tsx`, `a.js`,
/// `a.json`) is refused there, before its last component is looked for. Only
/// a name ending in `.ts` that is not a standard spelling reads its last
/// component (what follows the last `/` or `\`) for a `.d.`.
pub(crate) fn is_declaration_file_name<'a>(file_name: impl Into<JsStr<'a>>) -> bool {
    let file_name: JsStr<'a> = file_name.into();
    let bytes = file_name.as_bytes();
    if !bytes.ends_with(b"ts") {
        return false;
    }
    if bytes.ends_with(b".d.ts") || bytes.ends_with(b".d.cts") || bytes.ends_with(b".d.mts") {
        return true;
    }
    if !bytes.ends_with(b".ts") {
        return false;
    }
    let base_name = bytes
        .iter()
        .rposition(|byte| matches!(byte, b'/' | b'\\'))
        .map_or(bytes, |separator| &bytes[separator + 1..]);
    base_name.windows(3).any(|window| window == b".d.")
}

// Called once for each raw-text hit, including hits that select the same node.
#[allow(clippy::too_many_arguments)]
fn collect_dynamic_module_reference(
    parsed: &SourceFile,
    source: &PreparedSourceFile,
    node_id: NodeId,
    expanded: bool,
    static_mode: ResolutionMode,
    dynamic_mode: ResolutionMode,
    import_syntax_affects_resolution: bool,
    dynamic_occurrences: &mut Vec<ModuleRequestOccurrence>,
) -> Result<(), ResolutionError> {
    let javascript_file = parsed.is_source_file_js();
    let node = parsed.arena.node(node_id);
    match node.data() {
        NodeData::ImportType(import_type) => {
            if !expanded {
                return Err(unsupported_at(
                    source,
                    node.pos,
                    "an import type is outside the static-import slice",
                ));
            }
            if let Some(argument) = import_type.argument {
                if let Some(argument) = parsed.arena.node(argument).as_literal_type() {
                    if let Some(literal) = argument.literal {
                        let literal = parsed.arena.node(literal);
                        if let Some(literal_data) = literal.as_string_literal() {
                            let mode_override = import_type.attributes.and_then(|attributes| {
                                resolution_mode_override(parsed, attributes)
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
                                    parsed.arena.text_js(literal_data.text).to_owned(),
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
                                for alternate in [ResolutionMode::CommonJs, ResolutionMode::EsNext]
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
                                                parsed.arena.text_js(literal_data.text).to_owned(),
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
        NodeData::CallExpression(call) => {
            let callee = call.expression;
            // tsgo resolves no source phase import
            // (fileloader.go resolveImportsAndModuleAugmentations).
            if is_import_call(&parsed.arena, node_id)
                && !is_source_phase_import_call(&parsed.arena, node_id)
            {
                if !expanded {
                    return Err(unsupported_at(
                        source,
                        node.pos,
                        "a dynamic import call is outside the static-import slice",
                    ));
                }
                let arguments = call
                    .arguments
                    .map(|arguments| parsed.arena.node_array(arguments).nodes)
                    .unwrap_or_default();
                // TypeScript's collector records the first string-literal-like
                // argument whenever at least one argument exists. The
                // optional second argument carries import attributes and
                // does not suppress the host resolution request.
                if !arguments.is_empty() {
                    let argument = parsed.arena.node(arguments[0]);
                    if let Some(specifier) = string_literal_like_text(parsed, arguments[0]) {
                        dynamic_occurrences.push(ModuleRequestOccurrence {
                            pos: argument.pos,
                            end: argument.end,
                            loads_source: true,
                            key: ResolutionKey::new(
                                source.path().canonical().clone(),
                                specifier.to_owned(),
                                dynamic_mode,
                            ),
                        });
                    }
                }
            }
            let is_require = callee.is_some_and(|callee| {
                matches!(
                    parsed.arena.node(callee).data(),
                    NodeData::Identifier(identifier) if identifier.escaped_text == "require"
                )
            });
            let arguments = call
                .arguments
                .map(|arguments| parsed.arena.node_array(arguments).nodes)
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
                    // It records CommonJS even when ordinary static
                    // imports emit as ESM, and no mode when import syntax
                    // does not affect resolution (bundler without package
                    // maps).
                    let argument = arguments[0];
                    let specifier = string_literal_like_text(parsed, argument)
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
    Ok(())
}

struct ModuleRequestOccurrence {
    pos: u32,
    end: u32,
    loads_source: bool,
    key: ResolutionKey,
}

struct StaticModuleReferenceContext<'a> {
    parsed: &'a SourceFile,
    source: &'a PreparedSourceFile,
    expanded: bool,
    static_mode: ResolutionMode,
    import_syntax_affects_resolution: bool,
    static_occurrences: &'a mut Vec<ModuleRequestOccurrence>,
    augmentation_occurrences: &'a mut Vec<ModuleRequestOccurrence>,
    unpreprocessed_module_requests: &'a mut BTreeSet<ResolutionKey>,
}

/// `collectExternalModuleReferences`' statement-only static collector.
///
/// Unlike a generic syntax walk, this enters only a top-level ambient module
/// in a global script. External augmentations and nested non-relative ambient
/// declarations are terminal module-augmentation entries. Their bodies are
/// retained separately as checker-visible, host-unpreprocessed requests.
///
/// Source-phase imports contribute to the import count without requests.
fn collect_static_module_references(
    context: StaticModuleReferenceContext<'_>,
) -> Result<usize, ResolutionError> {
    let StaticModuleReferenceContext {
        parsed,
        source,
        expanded,
        static_mode,
        import_syntax_affects_resolution,
        static_occurrences,
        augmentation_occurrences,
        unpreprocessed_module_requests,
    } = context;
    let Some(root) = parsed.arena.node(parsed.root).as_source_file() else {
        return Err(unsupported(source, "the parsed root is not a source file"));
    };
    let statements = root
        .statements
        .map(|statements| parsed.arena.node_array(statements).nodes)
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
) -> Result<usize, ResolutionError> {
    let mut source_phase_import_count = 0;
    for &node_id in statements {
        let node = parsed.arena.node(node_id);
        match node.data() {
            NodeData::ImportDeclaration(import) => {
                if let Some(module_specifier) = import.module_specifier {
                    let module_specifier = parsed.arena.node(module_specifier);
                    if let Some(literal) = module_specifier.as_string_literal() {
                        if !parsed.arena.text(literal.text).is_empty()
                            && (!in_ambient_module
                                || !is_external_module_name_relative(
                                    parsed.arena.text_js(literal.text),
                                ))
                        {
                            if is_source_phase_import(&parsed.arena, node_id) {
                                source_phase_import_count += 1;
                                continue;
                            }
                            let mode = import
                                .attributes
                                .filter(|_| expanded)
                                .and_then(|attributes| {
                                    import.import_clause.filter(|&clause| {
                                        matches!(
                                            parsed.arena.node(clause).data(),
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
                                    parsed.arena.text_js(literal.text).to_owned(),
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
                if let Some(literal) = module_specifier.as_string_literal() {
                    if !parsed.arena.text(literal.text).is_empty()
                        && (!in_ambient_module
                            || !is_external_module_name_relative(
                                parsed.arena.text_js(literal.text),
                            ))
                    {
                        // A type-only export takes the `resolution-mode`
                        // attribute like a type-only import
                        // (getModeForUsageLocation, fileloader.go:1049-1066).
                        let mode = export
                            .attributes
                            .filter(|_| expanded && export.is_type_only)
                            .and_then(|attributes| resolution_mode_override(parsed, attributes))
                            .unwrap_or(static_mode);
                        static_occurrences.push(ModuleRequestOccurrence {
                            pos: module_specifier.pos,
                            end: module_specifier.end,
                            loads_source: true,
                            key: ResolutionKey::new(
                                source.path().canonical().clone(),
                                parsed.arena.text_js(literal.text).to_owned(),
                                mode,
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
                        parsed.arena.node(module_reference).data()
                    {
                        if let Some(expression) = reference.expression {
                            let expression = parsed.arena.node(expression);
                            if let Some(literal) = expression.as_string_literal() {
                                if !parsed.arena.text(literal.text).is_empty() {
                                    static_occurrences.push(ModuleRequestOccurrence {
                                        pos: expression.pos,
                                        end: expression.end,
                                        loads_source: true,
                                        key: ResolutionKey::new(
                                            source.path().canonical().clone(),
                                            parsed.arena.text_js(literal.text).to_owned(),
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
                        .modifier_flags
                        .intersects(tsc_types::ModifierFlags::AMBIENT)
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
                let name_text = match name_node.data() {
                    NodeData::StringLiteral(literal) => parsed.arena.text_js(literal.text),
                    NodeData::Identifier(identifier) => JsStr::from_str(identifier.text()),
                    _ => JsStr::from_str(""),
                };
                let is_augmentation = parsed.external_module_indicator.is_some()
                    || (in_ambient_module && !is_external_module_name_relative(name_text));
                if is_augmentation {
                    if let Some(literal) = name_node.as_string_literal() {
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
                                parsed.arena.text_js(literal.text).to_owned(),
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
                        source_phase_import_count += collect_static_module_reference_statements(
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
    Ok(source_phase_import_count)
}

/// Every import and export declaration's request, wherever it is. The plan
/// drops the keys `SourceFile.imports` owns; the rest are checker-visible,
/// host-unpreprocessed requests.
fn collect_nested_module_requests(
    parsed: &SourceFile,
    source: &PreparedSourceFile,
    expanded: bool,
    static_mode: ResolutionMode,
    import_syntax_affects_resolution: bool,
    requests: &mut BTreeSet<ResolutionKey>,
) -> Result<(), ResolutionError> {
    let mut occurrences = Vec::new();
    let mut augmentations = Vec::new();
    let mut stack = vec![parsed.root];
    while let Some(node_id) = stack.pop() {
        let node = parsed.arena.node(node_id);
        if matches!(
            node.kind,
            SyntaxKind::ImportDeclaration
                | SyntaxKind::ExportDeclaration
                | SyntaxKind::ImportEqualsDeclaration
        ) {
            collect_static_module_reference_statements(
                parsed,
                &[node_id],
                source,
                expanded,
                static_mode,
                import_syntax_affects_resolution,
                /*in_ambient_module*/ false,
                &mut occurrences,
                &mut augmentations,
                requests,
            )?;
            continue;
        }
        for_each_child(&parsed.arena, node, |child| {
            stack.push(child);
            false
        });
    }
    requests.extend(occurrences.into_iter().map(|occurrence| occurrence.key));
    Ok(())
}

fn module_body_statements(parsed: &SourceFile, body: Option<NodeId>) -> Option<&[NodeId]> {
    let block = parsed.arena.node(body?).as_module_block()?;
    Some(
        block
            .statements
            .map(|statements| parsed.arena.node_array(statements).nodes)
            .unwrap_or_default(),
    )
}

#[allow(clippy::too_many_arguments)]
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
    if let Some(module) = body.as_module_declaration() {
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
    let Some(block) = body.as_module_block() else {
        return;
    };
    let statements = block
        .statements
        .map(|statements| parsed.arena.node_array(statements).nodes)
        .unwrap_or_default();
    for &statement in statements {
        match parsed.arena.node(statement).data() {
            NodeData::ImportDeclaration(_) if is_source_phase_import(&parsed.arena, statement) => {}
            NodeData::ImportDeclaration(import) => {
                let Some(specifier) = import.module_specifier else {
                    continue;
                };
                let Some(literal) = parsed.arena.node(specifier).as_string_literal() else {
                    continue;
                };
                if parsed.arena.text(literal.text).is_empty() {
                    continue;
                }
                let mode = import
                    .attributes
                    .filter(|_| expanded)
                    .and_then(|attributes| {
                        import.import_clause.filter(|&clause| {
                            matches!(
                                parsed.arena.node(clause).data(),
                                NodeData::ImportClause(clause) if clause.is_type_only
                            )
                        })?;
                        resolution_mode_override(parsed, attributes)
                    })
                    .unwrap_or(static_mode);
                requests.insert(ResolutionKey::new(
                    source.path().canonical().clone(),
                    parsed.arena.text_js(literal.text).to_owned(),
                    mode,
                ));
            }
            NodeData::ExportDeclaration(export) => {
                let Some(specifier) = export.module_specifier else {
                    continue;
                };
                let Some(literal) = parsed.arena.node(specifier).as_string_literal() else {
                    continue;
                };
                if !parsed.arena.text(literal.text).is_empty() {
                    let mode = export
                        .attributes
                        .filter(|_| expanded && export.is_type_only)
                        .and_then(|attributes| resolution_mode_override(parsed, attributes))
                        .unwrap_or(static_mode);
                    requests.insert(ResolutionKey::new(
                        source.path().canonical().clone(),
                        parsed.arena.text_js(literal.text).to_owned(),
                        mode,
                    ));
                }
            }
            NodeData::ImportEqualsDeclaration(import_equals) => {
                let expression = import_equals
                    .module_reference
                    .and_then(|reference| match parsed.arena.node(reference).data() {
                        NodeData::ExternalModuleReference(reference) => reference.expression,
                        _ => None,
                    });
                let Some(expression) = expression else {
                    continue;
                };
                let Some(literal) = parsed.arena.node(expression).as_string_literal() else {
                    continue;
                };
                if !parsed.arena.text(literal.text).is_empty() {
                    requests.insert(ResolutionKey::new(
                        source.path().canonical().clone(),
                        parsed.arena.text_js(literal.text).to_owned(),
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

/// TypeScript 7.1 (`ast.ImportAttributesNode.GetResolutionModeOverride`)
/// finds the `resolution-mode` attribute among any others. The checker's
/// grammar check parses the attributes the same way, so the modes the
/// resolver prepares are the modes the checker asks for.
fn resolution_mode_override(parsed: &SourceFile, attributes: NodeId) -> Option<ResolutionMode> {
    let attributes = parsed.arena.node(attributes).as_import_attributes()?;
    let elements = attributes.elements?;
    let elements = &parsed.arena.node_array(elements).nodes;
    let attribute = elements.iter().find_map(|&element| {
        let attribute = parsed.arena.node(element).as_import_attribute()?;
        (string_literal_like_text(parsed, attribute.name?)? == "resolution-mode")
            .then_some(attribute)
    })?;
    match string_literal_like_text(parsed, attribute.value?)? {
        "import" => Some(ResolutionMode::EsNext),
        "require" => Some(ResolutionMode::CommonJs),
        _ => None,
    }
}

// This projection serves only resolution-mode's exact ASCII spellings above.
// A non-scalar value cannot equal any of those three strings; its original
// literal ownership and resolution key are unaffected.
fn string_literal_like_text(parsed: &SourceFile, node: NodeId) -> Option<&str> {
    match parsed.arena.node(node).data() {
        NodeData::StringLiteral(literal) => parsed.arena.text_js(literal.text).as_str(),
        NodeData::NoSubstitutionTemplateLiteral(literal) => {
            parsed.arena.text_js(literal.text).as_str()
        }
        _ => None,
    }
}

/// The import types under `root` that tsgo does not collect, as requests
/// whose lookup finds no resolution (see the end-of-file token's JSDoc in
/// [`plan_source_requests`]).
fn uncollected_import_types(
    parsed: &SourceFile,
    source: &PreparedSourceFile,
    root: NodeId,
    static_mode: ResolutionMode,
    unpreprocessed_module_requests: &mut BTreeSet<ResolutionKey>,
) {
    let mut stack = vec![root];
    while let Some(node_id) = stack.pop() {
        if let Some(import_type) = parsed.arena.node(node_id).as_import_type() {
            let literal = import_type
                .argument
                .and_then(|argument| match parsed.arena.node(argument).data() {
                    NodeData::LiteralType(argument) => argument.literal,
                    _ => None,
                })
                .map(|literal| parsed.arena.node(literal).data());
            if let Some(NodeData::StringLiteral(literal)) = literal {
                let mode = import_type
                    .attributes
                    .and_then(|attributes| resolution_mode_override(parsed, attributes))
                    .unwrap_or(static_mode);
                unpreprocessed_module_requests.insert(ResolutionKey::new(
                    source.path().canonical().clone(),
                    parsed.arena.text_js(literal.text).to_owned(),
                    mode,
                ));
            }
        }
        for_each_child(&parsed.arena, parsed.arena.node(node_id), |child| {
            stack.push(child);
            false
        });
    }
}

/// The imports tsgo synthesizes for a file
/// (`resolveImportsAndModuleAugmentations`, fileloader.go:834-851).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SyntheticImports {
    /// The import helpers module (`tslib`) is imported: `importHelpers` in a
    /// JavaScript file, or in a TypeScript file that is not a declaration
    /// file and is a module (or `isolatedModules` holds).
    pub import_helpers: bool,
    /// The JSX runtime module a JavaScript or `.tsx` file imports.
    pub jsx_runtime: Option<JsString>,
}

/// The imports tsgo synthesizes for a parsed file.
pub fn synthetic_imports(
    parsed: &SourceFile,
    javascript_file: bool,
    tsx_file: bool,
    options: &CompilerOptions,
) -> SyntheticImports {
    let computed_isolated_modules =
        options.isolated_modules == Some(true) || options.verbatim_module_syntax == Some(true);
    let import_helpers = options.import_helpers == Some(true)
        && (javascript_file
            || (!parsed.is_declaration_file
                && (computed_isolated_modules || parsed.external_module_indicator.is_some())));
    // A `.ts` file never gets a JSX runtime import (the script kind decides,
    // not the module-ness: an `isolatedModules` `.tsx` script does).
    let jsx_runtime = (javascript_file || tsx_file)
        .then(|| jsx_runtime_import_specifier(parsed, options))
        .flatten();
    SyntheticImports {
        import_helpers,
        jsx_runtime,
    }
}

/// Port of TypeScript's getJSXImplicitImportBase and getJSXRuntimeImport.
/// The parser keeps the final recognized pragma value, matching the
/// compiler's last-write-wins pragma map.
fn jsx_runtime_import_specifier(
    parsed: &SourceFile,
    options: &CompilerOptions,
) -> Option<JsString> {
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
        .map(JsStr::from)
        .or_else(|| {
            options
                .jsx_import_source
                .as_ref()
                .filter(|source| !source.is_empty())
                .map(JsString::as_js)
        })
        .or_else(|| matches!(options.jsx, Some(4 | 5)).then_some(JsStr::from("react")))
        .or_else(|| {
            parsed
                .jsx_runtime_pragma
                .as_deref()
                .filter(|runtime| runtime.eq_ignore_ascii_case("automatic"))
                .map(|_| JsStr::from("react"))
        })?;
    let runtime = if options.jsx == Some(5) {
        "jsx-dev-runtime"
    } else {
        "jsx-runtime"
    };
    let mut specifier = base.to_owned();
    specifier.push('/');
    specifier.push_str(runtime);
    Some(specifier)
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

/// tsc `getEmitModuleFormatOfFileWorker` for the representation exposed by
/// `PreparedSourceFile`: an authoritative effective implied format wins, then
/// the computed `module` kind is used.
fn file_emit_module_kind(
    source: &PreparedSourceFile,
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

    // An absent impliedNodeFormat is meaningful, including incompatible
    // Node module + Bundler resolution options. tsc falls back to module;
    // option diagnostics do not prevent construction of the request graph.
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
        0 | 2..=4 | 100..=199 => Ok(ResolutionMode::Unspecified),
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
        format!(
            "{}: {}",
            source.path().display().to_string_lossy(),
            detail.into()
        ),
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

#[cfg(test)]
mod declaration_file_name_tests {
    use super::*;

    /// The form the byte tests replaced: the standard spellings, then the
    /// last component by two splits and a search.
    fn by_splits(file_name: JsStr<'_>) -> bool {
        if file_name.ends_with(".d.ts")
            || file_name.ends_with(".d.cts")
            || file_name.ends_with(".d.mts")
        {
            return true;
        }
        let base_name = file_name.split_ascii(b'/').next_back().unwrap_or(file_name);
        let base_name = base_name
            .split_ascii(b'\\')
            .next_back()
            .unwrap_or(base_name);
        base_name.ends_with(".ts") && base_name.contains(".d.")
    }

    #[test]
    fn a_declaration_name_is_told_from_its_end_as_by_the_splits() {
        // Every text of up to five pieces over both separators, the dot,
        // `.d`, the three extensions' letters and a two-byte scalar.
        let pieces = ["/", "\\", ".", ".d", "ts", "c", "m", "\u{e9}"];
        let mut texts = vec![String::new()];
        let mut start = 0;
        for _ in 0..5 {
            let end = texts.len();
            for index in start..end {
                for piece in pieces {
                    let mut text = texts[index].clone();
                    text.push_str(piece);
                    texts.push(text);
                }
            }
            start = end;
        }
        assert_eq!(texts.len(), 37_449);
        let mut declarations = 0;
        for text in &texts {
            let name = JsStr::from(text.as_str());
            let expected = by_splits(name);
            declarations += usize::from(expected);
            assert_eq!(is_declaration_file_name(name), expected, "{text:?}");
        }
        assert!(declarations > 100, "{declarations} declaration names tried");
        for (name, expected) in [
            ("/project/a.d.ts", true),
            ("/project/a.d.mts", true),
            ("/project/a.d.cts", true),
            ("/project/style.d.css.ts", true),
            ("/project/dir.d.x/a.ts", false),
            ("C:\\project.d.x\\a.ts", false),
            ("/project/a.ts", false),
            ("/project/a.tsx", false),
            ("/project/a.d.tsx", false),
            ("/project/a.js", false),
            ("d.ts", false),
            (".d.ts", true),
            ("", false),
        ] {
            assert_eq!(is_declaration_file_name(name), expected, "{name:?}");
        }
    }
}

#[cfg(test)]
mod import_node_search_tests {
    use super::*;

    /// The search before the statement lists were searched by position:
    /// every level through the child visitor.
    fn by_child_visitor(source: &SourceFile, position: u32) -> NodeId {
        let contains = |id| {
            let node = source.arena.node(id);
            node.kind.value() > SyntaxKind::LastToken.value()
                && node.pos <= position
                && position < node.end
        };
        let mut current = source.root;
        loop {
            let node = source.arena.node(current);
            let mut child = None;
            for_each_child(&source.arena, node, |id| {
                if contains(id) {
                    child = Some(id);
                }
                child.is_some()
            });
            let Some(child) = child else {
                return current;
            };
            if source.arena.node(child).kind == SyntaxKind::MetaProperty {
                return current;
            }
            current = child;
        }
    }

    fn parse(name: &str, text: &str) -> SourceFile {
        let path = crate::ProgramPath::from_js_parts(name.into(), name.into()).unwrap();
        let source = PreparedSourceFile::new(path, text);
        parse_source_file_from_snapshot(
            source.path().display().to_owned(),
            Arc::clone(source.snapshot()),
            source_request_parse_options(&source, &CompilerOptions::default()),
            None,
        )
    }

    #[test]
    fn the_node_at_a_position_is_the_child_visitors_node_at_every_position() {
        let mut text = String::from(
            "// import require in a comment\n\
             import { a } from './a';\n\
             export declare const lazy: import('./types').Lazy<typeof import('./b')>;\n\
             namespace outer {\n  export namespace inner {\n    export const load = () => import('./c');\n  }\n\
               export function f(x: number) {\n    if (x) {\n      return import('./d');\n    }\n    { const required = x; }\n    return import.meta;\n  }\n}\n\
             class C {\n  m() {\n    return `${import('./e')} tail`;\n  }\n}\n\
             declare module 'ambient' {\n  export const v: import('./f').V;\n}\n\
             ;;\n\
             export default async function () {\n  const m = await import('./g');\n  for (const k of m.keys) {\n    await import(k);\n  }\n}\n",
        );
        // Many statements after the ones above: the list the search was written for.
        for index in 0..300 {
            text.push_str(&format!(
                "export const v{index} = {index}; // import{index}\n"
            ));
        }
        text.push_str("export const last = import('./last');\n// trailing require\n");
        let source = parse("/project/search.ts", &text);
        assert!(!source.is_source_file_js());
        let mut deepest = std::collections::BTreeSet::new();
        for position in 0..=text.len() as u32 {
            let expected = by_child_visitor(&source, position);
            assert_eq!(
                import_node_at_position(&source, position),
                expected,
                "position {position}"
            );
            deepest.insert(expected);
        }
        assert!(deepest.len() > 600, "{} nodes found", deepest.len());
        // An empty file and a file of one token: no statement holds anything.
        for text in ["", " ", ";"] {
            let source = parse("/project/empty.ts", text);
            for position in 0..=text.len() as u32 {
                assert_eq!(
                    import_node_at_position(&source, position),
                    by_child_visitor(&source, position),
                    "{text:?} at {position}"
                );
            }
        }
    }
}

#[cfg(test)]
mod direct_library_tests {
    use super::*;

    #[test]
    fn module_request_weights_follow_source_phase_ambient_boundaries() {
        let path =
            crate::ProgramPath::from_js_parts("/input.ts".into(), "/input.ts".into()).unwrap();
        let options = CompilerOptions::default();
        for (text, expected_count, expected_augmentation) in [
            (
                "import source a from 'a'; import source b from ''; import source c from './c';",
                2,
                None,
            ),
            (
                concat!(
                    "declare module 'ambient' { import source a from 'a'; ",
                    "import source b from ''; import source c from './c'; }"
                ),
                1,
                None,
            ),
            (
                "export {}; declare module 'ambient' { import source a from 'a'; }",
                0,
                Some("ambient"),
            ),
        ] {
            let source = PreparedSourceFile::new(path.clone(), text);
            let (plan, _) =
                plan_module_requests_worker(&source, &options, true, false, None).unwrap();
            assert_eq!(plan.import_count(), expected_count, "{text}");
            assert!(
                plan.module_requests()
                    .iter()
                    .map(ResolutionKey::specifier)
                    .eq(expected_augmentation.map(JsStr::from)),
                "{text}"
            );
        }
    }

    #[test]
    fn dynamic_import_offsets_merge_raw_non_overlapping_hits() {
        let text = "😀 // requirequire importrequire requireimport\n'require require';";
        assert_eq!(
            dynamic_import_offsets(text.as_bytes()).collect::<Vec<_>>(),
            [8, 21, 27, 35, 42, 50, 58]
        );
        assert_eq!(dynamic_import_offsets(b"impor requir").next(), None);
    }

    #[test]
    fn dynamic_import_scan_keeps_repeated_occurrences_and_propagates_errors() {
        let path =
            crate::ProgramPath::from_js_parts("/input.js".into(), "/input.js".into()).unwrap();
        let source = PreparedSourceFile::new(
            path,
            "// require import\nrequire(/* require */ './dep.js');\nimport('./esm.js');\nrequire('./tail.js');",
        );
        let parsed = parse_source_file_from_snapshot(
            source.path().display().to_owned(),
            Arc::clone(source.snapshot()),
            source_request_parse_options(&source, &CompilerOptions::default()),
            None,
        );
        let statements = parsed
            .arena
            .node(parsed.root)
            .as_source_file()
            .unwrap()
            .statements
            .unwrap();
        let calls: Vec<_> = parsed
            .arena
            .node_array(statements)
            .nodes
            .iter()
            .map(|&id| {
                parsed
                    .arena
                    .node(id)
                    .as_expression_statement()
                    .unwrap()
                    .expression
                    .unwrap()
            })
            .collect();
        let expected = [calls[0], calls[0], calls[0], calls[0], calls[1], calls[2]];
        let mut seen = Vec::new();
        let count = for_each_dynamic_import(&parsed, |id| {
            seen.push(id);
            Ok(())
        })
        .unwrap();
        assert_eq!(count, expected.len());
        assert_eq!(seen, expected);

        seen.clear();
        let result = for_each_dynamic_import(&parsed, |id| {
            seen.push(id);
            if id == calls[1] {
                return Err(ResolutionError::invalid_data("stop at the import call"));
            }
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(seen, expected[..expected.len() - 1]);

        let (plan, _) =
            plan_retaining_syntax::<LoaderOccurrences>(&source, &CompilerOptions::default(), None)
                .unwrap();
        let records = &plan.occurrences.records;
        assert_eq!(records.len(), expected.len());
        assert!(records[..4].iter().all(|record| record == &records[0]));
        assert_ne!(records[4], records[0]);
    }

    #[test]
    fn planning_keeps_the_supplied_ordinary_box() {
        let path =
            crate::ProgramPath::from_js_parts("/input.ts".into(), "/input.ts".into()).unwrap();
        let source =
            PreparedSourceFile::new(path, "import { value } from 'pkg'; export { value };");
        let options = CompilerOptions::default();
        let expected = plan_module_requests_worker(&source, &options, true, false, None)
            .unwrap()
            .0;
        let address = std::cell::Cell::new(0);
        let mut output = None;
        let parsed = plan_module_requests_with_parser(
            &source,
            &options,
            true,
            false,
            None,
            &mut output,
            |parse_options| {
                let parsed = parse_source_file_from_snapshot(
                    source.path().display().to_owned(),
                    Arc::clone(source.snapshot()),
                    parse_options.clone(),
                    None,
                );
                let owner = Box::new(PreparsedSourceFile::new(parse_options, parsed));
                address.set(std::ptr::from_ref(owner.as_ref()) as usize);
                PlanningConstruction::Parsed(owner)
            },
        )
        .unwrap();
        let PlanningParse::Parsed(owner) = parsed else {
            panic!("the ordinary parse retains its owner");
        };
        assert_eq!(std::ptr::from_ref(owner.as_ref()) as usize, address.get());
        assert_eq!(output.unwrap(), expected);
        assert_eq!(
            owner.parse_options(),
            &source_request_parse_options(&source, &options)
        );
        assert!(Arc::ptr_eq(owner.source().snapshot(), source.snapshot()));
        assert!(!owner.is_fresh_library());
    }

    #[test]
    fn library_transport_retains_the_pristine_box_and_metadata_fallback() {
        // Exercise a real typed capability with its supported image projection,
        // independently of the command's process-wide JSDoc-mode selection.
        let name = "bundled:///libs/__load_order_owner_fixture.d.ts";
        let path = crate::ProgramPath::from_js_parts(name.into(), name.into()).unwrap();
        let source = PreparedSourceFile::new(
            path,
            "/// <reference lib=\"es5\" />\ndeclare const value: number;",
        );
        let image_options = ParseOptions {
            js_doc_parsing_mode: JSDocParsingMode::ParseForTypeErrors,
            ..ParseOptions::default()
        };
        let ordinary = parse_source_file_from_snapshot(
            name,
            Arc::clone(source.snapshot()),
            image_options.clone(),
            None,
        );
        let bytes = tsc_syntax::library_image::SyntaxImage::capture(&ordinary)
            .unwrap()
            .encode()
            .unwrap();
        let catalog = Box::leak(Box::new([tsc_syntax::library_image::StaticLibraryImage {
            name: "__load_order_owner_fixture.d.ts",
            source: Box::leak(Box::new(source.snapshot().shared_text())),
            source_fingerprint: tsc_syntax::library_image::fingerprint(source.text().as_bytes()),
            syntax: Box::leak(bytes.into_boxed_slice()),
            binding: &[],
            edits: &[],
        }]));
        tsc_syntax::library_image::register(catalog);
        let image = &catalog[0];
        let domain = tsc_types::IdentityDomain::new();
        let options = CompilerOptions::default();
        let expected = plan_module_requests_worker(&source, &options, true, false, None)
            .unwrap()
            .0;
        for deferred in [false, true] {
            let address = std::cell::Cell::new(0);
            let mut output = None;
            let parsed = plan_module_requests_with_parser(
                &source,
                &options,
                true,
                false,
                None,
                &mut output,
                |_| {
                    if deferred {
                        return PlanningConstruction::Deferred(
                            image
                                .metadata(
                                    source.path().display(),
                                    source.snapshot(),
                                    &image_options,
                                )
                                .unwrap(),
                        );
                    }
                    let fresh = image
                        .restore_fresh_in_identity_domain(
                            source.path().display().to_owned(),
                            Arc::clone(source.snapshot()),
                            &image_options,
                            &domain,
                        )
                        .unwrap();
                    let construction = PlanningConstruction::library(fresh, image_options.clone());
                    let PlanningConstruction::Library(owner) = &construction else {
                        panic!("a library construction retains its typed owner");
                    };
                    address.set(std::ptr::from_ref(owner.as_ref()) as usize);
                    construction
                },
            )
            .unwrap();
            assert_eq!(output.unwrap(), expected);
            if deferred {
                assert!(matches!(parsed, PlanningParse::Deferred));
                continue;
            }
            let PlanningParse::Parsed(mut owner) = parsed else {
                panic!("the library parse retains its owner");
            };
            assert_eq!(std::ptr::from_ref(owner.as_ref()) as usize, address.get());
            assert!(owner.is_fresh_library());
            assert_eq!(owner.parse_options(), &image_options);
            assert!(Arc::ptr_eq(owner.source().snapshot(), source.snapshot()));
            assert!(owner.take_bind_data().is_none());
            assert!(owner.take_bind_reservations().is_none());
            let _ = owner.source_mut();
            assert!(
                !owner.is_fresh_library(),
                "mutable access ends pristine eligibility"
            );
            assert_eq!(std::ptr::from_ref(owner.as_ref()) as usize, address.get());
        }
    }

    #[test]
    fn loader_occurrences_match_expanded_grouped_spans() {
        let path =
            crate::ProgramPath::from_js_parts("/main.mts".into(), "/main.mts".into()).unwrap();
        let text = concat!(
            "// 😀\nimport 'tslib'; import 'a'; import 'b'; import 'a';",
            "import c = require('a'); declare module 'aug' {}",
        );
        let source =
            PreparedSourceFile::new(path, text).with_implied_node_format(ResolutionMode::EsNext);
        let options = CompilerOptions {
            module: Some(199),
            module_resolution: Some(99),
            import_helpers: Some(true),
            trace_resolution: Some(true),
            ..CompilerOptions::default()
        };
        let (public, _) = plan_source_requests_retaining_syntax(&source, &options, None).unwrap();
        let (loader, _) =
            plan_retaining_syntax::<LoaderOccurrences>(&source, &options, None).unwrap();
        assert_eq!(loader.module_request_order, public.module_request_order());
        assert_eq!(loader.module_requests, public.module_requests());
        assert_eq!(loader.module_request_loads, public.module_request_loads);
        assert_eq!(loader.import_count(), 5);
        assert_eq!(loader.import_count(), public.import_count());
        assert_eq!(
            loader.observed_request_occurrence_count(),
            public.observed_request_occurrence_count()
        );
        assert_eq!(loader.occurrences.synthetic_rows, 1);
        let mut expected = Vec::new();
        let mut cursors = vec![0; public.module_requests().len()];
        for &row in public.module_request_occurrences() {
            let key = &public.module_requests()[row as usize];
            let spans = public.module_request_spans(key);
            let count = public
                .module_request_occurrences()
                .iter()
                .filter(|&&r| r == row)
                .count();
            let synthetic_count = count - spans.len();
            let cursor = &mut cursors[row as usize];
            let synthetic = *cursor < synthetic_count;
            let (pos, end) = if synthetic {
                (0, 0)
            } else {
                spans[*cursor - synthetic_count]
            };
            expected.push(ImportOccurrence {
                request: row,
                pos,
                end,
                synthetic,
            });
            *cursor += 1;
        }
        assert_eq!(loader.occurrences.records, expected);
        assert_eq!(
            expected[1].pos as usize,
            text[..text.find("'tslib'").unwrap()].encode_utf16().count()
        );
        assert!(!loader.module_request_loads.last().unwrap());
    }

    #[test]
    fn a_planning_error_after_parsing_leaves_the_output_empty() {
        let path =
            crate::ProgramPath::from_js_parts("/input.ts".into(), "/input.ts".into()).unwrap();
        let source = PreparedSourceFile::new(path, "import('pkg');");
        let options = CompilerOptions {
            module: Some(199),
            module_resolution: Some(99),
            ..CompilerOptions::default()
        };
        let parsed = std::cell::Cell::new(false);
        let mut output: Option<LoaderRequestPlan> = None;
        let result = plan_module_requests_with_parser(
            &source,
            &options,
            false,
            false,
            None,
            &mut output,
            |parse_options| {
                parsed.set(true);
                let parsed = parse_source_file_from_snapshot(
                    source.path().display().to_owned(),
                    Arc::clone(source.snapshot()),
                    parse_options.clone(),
                    None,
                );
                PlanningConstruction::parsed(parsed, parse_options)
            },
        );
        assert!(parsed.get(), "failure must follow parser construction");
        assert!(result.is_err(), "static planning rejects a dynamic import");
        assert!(output.is_none(), "partial request vectors remain private");
    }

    #[test]
    fn document_script_kinds_and_load_bearing_library_text_keep_the_regular_parser() {
        let name = "bundled:///libs/custom.d.ts";
        let path = crate::ProgramPath::from_js_parts(name.into(), name.into()).unwrap();
        let options = CompilerOptions::default();
        for text in [
            "import type { X } from 'pkg';",
            "/** @import { X } from 'pkg' */ declare const value: number;",
            "/** @import X 'pkg' */ declare const value: number;",
            "export { X } from 'pkg';",
            "export {} from 'pkg';",
            "declare module 'pkg' {}",
            "/// <reference types=\"node\" />",
            "/** @type {import('pkg').X} */ declare const x: unknown;",
            "declare namespace N { export {} }",
        ] {
            let source = PreparedSourceFile::new(path.clone(), text);
            let expected = plan_module_requests_worker(&source, &options, true, false, None)
                .unwrap()
                .0;
            let (actual, syntax) =
                plan_source_requests_retaining_syntax(&source, &options, None).unwrap();
            assert_eq!(actual, expected, "{text}");
            assert!(
                syntax.is_available(),
                "unsupported metadata shape must retain a real parse"
            );
        }
        let source =
            PreparedSourceFile::new(path, "{}").with_script_kind(tsc_syntax::ScriptKind::Json);
        let (_, syntax) = plan_source_requests_retaining_syntax(&source, &options, None).unwrap();
        assert!(syntax.is_available());
    }

    #[test]
    fn bundled_library_metadata_plans_equal_the_complete_parser() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../vendor/typescript-native/7.1.0-dev-aa814927/upstream/tsc/internal/bundled/libs",
        );
        let mut count = 0;
        let mut entries: Vec<_> = std::fs::read_dir(directory)
            .unwrap()
            .map(Result::unwrap)
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            if !entry.file_name().to_string_lossy().ends_with(".d.ts") {
                continue;
            }
            let name = format!("bundled:///libs/{}", entry.file_name().to_string_lossy());
            let path =
                crate::ProgramPath::from_js_parts(name.as_str().into(), name.as_str().into())
                    .unwrap();
            let source =
                PreparedSourceFile::new(path, std::fs::read_to_string(entry.path()).unwrap());
            let image_options = ParseOptions {
                js_doc_parsing_mode: JSDocParsingMode::ParseForTypeErrors,
                ..ParseOptions::default()
            };
            let ordinary_image_source = parse_source_file_from_snapshot(
                source.path().display().to_owned(),
                Arc::clone(source.snapshot()),
                image_options.clone(),
                None,
            );
            let image_bytes =
                tsc_syntax::library_image::SyntaxImage::capture(&ordinary_image_source)
                    .unwrap()
                    .encode()
                    .unwrap();
            let image = tsc_syntax::library_image::StaticLibraryImage {
                name: Box::leak(
                    entry
                        .file_name()
                        .to_string_lossy()
                        .into_owned()
                        .into_boxed_str(),
                ),
                source: Box::leak(Box::new(source.snapshot().shared_text())),
                source_fingerprint: tsc_syntax::library_image::fingerprint(
                    source.text().as_bytes(),
                ),
                syntax: Box::leak(image_bytes.into_boxed_slice()),
                binding: &[],
                edits: &[],
            };
            let metadata = image
                .metadata(source.path().display(), source.snapshot(), &image_options)
                .unwrap();
            for kind in [1, 99, 100, 199, 200] {
                let options = CompilerOptions {
                    module: Some(kind),
                    ..CompilerOptions::default()
                };
                let expected = plan_module_requests_worker(&source, &options, true, false, None)
                    .unwrap()
                    .0;
                let (actual, syntax) =
                    plan_source_requests_retaining_syntax(&source, &options, None).unwrap();
                assert_eq!(actual, expected, "{name}, module {kind}");
                let baked =
                    library_image_plan(metadata.is_external_module, metadata.references.clone());
                assert_eq!(baked, expected, "image {name}, module {kind}");
                assert!(syntax.take().is_none(), "{name} was not deferred");
            }
            count += 1;
        }
        assert!(count >= 100, "complete bundled catalog, got {count}");
    }
}

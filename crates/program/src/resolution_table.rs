//! Immutable `resolvedModules[source][specifier, mode]` storage.
use super::*;
use crate::prepared::PreparedSourceFile;
use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::ops::Range;

const EXTERNAL: u8 = 1;
const TS_EXTENSION: u8 = 2;
const EXTRA_EXTENSION: u8 = 4;
const CUSTOM: u8 = 8;
const TYPES_PACKAGE: u8 = 16;
const BUNDLES_TYPES: u8 = 32;

fn flag(value: bool, mask: u8) -> u8 {
    if value {
        mask
    } else {
        0
    }
}

/// The text of arbitrary extensions lives only in `Rare`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
enum ExtensionTag {
    Ts,
    Tsx,
    Dts,
    Js,
    Jsx,
    Json,
    Mjs,
    Mts,
    Dmts,
    Cjs,
    Cts,
    Dcts,
    Arbitrary,
}

impl ExtensionTag {
    fn from_owned(extension: ModuleExtension) -> (Self, Option<ModuleExtension>) {
        use ModuleExtension as E;
        let tag = match extension {
            E::Ts => Self::Ts,
            E::Tsx => Self::Tsx,
            E::Dts => Self::Dts,
            E::Js => Self::Js,
            E::Jsx => Self::Jsx,
            E::Json => Self::Json,
            E::Mjs => Self::Mjs,
            E::Mts => Self::Mts,
            E::Dmts => Self::Dmts,
            E::Cjs => Self::Cjs,
            E::Cts => Self::Cts,
            E::Dcts => Self::Dcts,
            arbitrary @ E::Arbitrary(_) => return (Self::Arbitrary, Some(arbitrary)),
        };
        (tag, None)
    }

    fn view(self, rare: Option<&Rare>) -> &ModuleExtension {
        match self {
            Self::Ts => &ModuleExtension::Ts,
            Self::Tsx => &ModuleExtension::Tsx,
            Self::Dts => &ModuleExtension::Dts,
            Self::Js => &ModuleExtension::Js,
            Self::Jsx => &ModuleExtension::Jsx,
            Self::Json => &ModuleExtension::Json,
            Self::Mjs => &ModuleExtension::Mjs,
            Self::Mts => &ModuleExtension::Mts,
            Self::Dmts => &ModuleExtension::Dmts,
            Self::Cjs => &ModuleExtension::Cjs,
            Self::Cts => &ModuleExtension::Cts,
            Self::Dcts => &ModuleExtension::Dcts,
            Self::Arbitrary => rare
                .and_then(|r| r.arbitrary_extension.as_ref())
                .expect("arbitrary extension text"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Target {
    NotFound,
    Source(SourceFileId),
    // Unloaded paths cannot borrow a source. Keep their infrequent owned
    // paths outside the inline row, and account for these boxes separately.
    Unloaded {
        path: Box<ProgramPath>,
        reason: UnloadedModuleReason,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Rare {
    resolved_file: Option<ProgramPath>,
    original_path: Option<ProgramPath>,
    alternate_result: Option<ProgramPath>,
    diagnostics: DiagnosticList,
    arbitrary_extension: Option<ModuleExtension>,
}

impl Rare {
    fn is_empty(&self) -> bool {
        self.resolved_file.is_none()
            && self.original_path.is_none()
            && self.alternate_result.is_none()
            && self.diagnostics.is_empty()
            && self.arbitrary_extension.is_none()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Row {
    specifier: JsString,
    target: Target,
    rare: Option<Box<Rare>>,
    importing_source: SourceFileId,
    package_id: Option<NonZeroU32>,
    mode: ResolutionMode,
    extension: ExtensionTag,
    flags: u8,
}

const _: () = assert!(
    std::mem::size_of::<Row>() <= 88,
    "module row crossed the 88-byte cost class"
);
const _: () = assert!(std::mem::size_of::<ExtensionTag>() == 1);

/// Staging owns each specifier once. The per-source sorted indices detect
/// conflicts at add time without constructing a tree of fat key/value rows.
#[derive(Clone, Debug, Default)]
pub(crate) struct StagedModules {
    rows: Vec<StagedRow>,
    by_source: Vec<Vec<usize>>,
}

#[derive(Clone, Debug)]
struct StagedRow {
    source: SourceFileId,
    specifier: JsString,
    mode: ResolutionMode,
    resolution: ModuleResolution,
}

impl StagedModules {
    pub(crate) fn insert(
        &mut self,
        source: SourceFileId,
        key: ResolutionKey,
        resolution: ModuleResolution,
    ) -> Result<(), crate::PreparationError> {
        self.by_source
            .resize_with(self.by_source.len().max(source.index() + 1), Vec::new);
        let indices = &mut self.by_source[source.index()];
        match indices.binary_search_by(|index| {
            let row = &self.rows[*index];
            (row.specifier.as_js(), row.mode).cmp(&(key.specifier(), key.mode()))
        }) {
            Ok(index) if self.rows[indices[index]].resolution == resolution => Ok(()),
            Ok(_) => Err(crate::PreparationError::new_js(
                crate::PreparationErrorKind::IdentityConflict,
                crate::PreparationOperation::AddModuleResolution,
                Some(key.source().as_js()),
                format!(
                    "resolution key ({:?}, {:?}) has conflicting outcomes",
                    key.specifier(),
                    key.mode()
                ),
            )),
            Err(index) => {
                indices.insert(index, self.rows.len());
                self.rows.push(StagedRow {
                    source,
                    specifier: key.specifier,
                    mode: key.mode,
                    resolution,
                });
                Ok(())
            }
        }
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = &ModuleResolution> {
        self.rows.iter().map(|row| &row.resolution)
    }
}

/// Lookup order is separate from the observable (path, specifier, mode)
/// row order. A hash narrows each lookup; full equality handles collisions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ModuleLookup {
    hash: u64,
    row: u32,
}

fn module_lookup_hash(specifier: JsStr<'_>, mode: ResolutionMode) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    (specifier, mode).hash(&mut hasher);
    hasher.finish()
}

fn module_lookup_index(modules: &[Row], ranges: &[Range<u32>]) -> Vec<ModuleLookup> {
    let mut index: Vec<_> = modules
        .iter()
        .enumerate()
        .map(|(row, module)| ModuleLookup {
            hash: module_lookup_hash(module.specifier.as_js(), module.mode),
            row: u32::try_from(row).expect("module row index fits u32"),
        })
        .collect();
    for range in ranges {
        index[range.start as usize..range.end as usize]
            .sort_unstable_by_key(|entry| (entry.hash, entry.row));
    }
    index
}

/// Authoritative tables, frozen at `PreparedProgramBuilder::build()`.
/// Module rows retain the old `(canonical path, specifier, mode)` byte order.
/// There is no mutation API; borrowed views cannot replace their facts.
///
/// ```compile_fail
/// fn mutate(program: &tsc_program::PreparedProgram) {
///     program.resolutions().modules_mut().clear();
/// }
/// ```
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResolutionTable {
    modules: Vec<Row>,
    module_lookup: Vec<ModuleLookup>,
    source_ranges: Vec<Range<u32>>,
    package_ids: Vec<PackageId>,
    type_references: BTreeMap<TypeReferenceResolutionKey, TypeReferenceResolution>,
}

impl ResolutionTable {
    pub(crate) fn build(
        staged: StagedModules,
        type_references: BTreeMap<TypeReferenceResolutionKey, TypeReferenceResolution>,
        sources: &[PreparedSourceFile],
    ) -> Self {
        let StagedModules { rows, by_source } = staged;
        // Staging already sorts each source's indices by (specifier, mode).
        // Source canonical paths are unique in a prepared program; ordering
        // the nonempty groups therefore reproduces the old global row sort.
        let mut groups: Vec<_> = by_source
            .iter()
            .enumerate()
            .filter_map(|(source, indices)| (!indices.is_empty()).then_some(source))
            .collect();
        groups.sort_unstable_by(|a, b| {
            sources[*a]
                .path()
                .canonical()
                .cmp(sources[*b].path().canonical())
        });
        // Take each staged row once, without cloning its owned facts. The
        // optional slots retain staging's allocation while rows are frozen.
        let mut rows: Vec<_> = rows.into_iter().map(Some).collect();
        let mut modules = Vec::with_capacity(rows.len());
        let mut source_ranges = vec![0..0; sources.len()];
        let mut packages = BTreeMap::new();
        for index in groups
            .into_iter()
            .flat_map(|source| by_source[source].iter().copied())
        {
            let StagedRow {
                source: importing_source,
                specifier,
                mode,
                resolution,
            } = rows[index]
                .take()
                .expect("each staged row has one source index");
            let start = u32::try_from(modules.len()).expect("module row index fits u32");
            let range = &mut source_ranges[importing_source.index()];
            if range.start == range.end {
                range.start = start;
            }
            range.end = start.checked_add(1).expect("module range fits u32");
            let ModuleResolution {
                outcome,
                alternate_result,
                types_package_exists,
                package_bundles_types,
                diagnostics,
            } = resolution;
            let mut rare = Rare {
                alternate_result,
                diagnostics,
                ..Rare::default()
            };
            let mut flags = flag(types_package_exists, TYPES_PACKAGE)
                | flag(package_bundles_types, BUNDLES_TYPES);
            let mut extension = ExtensionTag::Ts;
            let mut package_id = None;
            let target = match outcome {
                ResolutionOutcome::NotFound => Target::NotFound,
                ResolutionOutcome::Resolved(module) => {
                    let ResolvedModule {
                        target,
                        extension: ext,
                        original_path,
                        is_external_library_import,
                        resolved_using_ts_extension,
                        resolved_using_extra_extensions,
                        is_custom_resolution,
                        package_id: package,
                    } = module;
                    (extension, rare.arbitrary_extension) = ExtensionTag::from_owned(ext);
                    rare.original_path = original_path;
                    flags |= flag(is_external_library_import, EXTERNAL)
                        | flag(resolved_using_ts_extension, TS_EXTENSION)
                        | flag(resolved_using_extra_extensions, EXTRA_EXTENSION)
                        | flag(is_custom_resolution, CUSTOM);
                    if let Some(package) = package {
                        let next = NonZeroU32::new(
                            u32::try_from(packages.len() + 1).expect("package id fits u32"),
                        )
                        .unwrap();
                        package_id = Some(*packages.entry(package).or_insert(next));
                    }
                    match target {
                        ResolvedModuleTarget::Source {
                            source,
                            resolved_file,
                        } => {
                            // ProgramPath equality includes BOTH display and canonical.
                            if &resolved_file != sources[source.index()].path() {
                                rare.resolved_file = Some(resolved_file);
                            }
                            Target::Source(source)
                        }
                        ResolvedModuleTarget::Unloaded {
                            resolved_file,
                            reason,
                        } => Target::Unloaded {
                            path: Box::new(resolved_file),
                            reason,
                        },
                    }
                }
            };
            modules.push(Row {
                specifier,
                mode,
                importing_source,
                target,
                extension,
                flags,
                package_id,
                rare: (!rare.is_empty()).then(|| Box::new(rare)),
            });
        }
        // The temporary interning index owns the identities during building;
        // consume it so the immutable side table owns each identity only once.
        let mut packages = packages
            .into_iter()
            .map(|(package, id)| (id, package))
            .collect::<Vec<_>>();
        packages.sort_unstable_by_key(|(id, _)| *id);
        let package_ids = packages.into_iter().map(|(_, package)| package).collect();
        Self {
            module_lookup: module_lookup_index(&modules, &source_ranges),
            modules,
            source_ranges,
            package_ids,
            type_references,
        }
    }

    pub(crate) fn view<'a>(
        &'a self,
        sources: &'a [PreparedSourceFile],
        source_by_canonical: &'a rustc_hash::FxHashMap<CanonicalPath, SourceFileId>,
    ) -> ResolutionTableView<'a> {
        ResolutionTableView {
            table: self,
            sources,
            source_by_canonical,
        }
    }
}

/// Borrowed access to the immutable table and the source paths it shares.
#[derive(Clone, Copy, Debug)]
pub struct ResolutionTableView<'a> {
    table: &'a ResolutionTable,
    sources: &'a [PreparedSourceFile],
    source_by_canonical: &'a rustc_hash::FxHashMap<CanonicalPath, SourceFileId>,
}

impl PartialEq for ResolutionTableView<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.module_len() == other.module_len()
            && self.type_references().eq(other.type_references())
            && self
                .modules()
                .zip(other.modules())
                .all(|((a, left), (b, right))| {
                    a.source() == b.source()
                        && a.specifier() == b.specifier()
                        && a.mode() == b.mode()
                        && left == right
                })
    }
}
impl Eq for ResolutionTableView<'_> {}

impl<'a> ResolutionTableView<'a> {
    pub fn module(
        self,
        source: SourceFileId,
        specifier: JsStr<'_>,
        mode: ResolutionMode,
    ) -> Option<ModuleResolutionView<'a>> {
        let range = self.table.source_ranges.get(source.index())?;
        let index = &self.table.module_lookup[range.start as usize..range.end as usize];
        if index.is_empty() {
            return None;
        }
        let hash = module_lookup_hash(specifier, mode);
        let start = index.partition_point(|entry| entry.hash < hash);
        let row = index[start..]
            .iter()
            .take_while(|entry| entry.hash == hash)
            .map(|entry| &self.table.modules[entry.row as usize])
            .find(|row| row.mode == mode && row.specifier.as_js() == specifier)?;
        Some(ModuleResolutionView {
            row,
            table: self.table,
            sources: self.sources,
        })
    }

    pub fn module_by_path(
        self,
        path: &CanonicalPath,
        specifier: JsStr<'_>,
        mode: ResolutionMode,
    ) -> Option<ModuleResolutionView<'a>> {
        let source = *self.source_by_canonical.get(path)?;
        if self.sources.get(source.index())?.path().canonical() != path {
            return None;
        }
        self.module(source, specifier, mode)
    }

    /// Compatibility/error-producing path entry point. Hot lookups use
    /// `PreparedProgram::module_resolution` and never construct a key.
    pub fn require_module(
        self,
        key: &ResolutionKey,
    ) -> Result<ModuleResolutionView<'a>, MissingResolutionError> {
        self.module_by_path(key.source(), key.specifier(), key.mode())
            .ok_or_else(|| MissingResolutionError::module(key))
    }

    pub fn require_type_reference(
        self,
        key: &TypeReferenceResolutionKey,
    ) -> Result<&'a TypeReferenceResolution, MissingResolutionError> {
        self.table
            .type_references
            .get(key)
            .ok_or_else(|| MissingResolutionError::type_reference(key))
    }

    pub fn type_references(
        self,
    ) -> impl ExactSizeIterator<Item = (&'a TypeReferenceResolutionKey, &'a TypeReferenceResolution)>
    {
        self.table.type_references.iter()
    }

    pub fn modules(
        self,
    ) -> impl ExactSizeIterator<Item = (ResolutionKeyView<'a>, ModuleResolutionView<'a>)> {
        self.table.modules.iter().map(move |row| {
            (
                ResolutionKeyView {
                    source: self.sources[row.importing_source.index()]
                        .path()
                        .canonical(),
                    source_id: row.importing_source,
                    specifier: row.specifier.as_js(),
                    mode: row.mode,
                },
                ModuleResolutionView {
                    row,
                    table: self.table,
                    sources: self.sources,
                },
            )
        })
    }

    pub fn module_len(self) -> usize {
        self.table.modules.len()
    }
    pub fn type_reference_len(self) -> usize {
        self.table.type_references.len()
    }

    /// Allocation payloads only: excludes allocator/node headers and strings.
    pub fn module_storage(self) -> ModuleStorageStats {
        let rare_boxes = self
            .table
            .modules
            .iter()
            .filter(|row| row.rare.is_some())
            .count();
        let unloaded_paths = self
            .table
            .modules
            .iter()
            .filter(|row| matches!(row.target, Target::Unloaded { .. }))
            .count();
        ModuleStorageStats {
            row_size: std::mem::size_of::<Row>(),
            row_bytes: self.table.modules.capacity() * std::mem::size_of::<Row>(),
            range_bytes: self.table.source_ranges.capacity() * std::mem::size_of::<Range<u32>>(),
            index_bytes: self.table.module_lookup.capacity() * std::mem::size_of::<ModuleLookup>(),
            rare_boxes,
            rare_bytes: rare_boxes * std::mem::size_of::<Rare>(),
            unloaded_paths,
            unloaded_path_bytes: unloaded_paths * std::mem::size_of::<ProgramPath>(),
            package_ids: self.table.package_ids.len(),
            package_id_bytes: self.table.package_ids.capacity() * std::mem::size_of::<PackageId>(),
            package_string_bytes: self
                .table
                .package_ids
                .iter()
                .map(|id| {
                    id.name.heap_bytes()
                        + id.submodule_name.heap_bytes()
                        + id.version.heap_bytes()
                        + id.peer_dependencies
                            .as_ref()
                            .map_or(0, JsString::heap_bytes)
                })
                .sum(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleStorageStats {
    pub row_size: usize,
    pub row_bytes: usize,
    pub range_bytes: usize,
    pub index_bytes: usize,
    pub rare_boxes: usize,
    pub rare_bytes: usize,
    pub unloaded_paths: usize,
    pub unloaded_path_bytes: usize,
    pub package_ids: usize,
    pub package_id_bytes: usize,
    pub package_string_bytes: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct ResolutionKeyView<'a> {
    source: &'a CanonicalPath,
    source_id: SourceFileId,
    specifier: JsStr<'a>,
    mode: ResolutionMode,
}
impl PartialEq for ResolutionKeyView<'_> {
    fn eq(&self, other: &Self) -> bool {
        (self.source, self.specifier, self.mode) == (other.source, other.specifier, other.mode)
    }
}
impl Eq for ResolutionKeyView<'_> {}

impl<'a> ResolutionKeyView<'a> {
    pub fn source(self) -> &'a CanonicalPath {
        self.source
    }
    pub fn source_id(self) -> SourceFileId {
        self.source_id
    }
    pub fn specifier(self) -> JsStr<'a> {
        self.specifier
    }
    pub fn mode(self) -> ResolutionMode {
        self.mode
    }
    pub fn to_owned(self) -> ResolutionKey {
        ResolutionKey::new(self.source.clone(), self.specifier, self.mode)
    }
}

#[derive(Clone, Copy)]
pub struct ModuleResolutionView<'a> {
    row: &'a Row,
    table: &'a ResolutionTable,
    sources: &'a [PreparedSourceFile],
}
impl fmt::Debug for ModuleResolutionView<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModuleResolutionView")
            .field("outcome", &self.outcome())
            .field("alternate_result", &self.alternate_result())
            .field("types_package_exists", &self.types_package_exists())
            .field("package_bundles_types", &self.package_bundles_types())
            .field("diagnostics", &self.diagnostics())
            .finish()
    }
}
impl PartialEq for ModuleResolutionView<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.outcome() == other.outcome()
            && self.alternate_result() == other.alternate_result()
            && self.types_package_exists() == other.types_package_exists()
            && self.package_bundles_types() == other.package_bundles_types()
            && self.diagnostics() == other.diagnostics()
    }
}
impl Eq for ModuleResolutionView<'_> {}

impl<'a> ModuleResolutionView<'a> {
    pub fn outcome(self) -> ResolutionOutcome<ResolvedModuleView<'a>> {
        let rare = self.row.rare.as_deref();
        let target = match &self.row.target {
            Target::NotFound => return ResolutionOutcome::NotFound,
            Target::Source(source) => ResolvedModuleTarget::Source {
                source: *source,
                resolved_file: rare
                    .and_then(|r| r.resolved_file.as_ref())
                    .unwrap_or_else(|| self.sources[source.index()].path()),
            },
            Target::Unloaded { path, reason } => ResolvedModuleTarget::Unloaded {
                resolved_file: path.as_ref(),
                reason: *reason,
            },
        };
        ResolutionOutcome::Resolved(ResolvedModuleView {
            target,
            extension: self.row.extension.view(rare),
            original_path: rare.and_then(|r| r.original_path.as_ref()),
            flags: self.row.flags & (EXTERNAL | TS_EXTENSION | EXTRA_EXTENSION | CUSTOM),
            package_id: self
                .row
                .package_id
                .map(|id| &self.table.package_ids[id.get() as usize - 1]),
        })
    }
    pub fn alternate_result(self) -> Option<&'a ProgramPath> {
        self.row
            .rare
            .as_deref()
            .and_then(|r| r.alternate_result.as_ref())
    }
    pub fn types_package_exists(self) -> bool {
        self.row.flags & TYPES_PACKAGE != 0
    }
    pub fn package_bundles_types(self) -> bool {
        self.row.flags & BUNDLES_TYPES != 0
    }
    pub fn diagnostics(self) -> &'a [Diagnostic] {
        self.row
            .rare
            .as_deref()
            .map_or(&[], |r| r.diagnostics.as_slice())
    }

    /// Materialize an owned staging value for clients that need a new builder.
    pub fn to_owned(self) -> ModuleResolution {
        ModuleResolution {
            outcome: match self.outcome() {
                ResolutionOutcome::NotFound => ResolutionOutcome::NotFound,
                ResolutionOutcome::Resolved(module) => {
                    ResolutionOutcome::Resolved(module.to_owned())
                }
            },
            alternate_result: self.alternate_result().cloned(),
            types_package_exists: self.types_package_exists(),
            package_bundles_types: self.package_bundles_types(),
            diagnostics: self.diagnostics().to_vec(),
        }
    }
}

/// Resolved facts borrowing the table's rare metadata and source paths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedModuleView<'a> {
    target: ResolvedModuleTarget<&'a ProgramPath>,
    extension: &'a ModuleExtension,
    original_path: Option<&'a ProgramPath>,
    package_id: Option<&'a PackageId>,
    flags: u8,
}
impl<'a> ResolvedModuleView<'a> {
    pub fn target(&self) -> &ResolvedModuleTarget<&'a ProgramPath> {
        &self.target
    }
    pub fn extension(&self) -> &'a ModuleExtension {
        self.extension
    }
    pub fn original_path(&self) -> Option<&'a ProgramPath> {
        self.original_path
    }
    pub fn package_id(&self) -> Option<&'a PackageId> {
        self.package_id
    }
    pub fn is_external_library_import(&self) -> bool {
        self.flags & EXTERNAL != 0
    }
    pub fn resolved_using_ts_extension(&self) -> bool {
        self.flags & TS_EXTENSION != 0
    }
    pub fn resolved_using_extra_extensions(&self) -> bool {
        self.flags & EXTRA_EXTENSION != 0
    }
    pub fn is_custom_resolution(&self) -> bool {
        self.flags & CUSTOM != 0
    }
    pub fn to_owned(self) -> ResolvedModule {
        let target = match self.target {
            ResolvedModuleTarget::Source {
                source,
                resolved_file,
            } => ResolvedModuleTarget::Source {
                source,
                resolved_file: resolved_file.clone(),
            },
            ResolvedModuleTarget::Unloaded {
                resolved_file,
                reason,
            } => ResolvedModuleTarget::Unloaded {
                resolved_file: resolved_file.clone(),
                reason,
            },
        };
        ResolvedModule {
            target,
            extension: self.extension.clone(),
            original_path: self.original_path.cloned(),
            package_id: self.package_id.cloned(),
            is_external_library_import: self.is_external_library_import(),
            resolved_using_ts_extension: self.resolved_using_ts_extension(),
            resolved_using_extra_extensions: self.resolved_using_extra_extensions(),
            is_custom_resolution: self.is_custom_resolution(),
        }
    }
}

impl ResolutionTable {
    pub(crate) fn add_memory_usage(&self, usage: &mut tsc_diagnostics::MemoryUsage) {
        usage.vec("resolution rows", &self.modules);
        usage.vec("resolution lookup and ranges", &self.module_lookup);
        usage.vec("resolution lookup and ranges", &self.source_ranges);
        usage.vec("resolution package ids", &self.package_ids);
        for package in &self.package_ids {
            package.add_memory_usage(usage);
        }
        for row in &self.modules {
            usage.js_string("strings: stable owners", &row.specifier);
            if let Target::Unloaded { path, .. } = &row.target {
                usage.add(
                    "resolution boxed paths",
                    1,
                    std::mem::size_of::<ProgramPath>(),
                    std::mem::size_of::<ProgramPath>(),
                );
                path.add_memory_usage(usage);
            }
            if let Some(rare) = &row.rare {
                usage.add(
                    "resolution rare rows",
                    1,
                    std::mem::size_of::<Rare>(),
                    std::mem::size_of::<Rare>(),
                );
                for path in [
                    &rare.resolved_file,
                    &rare.original_path,
                    &rare.alternate_result,
                ]
                .into_iter()
                .flatten()
                {
                    path.add_memory_usage(usage);
                }
                usage.diagnostics(&rare.diagnostics);
                if let Some(ModuleExtension::Arbitrary(text)) = &rare.arbitrary_extension {
                    usage.js_string("strings: stable owners", text);
                }
            }
        }
        usage.btree(
            "type-reference resolution tree (estimate)",
            &self.type_references,
        );
        for (key, value) in &self.type_references {
            key.origin.canonical_path().add_memory_usage(usage);
            usage.js_string("strings: stable owners", &key.specifier);
            usage.diagnostics(&value.diagnostics);
            if let ResolutionOutcome::Resolved(target) = &value.outcome {
                target.target.add_memory_usage(usage);
                if let Some(path) = &target.original_path {
                    path.add_memory_usage(usage);
                }
                if let Some(package) = &target.package_id {
                    package.add_memory_usage(usage);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/resolution_table.rs"]
mod tests;

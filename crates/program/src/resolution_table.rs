//! Immutable `resolvedModules[source][specifier, mode]` storage.
use super::*;
use crate::prepared::PreparedSourceFile;
use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::ops::Range;
use std::sync::Arc;

#[path = "prepared_package_maps.rs"]
mod package_maps;
pub(crate) use package_maps::PreparedPackageMaps;
#[path = "loaded_package_ids.rs"]
mod loaded_packages;
use loaded_packages::LoadedPackageIds;
#[path = "resolution_templates.rs"]
mod templates;
pub(crate) use templates::{
    ModuleTemplates, TemplateDisposition, TemplateHandle, TemplatePreparation,
};

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
    package_id: Option<NonZeroU32>,
    mode: ResolutionMode,
    extension: ExtensionTag,
    flags: u8,
}

const _: () = assert!(
    std::mem::size_of::<Row>() <= 56,
    "module row crossed the 56-byte cost class"
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
    /// A queued target with one spelling and no project/package redirect
    /// has exactly the spelling of its collected source.
    exact_source_path: bool,
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
                    exact_source_path: false,
                });
                Ok(())
            }
        }
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = &ModuleResolution> {
        self.rows.iter().map(|row| &row.resolution)
    }
}

// Package comparison and hashing use only the immutable PackageId value.
#[allow(clippy::mutable_key_type)]
fn compact_module_row(
    specifier: JsString,
    mode: ResolutionMode,
    resolution: ModuleResolution,
    exact_source_path: bool,
    sources: &[PreparedSourceFile],
    packages: &mut LoadedPackageIds,
) -> Row {
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
    let mut flags =
        flag(types_package_exists, TYPES_PACKAGE) | flag(package_bundles_types, BUNDLES_TYPES);
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
                package_id = Some(packages.intern_owned(package));
            }
            match target {
                ResolvedModuleTarget::Source {
                    source,
                    resolved_file,
                } => {
                    // ProgramPath equality includes BOTH display and canonical.
                    debug_assert!(
                        !exact_source_path || &resolved_file == sources[source.index()].path()
                    );
                    if !exact_source_path && &resolved_file != sources[source.index()].path() {
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
    Row {
        specifier,
        mode,
        target,
        extension,
        flags,
        package_id,
        rare: (!rare.is_empty()).then(|| Box::new(rare)),
    }
}

/// Source-local permutations; neither depends on final source identities.
#[derive(Default)]
pub(crate) struct PreparedModuleIndices {
    order: Vec<u32>,
    lookup: Vec<ModuleLookup>,
}

impl PreparedModuleIndices {
    pub(crate) fn new(keys: &[ResolutionKey]) -> Self {
        let mut order: Vec<_> = (0..keys.len())
            .map(|i| u32::try_from(i).expect("source module count fits u32"))
            .collect();
        order.sort_unstable_by(|&a, &b| {
            let a = &keys[a as usize];
            let b = &keys[b as usize];
            (a.specifier(), a.mode()).cmp(&(b.specifier(), b.mode()))
        });
        let mut lookup: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(row, key)| ModuleLookup {
                hash: module_lookup_hash(key.specifier(), key.mode()),
                row: u32::try_from(row).expect("source module count fits u32"),
            })
            .collect();
        lookup.sort_unstable_by_key(|entry| (entry.hash, entry.row));
        Self { order, lookup }
    }
}

/// A final-format row with source membership and package interning deferred.
/// The symbolic path and manifest observation are released at publication.
#[derive(Clone)]
pub(crate) struct PreparedModule {
    row: Row,
    path: Option<ProgramPath>,
    package: Option<Arc<PreparedPackage>>,
}

#[derive(Clone)]
struct PreparedPackage {
    id: Option<PreparedPackageRef>,
    metadata: Option<std::sync::Arc<crate::PackageMetadata>>,
}

#[derive(Clone, Copy)]
pub(crate) struct PreparedResolvedModule<'a>(&'a PreparedModule);

impl<'a> PreparedResolvedModule<'a> {
    pub(crate) fn resolved_file(self) -> &'a ProgramPath {
        self.0.path.as_ref().expect("resolved path")
    }
    pub(crate) fn extension(self) -> &'a ModuleExtension {
        self.0.row.extension.view(self.0.row.rare.as_deref())
    }
    pub(crate) fn original_path(self) -> Option<&'a ProgramPath> {
        self.0
            .row
            .rare
            .as_ref()
            .and_then(|r| r.original_path.as_ref())
    }
    pub(crate) fn package_identity(self) -> Option<&'a PreparedPackageRef> {
        self.0
            .package
            .as_ref()
            .and_then(|package| package.id.as_ref())
    }
    pub(crate) fn package_metadata(self) -> Option<&'a crate::PackageMetadata> {
        self.0.package.as_ref().and_then(|p| p.metadata.as_deref())
    }
    pub(crate) fn is_external_library_import(self) -> bool {
        self.0.row.flags & EXTERNAL != 0
    }
    pub(crate) fn resolved_using_extra_extensions(self) -> bool {
        self.0.row.flags & EXTRA_EXTENSION != 0
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ModuleBinding {
    pub source: SourceFileId,
    pub target: Option<SourceFileId>,
    pub loads_source: bool,
    pub unloaded_reason: Option<UnloadedModuleReason>,
    pub no_resolve: bool,
    pub exact_path: bool,
}

impl PreparedModule {
    pub(crate) fn new(
        host: crate::module_resolution::HostModuleResolution,
        mode: ResolutionMode,
        identities: &PreparedPackageIds,
    ) -> Self {
        let parts = host.into_load_parts();
        let mut rare = Rare {
            diagnostics: parts.diagnostics,
            ..Rare::default()
        };
        let mut extension = ExtensionTag::Ts;
        let mut flags = 0;
        let mut package = None;
        let path = parts.module.map(|module| {
            (extension, rare.arbitrary_extension) = ExtensionTag::from_owned(module.extension);
            rare.original_path = module.original_path;
            rare.alternate_result = module.alternate_result;
            flags = flag(module.external, EXTERNAL)
                | flag(module.ts_extension, TS_EXTENSION)
                | flag(module.extra_extension, EXTRA_EXTENSION)
                | flag(module.custom, CUSTOM);
            if module.package_id.is_some() || module.package_metadata.is_some() {
                package = Some(Arc::new(PreparedPackage {
                    id: module.package_id.map(|id| identities.intern(id)),
                    metadata: module.package_metadata,
                }));
            }
            module.resolved_file
        });
        Self {
            row: Row {
                specifier: JsString::default(),
                mode,
                target: Target::NotFound,
                rare: (!rare.is_empty()).then(|| Box::new(rare)),
                package_id: None,
                extension,
                flags,
            },
            path,
            package,
        }
    }
    pub(crate) fn resolved(&self) -> Option<PreparedResolvedModule<'_>> {
        self.path.as_ref().map(|_| PreparedResolvedModule(self))
    }
    pub(crate) fn needs_validation(&self) -> bool {
        self.row.flags & CUSTOM != 0
            || self
                .row
                .rare
                .as_ref()
                .is_some_and(|r| !r.diagnostics.is_empty())
    }
    pub(crate) fn into_host(self) -> crate::module_resolution::HostModuleResolution {
        let rare = self.row.rare.map(|r| *r).unwrap_or_default();
        let module = self.path.map(|resolved_file| {
            let (package_id, package_metadata) = self
                .package
                .map(|p| {
                    (
                        p.id.as_ref().map(|id| id.value().clone()),
                        p.metadata.clone(),
                    )
                })
                .unwrap_or_default();
            crate::module_resolution::LoadResolvedModuleParts {
                resolved_file,
                extension: self.row.extension.view(Some(&rare)).clone(),
                original_path: rare.original_path.clone(),
                alternate_result: rare.alternate_result.clone(),
                package_id,
                package_metadata,
                external: self.row.flags & EXTERNAL != 0,
                ts_extension: self.row.flags & TS_EXTENSION != 0,
                extra_extension: self.row.flags & EXTRA_EXTENSION != 0,
                custom: self.row.flags & CUSTOM != 0,
            }
        });
        crate::module_resolution::HostModuleResolution::from_load_parts(
            crate::module_resolution::LoadModuleParts {
                module,
                diagnostics: rare.diagnostics,
            },
        )
    }
    // The lazy display cache does not participate in package ordering.
    #[allow(clippy::mutable_key_type)]
    fn bind(
        mut self,
        binding: &ModuleBinding,
        sources: &[PreparedSourceFile],
        package_map: &PreparedPackageMaps,
        packages: &mut LoadedPackageIds,
        package_indices: &mut Vec<Option<NonZeroU32>>,
    ) -> Result<Row, ResolutionError> {
        if let Some(path) = self.path.take() {
            let extension = self.row.extension.view(self.row.rare.as_deref());
            let membership = if binding.no_resolve && binding.target.is_none() {
                Err(binding
                    .unloaded_reason
                    .unwrap_or(UnloadedModuleReason::NoResolve))
            } else if let (None, Some(reason)) = (binding.target, binding.unloaded_reason) {
                Err(reason)
            } else if extension.is_javascript() && binding.target.is_none() {
                return Err(ResolutionError::unsupported(
                    "unexplained-unloaded-javascript",
                    format!(
                        "resolved JavaScript target {} has no source-membership exclusion",
                        path.display().to_string_lossy()
                    ),
                ));
            } else if matches!(extension, ModuleExtension::Arbitrary(text) if text.starts_with(".d.") && text.ends_with(".ts"))
                && binding.target.is_none()
            {
                Err(if binding.loads_source {
                    UnloadedModuleReason::ArbitraryExtensionWithoutOption
                } else {
                    UnloadedModuleReason::ResolutionOnly
                })
            } else if let Some(source) = binding.target {
                Ok(source)
            } else {
                return Err(ResolutionError::unsupported(
                    "resolution-only-source-target",
                    format!(
                        "resolved non-JavaScript target {} has no independent program membership",
                        path.display().to_string_lossy()
                    ),
                ));
            };
            self.row.target = match membership {
                Ok(source) => {
                    debug_assert!(!binding.exact_path || &path == sources[source.index()].path());
                    if !binding.exact_path && &path != sources[source.index()].path() {
                        self.row
                            .rare
                            .get_or_insert_with(|| Box::new(Rare::default()))
                            .resolved_file = Some(path);
                    }
                    Target::Source(source)
                }
                Err(reason) => Target::Unloaded {
                    path: Box::new(path),
                    reason,
                },
            };
            if let Some(package) = self.package {
                if let Some(package) = &package.id {
                    if package.index >= package_indices.len() {
                        package_indices.resize(package.index + 1, None);
                    }
                    let id = *package_indices[package.index]
                        .get_or_insert_with(|| packages.intern_prepared(package));
                    self.row.package_id = Some(id);
                    self.row.flags |= package_map.flags(package);
                }
            }
        }
        Ok(self.row)
    }
}

/// Loader batches have unique requests. Publication binds prepared rows and
/// installs their source-local indices; API rows keep the sorting fallback.
#[derive(Default)]
pub(crate) struct LoadedModules {
    modules: Vec<Row>,
    module_lookup: Vec<ModuleLookup>,
    source_ranges: Vec<Range<u32>>,
    packages: LoadedPackageIds,
    prepared_order: Vec<Option<Vec<u32>>>,
    package_indices: Vec<Option<NonZeroU32>>,
}

impl LoadedModules {
    pub(crate) fn new(
        source_count: usize,
        row_count: usize,
        batches: impl IntoIterator<Item = (SourceFileId, usize)>,
    ) -> Self {
        let mut loaded = Self {
            modules: Vec::with_capacity(row_count),
            module_lookup: Vec::with_capacity(row_count),
            source_ranges: vec![0..0; source_count],
            packages: LoadedPackageIds::default(),
            prepared_order: (0..source_count).map(|_| None).collect(),
            package_indices: Vec::new(),
        };
        let mut end = 0u32;
        for (source, count) in batches {
            if count == 0 {
                continue;
            }
            let range = &mut loaded.source_ranges[source.index()];
            if range.start == range.end {
                range.start = end;
            }
            // The loader keeps each retained source's complete batch in
            // publication order. Its counts fix the destination ranges.
            debug_assert_eq!(range.end.max(range.start), end);
            end = end
                .checked_add(u32::try_from(count).expect("module batch fits u32"))
                .expect("module range fits u32");
            range.end = end;
        }
        debug_assert_eq!(end as usize, row_count);
        loaded
    }

    pub(crate) fn set_source_indices(
        &mut self,
        source: SourceFileId,
        indices: PreparedModuleIndices,
    ) {
        let range = &self.source_ranges[source.index()];
        debug_assert_eq!(indices.order.len(), (range.end - range.start) as usize);
        for (slot, mut entry) in self.module_lookup[range.start as usize..range.end as usize]
            .iter_mut()
            .zip(indices.lookup)
        {
            entry.row += range.start;
            *slot = entry;
        }
        self.prepared_order[source.index()] = Some(indices.order);
    }

    pub(crate) fn push_prepared(
        &mut self,
        prepared: Arc<PreparedModule>,
        specifier: &mut JsString,
        binding: ModuleBinding,
        sources: &[PreparedSourceFile],
        package_map: &PreparedPackageMaps,
    ) -> Result<(), ResolutionError> {
        let row = Arc::unwrap_or_clone(prepared).bind(
            &binding,
            sources,
            package_map,
            &mut self.packages,
            &mut self.package_indices,
        )?;
        self.push_final_row(row, specifier, binding.source);
        Ok(())
    }

    fn push_final_row(&mut self, mut row: Row, specifier: &mut JsString, source: SourceFileId) {
        row.specifier = std::mem::take(specifier);
        let index = u32::try_from(self.modules.len()).expect("module row index fits u32");
        debug_assert!(self.source_ranges[source.index()].contains(&index));
        // The source-local lookup permutation replaces these slots after
        // publication has fixed the batch's offset in the final row vector.
        self.module_lookup.push(ModuleLookup {
            hash: 0,
            row: index,
        });
        self.modules.push(row);
    }

    pub(crate) fn push(
        &mut self,
        source: SourceFileId,
        key: ResolutionKey,
        resolution: ModuleResolution,
        exact_source_path: bool,
        sources: &[PreparedSourceFile],
    ) {
        let index = u32::try_from(self.modules.len()).expect("module row index fits u32");
        debug_assert!(self.source_ranges[source.index()].contains(&index));
        self.module_lookup.push(ModuleLookup {
            hash: module_lookup_hash(key.specifier(), key.mode()),
            row: index,
        });
        let row = compact_module_row(
            key.specifier,
            key.mode,
            resolution,
            exact_source_path,
            sources,
            &mut self.packages,
        );
        self.modules.push(row);
    }

    pub(crate) fn finish(
        mut self,
        sources: &[PreparedSourceFile],
        type_references: BTreeMap<TypeReferenceResolutionKey, TypeReferenceResolution>,
    ) -> ResolutionTable {
        let mut group_sources: Vec<_> = self
            .source_ranges
            .iter()
            .enumerate()
            .filter_map(|(source, range)| {
                (!range.is_empty()).then_some(SourceFileId::from_raw(source as u32))
            })
            .collect();
        group_sources.sort_unstable_by(|a, b| {
            sources[a.index()]
                .path()
                .canonical()
                .cmp(sources[b.index()].path().canonical())
        });
        let mut module_order = Vec::with_capacity(self.modules.len());
        for source in group_sources.iter().map(|source| source.index()) {
            let range = &self.source_ranges[source];
            if let Some(order) = self.prepared_order[source].take() {
                module_order.extend(order.into_iter().map(|row| row + range.start));
            } else {
                let start = module_order.len();
                module_order.extend(range.clone());
                module_order[start..].sort_unstable_by(|a, b| {
                    let a = &self.modules[*a as usize];
                    let b = &self.modules[*b as usize];
                    (a.specifier.as_js(), a.mode).cmp(&(b.specifier.as_js(), b.mode))
                });
                self.module_lookup[range.start as usize..range.end as usize]
                    .sort_unstable_by_key(|entry| (entry.hash, entry.row));
            }
        }
        // IDs are private indices. Keep publication's interning order; views
        // expose package values, and iteration uses canonical module_order.
        ResolutionTable {
            modules: self.modules,
            group_sources,
            module_order,
            module_lookup: self.module_lookup,
            source_ranges: self.source_ranges,
            package_ids: self.packages.into_values(),
            type_references,
        }
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
#[derive(Clone, Debug, Default)]
pub struct ResolutionTable {
    modules: Vec<Row>,
    group_sources: Vec<SourceFileId>,
    // Empty for the public builder, whose rows are already in canonical order.
    module_order: Vec<u32>,
    module_lookup: Vec<ModuleLookup>,
    source_ranges: Vec<Range<u32>>,
    package_ids: Vec<PackageId>,
    type_references: BTreeMap<TypeReferenceResolutionKey, TypeReferenceResolution>,
}

impl PartialEq for ResolutionTable {
    fn eq(&self, other: &Self) -> bool {
        self.modules.len() == other.modules.len()
            && self.type_references == other.type_references
            && self.ordered_rows().zip(other.ordered_rows()).all(
                |((a_source, a), (b_source, b))| {
                    a_source == b_source
                        && a.specifier == b.specifier
                        && a.mode == b.mode
                        && a.target == b.target
                        && a.rare == b.rare
                        && a.extension == b.extension
                        && a.flags == b.flags
                        && a.package_id
                            .map(|id| &self.package_ids[id.get() as usize - 1])
                            == b.package_id
                                .map(|id| &other.package_ids[id.get() as usize - 1])
                },
            )
    }
}

impl Eq for ResolutionTable {}

impl ResolutionTable {
    fn ordered_rows(&self) -> ModuleRows<'_> {
        ModuleRows {
            rows: &self.modules,
            order: &self.module_order,
            indices: 0..self.modules.len(),
            group_sources: &self.group_sources,
            source_ranges: &self.source_ranges,
            next_group: 0,
            group_end: 0,
            source: SourceFileId::from_raw(0),
        }
    }

    pub(crate) fn build(
        staged: StagedModules,
        type_references: BTreeMap<TypeReferenceResolutionKey, TypeReferenceResolution>,
        sources: &[PreparedSourceFile],
    ) -> Self {
        let StagedModules { rows, by_source } = staged;
        // Staging already sorts each source's indices by (specifier, mode).
        // Source canonical paths are unique in a prepared program; ordering
        // the nonempty groups therefore reproduces the old global row sort.
        let mut group_sources: Vec<_> = by_source
            .iter()
            .enumerate()
            .filter_map(|(source, indices)| {
                (!indices.is_empty()).then_some(SourceFileId::from_raw(source as u32))
            })
            .collect();
        group_sources.sort_unstable_by(|a, b| {
            sources[a.index()]
                .path()
                .canonical()
                .cmp(sources[b.index()].path().canonical())
        });
        // Take each staged row once, without cloning its owned facts. The
        // optional slots retain staging's allocation while rows are frozen.
        let mut rows: Vec<_> = rows.into_iter().map(Some).collect();
        let mut modules = Vec::with_capacity(rows.len());
        let mut source_ranges = vec![0..0; sources.len()];
        let mut packages = LoadedPackageIds::default();
        for index in group_sources
            .iter()
            .flat_map(|source| by_source[source.index()].iter().copied())
        {
            let StagedRow {
                source: importing_source,
                specifier,
                mode,
                resolution,
                exact_source_path,
            } = rows[index]
                .take()
                .expect("each staged row has one source index");
            let start = u32::try_from(modules.len()).expect("module row index fits u32");
            let range = &mut source_ranges[importing_source.index()];
            if range.start == range.end {
                range.start = start;
            }
            range.end = start.checked_add(1).expect("module range fits u32");
            modules.push(compact_module_row(
                specifier,
                mode,
                resolution,
                exact_source_path,
                sources,
                &mut packages,
            ));
        }
        // The temporary interning index owns the identities during building;
        // consume it so the immutable side table owns each identity only once.
        let package_ids = packages.into_values();
        Self {
            module_lookup: module_lookup_index(&modules, &source_ranges),
            module_order: Vec::new(),
            group_sources,
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
        self.table.ordered_rows().map(move |(source, row)| {
            (
                ResolutionKeyView {
                    source: self.sources[source.index()].path().canonical(),
                    source_id: source,
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
            range_bytes: self.table.source_ranges.capacity() * std::mem::size_of::<Range<u32>>()
                + self.table.group_sources.capacity() * std::mem::size_of::<SourceFileId>(),
            index_bytes: self.table.module_lookup.capacity() * std::mem::size_of::<ModuleLookup>()
                + self.table.module_order.capacity() * std::mem::size_of::<u32>(),
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

/// The public builder owns sorted rows; the loader owns publication-order
/// rows and a compact canonical iteration index. Both expose the same rows.
struct ModuleRows<'a> {
    rows: &'a [Row],
    order: &'a [u32],
    indices: Range<usize>,
    group_sources: &'a [SourceFileId],
    source_ranges: &'a [Range<u32>],
    next_group: usize,
    group_end: usize,
    source: SourceFileId,
}

impl<'a> Iterator for ModuleRows<'a> {
    type Item = (SourceFileId, &'a Row);

    fn next(&mut self) -> Option<Self::Item> {
        let index = self.indices.next()?;
        if index == self.group_end {
            self.source = self.group_sources[self.next_group];
            self.next_group += 1;
            let range = &self.source_ranges[self.source.index()];
            self.group_end += (range.end - range.start) as usize;
        }
        Some((
            self.source,
            &self.rows[if self.order.is_empty() {
                index
            } else {
                self.order[index] as usize
            }],
        ))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.indices.size_hint()
    }
}

impl ExactSizeIterator for ModuleRows<'_> {}

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
        usage.vec("resolution lookup and ranges", &self.module_order);
        usage.vec("resolution lookup and ranges", &self.source_ranges);
        usage.vec("resolution lookup and ranges", &self.group_sources);
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

#[path = "prepared_package_id.rs"]
mod package_identity;
pub(crate) use package_identity::{PreparedPackageId, PreparedPackageIds, PreparedPackageRef};

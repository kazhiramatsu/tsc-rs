//! Source-local facts prepared before the ordered task walk.
use super::*;
use crate::module_requests::{AdmissionPlanParts, ImportOccurrence, LoaderRequestPlan};
use crate::resolution::{PreparedModule, PreparedResolvedModule};

#[derive(Default)]
pub(super) struct PreparedModuleRequests {
    pub rows: Vec<StagedModuleResolution>,
    pub staged: bool,
    pub all_ready: bool,
    pub occurrences: Box<[ImportOccurrence]>,
    pub indices: crate::resolution::PreparedModuleIndices,
}

impl PreparedModuleRequests {
    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

pub(super) struct PreparedSourceRequests {
    external: bool,
    import_count: usize,
    observed: usize,
    admission: AdmissionPlanParts,
    modules: PreparedModuleRequests,
    source: Option<CanonicalPath>,
}

impl PreparedSourceRequests {
    pub fn new(plan: LoaderRequestPlan) -> Self {
        let external = plan.is_external_module();
        let import_count = plan.import_count();
        let observed = plan.observed_request_occurrence_count();
        let (admission, keys, loads_source, occurrences) = plan.into_loader_parts();
        let source = keys.first().map(|key| key.source().clone());
        let indices = crate::resolution::PreparedModuleIndices::new(&keys);
        Self {
            external,
            import_count,
            observed,
            admission,
            source,
            modules: PreparedModuleRequests {
                rows: keys
                    .into_iter()
                    .zip(loads_source)
                    .enumerate()
                    .map(|(index, (key, loads_source))| StagedModuleResolution {
                        host: PreparedModuleResolution::empty(),
                        key: SourceModuleKey {
                            mode: key.mode(),
                            specifier: key.into_parts().1,
                        },
                        target: StagedModuleTarget::Lookup,
                        loads_source,
                        synthetic_count: u32::from(index < occurrences.synthetic_rows),
                        unloaded_reason: None,
                        ready: false,
                    })
                    .collect(),
                staged: false,
                all_ready: false,
                occurrences: occurrences.records.into_boxed_slice(),
                indices,
            },
        }
    }
    pub fn is_external_module(&self) -> bool {
        self.external
    }
    pub fn import_count(&self) -> usize {
        self.import_count
    }
    pub fn observed_request_occurrence_count(&self) -> usize {
        self.observed
    }
    pub fn module_rows_mut(&mut self) -> impl Iterator<Item = &mut StagedModuleResolution> {
        self.modules.rows.iter_mut()
    }
    pub fn finish_preparation(&mut self) {
        self.modules.all_ready = self.modules.rows.iter().all(|row| row.ready);
    }
    pub fn into_resolutions(self) -> impl Iterator<Item = (ResolutionKey, Arc<PreparedModule>)> {
        let source = self.source;
        self.modules.rows.into_iter().filter_map(move |row| {
            row.ready.then(|| {
                (
                    ResolutionKey::new(
                        source.as_ref().expect("a request source").clone(),
                        row.key.specifier,
                        row.key.mode,
                    ),
                    row.host.into_facts(),
                )
            })
        })
    }
    pub fn module_requests_with_loadability(
        &self,
    ) -> impl Iterator<Item = (&SourceModuleKey, bool)> {
        self.modules
            .rows
            .iter()
            .map(|row| (&row.key, row.loads_source))
    }
    pub fn path_references(&self) -> &[PlannedPathReference] {
        &self.admission.path_references
    }
    pub fn type_reference_directives(&self) -> &[PlannedTypeReferenceDirective] {
        &self.admission.type_reference_directives
    }
    pub fn lib_reference_directives(&self) -> &[PlannedLibReferenceDirective] {
        &self.admission.lib_reference_directives
    }
    pub fn take_admission_parts(&mut self) -> AdmissionPlanParts {
        std::mem::take(&mut self.admission)
    }
    pub fn take_loader_module_requests(&mut self) -> PreparedModuleRequests {
        std::mem::take(&mut self.modules)
    }
}

#[derive(Clone, Copy)]
pub(super) enum ImportPolicy {
    Unprepared,
    Missing,
    Skip(UnloadedModuleReason),
    Load {
        external: bool,
        elide_on_depth: bool,
    },
    UnsupportedJson,
    UnsupportedExtension,
}

pub(super) struct PreparedModuleResolution {
    pub facts: Option<Arc<PreparedModule>>,
    pub policy: ImportPolicy,
    pub policy_prepared: bool,
    pub symbol: Option<std::num::NonZeroU32>,
}

const _: () = assert!(std::mem::size_of::<PreparedModuleResolution>() <= 128);

impl PreparedModuleResolution {
    pub fn empty() -> Self {
        Self {
            facts: None,
            policy: ImportPolicy::Unprepared,
            policy_prepared: false,
            symbol: None,
        }
    }
    pub fn new(
        host: HostModuleResolution,
        mode: ResolutionMode,
        identities: &crate::resolution::PreparedPackageIds,
    ) -> Self {
        Self::from_facts(Arc::new(PreparedModule::new(host, mode, identities)))
    }
    pub fn from_facts(facts: Arc<PreparedModule>) -> Self {
        Self {
            facts: Some(facts),
            policy: ImportPolicy::Unprepared,
            policy_prepared: false,
            symbol: None,
        }
    }
    pub fn resolved(&self) -> Option<PreparedResolvedModule<'_>> {
        self.facts.as_ref().and_then(|facts| facts.resolved())
    }
    pub fn into_facts(self) -> Arc<PreparedModule> {
        self.facts.expect("resolved request facts")
    }
    pub fn into_host(self) -> HostModuleResolution {
        Arc::unwrap_or_clone(self.into_facts()).into_host()
    }
    pub fn policy_for(
        &self,
        options: &CompilerOptions,
        declaration: bool,
        loads_source: bool,
        redirected: impl FnOnce(&ProgramPath) -> bool,
        mapped: impl FnOnce(&ProgramPath) -> bool,
    ) -> ImportPolicy {
        match self.resolved() {
            None => ImportPolicy::Missing,
            Some(target) => {
                let extension = target.extension();
                if let Some(reason) = (!target.resolved_using_extra_extensions())
                    .then(|| {
                        resolution_diagnostic_unloaded_reason(
                            extension,
                            options,
                            declaration,
                            loads_source,
                        )
                    })
                    .flatten()
                {
                    ImportPolicy::Skip(reason)
                } else {
                    let redirected = redirected(target.resolved_file());
                    let is_js = extension.is_javascript() && !redirected;
                    let external = target.is_external_library_import();
                    let elide_on_depth = is_js
                        && external
                        && path_contains_node_modules(target.resolved_file().canonical().as_js());
                    let javascript_reason = is_js
                        .then(|| {
                            unloaded_javascript_reason(
                                extension,
                                options,
                                external,
                                target.original_path().is_some(),
                                target.resolved_file().canonical(),
                                loads_source,
                            )
                        })
                        .flatten();
                    if let Some(reason) = javascript_reason {
                        ImportPolicy::Skip(reason)
                    } else if options.no_resolve == Some(true) {
                        ImportPolicy::Skip(UnloadedModuleReason::NoResolve)
                    } else if !loads_source {
                        ImportPolicy::Skip(UnloadedModuleReason::ResolutionOnly)
                    } else if matches!(extension, ModuleExtension::Json)
                        && !options.resolve_json_module_effective()
                    {
                        ImportPolicy::UnsupportedJson
                    } else if !matches!(extension, ModuleExtension::Json)
                        && !is_loadable_typescript_extension(extension)
                        && !extension.is_javascript()
                        && !mapped(target.resolved_file())
                    {
                        ImportPolicy::UnsupportedExtension
                    } else {
                        ImportPolicy::Load {
                            external,
                            elide_on_depth,
                        }
                    }
                }
            }
        }
    }
}

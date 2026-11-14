//! Source-local facts prepared before the ordered task walk.
use super::*;
use crate::module_requests::AdmissionPlanParts;
use crate::resolution::{PreparedModule, PreparedResolvedModule};

#[derive(Clone, Copy)]
pub(super) struct ImportOccurrence {
    pub request: u32,
    pub pos: u32,
    pub end: u32,
    pub synthetic: bool,
}

const _: () = assert!(std::mem::size_of::<ImportOccurrence>() <= 32);

#[derive(Default)]
pub(super) struct PreparedModuleRequests {
    pub keys: Vec<ResolutionKey>,
    pub loads_source: Vec<bool>,
    pub synthetic_counts: Vec<u32>,
    pub occurrences: Vec<ImportOccurrence>,
    pub indices: crate::resolution::PreparedModuleIndices,
}

impl PreparedModuleRequests {
    pub fn len(&self) -> usize {
        self.loads_source.len()
    }
}

pub(super) struct PreparedSourceRequests {
    external: bool,
    import_count: usize,
    observed: usize,
    admission: AdmissionPlanParts,
    modules: PreparedModuleRequests,
}

impl PreparedSourceRequests {
    pub fn new(mut plan: SourceRequestPlan) -> Self {
        let external = plan.is_external_module();
        let import_count = plan.import_count();
        let observed = plan.observed_request_occurrence_count();
        let mut admission = plan.take_admission_parts();
        let modules = plan.take_loader_module_requests();
        drop(plan);
        let synthetic_counts: Vec<_> = modules
            .occurrence_counts
            .iter()
            .zip(&modules.spans)
            .map(|(&count, spans)| count.saturating_sub(spans.len() as u32))
            .collect();
        let mut next = vec![0usize; modules.len()];
        let requests = std::mem::take(&mut admission.module_request_occurrences);
        let mut occurrences = Vec::with_capacity(requests.len());
        for request in requests {
            let index = request as usize;
            let occurrence = next[index];
            next[index] += 1;
            let synthetic = occurrence < synthetic_counts[index] as usize;
            let (pos, end) = if synthetic {
                (0, 0)
            } else {
                let Some(&span) =
                    modules.spans[index].get(occurrence - synthetic_counts[index] as usize)
                else {
                    continue;
                };
                span
            };
            occurrences.push(ImportOccurrence {
                request,
                pos,
                end,
                synthetic,
            });
        }
        let keys = modules.keys;
        let loads_source = modules.loads_source;
        drop(modules.spans);
        drop(modules.occurrence_counts);
        drop(next);
        let indices = crate::resolution::PreparedModuleIndices::new(&keys);
        Self {
            external,
            import_count,
            observed,
            admission,
            modules: PreparedModuleRequests {
                keys,
                loads_source,
                synthetic_counts,
                occurrences,
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
    pub fn module_requests(&self) -> &[ResolutionKey] {
        &self.modules.keys
    }
    pub fn module_requests_with_loadability(&self) -> impl Iterator<Item = (&ResolutionKey, bool)> {
        self.modules
            .keys
            .iter()
            .zip(self.modules.loads_source.iter().copied())
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
    pub facts: PreparedModule,
    pub policy: ImportPolicy,
    pub policy_prepared: bool,
}

const _: () = assert!(std::mem::size_of::<PreparedModuleResolution>() <= 128);

impl PreparedModuleResolution {
    pub fn new(host: HostModuleResolution, key: &ResolutionKey) -> Self {
        Self {
            facts: PreparedModule::new(host, key),
            policy: ImportPolicy::Unprepared,
            policy_prepared: false,
        }
    }
    pub fn resolved(&self) -> Option<PreparedResolvedModule<'_>> {
        self.facts.resolved()
    }
    pub fn into_host(self) -> HostModuleResolution {
        self.facts.into_host()
    }
    pub fn policy_for(
        &self,
        options: &CompilerOptions,
        declaration: bool,
        loads_source: bool,
        redirected: impl FnOnce(&ProgramPath) -> bool,
        mapped: impl FnOnce(&ProgramPath) -> bool,
    ) -> ImportPolicy {
        match self.facts.resolved() {
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

//! The project references of a Program.
//!
//! tsgo parses every config reachable through `references` (compiler/
//! projectreferenceparser.go, each config through the host's
//! `GetResolvedProjectReference`) and maps each referenced project's source
//! files to their output declaration files (tsoptions ParseInputOutputNames).
//! The command loads the output in place of the source when a resolution
//! reaches one; the project system's programs read the sources instead and
//! load a source in place of its output (compiler/
//! projectreferencefilemapper.go getParseFileRedirect,
//! `UseSourceOfProjectReference`). The resolved graph is computed once per
//! Program and shared with the loader through [`ProgramOptions`].
//!
//! [`ProgramOptions`]: crate::ProgramOptions

use std::collections::BTreeMap;
use std::sync::Arc;

use rustc_hash::FxHashMap;
use tsc_diagnostics::{JsStr, JsString};
use tsc_types::CompilerOptions;

use crate::config::{
    parse_config_root_plan_with_cache, ConfigExtendedCache, ConfigParseError, ConfigParseErrorKind,
    ConfigParseHost, ConfigProjectReference, ConfigRootPlan, ConfigRootPlanRequest,
};
use crate::js_path::{combine_paths, file_name_key};
use crate::module_requests::is_declaration_file_name;
use crate::output_directories::{
    build_info_file_name, common_source_directory, output_declaration_file_name,
};
use crate::path::CanonicalPath;

/// One parsed referenced project (tsgo's `ParsedCommandLine` of a
/// reference): its options, file names and own references.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedProjectReference {
    config_file_name: JsString,
    canonical: CanonicalPath,
    plan: Arc<ConfigRootPlan>,
    build_info_file_name: Option<JsString>,
    /// What the project's files take from its options.
    options: Arc<crate::ReferencedProjectOptions>,
}

impl ResolvedProjectReference {
    /// The normalized absolute config file name.
    pub fn config_file_name(&self) -> JsStr<'_> {
        self.config_file_name.as_js()
    }

    pub fn canonical(&self) -> &CanonicalPath {
        &self.canonical
    }

    pub fn compiler_options(&self) -> &CompilerOptions {
        self.plan.compiler_options()
    }

    /// The referenced project's parsed config.
    pub fn plan(&self) -> &ConfigRootPlan {
        &self.plan
    }

    /// tsgo `ParsedCommandLine.GetBuildInfoFileName`: the build info file the
    /// project writes, when it is incremental or composite.
    pub fn build_info_file_name(&self) -> Option<JsStr<'_>> {
        self.build_info_file_name.as_ref().map(JsString::as_js)
    }

    /// The project's options its files are checked under (tsgo
    /// `getCompilerOptionsForFile`).
    pub fn referenced_options(&self) -> &Arc<crate::ReferencedProjectOptions> {
        &self.options
    }
}

/// A source file of a referenced project and the declaration file its
/// project emits for it (tsgo `SourceOutputAndProjectReference`).
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectReferenceSourceOutput {
    source: JsString,
    output_dts: Option<JsString>,
    project: Arc<ResolvedProjectReference>,
}

impl ProjectReferenceSourceOutput {
    pub fn source(&self) -> JsStr<'_> {
        self.source.as_js()
    }

    /// `None` for a source the project emits no declaration for (a `.d.ts`
    /// or a JSON file).
    pub fn output_dts(&self) -> Option<JsStr<'_>> {
        self.output_dts.as_ref().map(JsString::as_js)
    }

    pub fn project(&self) -> &Arc<ResolvedProjectReference> {
        &self.project
    }
}

/// One entry of a config's `references`, resolved: the config it names
/// (`None` when no file exists there: TS6053).
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedReferenceEntry {
    reference: ConfigProjectReference,
    config_file_name: JsString,
    project: Option<Arc<ResolvedProjectReference>>,
}

impl ResolvedReferenceEntry {
    pub fn reference(&self) -> &ConfigProjectReference {
        &self.reference
    }

    /// tsgo `ResolveConfigFileNameOfProjectReference`: the written path, or
    /// its `tsconfig.json` when it is not a `.json` file.
    pub fn config_file_name(&self) -> JsStr<'_> {
        self.config_file_name.as_js()
    }

    pub fn project(&self) -> Option<&Arc<ResolvedProjectReference>> {
        self.project.as_ref()
    }
}

/// Every project reachable from the root config's `references`, with the
/// source-to-output maps the loader redirects through (tsgo
/// `projectReferenceFileMapper`). One graph is resolved per command and
/// shared; two graphs compare equal only when they are the same one.
#[derive(Debug, Default)]
pub struct ResolvedProjectReferences {
    /// The root config's own references, in order.
    root_references: Vec<ResolvedReferenceEntry>,
    /// Each referenced project's own references, in order (tsgo
    /// `referencesInConfigFile`), by the project's canonical config path.
    references_in_config: BTreeMap<CanonicalPath, Vec<ResolvedReferenceEntry>>,
    source_to_output: FxHashMap<CanonicalPath, Arc<ProjectReferenceSourceOutput>>,
    output_to_source: FxHashMap<CanonicalPath, Arc<ProjectReferenceSourceOutput>>,
    /// Every project parsed, by its canonical config path (tsgo
    /// `configToProjectReference`; a config that does not exist is absent).
    projects: BTreeMap<CanonicalPath, Arc<ResolvedProjectReference>>,
}

impl PartialEq for ResolvedProjectReferences {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl Eq for ResolvedProjectReferences {}

impl ResolvedProjectReferences {
    pub fn root_references(&self) -> &[ResolvedReferenceEntry] {
        &self.root_references
    }

    /// The references of a referenced project.
    pub fn references_in_config(&self, config: &CanonicalPath) -> &[ResolvedReferenceEntry] {
        self.references_in_config
            .get(config)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// tsgo `getProjectReferenceFromSource`: the output a source file of a
    /// referenced project is loaded as.
    pub fn output_for_source(
        &self,
        source: &CanonicalPath,
    ) -> Option<&Arc<ProjectReferenceSourceOutput>> {
        self.source_to_output.get(source)
    }

    /// tsgo `getProjectReferenceFromOutputDts`: the referenced project whose
    /// output declaration file this is.
    pub fn source_for_output(
        &self,
        output: &CanonicalPath,
    ) -> Option<&Arc<ProjectReferenceSourceOutput>> {
        self.output_to_source.get(output)
    }

    /// tsgo `getRedirectForResolution`: the referenced project whose options
    /// resolve the module names of `file` (a source or an output of it).
    pub fn project_for_resolution(
        &self,
        file: &CanonicalPath,
    ) -> Option<&Arc<ResolvedProjectReference>> {
        self.source_to_output
            .get(file)
            .or_else(|| self.output_to_source.get(file))
            .map(|output| output.project())
    }

    pub fn is_empty(&self) -> bool {
        self.root_references.is_empty()
    }

    /// Whether a referenced project has an output declaration file.
    pub(crate) fn has_outputs(&self) -> bool {
        !self.output_to_source.is_empty()
    }

    /// Every parsed project, in canonical config path order.
    pub fn projects(&self) -> impl Iterator<Item = &Arc<ResolvedProjectReference>> {
        self.projects.values()
    }
}

/// tsgo core.ResolveConfigFileNameOfProjectReference.
pub fn resolve_config_file_name_of_project_reference(path: JsStr<'_>) -> JsString {
    if path.ends_with(".json") {
        return path.to_owned();
    }
    combine_paths(path, "tsconfig.json".into())
}

/// The program-visible paths of a project's config file (tsgo's
/// `ParsedCommandLine` identity): the normalized absolute name and its
/// canonical form.
fn canonical_config_path(
    file_name: JsStr<'_>,
    case_sensitive: bool,
) -> Result<CanonicalPath, ConfigParseError> {
    CanonicalPath::from_js_normalized(file_name_key(file_name, case_sensitive).as_js()).map_err(
        |error| {
            ConfigParseError::new_js(
                ConfigParseErrorKind::InvalidPath,
                Some(file_name.to_owned()),
                error.to_string(),
            )
        },
    )
}

/// tsgo projectReferenceParser.parse + initMapper for the command: parse
/// every config reachable from `root`'s references (each once) through the
/// host, then map the files of every project but the root to their outputs,
/// parents before children so a file in several projects belongs to the
/// last (deepest) one listed.
pub fn resolve_project_references(
    host: &dyn ConfigParseHost,
    root: &ConfigRootPlan,
    current_directory: JsStr<'_>,
) -> Result<ResolvedProjectReferences, ConfigParseError> {
    let mut cache = ConfigExtendedCache::default();
    resolve_project_references_with(
        Some(root.config_file_name()),
        root.project_references().unwrap_or_default(),
        current_directory,
        host.use_case_sensitive_file_names(),
        &mut |config_file_name| {
            let Some(text) = host.read_file(config_file_name)? else {
                return Ok(None);
            };
            parse_config_root_plan_with_cache(
                host,
                ConfigRootPlanRequest {
                    file_name: config_file_name.to_owned(),
                    text,
                    base_path: current_directory.to_owned(),
                },
                &mut cache,
            )
            .map(|plan| Some(Arc::new(plan)))
        },
    )
}

/// The parse of a referenced config by its file name (tsgo's host
/// `GetResolvedProjectReference`): `None` when no config exists there.
pub type ReferencedConfigParse<'a, E> =
    dyn FnMut(JsStr<'_>) -> Result<Option<Arc<ConfigRootPlan>>, E> + 'a;

/// [`resolve_project_references`] with each referenced config's parse
/// supplied by `parse`, for a root config or, with no
/// `root_config_file_name`, for a Program made from root names.
pub fn resolve_project_references_with<E: From<ConfigParseError>>(
    root_config_file_name: Option<JsStr<'_>>,
    references: &[ConfigProjectReference],
    current_directory: JsStr<'_>,
    case_sensitive: bool,
    parse: &mut ReferencedConfigParse<'_, E>,
) -> Result<ResolvedProjectReferences, E> {
    if references.is_empty() {
        return Ok(ResolvedProjectReferences::default());
    }
    let root_canonical = root_config_file_name
        .map(|file_name| canonical_config_path(file_name, case_sensitive))
        .transpose()?;
    let mut parser = ReferenceParser {
        parse,
        current_directory,
        case_sensitive,
        root_canonical,
        parsed: BTreeMap::new(),
        resolved: ResolvedProjectReferences::default(),
    };
    let root_entries = parser.resolve_entries(references)?;
    parser.map_projects(&root_entries)?;
    parser.resolved.root_references = root_entries;
    parser.resolved.projects = parser
        .parsed
        .into_iter()
        .filter_map(|(canonical, project)| Some((canonical, project?)))
        .collect();
    Ok(parser.resolved)
}

struct ReferenceParser<'a, E> {
    parse: &'a mut ReferencedConfigParse<'a, E>,
    current_directory: JsStr<'a>,
    case_sensitive: bool,
    /// The Program's own config, whose files are not mapped.
    root_canonical: Option<CanonicalPath>,
    /// Every config parsed so far (tsgo `tasksByFileName`): `None` when the
    /// file does not exist.
    parsed: BTreeMap<CanonicalPath, Option<Arc<ResolvedProjectReference>>>,
    resolved: ResolvedProjectReferences,
}

impl<E: From<ConfigParseError>> ReferenceParser<'_, E> {
    /// Resolve the entries of one config's `references`, parsing each
    /// project the first time it is named.
    fn resolve_entries(
        &mut self,
        references: &[ConfigProjectReference],
    ) -> Result<Vec<ResolvedReferenceEntry>, E> {
        let mut entries = Vec::with_capacity(references.len());
        for reference in references {
            let config_file_name =
                resolve_config_file_name_of_project_reference(reference.path.as_js());
            let canonical = canonical_config_path(config_file_name.as_js(), self.case_sensitive)?;
            let project = self.parse_project(config_file_name.as_js(), canonical)?;
            entries.push(ResolvedReferenceEntry {
                reference: reference.clone(),
                config_file_name,
                project,
            });
        }
        Ok(entries)
    }

    fn parse_project(
        &mut self,
        config_file_name: JsStr<'_>,
        canonical: CanonicalPath,
    ) -> Result<Option<Arc<ResolvedProjectReference>>, E> {
        if let Some(parsed) = self.parsed.get(&canonical) {
            return Ok(parsed.clone());
        }
        // Mark before parsing: a cycle through `references` ends here.
        self.parsed.insert(canonical.clone(), None);
        let Some(plan) = (self.parse)(config_file_name)? else {
            return Ok(None);
        };
        let build_info_file_name = build_info_file_name(
            plan.compiler_options(),
            Some(plan.config_file_name()),
            self.current_directory,
            self.case_sensitive,
        );
        let options = Arc::new(crate::ReferencedProjectOptions::of(
            &plan,
            self.current_directory,
            self.case_sensitive,
        ));
        let project = Arc::new(ResolvedProjectReference {
            config_file_name: config_file_name.to_owned(),
            canonical: canonical.clone(),
            plan,
            build_info_file_name,
            options,
        });
        self.parsed
            .insert(canonical.clone(), Some(Arc::clone(&project)));
        let references = project
            .plan
            .project_references()
            .map(<[_]>::to_vec)
            .unwrap_or_default();
        let entries = self.resolve_entries(&references)?;
        self.resolved
            .references_in_config
            .insert(canonical, entries);
        Ok(Some(project))
    }

    /// tsgo initMapperWorker: pre-order over the reference graph, each
    /// project once.
    fn map_projects(&mut self, entries: &[ResolvedReferenceEntry]) -> Result<(), E> {
        let mut seen = std::collections::BTreeSet::new();
        let mut pending = entries
            .iter()
            .filter_map(|entry| entry.project.clone())
            .rev()
            .collect::<Vec<_>>();
        while let Some(project) = pending.pop() {
            if !seen.insert(project.canonical.clone()) {
                continue;
            }
            if Some(&project.canonical) != self.root_canonical.as_ref() {
                self.map_project_files(&project)?;
            }
            let children = self
                .resolved
                .references_in_config
                .get(&project.canonical)
                .map(|entries| {
                    entries
                        .iter()
                        .filter_map(|entry| entry.project.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            pending.extend(children.into_iter().rev());
        }
        Ok(())
    }

    /// tsoptions ParseInputOutputNames for one project: a declaration file
    /// or a JSON file has no output; every other file maps to its output
    /// declaration file.
    fn map_project_files(&mut self, project: &Arc<ResolvedProjectReference>) -> Result<(), E> {
        let options = project.plan.compiler_options();
        let file_names = project
            .plan
            .file_names()
            .iter()
            .map(JsString::as_js)
            .collect::<Vec<_>>();
        let common = common_source_directory(
            options,
            Some(project.plan.config_file_name()),
            &file_names,
            self.current_directory,
            self.case_sensitive,
        );
        for file_name in project.plan.file_names() {
            let output_dts = (!is_declaration_file_name(file_name.as_js())
                && !file_name.as_js().ends_with(".json"))
            .then(|| {
                output_declaration_file_name(
                    file_name.as_js(),
                    options,
                    common.as_js(),
                    self.current_directory,
                    self.case_sensitive,
                )
            });
            let entry = Arc::new(ProjectReferenceSourceOutput {
                source: file_name.clone(),
                output_dts: output_dts.clone(),
                project: Arc::clone(project),
            });
            if let Some(output_dts) = output_dts {
                let canonical = CanonicalPath::from_js_normalized(
                    file_name_key(output_dts.as_js(), self.case_sensitive).as_js(),
                )
                .map_err(|error| {
                    ConfigParseError::new_js(
                        ConfigParseErrorKind::InvalidPath,
                        Some(output_dts.clone()),
                        error.to_string(),
                    )
                })?;
                self.resolved
                    .output_to_source
                    .insert(canonical, Arc::clone(&entry));
            }
            let canonical = CanonicalPath::from_js_normalized(
                file_name_key(file_name.as_js(), self.case_sensitive).as_js(),
            )
            .map_err(|error| {
                ConfigParseError::new_js(
                    ConfigParseErrorKind::InvalidPath,
                    Some(file_name.clone()),
                    error.to_string(),
                )
            })?;
            self.resolved.source_to_output.insert(canonical, entry);
        }
        Ok(())
    }
}

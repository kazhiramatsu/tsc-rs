//! Lossless, deterministic execution inputs for the pinned TypeScript suites.
//!
//! The expansion manifest records the complete case inventory without copying
//! source text into a large JSON artifact. This module joins that inventory
//! back to the pinned corpus. Source bytes are verified for every recorded
//! path, decoded once per Git blob, and shared by every matrix variant.
//!
//! Config-file parsing deliberately remains outside this layer. A config-driven
//! plan therefore exposes candidate units and config provenance, but never
//! pretends that program roots have already been selected.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use super::compiler::{
    expand_configurations, extract_compiler_settings, is_config_file_name, make_units_from_test,
    ParsedUnit,
};
use super::{
    collect_suite_paths, decode_source, error, git_blob_sha1, path_from_posix,
    read_recorded_manifest, sha256_hex, CaseConfiguration, CompilerFixtureExpansion,
    ExecutionState, ExpansionManifest, OrderedSetting, ProjectInputFiles, ProjectModule,
    SourceEncoding, SourceInventoryEntry, SuiteName, UnitContent, VIRTUAL_SOURCE_ROOT,
};
use crate::HarnessResult;

/// A fully verified, canonically ordered set of execution plans.
#[derive(Clone, Debug)]
pub struct UpstreamExecutionCorpus {
    pub manifest: Arc<ExpansionManifest>,
    pub plans: Arc<[UpstreamExecutionPlan]>,
    pub cache_stats: SourceCacheStats,
}

/// One entry in the recorded 7,908-case execution order.
#[derive(Clone, Debug)]
pub struct UpstreamExecutionPlan {
    pub provenance: CaseProvenance,
    pub input: UpstreamExecutionInput,
}

#[derive(Clone, Debug)]
pub enum UpstreamExecutionInput {
    Compiler(CompilerExecutionPlan),
    Project(ProjectExecutionPlan),
}

#[derive(Clone, Debug)]
pub struct CaseProvenance {
    /// Stable manifest offset for deterministic sharding and result assembly.
    pub case_index: u32,
    pub case_id: Arc<str>,
    pub suite: SuiteName,
    pub source_index: u32,
    pub source_path: Arc<str>,
    pub upstream_path: Arc<str>,
    pub git_blob_sha1: Arc<str>,
    pub source_commit: &'static str,
    pub initial_execution_state: ExecutionState,
}

/// Content and identity for one source path in the pinned corpus.
#[derive(Clone, Debug)]
pub struct VerifiedSource {
    pub index: u32,
    pub suite: SuiteName,
    pub relative_path: Arc<str>,
    pub upstream_path: Arc<str>,
    pub workspace_path: Arc<PathBuf>,
    pub git_blob_sha1: Arc<str>,
    pub raw: Arc<[u8]>,
    pub encoding: SourceEncoding,
    pub decoded: Arc<str>,
}

/// Observable work avoided by the blob-keyed source cache.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SourceCacheStats {
    pub verified_source_paths: usize,
    pub verified_source_bytes: u64,
    pub unique_raw_blobs: usize,
    pub reused_raw_blobs: usize,
    pub decode_requests: usize,
    pub unique_decoded_blobs: usize,
    pub reused_decoded_blobs: usize,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CompilerUnitId(pub u32);

#[derive(Clone, Debug)]
pub struct CompilerFixtureInput {
    pub source: Arc<VerifiedSource>,
    /// Original unit occurrence order, including the virtual config file.
    pub units: Arc<[CompilerUnitInput]>,
    pub config_unit: Option<CompilerUnitId>,
    pub settings: Arc<[OrderedSetting]>,
    pub configurations: Arc<[super::CompilerConfiguration]>,
    /// Lossless `@link` directive order before JavaScript object assignment.
    pub global_symlink_directives: Arc<[CompilerSymlinkOperation]>,
    /// Effective FileSet order. Repeated normalized link keys replace their
    /// target without moving the key's first insertion position.
    pub global_symlinks: Arc<[CompilerSymlinkOperation]>,
}

#[derive(Clone, Debug)]
pub struct CompilerUnitInput {
    pub id: CompilerUnitId,
    pub name: Arc<str>,
    /// `None` preserves JavaScript `undefined` for an intermediate empty unit.
    pub content: Option<Arc<str>>,
    pub file_options: Arc<[OrderedSetting]>,
    pub original_fixture_path: Arc<str>,
    /// TypeScript 6.0.3 initializes this array but does not populate it here.
    pub references: Arc<[Arc<str>]>,
    pub document_symlinks: Arc<[CompilerSymlinkOperation]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilerSymlinkPhase {
    Document,
    Global,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerSymlinkOperation {
    pub phase: CompilerSymlinkPhase,
    /// For document symlinks this is the unit path; for global `@link` it is
    /// the directive's target spelling.
    pub raw_target: Arc<str>,
    pub raw_link_path: Arc<str>,
    pub anchor: Arc<str>,
    pub normalized_target: Arc<str>,
    pub normalized_link_path: Arc<str>,
}

#[derive(Clone, Debug)]
pub struct CompilerExecutionPlan {
    pub fixture: Arc<CompilerFixtureInput>,
    pub variant: CompilerVariant,
    /// Object-spread result in JavaScript property order. Exact-case duplicate
    /// keys replace their value without moving their original position.
    pub effective_settings: Arc<[OrderedSetting]>,
    pub current_directory: Arc<str>,
    pub use_case_sensitive_file_names: bool,
    pub root_selection: CompilerRootSelection,
}

#[derive(Clone, Debug)]
pub struct CompilerVariant {
    pub configuration_index: u32,
    pub key: Arc<str>,
    pub description: Arc<str>,
    pub upstream_name: Arc<str>,
    pub overrides: Arc<[OrderedSetting]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilerExplicitRootReason {
    AllUnits,
    LastUnitImplicitReferences,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompilerRootSelection {
    Explicit {
        reason: CompilerExplicitRootReason,
        root_units: Arc<[CompilerUnitId]>,
        other_units: Arc<[CompilerUnitId]>,
        vfs_write_order: Arc<[CompilerUnitId]>,
        /// Roots with TypeScript's exact lowercase `.json` extension removed
        /// at the final `createProgram` argument filtering step.
        program_root_units: Arc<[CompilerUnitId]>,
    },
    ConfigDriven {
        config_unit: CompilerUnitId,
        /// The config parse host sees every original occurrence, including the
        /// config unit itself.
        config_host_units: Arc<[CompilerUnitId]>,
        candidate_units: Arc<[CompilerUnitId]>,
    },
}

#[derive(Clone, Debug)]
pub struct ProjectFixtureInput {
    pub source: Arc<VerifiedSource>,
    /// Raw source is retained because it is the ultimate lossless descriptor.
    pub descriptor_raw: Arc<[u8]>,
    pub descriptor_text: Arc<str>,
    /// Top-level JavaScript property order, including case-distinct keys.
    pub properties: Arc<[OrderedJsonProperty]>,
    pub scenario: Arc<str>,
    pub project_root: Arc<str>,
    pub input_files: ProjectInputFiles,
    pub current_directory: Arc<str>,
    pub mount: Arc<ProjectMount>,
    pub root_selection: ProjectRootSelection,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OrderedJsonProperty {
    pub name: Arc<str>,
    pub value: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectMount {
    pub workspace_path: Arc<PathBuf>,
    pub virtual_path: Arc<str>,
    pub case_sensitive: bool,
    pub read_only: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectRootSelection {
    Explicit {
        /// Raw descriptor order, spelling, and duplicates are retained.
        input_names: Arc<[Arc<str>]>,
    },
    ProjectConfig {
        raw_project: Arc<str>,
        config_file_name: Arc<str>,
        resolved_config_path: Arc<str>,
    },
    DiscoverConfig,
}

#[derive(Clone, Debug)]
pub struct ProjectExecutionPlan {
    pub fixture: Arc<ProjectFixtureInput>,
    /// Module variant identity remains separate from a descriptor-level module
    /// property, which the next option-projection stage may apply afterwards.
    pub module_variant: ProjectModule,
    pub baseline_folder: Arc<str>,
    pub descriptor_module_override: Option<Value>,
}

/// Verify the recorded corpus and construct every compiler/project plan in its
/// canonical manifest order.
pub fn load_recorded_execution_plans(workspace: &Path) -> HarnessResult<UpstreamExecutionCorpus> {
    let (manifest, _) = read_recorded_manifest(workspace)?;
    let manifest = Arc::new(manifest);
    let mut cache = SourceCache::load(workspace, &manifest)?;

    let mut compiler_fixtures = HashMap::with_capacity(manifest.compiler_fixtures.len());
    for fixture in &manifest.compiler_fixtures {
        let source = cache.decoded_source(&manifest, fixture.source)?;
        let input = Arc::new(build_compiler_fixture(fixture, source)?);
        if compiler_fixtures.insert(fixture.source, input).is_some() {
            return Err(error(format!(
                "duplicate compiler fixture source index {}",
                fixture.source
            )));
        }
    }

    let tests_mount = Arc::new(ProjectMount {
        workspace_path: Arc::new(workspace.join("ts-tests/tests")),
        virtual_path: Arc::from("/.src/tests"),
        case_sensitive: true,
        read_only: true,
    });
    let mut project_fixtures = HashMap::with_capacity(manifest.project_fixtures.len());
    for fixture in &manifest.project_fixtures {
        let source = cache.decoded_source(&manifest, fixture.source)?;
        let input = Arc::new(build_project_fixture(
            fixture,
            source,
            Arc::clone(&tests_mount),
        )?);
        if project_fixtures.insert(fixture.source, input).is_some() {
            return Err(error(format!(
                "duplicate project fixture source index {}",
                fixture.source
            )));
        }
    }

    let mut plans = Vec::with_capacity(manifest.cases.len());
    for (case_index, case) in manifest.cases.iter().enumerate() {
        let input = match &case.configuration {
            CaseConfiguration::Compiler { configuration } => {
                let fixture = compiler_fixtures.get(&case.source).ok_or_else(|| {
                    error(format!("compiler case {:?} has no fixture input", case.id))
                })?;
                UpstreamExecutionInput::Compiler(build_compiler_plan(
                    Arc::clone(fixture),
                    *configuration,
                )?)
            }
            CaseConfiguration::Project {
                module,
                baseline_folder,
            } => {
                let fixture = project_fixtures.get(&case.source).ok_or_else(|| {
                    error(format!("project case {:?} has no fixture input", case.id))
                })?;
                UpstreamExecutionInput::Project(build_project_plan(
                    Arc::clone(fixture),
                    *module,
                    baseline_folder,
                ))
            }
        };
        let verified_source = match &input {
            UpstreamExecutionInput::Compiler(plan) => &plan.fixture.source,
            UpstreamExecutionInput::Project(plan) => &plan.fixture.source,
        };
        let provenance = CaseProvenance {
            case_index: u32::try_from(case_index)
                .map_err(|_| error("upstream execution case index overflow"))?,
            case_id: Arc::from(case.id.as_str()),
            suite: case.suite,
            source_index: case.source,
            source_path: Arc::clone(&verified_source.relative_path),
            upstream_path: Arc::clone(&verified_source.upstream_path),
            git_blob_sha1: Arc::clone(&verified_source.git_blob_sha1),
            source_commit: super::SOURCE_COMMIT,
            initial_execution_state: case.initial_execution_state,
        };
        plans.push(UpstreamExecutionPlan { provenance, input });
    }

    Ok(UpstreamExecutionCorpus {
        manifest,
        plans: Arc::from(plans),
        cache_stats: cache.stats,
    })
}

struct SourceCache {
    workspace_paths: Vec<Arc<PathBuf>>,
    raw_sources: Vec<Arc<[u8]>>,
    raw_by_blob: HashMap<String, Arc<[u8]>>,
    decoded_by_blob: HashMap<String, (SourceEncoding, Arc<str>)>,
    stats: SourceCacheStats,
}

impl SourceCache {
    fn load(workspace: &Path, manifest: &ExpansionManifest) -> HarnessResult<Self> {
        verify_suite_path_sets(workspace, manifest)?;

        let mut cache = Self {
            workspace_paths: Vec::with_capacity(manifest.sources.len()),
            raw_sources: Vec::with_capacity(manifest.sources.len()),
            raw_by_blob: HashMap::new(),
            decoded_by_blob: HashMap::new(),
            stats: SourceCacheStats::default(),
        };

        for source in &manifest.sources {
            let suite = suite_identity(manifest, source.suite)?;
            let path = workspace
                .join(&suite.vendored_path)
                .join(path_from_posix(&source.path)?);
            let raw = fs::read(&path).map_err(|source_error| {
                error(format!(
                    "failed to read pinned execution source {}: {source_error}",
                    path.display()
                ))
            })?;
            verify_source_bytes(source, &path, &raw)?;

            cache.stats.verified_source_paths += 1;
            cache.stats.verified_source_bytes = cache
                .stats
                .verified_source_bytes
                .checked_add(raw.len() as u64)
                .ok_or_else(|| error("verified source byte count overflow"))?;

            let shared = if let Some(existing) = cache.raw_by_blob.get(&source.git_blob_sha1) {
                if existing.as_ref() != raw.as_slice() {
                    return Err(error(format!(
                        "Git blob collision while loading {}",
                        path.display()
                    )));
                }
                cache.stats.reused_raw_blobs += 1;
                Arc::clone(existing)
            } else {
                let shared: Arc<[u8]> = Arc::from(raw);
                cache
                    .raw_by_blob
                    .insert(source.git_blob_sha1.clone(), Arc::clone(&shared));
                shared
            };
            cache.workspace_paths.push(Arc::new(path));
            cache.raw_sources.push(shared);
        }
        cache.stats.unique_raw_blobs = cache.raw_by_blob.len();
        Ok(cache)
    }

    fn decoded_source(
        &mut self,
        manifest: &ExpansionManifest,
        index: u32,
    ) -> HarnessResult<Arc<VerifiedSource>> {
        let source = source_entry(manifest, index)?;
        let raw = self
            .raw_sources
            .get(index as usize)
            .cloned()
            .ok_or_else(|| error(format!("source index {index} was not loaded")))?;
        let workspace_path = self
            .workspace_paths
            .get(index as usize)
            .cloned()
            .ok_or_else(|| error(format!("source index {index} has no workspace path")))?;
        self.stats.decode_requests += 1;
        let (encoding, decoded) =
            if let Some((encoding, decoded)) = self.decoded_by_blob.get(&source.git_blob_sha1) {
                self.stats.reused_decoded_blobs += 1;
                (*encoding, Arc::clone(decoded))
            } else {
                let (encoding, decoded) = decode_source(raw.as_ref());
                let decoded: Arc<str> = Arc::from(decoded);
                self.decoded_by_blob.insert(
                    source.git_blob_sha1.clone(),
                    (encoding, Arc::clone(&decoded)),
                );
                (encoding, decoded)
            };
        self.stats.unique_decoded_blobs = self.decoded_by_blob.len();

        let suite = suite_identity(manifest, source.suite)?;
        Ok(Arc::new(VerifiedSource {
            index,
            suite: source.suite,
            relative_path: Arc::from(source.path.as_str()),
            upstream_path: Arc::from(join_posix(&suite.source_path, &source.path)),
            workspace_path,
            git_blob_sha1: Arc::from(source.git_blob_sha1.as_str()),
            raw,
            encoding,
            decoded,
        }))
    }
}

fn verify_suite_path_sets(workspace: &Path, manifest: &ExpansionManifest) -> HarnessResult<()> {
    for suite in &manifest.corpus_pin.suites {
        let root = workspace.join(&suite.vendored_path);
        let actual = collect_suite_paths(&root)?;
        let expected = manifest
            .sources
            .iter()
            .filter(|source| source.suite == suite.name)
            .map(|source| source.path.clone())
            .collect::<Vec<_>>();
        if actual != expected {
            return Err(error(format!(
                "execution corpus path set for {} no longer matches the recorded manifest",
                suite.name.as_str()
            )));
        }
    }
    Ok(())
}

fn verify_source_bytes(
    source: &SourceInventoryEntry,
    path: &Path,
    raw: &[u8],
) -> HarnessResult<()> {
    if raw.len() as u64 != source.bytes
        || sha256_hex(raw) != source.sha256
        || git_blob_sha1(raw) != source.git_blob_sha1
    {
        return Err(error(format!(
            "execution source {} does not match its recorded byte and blob identity",
            path.display()
        )));
    }
    Ok(())
}

fn build_compiler_fixture(
    recorded: &CompilerFixtureExpansion,
    source: Arc<VerifiedSource>,
) -> HarnessResult<CompilerFixtureInput> {
    if source.encoding != recorded.encoding
        || source.decoded.len() as u64 != recorded.decoded_utf8_bytes
        || sha256_hex(source.decoded.as_bytes()) != recorded.decoded_sha256
    {
        return Err(error(format!(
            "decoded compiler fixture {:?} does not match the manifest",
            source.relative_path
        )));
    }
    let settings = extract_compiler_settings(&source.decoded);
    if settings != recorded.settings {
        return Err(error(format!(
            "compiler settings for {:?} no longer match the manifest",
            source.relative_path
        )));
    }
    let configurations = expand_configurations(source.relative_path.as_ref(), &settings)?;
    if configurations != recorded.configurations {
        return Err(error(format!(
            "compiler configurations for {:?} no longer match the source settings",
            source.relative_path
        )));
    }
    let (parsed_units, links) =
        make_units_from_test(&source.decoded, source.upstream_path.as_ref())?;
    let config_offset = parsed_units
        .iter()
        .position(|unit| is_config_file_name(&unit.name));
    verify_parsed_units(recorded, &parsed_units, config_offset, &source)?;
    if links != recorded.links {
        return Err(error(format!(
            "compiler @link directives for {:?} no longer match the manifest",
            source.relative_path
        )));
    }

    let current_directory = compiler_current_directory(&settings)?;
    let original_fixture_path = Arc::clone(&source.upstream_path);
    let units = parsed_units
        .into_iter()
        .enumerate()
        .map(|(index, unit)| {
            build_compiler_unit(
                CompilerUnitId(index as u32),
                unit,
                Arc::clone(&original_fixture_path),
                &current_directory,
            )
        })
        .collect::<HarnessResult<Vec<_>>>()?;
    let global_symlink_directives = links
        .into_iter()
        .map(|link| {
            let anchor: Arc<str> = Arc::from(VIRTUAL_SOURCE_ROOT);
            Ok(CompilerSymlinkOperation {
                phase: CompilerSymlinkPhase::Global,
                raw_target: Arc::from(link.target.as_str()),
                raw_link_path: Arc::from(link.link_path.as_str()),
                normalized_target: Arc::from(normalize_virtual_path(
                    anchor.as_ref(),
                    &link.target,
                )?),
                normalized_link_path: Arc::from(normalize_virtual_path(
                    anchor.as_ref(),
                    &link.link_path,
                )?),
                anchor,
            })
        })
        .collect::<HarnessResult<Vec<_>>>()?;
    let global_symlinks = effective_global_symlinks(&global_symlink_directives);

    Ok(CompilerFixtureInput {
        source,
        units: Arc::from(units),
        config_unit: config_offset.map(|index| CompilerUnitId(index as u32)),
        settings: Arc::from(settings),
        configurations: Arc::from(recorded.configurations.clone()),
        global_symlink_directives: Arc::from(global_symlink_directives),
        global_symlinks: Arc::from(global_symlinks),
    })
}

fn effective_global_symlinks(
    directives: &[CompilerSymlinkOperation],
) -> Vec<CompilerSymlinkOperation> {
    let mut effective: Vec<CompilerSymlinkOperation> = Vec::with_capacity(directives.len());
    for directive in directives {
        if let Some(existing) = effective
            .iter_mut()
            .find(|existing| existing.normalized_link_path == directive.normalized_link_path)
        {
            existing.clone_from(directive);
        } else {
            effective.push(directive.clone());
        }
    }
    effective
}

fn verify_parsed_units(
    recorded: &CompilerFixtureExpansion,
    parsed: &[ParsedUnit],
    config_offset: Option<usize>,
    source: &VerifiedSource,
) -> HarnessResult<()> {
    let mut normal_offset = 0;
    for (index, unit) in parsed.iter().enumerate() {
        let expected = if Some(index) == config_offset {
            recorded.virtual_config.as_ref()
        } else {
            let expected = recorded.normal_units.get(normal_offset);
            normal_offset += 1;
            expected
        }
        .ok_or_else(|| {
            error(format!(
                "compiler unit occurrence {index} for {:?} is absent from the manifest",
                source.relative_path
            ))
        })?;
        if unit.name != expected.name || unit.file_options != expected.file_options {
            return Err(error(format!(
                "compiler unit occurrence {index} for {:?} no longer matches the manifest",
                source.relative_path
            )));
        }
        let actual_content = match &unit.content {
            Some(content) => UnitContent::Present {
                utf8_bytes: content.len() as u64,
                sha256: sha256_hex(content.as_bytes()),
            },
            None => UnitContent::Missing,
        };
        if actual_content != expected.content {
            return Err(error(format!(
                "compiler unit content {index} for {:?} no longer matches the manifest",
                source.relative_path
            )));
        }
        let symlinks = exact_setting(&unit.file_options, "symlink")
            .filter(|value| !value.is_empty())
            .map(|value| value.split(',').map(js_trim).collect::<Vec<_>>())
            .unwrap_or_default();
        if symlinks != expected.document_symlinks {
            return Err(error(format!(
                "compiler document symlinks {index} for {:?} no longer match the manifest",
                source.relative_path
            )));
        }
    }
    if normal_offset != recorded.normal_units.len()
        || config_offset.is_some() != recorded.virtual_config.is_some()
    {
        return Err(error(format!(
            "compiler unit partition for {:?} no longer matches the manifest",
            source.relative_path
        )));
    }
    Ok(())
}

fn build_compiler_unit(
    id: CompilerUnitId,
    unit: ParsedUnit,
    original_fixture_path: Arc<str>,
    current_directory: &str,
) -> HarnessResult<CompilerUnitInput> {
    let name: Arc<str> = Arc::from(unit.name);
    let document_symlinks = exact_setting(&unit.file_options, "symlink")
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .split(',')
                .map(js_trim)
                .map(|link_path| {
                    let anchor: Arc<str> = Arc::from(current_directory);
                    Ok(CompilerSymlinkOperation {
                        phase: CompilerSymlinkPhase::Document,
                        raw_target: Arc::clone(&name),
                        raw_link_path: Arc::from(link_path),
                        normalized_target: Arc::from(normalize_virtual_path(
                            current_directory,
                            name.as_ref(),
                        )?),
                        normalized_link_path: Arc::from(normalize_virtual_path(
                            current_directory,
                            link_path,
                        )?),
                        anchor,
                    })
                })
                .collect::<HarnessResult<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();
    Ok(CompilerUnitInput {
        id,
        name,
        content: unit.content.map(Arc::from),
        file_options: Arc::from(unit.file_options),
        original_fixture_path,
        references: Arc::from([]),
        document_symlinks: Arc::from(document_symlinks),
    })
}

fn build_compiler_plan(
    fixture: Arc<CompilerFixtureInput>,
    configuration_index: u32,
) -> HarnessResult<CompilerExecutionPlan> {
    let recorded = fixture.source.index;
    let manifest_configuration = fixture
        .configurations
        .get(configuration_index as usize)
        .ok_or_else(|| {
            error(format!(
                "compiler source index {recorded} has no configuration {configuration_index}"
            ))
        })?
        .clone();
    let effective_settings = if manifest_configuration.settings.is_empty() {
        Arc::clone(&fixture.settings)
    } else {
        Arc::from(merge_ordered_settings(
            fixture.settings.as_ref(),
            &manifest_configuration.settings,
        ))
    };
    let current_directory = compiler_current_directory(&effective_settings)?;
    let use_case_sensitive_file_names = compiler_case_sensitivity(&effective_settings);
    let root_selection = compiler_root_selection(&fixture, &effective_settings)?;

    Ok(CompilerExecutionPlan {
        fixture,
        variant: CompilerVariant {
            configuration_index,
            key: Arc::from(manifest_configuration.variant.as_str()),
            description: Arc::from(manifest_configuration.description.as_str()),
            upstream_name: Arc::from(manifest_configuration.upstream_name.as_str()),
            overrides: Arc::from(manifest_configuration.settings.clone()),
        },
        effective_settings,
        current_directory: Arc::from(current_directory),
        use_case_sensitive_file_names,
        root_selection,
    })
}

fn compiler_root_selection(
    fixture: &CompilerFixtureInput,
    settings: &[OrderedSetting],
) -> HarnessResult<CompilerRootSelection> {
    let candidates = fixture
        .units
        .iter()
        .filter(|unit| Some(unit.id) != fixture.config_unit)
        .map(|unit| unit.id)
        .collect::<Vec<_>>();
    if let Some(config_unit) = fixture.config_unit {
        return Ok(CompilerRootSelection::ConfigDriven {
            config_unit,
            config_host_units: Arc::from(
                fixture.units.iter().map(|unit| unit.id).collect::<Vec<_>>(),
            ),
            candidate_units: Arc::from(candidates),
        });
    }
    let last = candidates
        .last()
        .copied()
        .ok_or_else(|| error("compiler fixture has no normal unit"))?;
    let last_unit = fixture
        .units
        .get(last.0 as usize)
        .ok_or_else(|| error("compiler fixture last unit is out of bounds"))?;
    let last_content = last_unit.content.as_deref().unwrap_or_default();
    let implicit_references = exact_setting(settings, "noImplicitReferences")
        .is_some_and(|value| !value.is_empty())
        || last_content.contains("require(")
        || contains_reference_path(last_content);
    if implicit_references {
        let other_units = candidates
            .into_iter()
            .filter(|id| {
                fixture
                    .units
                    .get(id.0 as usize)
                    .is_some_and(|unit| unit.name != last_unit.name)
            })
            .collect::<Vec<_>>();
        Ok(explicit_compiler_roots(
            fixture,
            CompilerExplicitRootReason::LastUnitImplicitReferences,
            vec![last],
            other_units,
        ))
    } else {
        Ok(explicit_compiler_roots(
            fixture,
            CompilerExplicitRootReason::AllUnits,
            candidates,
            Vec::new(),
        ))
    }
}

fn explicit_compiler_roots(
    fixture: &CompilerFixtureInput,
    reason: CompilerExplicitRootReason,
    root_units: Vec<CompilerUnitId>,
    other_units: Vec<CompilerUnitId>,
) -> CompilerRootSelection {
    let vfs_write_order = root_units
        .iter()
        .chain(&other_units)
        .copied()
        .collect::<Vec<_>>();
    let program_root_units = root_units
        .iter()
        .copied()
        .filter(|id| {
            fixture.units.get(id.0 as usize).is_some_and(|unit| {
                unit.name.len() <= ".json".len() || !unit.name.ends_with(".json")
            })
        })
        .collect::<Vec<_>>();
    CompilerRootSelection::Explicit {
        reason,
        root_units: Arc::from(root_units),
        other_units: Arc::from(other_units),
        vfs_write_order: Arc::from(vfs_write_order),
        program_root_units: Arc::from(program_root_units),
    }
}

fn build_project_fixture(
    recorded: &super::ProjectFixtureExpansion,
    source: Arc<VerifiedSource>,
    tests_mount: Arc<ProjectMount>,
) -> HarnessResult<ProjectFixtureInput> {
    if source.encoding != recorded.encoding {
        return Err(error(format!(
            "project descriptor {:?} encoding no longer matches the manifest",
            source.relative_path
        )));
    }
    let properties = parse_ordered_json_object(&source.decoded, source.relative_path.as_ref())?;
    let scenario =
        required_ordered_string(&properties, "scenario", source.relative_path.as_ref())?.to_owned();
    let project_root =
        required_ordered_string(&properties, "projectRoot", source.relative_path.as_ref())?
            .to_owned();
    if scenario != recorded.scenario || project_root != recorded.project_root {
        return Err(error(format!(
            "project descriptor {:?} identity no longer matches the manifest",
            source.relative_path
        )));
    }
    verify_project_inputs(
        &properties,
        &recorded.input_files,
        source.relative_path.as_ref(),
    )?;
    let current_directory = normalize_virtual_path(VIRTUAL_SOURCE_ROOT, &project_root)?;
    let root_selection =
        project_root_selection(&properties, &recorded.input_files, &current_directory)?;
    Ok(ProjectFixtureInput {
        descriptor_raw: Arc::clone(&source.raw),
        descriptor_text: Arc::clone(&source.decoded),
        properties: Arc::from(
            properties
                .into_iter()
                .map(|(name, value)| OrderedJsonProperty {
                    name: Arc::from(name),
                    value,
                })
                .collect::<Vec<_>>(),
        ),
        scenario: Arc::from(scenario),
        project_root: Arc::from(project_root),
        input_files: recorded.input_files.clone(),
        current_directory: Arc::from(current_directory),
        mount: tests_mount,
        root_selection,
        source,
    })
}

fn build_project_plan(
    fixture: Arc<ProjectFixtureInput>,
    module: ProjectModule,
    baseline_folder: &str,
) -> ProjectExecutionPlan {
    let descriptor_module_override = fixture
        .properties
        .iter()
        .filter(|property| property.name.as_ref() == "module")
        .map(|property| property.value.clone())
        .next_back();
    ProjectExecutionPlan {
        fixture,
        module_variant: module,
        baseline_folder: Arc::from(baseline_folder),
        descriptor_module_override,
    }
}

fn project_root_selection(
    properties: &[(String, Value)],
    inputs: &ProjectInputFiles,
    current_directory: &str,
) -> HarnessResult<ProjectRootSelection> {
    let raw_project = properties
        .iter()
        .filter(|(name, _)| name == "project")
        .filter_map(|(_, value)| value.as_str())
        .next_back()
        .filter(|value| !value.is_empty());
    if let Some(project) = raw_project {
        if matches!(inputs, ProjectInputFiles::Present { inputs } if !inputs.is_empty()) {
            return Err(error(
                "project descriptor cannot combine a project option with explicit input files",
            ));
        }
        let config_file_name = normalize_posix_path(&join_posix(project, "tsconfig.json"), false)?;
        let resolved_config_path = normalize_virtual_path(current_directory, &config_file_name)?;
        return Ok(ProjectRootSelection::ProjectConfig {
            raw_project: Arc::from(project),
            config_file_name: Arc::from(config_file_name),
            resolved_config_path: Arc::from(resolved_config_path),
        });
    }
    match inputs {
        ProjectInputFiles::Present { inputs } if !inputs.is_empty() => {
            Ok(ProjectRootSelection::Explicit {
                input_names: Arc::from(
                    inputs
                        .iter()
                        .map(|input| Arc::from(input.path.as_str()))
                        .collect::<Vec<_>>(),
                ),
            })
        }
        ProjectInputFiles::Absent | ProjectInputFiles::Present { .. } => {
            Ok(ProjectRootSelection::DiscoverConfig)
        }
    }
}

fn verify_project_inputs(
    properties: &[(String, Value)],
    recorded: &ProjectInputFiles,
    fixture_path: &str,
) -> HarnessResult<()> {
    let value = properties
        .iter()
        .find(|(name, _)| name == "inputFiles")
        .map(|(_, value)| value);
    match (value, recorded) {
        (None, ProjectInputFiles::Absent) => Ok(()),
        (Some(Value::Array(values)), ProjectInputFiles::Present { inputs }) => {
            let actual = values
                .iter()
                .map(|value| {
                    value.as_str().ok_or_else(|| {
                        error(format!(
                            "project descriptor {fixture_path:?} inputFiles entries must be strings"
                        ))
                    })
                })
                .collect::<HarnessResult<Vec<_>>>()?;
            if actual
                == inputs
                    .iter()
                    .map(|input| input.path.as_str())
                    .collect::<Vec<_>>()
            {
                Ok(())
            } else {
                Err(error(format!(
                    "project descriptor {fixture_path:?} inputs no longer match the manifest"
                )))
            }
        }
        _ => Err(error(format!(
            "project descriptor {fixture_path:?} inputFiles state no longer matches the manifest"
        ))),
    }
}

#[derive(Debug)]
struct OrderedObject(Vec<(String, Value)>);

impl<'de> Deserialize<'de> for OrderedObject {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct OrderedObjectVisitor;

        impl<'de> Visitor<'de> for OrderedObjectVisitor {
            type Value = OrderedObject;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON object")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut properties = Vec::with_capacity(map.size_hint().unwrap_or(0));
                while let Some((name, value)) = map.next_entry::<String, Value>()? {
                    if let Some((_, existing)) = properties
                        .iter_mut()
                        .find(|(existing, _)| existing == &name)
                    {
                        *existing = value;
                    } else {
                        properties.push((name, value));
                    }
                }
                Ok(OrderedObject(properties))
            }
        }

        deserializer.deserialize_map(OrderedObjectVisitor)
    }
}

fn parse_ordered_json_object(text: &str, path: &str) -> HarnessResult<Vec<(String, Value)>> {
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let object = OrderedObject::deserialize(&mut deserializer).map_err(|source| {
        error(format!(
            "project descriptor {path:?} is invalid JSON: {source}"
        ))
    })?;
    deserializer.end().map_err(|source| {
        error(format!(
            "project descriptor {path:?} has trailing JSON content: {source}"
        ))
    })?;
    Ok(object.0)
}

fn required_ordered_string<'a>(
    properties: &'a [(String, Value)],
    name: &str,
    path: &str,
) -> HarnessResult<&'a str> {
    properties
        .iter()
        .find(|(candidate, _)| candidate == name)
        .and_then(|(_, value)| value.as_str())
        .ok_or_else(|| {
            error(format!(
                "project descriptor {path:?} field {name:?} must be a string"
            ))
        })
}

fn compiler_current_directory(settings: &[OrderedSetting]) -> HarnessResult<String> {
    exact_setting(settings, "currentDirectory")
        .map(|value| normalize_virtual_path(VIRTUAL_SOURCE_ROOT, value))
        .transpose()
        .map(|value| value.unwrap_or_else(|| VIRTUAL_SOURCE_ROOT.to_owned()))
}

fn compiler_case_sensitivity(settings: &[OrderedSetting]) -> bool {
    settings
        .iter()
        .filter(|setting| {
            setting
                .name
                .eq_ignore_ascii_case("useCaseSensitiveFileNames")
        })
        .map(|setting| setting.value.eq_ignore_ascii_case("true"))
        .next_back()
        .unwrap_or(true)
}

fn merge_ordered_settings(
    base: &[OrderedSetting],
    overrides: &[OrderedSetting],
) -> Vec<OrderedSetting> {
    let mut result = base.to_vec();
    for setting in overrides {
        if let Some(existing) = result
            .iter_mut()
            .find(|existing| existing.name == setting.name)
        {
            existing.value.clone_from(&setting.value);
        } else {
            result.push(setting.clone());
        }
    }
    result
}

fn exact_setting<'a>(settings: &'a [OrderedSetting], name: &str) -> Option<&'a str> {
    settings
        .iter()
        .find(|setting| setting.name == name)
        .map(|setting| setting.value.as_str())
}

fn contains_reference_path(text: &str) -> bool {
    text.match_indices("reference").any(|(index, _)| {
        let suffix = &text[index + "reference".len()..];
        let mut chars = suffix.chars();
        chars.next().is_some_and(is_js_whitespace) && chars.as_str().starts_with("path")
    })
}

fn is_js_whitespace(ch: char) -> bool {
    matches!(
        ch,
        '\u{0009}'
            | '\u{000a}'
            | '\u{000b}'
            | '\u{000c}'
            | '\u{000d}'
            | '\u{0020}'
            | '\u{00a0}'
            | '\u{1680}'
            | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

fn js_trim(value: &str) -> &str {
    value.trim_matches(is_js_whitespace)
}

fn normalize_virtual_path(base: &str, path: &str) -> HarnessResult<String> {
    let path = path.replace('\\', "/");
    let combined = if path.starts_with('/') {
        path
    } else if base.ends_with('/') {
        format!("{base}{path}")
    } else {
        format!("{base}/{path}")
    };
    normalize_posix_path(&combined, true)
}

fn normalize_posix_path(path: &str, require_absolute: bool) -> HarnessResult<String> {
    if path.contains('\0') {
        return Err(error(format!("virtual path contains NUL: {path:?}")));
    }
    let path = path.replace('\\', "/");
    let absolute = path.starts_with('/');
    if require_absolute && !absolute {
        return Err(error(format!("virtual path is not absolute: {path:?}")));
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|part| *part != "..") {
                    parts.pop();
                } else if !absolute {
                    parts.push("..");
                }
            }
            part => parts.push(part),
        }
    }
    let body = parts.join("/");
    if absolute {
        Ok(if body.is_empty() {
            "/".to_owned()
        } else {
            format!("/{body}")
        })
    } else {
        Ok(if body.is_empty() {
            ".".to_owned()
        } else {
            body
        })
    }
}

fn source_entry(manifest: &ExpansionManifest, index: u32) -> HarnessResult<&SourceInventoryEntry> {
    manifest.sources.get(index as usize).ok_or_else(|| {
        error(format!(
            "execution plan references missing source index {index}"
        ))
    })
}

fn suite_identity(
    manifest: &ExpansionManifest,
    suite: SuiteName,
) -> HarnessResult<&super::CorpusSuiteIdentity> {
    manifest
        .corpus_pin
        .suites
        .iter()
        .find(|identity| identity.name == suite)
        .ok_or_else(|| error(format!("manifest has no {} suite identity", suite.as_str())))
}

fn join_posix(left: &str, right: &str) -> String {
    if left.is_empty() {
        right.to_owned()
    } else if right.is_empty() {
        left.to_owned()
    } else {
        format!(
            "{}/{}",
            left.trim_end_matches('/'),
            right.trim_start_matches('/')
        )
    }
}

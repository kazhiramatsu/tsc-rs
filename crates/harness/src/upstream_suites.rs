//! Deterministic inventory and expansion of the TypeScript 6.0.3 compiler and
//! project test suites.
//!
//! The inventory itself deliberately stops before checker, emit, and baseline
//! execution. Every expanded case is recorded as [`ExecutionState::NotRun`],
//! so later executors can shard work without weakening the completeness
//! contract captured here. [`execution::load_compiler_no_emit`] and the
//! focused project adapters expose the bounded source/config/loader seams
//! without claiming that the full upstream runners have executed.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{HarnessError, HarnessResult};

mod compiler;
pub mod execution;

pub const SCHEMA: u32 = 1;
pub const TYPESCRIPT_VERSION: &str = "6.0.3";
pub const SOURCE_REPOSITORY: &str = "https://github.com/microsoft/TypeScript.git";
pub const SOURCE_COMMIT: &str = "050880ce59e30b356b686bd3144efe24f875ebc8";
pub const CORPUS_PIN_RELATIVE_PATH: &str = "vendor/typescript-6.0.3/test-suites-pin.v1.json";
pub const CORPUS_PIN_SHA256: &str =
    "f231d984c31d5d16a6fb845e66a25bc9601ffd23212d548cb337149e40397da9";
pub const MANIFEST_RELATIVE_PATH: &str = "vendor/typescript-6.0.3/test-suite-expansion.v1.json";
pub const VIRTUAL_SOURCE_ROOT: &str = "/.src";
pub const MAX_COMPILER_VARIATIONS: usize = 25;

const EXPECTED_CORPUS_FILES: u64 = 7_086;
const EXPECTED_CORPUS_BYTES: u64 = 4_718_142;
const EXPECTED_SOURCE_INVENTORY_SHA256: &str =
    "56f9d1e9d8f088fc5656b79f72bfc16a77da0b8f41448bcaa3a26966d61a897b";
const EXPECTED_COMPILER_SOURCES: u64 = 6_537;
const EXPECTED_COMPILER_DEFAULT_FIXTURES: u64 = 5_982;
const EXPECTED_COMPILER_MATRIX_FIXTURES: u64 = 555;
const EXPECTED_COMPILER_CASES: u64 = 7_276;
const EXPECTED_COMPILER_NORMAL_UNITS: u64 = 8_592;
const EXPECTED_COMPILER_VIRTUAL_CONFIGS: u64 = 103;
const EXPECTED_COMPILER_PRESENT_EMPTY_UNITS: u64 = 27;
const EXPECTED_COMPILER_MISSING_CONTENT_UNITS: u64 = 1;
const EXPECTED_COMPILER_LINK_DIRECTIVES: u64 = 35;
const EXPECTED_COMPILER_DOCUMENT_SYMLINK_DIRECTIVES: u64 = 5;
const EXPECTED_COMPILER_DOCUMENT_SYMLINK_PATHS: u64 = 7;
const EXPECTED_PROJECT_DESCRIPTORS: u64 = 316;
const EXPECTED_PROJECT_BACKING_FILES: u64 = 233;
const EXPECTED_PROJECT_CASES: u64 = 632;
const EXPECTED_PROJECT_DECLARED_INPUTS: u64 = 302;
const EXPECTED_PROJECT_MISSING_BACKING_INPUTS: u64 = 3;
const EXPECTED_TOTAL_CASES: u64 = 7_908;

const EXPECTED_SUITES: [(&str, &str, &str); 3] = [
    (
        "compiler",
        "tests/cases/compiler",
        "ts-tests/tests/cases/compiler",
    ),
    (
        "project",
        "tests/cases/project",
        "ts-tests/tests/cases/project",
    ),
    (
        "projects",
        "tests/cases/projects",
        "ts-tests/tests/cases/projects",
    ),
];

struct ExpectedSuiteIdentity {
    git_tree_sha1: &'static str,
    blob_inventory_sha256: &'static str,
    files: u64,
    bytes: u64,
    unique_blobs: u64,
    executable_paths: &'static [&'static str],
}

const EXPECTED_COMPILER_IDENTITY: ExpectedSuiteIdentity = ExpectedSuiteIdentity {
    git_tree_sha1: "9982425f8fc156678687afd21aa6f7ac681a7c01",
    blob_inventory_sha256: "7d21dca57fe92942f818333b9b12b51139b740bc334179d4e89cd73e2251c434",
    files: 6_537,
    bytes: 4_588_680,
    unique_blobs: 6_523,
    executable_paths: &[
        "parserPrivateIdentifierInArrayAssignment.ts",
        "taggedTemplateWithoutDeclaredHelper.ts",
    ],
};

const EXPECTED_PROJECT_IDENTITY: ExpectedSuiteIdentity = ExpectedSuiteIdentity {
    git_tree_sha1: "c13353cca2c55fb831ba40ca022b29839fdfab26",
    blob_inventory_sha256: "04fe8a0e3777b9a51bf150377e15363dd930d13de2608cd25de306dedb45cde5",
    files: 316,
    bytes: 100_994,
    unique_blobs: 316,
    executable_paths: &[],
};

const EXPECTED_PROJECTS_IDENTITY: ExpectedSuiteIdentity = ExpectedSuiteIdentity {
    git_tree_sha1: "85282d8f70ef48a26a7b9f9c4e0bee88a1cf16e7",
    blob_inventory_sha256: "a881ee1ae51c01e8455bdedf040900b6bfa403aa745a2c809420469b9d1b4926",
    files: 233,
    bytes: 28_468,
    unique_blobs: 154,
    executable_paths: &[],
};

const IMPLEMENTATION_SOURCE_PINS: [(&str, &str); 6] = [
    (
        "src/compiler/commandLineParser.ts",
        "c17cc4ef9ca01cedd915a7040efb248aa19d2e18",
    ),
    (
        "src/compiler/sys.ts",
        "c8f176603002b7314f012afa412b3caba3486d5b",
    ),
    (
        "src/harness/harnessIO.ts",
        "a06bde1c95182ea1bfad0ddf9af73053501a6dc7",
    ),
    (
        "src/harness/vfsUtil.ts",
        "b217fb57bba950c13d5d2e821b0652eacce0e65f",
    ),
    (
        "src/testRunner/compilerRunner.ts",
        "aed00f47656b316f3f20c913e2408a128d4671cb",
    ),
    (
        "src/testRunner/projectsRunner.ts",
        "5befdf497dff2accd67e08c3c51100b66f1b14b5",
    ),
];

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SuiteName {
    Compiler,
    Project,
    Projects,
}

impl SuiteName {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Compiler => "compiler",
            Self::Project => "project",
            Self::Projects => "projects",
        }
    }

    fn parse(value: &str) -> HarnessResult<Self> {
        match value {
            "compiler" => Ok(Self::Compiler),
            "project" => Ok(Self::Project),
            "projects" => Ok(Self::Projects),
            _ => Err(error(format!("unknown upstream suite {value:?}"))),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpansionManifest {
    pub schema: u32,
    pub typescript_version: String,
    pub source_repository: String,
    pub source_commit: String,
    pub corpus_pin: CorpusPinIdentity,
    pub implementation_sources: Vec<ImplementationSourcePin>,
    pub virtual_source_root: String,
    pub source_inventory_sha256: String,
    pub sources: Vec<SourceInventoryEntry>,
    pub compiler_fixtures: Vec<CompilerFixtureExpansion>,
    pub project_fixtures: Vec<ProjectFixtureExpansion>,
    pub cases: Vec<ExpandedCase>,
    pub summary: ExpansionSummary,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusPinIdentity {
    pub path: String,
    pub sha256: String,
    pub suites: Vec<CorpusSuiteIdentity>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusSuiteIdentity {
    pub name: SuiteName,
    pub source_path: String,
    pub vendored_path: String,
    pub git_tree_sha1: String,
    pub blob_inventory_sha256: String,
    pub files: u64,
    pub bytes: u64,
    pub unique_blobs: u64,
    pub executable_paths: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImplementationSourcePin {
    pub source_path: String,
    pub git_blob_sha1: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceInventoryEntry {
    pub suite: SuiteName,
    pub path: String,
    pub mode: String,
    pub bytes: u64,
    pub sha256: String,
    pub git_blob_sha1: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SourceEncoding {
    #[serde(rename = "utf-8")]
    Utf8,
    #[serde(rename = "utf-8-bom")]
    Utf8Bom,
    #[serde(rename = "utf-16le")]
    Utf16Le,
    #[serde(rename = "utf-16be")]
    Utf16Be,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompilerFixtureExpansion {
    pub source: u32,
    pub encoding: SourceEncoding,
    pub decoded_utf8_bytes: u64,
    pub decoded_sha256: String,
    pub settings: Vec<OrderedSetting>,
    pub normal_units: Vec<CompilerUnit>,
    pub virtual_config: Option<CompilerUnit>,
    pub links: Vec<CompilerLink>,
    pub configurations: Vec<CompilerConfiguration>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OrderedSetting {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompilerUnit {
    pub name: String,
    pub file_options: Vec<OrderedSetting>,
    pub content: UnitContent,
    pub document_symlinks: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, tag = "state", rename_all = "kebab-case")]
pub enum UnitContent {
    Present { utf8_bytes: u64, sha256: String },
    Missing,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompilerLink {
    pub target: String,
    pub link_path: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompilerConfiguration {
    pub variant: String,
    pub description: String,
    pub upstream_name: String,
    pub settings: Vec<OrderedSetting>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectFixtureExpansion {
    pub source: u32,
    pub encoding: SourceEncoding,
    pub scenario: String,
    pub project_root: String,
    pub input_files: ProjectInputFiles,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, tag = "state", rename_all = "kebab-case")]
pub enum ProjectInputFiles {
    Absent,
    Present { inputs: Vec<ProjectInput> },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectInput {
    pub path: String,
    pub resolved_backing_path: String,
    pub presence: ProjectInputPresence,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, tag = "state", rename_all = "kebab-case")]
pub enum ProjectInputPresence {
    Present { source: u32 },
    Missing,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpandedCase {
    pub id: String,
    pub suite: SuiteName,
    pub source: u32,
    pub configuration: CaseConfiguration,
    pub initial_execution_state: ExecutionState,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "kebab-case")]
pub enum CaseConfiguration {
    Compiler {
        configuration: u32,
    },
    Project {
        module: ProjectModule,
        baseline_folder: String,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectModule {
    Commonjs,
    Amd,
}

impl ProjectModule {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Commonjs => "commonjs",
            Self::Amd => "amd",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ExecutionState {
    #[serde(rename = "not-run")]
    NotRun,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpansionSummary {
    pub corpus_files: u64,
    pub corpus_bytes: u64,
    pub compiler_sources: u64,
    pub compiler_default_fixtures: u64,
    pub compiler_matrix_fixtures: u64,
    pub compiler_cases: u64,
    pub compiler_normal_units: u64,
    pub compiler_virtual_configs: u64,
    pub compiler_present_empty_units: u64,
    pub compiler_missing_content_units: u64,
    pub compiler_link_directives: u64,
    pub compiler_document_symlink_directives: u64,
    pub compiler_document_symlink_paths: u64,
    pub project_descriptors: u64,
    pub project_backing_files: u64,
    pub project_cases: u64,
    pub project_declared_inputs: u64,
    pub project_missing_backing_inputs: u64,
    pub total_cases: u64,
    pub not_run_cases: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TestSuitesPin {
    schema: u32,
    typescript_version: String,
    source_repository: String,
    source_commit: String,
    suites: Vec<SuitePin>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SuitePin {
    name: String,
    source_path: String,
    vendored_path: String,
    git_tree_sha1: String,
    blob_inventory_sha256: String,
    files: u64,
    bytes: u64,
    unique_blobs: u64,
    executable_paths: Vec<String>,
}

#[derive(Debug)]
struct CollectedSource {
    entry: SourceInventoryEntry,
    raw: Vec<u8>,
}

pub fn generate_manifest(workspace: &Path) -> HarnessResult<ExpansionManifest> {
    let (pin, corpus_pin) = read_and_validate_pin(workspace)?;
    let collected = collect_corpus(workspace, &pin)?;
    let sources = collected
        .iter()
        .map(|source| source.entry.clone())
        .collect::<Vec<_>>();
    let source_inventory_sha256 = source_inventory_sha256(&sources);
    let source_lookup = sources
        .iter()
        .enumerate()
        .map(|(index, source)| Ok(((source.suite, source.path.clone()), source_index(index)?)))
        .collect::<HarnessResult<BTreeMap<_, _>>>()?;

    let mut compiler_fixtures = Vec::with_capacity(EXPECTED_COMPILER_SOURCES as usize);
    let mut project_fixtures = Vec::with_capacity(EXPECTED_PROJECT_DESCRIPTORS as usize);
    let mut cases = Vec::with_capacity(EXPECTED_TOTAL_CASES as usize);

    for (index, source) in collected.iter().enumerate() {
        if source.entry.suite != SuiteName::Compiler {
            continue;
        }
        if !is_compiler_fixture_path(&source.entry.path) {
            return Err(error(format!(
                "compiler suite contains a non-TypeScript fixture {:?}",
                source.entry.path
            )));
        }
        let source_index = source_index(index)?;
        let fixture =
            compiler::expand_compiler_fixture(source_index, &source.entry.path, &source.raw)?;
        for (configuration, expanded) in fixture.configurations.iter().enumerate() {
            cases.push(ExpandedCase {
                id: case_id(SuiteName::Compiler, &source.entry.path, &expanded.variant),
                suite: SuiteName::Compiler,
                source: source_index,
                configuration: CaseConfiguration::Compiler {
                    configuration: source_index_from_usize(configuration, "configuration")?,
                },
                initial_execution_state: ExecutionState::NotRun,
            });
        }
        compiler_fixtures.push(fixture);
    }

    for (index, source) in collected.iter().enumerate() {
        if source.entry.suite != SuiteName::Project {
            continue;
        }
        if !source.entry.path.ends_with(".json") {
            return Err(error(format!(
                "project suite contains a non-JSON descriptor {:?}",
                source.entry.path
            )));
        }
        let source_index = source_index(index)?;
        let fixture = expand_project_fixture(
            source_index,
            &source.entry.path,
            &source.raw,
            &source_lookup,
        )?;
        for (module, baseline_folder) in [
            (ProjectModule::Commonjs, "node"),
            (ProjectModule::Amd, "amd"),
        ] {
            let variant = format!("module={}", module.as_str());
            cases.push(ExpandedCase {
                id: case_id(SuiteName::Project, &source.entry.path, &variant),
                suite: SuiteName::Project,
                source: source_index,
                configuration: CaseConfiguration::Project {
                    module,
                    baseline_folder: baseline_folder.to_owned(),
                },
                initial_execution_state: ExecutionState::NotRun,
            });
        }
        project_fixtures.push(fixture);
    }

    let implementation_sources = IMPLEMENTATION_SOURCE_PINS
        .iter()
        .map(|(source_path, git_blob_sha1)| ImplementationSourcePin {
            source_path: (*source_path).to_owned(),
            git_blob_sha1: (*git_blob_sha1).to_owned(),
        })
        .collect();
    let mut manifest = ExpansionManifest {
        schema: SCHEMA,
        typescript_version: TYPESCRIPT_VERSION.to_owned(),
        source_repository: SOURCE_REPOSITORY.to_owned(),
        source_commit: SOURCE_COMMIT.to_owned(),
        corpus_pin,
        implementation_sources,
        virtual_source_root: VIRTUAL_SOURCE_ROOT.to_owned(),
        source_inventory_sha256,
        sources,
        compiler_fixtures,
        project_fixtures,
        cases,
        summary: ExpansionSummary::default(),
    };
    manifest.summary = derive_summary(&manifest)?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

pub fn render_manifest(manifest: &ExpansionManifest) -> HarnessResult<Vec<u8>> {
    validate_manifest(manifest)?;
    let mut bytes = serde_json::to_vec_pretty(manifest).map_err(|source| {
        error(format!(
            "failed to serialize upstream suite manifest: {source}"
        ))
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub fn validate_manifest(manifest: &ExpansionManifest) -> HarnessResult<ExpansionSummary> {
    if manifest.schema != SCHEMA
        || manifest.typescript_version != TYPESCRIPT_VERSION
        || manifest.source_repository != SOURCE_REPOSITORY
        || manifest.source_commit != SOURCE_COMMIT
        || manifest.virtual_source_root != VIRTUAL_SOURCE_ROOT
    {
        return Err(error(
            "upstream suite manifest metadata does not match the TypeScript 6.0.3 schema",
        ));
    }
    validate_corpus_identity(&manifest.corpus_pin)?;
    validate_implementation_source_pins(&manifest.implementation_sources)?;
    validate_sources(manifest)?;
    validate_fixtures_and_cases(manifest)?;

    let derived = derive_summary(manifest)?;
    if manifest.summary != derived {
        return Err(error(format!(
            "upstream suite manifest summary is stale: recorded={:?}, derived={derived:?}",
            manifest.summary
        )));
    }
    let expected = expected_summary();
    if derived != expected {
        return Err(error(format!(
            "TypeScript 6.0.3 upstream suite expansion changed: actual={derived:?}, expected={expected:?}"
        )));
    }
    Ok(derived)
}

fn validate_corpus_identity(identity: &CorpusPinIdentity) -> HarnessResult<()> {
    if identity.path != CORPUS_PIN_RELATIVE_PATH || identity.sha256 != CORPUS_PIN_SHA256 {
        return Err(error("manifest is not bound to the immutable corpus pin"));
    }
    if identity.suites.len() != EXPECTED_SUITES.len() {
        return Err(error(
            "manifest corpus identity must contain exactly three suites",
        ));
    }
    for (suite, (name, source_path, vendored_path)) in identity.suites.iter().zip(EXPECTED_SUITES) {
        if suite.name.as_str() != name
            || suite.source_path != source_path
            || suite.vendored_path != vendored_path
        {
            return Err(error(format!(
                "manifest corpus suite {:?} is out of canonical order or has wrong paths",
                suite.name
            )));
        }
        validate_lower_hex(&suite.git_tree_sha1, 40, "Git tree SHA-1")?;
        validate_lower_hex(&suite.blob_inventory_sha256, 64, "blob inventory SHA-256")?;
        let executable = validated_executable_paths(&suite.executable_paths)?;
        if executable.len() != suite.executable_paths.len() {
            return Err(error(format!(
                "suite {} executable paths are not unique",
                suite.name.as_str()
            )));
        }
        let expected = expected_suite_identity(suite.name);
        let expected_executable = expected
            .executable_paths
            .iter()
            .map(|path| (*path).to_owned())
            .collect::<BTreeSet<_>>();
        if suite.git_tree_sha1 != expected.git_tree_sha1
            || suite.blob_inventory_sha256 != expected.blob_inventory_sha256
            || suite.files != expected.files
            || suite.bytes != expected.bytes
            || suite.unique_blobs != expected.unique_blobs
            || executable != expected_executable
        {
            return Err(error(format!(
                "manifest corpus suite {} does not match the immutable TypeScript 6.0.3 pin identity",
                suite.name.as_str()
            )));
        }
    }
    Ok(())
}

const fn expected_suite_identity(suite: SuiteName) -> &'static ExpectedSuiteIdentity {
    match suite {
        SuiteName::Compiler => &EXPECTED_COMPILER_IDENTITY,
        SuiteName::Project => &EXPECTED_PROJECT_IDENTITY,
        SuiteName::Projects => &EXPECTED_PROJECTS_IDENTITY,
    }
}

fn validate_implementation_source_pins(pins: &[ImplementationSourcePin]) -> HarnessResult<()> {
    if pins.len() != IMPLEMENTATION_SOURCE_PINS.len() {
        return Err(error(
            "manifest has an incomplete implementation source pin set",
        ));
    }
    for (pin, (source_path, git_blob_sha1)) in pins.iter().zip(IMPLEMENTATION_SOURCE_PINS) {
        if pin.source_path != source_path || pin.git_blob_sha1 != git_blob_sha1 {
            return Err(error(format!(
                "implementation source pin {:?} does not match TypeScript 6.0.3",
                pin.source_path
            )));
        }
        validate_lower_hex(&pin.git_blob_sha1, 40, "implementation Git blob SHA-1")?;
    }
    Ok(())
}

fn validate_sources(manifest: &ExpansionManifest) -> HarnessResult<()> {
    let derived_inventory_sha256 = source_inventory_sha256(&manifest.sources);
    if manifest.source_inventory_sha256 != derived_inventory_sha256
        || manifest.source_inventory_sha256 != EXPECTED_SOURCE_INVENTORY_SHA256
    {
        return Err(error(
            "manifest source inventory SHA-256 does not match the immutable source table",
        ));
    }
    let mut previous: Option<(SuiteName, String)> = None;
    for source in &manifest.sources {
        validate_relative_posix_path(&source.path)?;
        validate_lower_hex(&source.sha256, 64, "source SHA-256")?;
        validate_lower_hex(&source.git_blob_sha1, 40, "source Git blob SHA-1")?;
        if source.mode != "100644" && source.mode != "100755" {
            return Err(error(format!(
                "source {}/{} has unsupported mode {:?}",
                source.suite.as_str(),
                source.path,
                source.mode
            )));
        }
        let key = (source.suite, source.path.clone());
        if previous.as_ref().is_some_and(|previous| previous >= &key) {
            return Err(error(format!(
                "manifest source table is not strictly canonical at {}/{}",
                source.suite.as_str(),
                source.path
            )));
        }
        previous = Some(key);
    }

    for suite in &manifest.corpus_pin.suites {
        let executable = suite
            .executable_paths
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let suite_sources = manifest
            .sources
            .iter()
            .filter(|source| source.suite == suite.name)
            .collect::<Vec<_>>();
        let mut bytes = 0_u64;
        let mut blobs = BTreeSet::new();
        let mut inventory = Vec::new();
        let mut observed_executable = BTreeSet::new();
        for source in &suite_sources {
            bytes = bytes
                .checked_add(source.bytes)
                .ok_or_else(|| error("manifest source byte count overflow"))?;
            blobs.insert(source.git_blob_sha1.as_str());
            if source.mode == "100755" {
                observed_executable.insert(source.path.clone());
            }
            inventory.extend_from_slice(source.mode.as_bytes());
            inventory.extend_from_slice(b" blob ");
            inventory.extend_from_slice(source.git_blob_sha1.as_bytes());
            inventory.push(b' ');
            inventory.extend_from_slice(source.bytes.to_string().as_bytes());
            inventory.push(b'\t');
            inventory.extend_from_slice(source.path.as_bytes());
            inventory.push(0);
        }
        let files = u64::try_from(suite_sources.len())
            .map_err(|_| error("manifest source count overflow"))?;
        let unique_blobs =
            u64::try_from(blobs.len()).map_err(|_| error("manifest blob count overflow"))?;
        if files != suite.files
            || bytes != suite.bytes
            || unique_blobs != suite.unique_blobs
            || sha256_hex(&inventory) != suite.blob_inventory_sha256
            || observed_executable != executable
        {
            return Err(error(format!(
                "manifest source table does not reconstruct pinned {} blob inventory",
                suite.name.as_str()
            )));
        }
    }
    Ok(())
}

fn validate_fixtures_and_cases(manifest: &ExpansionManifest) -> HarnessResult<()> {
    let compiler_sources = manifest
        .sources
        .iter()
        .enumerate()
        .filter(|(_, source)| source.suite == SuiteName::Compiler)
        .map(|(index, _)| source_index(index))
        .collect::<HarnessResult<Vec<_>>>()?;
    let project_sources = manifest
        .sources
        .iter()
        .enumerate()
        .filter(|(_, source)| source.suite == SuiteName::Project)
        .map(|(index, _)| source_index(index))
        .collect::<HarnessResult<Vec<_>>>()?;
    if manifest.compiler_fixtures.len() != compiler_sources.len()
        || manifest.project_fixtures.len() != project_sources.len()
    {
        return Err(error(
            "fixture expansion does not cover every suite source exactly once",
        ));
    }

    let mut case_offset = 0_usize;
    let mut case_ids = BTreeSet::new();
    for (fixture, expected_source) in manifest.compiler_fixtures.iter().zip(compiler_sources) {
        if fixture.source != expected_source || fixture.configurations.is_empty() {
            return Err(error(
                "compiler fixtures are incomplete or out of source order",
            ));
        }
        let source = source_at(manifest, fixture.source)?;
        validate_lower_hex(&fixture.decoded_sha256, 64, "decoded source SHA-256")?;
        validate_ordered_settings(&fixture.settings, "compiler settings")?;
        for unit in fixture
            .normal_units
            .iter()
            .chain(fixture.virtual_config.iter())
        {
            validate_compiler_unit(unit)?;
        }
        for link in &fixture.links {
            if link.target.is_empty() || link.link_path.is_empty() {
                return Err(error(format!(
                    "compiler fixture {:?} contains an empty @link endpoint",
                    source.path
                )));
            }
        }
        let mut variants = BTreeSet::new();
        for (configuration_index, configuration) in fixture.configurations.iter().enumerate() {
            validate_compiler_configuration(&source.path, configuration)?;
            if !variants.insert(configuration.variant.as_str()) {
                return Err(error(format!(
                    "compiler fixture {:?} has duplicate variant {:?}",
                    source.path, configuration.variant
                )));
            }
            let case = manifest.cases.get(case_offset).ok_or_else(|| {
                error(format!(
                    "missing case for compiler fixture {:?}",
                    source.path
                ))
            })?;
            let expected_configuration =
                source_index_from_usize(configuration_index, "configuration")?;
            if case.id != case_id(SuiteName::Compiler, &source.path, &configuration.variant)
                || case.suite != SuiteName::Compiler
                || case.source != fixture.source
                || case.initial_execution_state != ExecutionState::NotRun
                || case.configuration
                    != (CaseConfiguration::Compiler {
                        configuration: expected_configuration,
                    })
            {
                return Err(error(format!(
                    "compiler case at offset {case_offset} does not match fixture {:?}",
                    source.path
                )));
            }
            if !case_ids.insert(case.id.as_str()) {
                return Err(error(format!("duplicate expanded case ID {:?}", case.id)));
            }
            case_offset += 1;
        }
    }

    let source_lookup = manifest
        .sources
        .iter()
        .enumerate()
        .map(|(index, source)| Ok(((source.suite, source.path.as_str()), source_index(index)?)))
        .collect::<HarnessResult<BTreeMap<_, _>>>()?;
    for (fixture, expected_source) in manifest.project_fixtures.iter().zip(project_sources) {
        if fixture.source != expected_source || fixture.scenario.is_empty() {
            return Err(error(
                "project fixtures are incomplete or out of source order",
            ));
        }
        let source = source_at(manifest, fixture.source)?;
        if let ProjectInputFiles::Present { inputs } = &fixture.input_files {
            for input in inputs {
                let expected_path = resolve_project_input(&fixture.project_root, &input.path)?;
                if input.resolved_backing_path != expected_path {
                    return Err(error(format!(
                        "project fixture {:?} has a stale resolved input {:?}",
                        source.path, input.path
                    )));
                }
                match (
                    &input.presence,
                    source_lookup.get(&(SuiteName::Projects, expected_path.as_str())),
                ) {
                    (ProjectInputPresence::Present { source }, Some(expected))
                        if source == expected => {}
                    (ProjectInputPresence::Missing, None) => {}
                    _ => {
                        return Err(error(format!(
                            "project fixture {:?} records the wrong backing presence for {:?}",
                            source.path, input.path
                        )))
                    }
                }
            }
        }
        for (module, baseline_folder) in [
            (ProjectModule::Commonjs, "node"),
            (ProjectModule::Amd, "amd"),
        ] {
            let case = manifest.cases.get(case_offset).ok_or_else(|| {
                error(format!(
                    "missing case for project fixture {:?}",
                    source.path
                ))
            })?;
            let variant = format!("module={}", module.as_str());
            if case.id != case_id(SuiteName::Project, &source.path, &variant)
                || case.suite != SuiteName::Project
                || case.source != fixture.source
                || case.initial_execution_state != ExecutionState::NotRun
                || case.configuration
                    != (CaseConfiguration::Project {
                        module,
                        baseline_folder: baseline_folder.to_owned(),
                    })
            {
                return Err(error(format!(
                    "project case at offset {case_offset} does not match fixture {:?}",
                    source.path
                )));
            }
            if !case_ids.insert(case.id.as_str()) {
                return Err(error(format!("duplicate expanded case ID {:?}", case.id)));
            }
            case_offset += 1;
        }
    }
    if case_offset != manifest.cases.len() {
        return Err(error(format!(
            "manifest has {} unreferenced expanded cases",
            manifest.cases.len() - case_offset
        )));
    }
    Ok(())
}

fn source_at(manifest: &ExpansionManifest, source: u32) -> HarnessResult<&SourceInventoryEntry> {
    manifest.sources.get(source as usize).ok_or_else(|| {
        error(format!(
            "fixture references nonexistent source index {source}"
        ))
    })
}

fn validate_compiler_unit(unit: &CompilerUnit) -> HarnessResult<()> {
    if unit.name.is_empty() {
        return Err(error("compiler unit has an empty name"));
    }
    validate_ordered_settings(&unit.file_options, "compiler unit file options")?;
    if let UnitContent::Present { sha256, .. } = &unit.content {
        validate_lower_hex(sha256, 64, "compiler unit content SHA-256")?;
    }
    if unit.document_symlinks.iter().any(|link| link.is_empty()) {
        return Err(error(format!(
            "compiler unit {:?} has an empty @symlink path",
            unit.name
        )));
    }
    Ok(())
}

fn validate_ordered_settings(settings: &[OrderedSetting], description: &str) -> HarnessResult<()> {
    let mut names = BTreeSet::new();
    for setting in settings {
        if setting.name.is_empty() || !names.insert(setting.name.as_str()) {
            return Err(error(format!(
                "{description} contain an empty or duplicate exact-case key {:?}",
                setting.name
            )));
        }
    }
    Ok(())
}

fn validate_compiler_configuration(
    fixture_path: &str,
    configuration: &CompilerConfiguration,
) -> HarnessResult<()> {
    validate_ordered_settings(&configuration.settings, "compiler configuration settings")?;
    let basename = fixture_path.rsplit('/').next().unwrap_or(fixture_path);
    if configuration.settings.is_empty() {
        if configuration.variant != "default"
            || !configuration.description.is_empty()
            || configuration.upstream_name != basename
        {
            return Err(error(format!(
                "default configuration for {fixture_path:?} is not canonical"
            )));
        }
        return Ok(());
    }
    let mut sorted = configuration.settings.iter().collect::<Vec<_>>();
    sorted.sort_by(|left, right| left.name.cmp(&right.name));
    let variant = sorted
        .iter()
        .map(|setting| {
            format!(
                "{}={}",
                setting.name.to_lowercase(),
                setting.value.to_lowercase()
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let description = sorted
        .iter()
        .map(|setting| format!("@{}: {}", setting.name, setting.value))
        .collect::<Vec<_>>()
        .join(", ");
    let (stem, extension) = compiler_basename_parts(basename)?;
    let upstream_name = format!("{stem}({variant}){extension}");
    if configuration.variant != variant
        || configuration.description != description
        || configuration.upstream_name != upstream_name
    {
        return Err(error(format!(
            "configuration for {fixture_path:?} is not in upstream canonical form"
        )));
    }
    Ok(())
}

fn compiler_basename_parts(basename: &str) -> HarnessResult<(&str, &str)> {
    for extension in [".tsx", ".ts"] {
        if let Some(stem) = basename.strip_suffix(extension) {
            return Ok((stem, extension));
        }
    }
    Err(error(format!(
        "compiler fixture {basename:?} has unsupported extension"
    )))
}

fn derive_summary(manifest: &ExpansionManifest) -> HarnessResult<ExpansionSummary> {
    let mut summary = ExpansionSummary {
        corpus_files: u64::try_from(manifest.sources.len())
            .map_err(|_| error("corpus file count overflow"))?,
        corpus_bytes: manifest.sources.iter().try_fold(0_u64, |total, source| {
            total
                .checked_add(source.bytes)
                .ok_or_else(|| error("corpus byte count overflow"))
        })?,
        compiler_sources: u64::try_from(manifest.compiler_fixtures.len())
            .map_err(|_| error("compiler fixture count overflow"))?,
        project_descriptors: u64::try_from(manifest.project_fixtures.len())
            .map_err(|_| error("project fixture count overflow"))?,
        project_backing_files: u64::try_from(
            manifest
                .sources
                .iter()
                .filter(|source| source.suite == SuiteName::Projects)
                .count(),
        )
        .map_err(|_| error("project backing file count overflow"))?,
        total_cases: u64::try_from(manifest.cases.len())
            .map_err(|_| error("expanded case count overflow"))?,
        not_run_cases: u64::try_from(
            manifest
                .cases
                .iter()
                .filter(|case| case.initial_execution_state == ExecutionState::NotRun)
                .count(),
        )
        .map_err(|_| error("not-run case count overflow"))?,
        ..ExpansionSummary::default()
    };
    for fixture in &manifest.compiler_fixtures {
        if fixture.configurations.len() == 1 && fixture.configurations[0].variant == "default" {
            summary.compiler_default_fixtures += 1;
        } else {
            summary.compiler_matrix_fixtures += 1;
        }
        summary.compiler_cases = summary
            .compiler_cases
            .checked_add(fixture.configurations.len() as u64)
            .ok_or_else(|| error("compiler case count overflow"))?;
        summary.compiler_normal_units = summary
            .compiler_normal_units
            .checked_add(fixture.normal_units.len() as u64)
            .ok_or_else(|| error("compiler unit count overflow"))?;
        summary.compiler_virtual_configs += u64::from(fixture.virtual_config.is_some());
        for unit in &fixture.normal_units {
            match unit.content {
                UnitContent::Present { utf8_bytes: 0, .. } => {
                    summary.compiler_present_empty_units += 1;
                }
                UnitContent::Missing => summary.compiler_missing_content_units += 1,
                UnitContent::Present { .. } => {}
            }
        }
        summary.compiler_link_directives += fixture.links.len() as u64;
        for unit in fixture
            .normal_units
            .iter()
            .chain(fixture.virtual_config.iter())
        {
            if unit
                .file_options
                .iter()
                .any(|setting| setting.name == "symlink")
            {
                summary.compiler_document_symlink_directives += 1;
            }
            summary.compiler_document_symlink_paths += unit.document_symlinks.len() as u64;
        }
    }
    for fixture in &manifest.project_fixtures {
        summary.project_cases += 2;
        if let ProjectInputFiles::Present { inputs } = &fixture.input_files {
            summary.project_declared_inputs += inputs.len() as u64;
            summary.project_missing_backing_inputs += inputs
                .iter()
                .filter(|input| input.presence == ProjectInputPresence::Missing)
                .count() as u64;
        }
    }
    Ok(summary)
}

fn expected_summary() -> ExpansionSummary {
    ExpansionSummary {
        corpus_files: EXPECTED_CORPUS_FILES,
        corpus_bytes: EXPECTED_CORPUS_BYTES,
        compiler_sources: EXPECTED_COMPILER_SOURCES,
        compiler_default_fixtures: EXPECTED_COMPILER_DEFAULT_FIXTURES,
        compiler_matrix_fixtures: EXPECTED_COMPILER_MATRIX_FIXTURES,
        compiler_cases: EXPECTED_COMPILER_CASES,
        compiler_normal_units: EXPECTED_COMPILER_NORMAL_UNITS,
        compiler_virtual_configs: EXPECTED_COMPILER_VIRTUAL_CONFIGS,
        compiler_present_empty_units: EXPECTED_COMPILER_PRESENT_EMPTY_UNITS,
        compiler_missing_content_units: EXPECTED_COMPILER_MISSING_CONTENT_UNITS,
        compiler_link_directives: EXPECTED_COMPILER_LINK_DIRECTIVES,
        compiler_document_symlink_directives: EXPECTED_COMPILER_DOCUMENT_SYMLINK_DIRECTIVES,
        compiler_document_symlink_paths: EXPECTED_COMPILER_DOCUMENT_SYMLINK_PATHS,
        project_descriptors: EXPECTED_PROJECT_DESCRIPTORS,
        project_backing_files: EXPECTED_PROJECT_BACKING_FILES,
        project_cases: EXPECTED_PROJECT_CASES,
        project_declared_inputs: EXPECTED_PROJECT_DECLARED_INPUTS,
        project_missing_backing_inputs: EXPECTED_PROJECT_MISSING_BACKING_INPUTS,
        total_cases: EXPECTED_TOTAL_CASES,
        not_run_cases: EXPECTED_TOTAL_CASES,
    }
}

pub fn check_recorded_manifest(workspace: &Path) -> HarnessResult<ExpansionSummary> {
    let (parsed, recorded) = read_recorded_manifest(workspace)?;
    let summary = parsed.summary.clone();
    let generated = generate_manifest(workspace)?;
    let canonical_generated = render_manifest(&generated)?;
    if recorded != canonical_generated {
        return Err(error(format!(
            "recorded upstream suite manifest {} is stale; regenerate it with cargo xtask upstream-suites manifest --write",
            workspace.join(MANIFEST_RELATIVE_PATH).display()
        )));
    }
    Ok(summary)
}

fn read_recorded_manifest(workspace: &Path) -> HarnessResult<(ExpansionManifest, Vec<u8>)> {
    let path = workspace.join(MANIFEST_RELATIVE_PATH);
    let recorded = fs::read(&path).map_err(|source| {
        error(format!(
            "failed to read recorded upstream suite manifest {}: {source}",
            path.display()
        ))
    })?;
    let parsed: ExpansionManifest = serde_json::from_slice(&recorded).map_err(|source| {
        error(format!(
            "recorded upstream suite manifest {} is invalid JSON: {source}",
            path.display()
        ))
    })?;
    let summary = validate_manifest(&parsed)?;
    let canonical_recorded = render_manifest(&parsed)?;
    if recorded != canonical_recorded {
        return Err(error(format!(
            "recorded upstream suite manifest {} is not canonical JSON",
            path.display()
        )));
    }
    debug_assert_eq!(summary, parsed.summary);
    Ok((parsed, recorded))
}

fn read_and_validate_pin(workspace: &Path) -> HarnessResult<(TestSuitesPin, CorpusPinIdentity)> {
    let path = workspace.join(CORPUS_PIN_RELATIVE_PATH);
    let bytes = fs::read(&path).map_err(|source| {
        error(format!(
            "failed to read upstream suite pin {}: {source}",
            path.display()
        ))
    })?;
    let actual_sha256 = sha256_hex(&bytes);
    if actual_sha256 != CORPUS_PIN_SHA256 {
        return Err(error(format!(
            "upstream suite pin {} has SHA-256 {actual_sha256}, expected immutable anchor {CORPUS_PIN_SHA256}",
            path.display()
        )));
    }
    let pin: TestSuitesPin = serde_json::from_slice(&bytes).map_err(|source| {
        error(format!(
            "upstream suite pin {} is invalid JSON: {source}",
            path.display()
        ))
    })?;
    if pin.schema != 1
        || pin.typescript_version != TYPESCRIPT_VERSION
        || pin.source_repository != SOURCE_REPOSITORY
        || pin.source_commit != SOURCE_COMMIT
    {
        return Err(error(
            "upstream suite pin metadata does not match TypeScript 6.0.3",
        ));
    }
    if pin.suites.len() != EXPECTED_SUITES.len() {
        return Err(error(format!(
            "upstream suite pin has {} suites, expected {}",
            pin.suites.len(),
            EXPECTED_SUITES.len()
        )));
    }

    let mut suites = Vec::with_capacity(pin.suites.len());
    for (suite, (name, source_path, vendored_path)) in pin.suites.iter().zip(EXPECTED_SUITES) {
        if suite.name != name
            || suite.source_path != source_path
            || suite.vendored_path != vendored_path
        {
            return Err(error(format!(
                "upstream suite pin entry {:?} does not match expected suite {name:?}",
                suite.name
            )));
        }
        validate_lower_hex(&suite.git_tree_sha1, 40, "Git tree SHA-1")?;
        validate_lower_hex(&suite.blob_inventory_sha256, 64, "blob inventory SHA-256")?;
        let executable_paths = validated_executable_paths(&suite.executable_paths)?;
        suites.push(CorpusSuiteIdentity {
            name: SuiteName::parse(&suite.name)?,
            source_path: suite.source_path.clone(),
            vendored_path: suite.vendored_path.clone(),
            git_tree_sha1: suite.git_tree_sha1.clone(),
            blob_inventory_sha256: suite.blob_inventory_sha256.clone(),
            files: suite.files,
            bytes: suite.bytes,
            unique_blobs: suite.unique_blobs,
            executable_paths: executable_paths.into_iter().collect(),
        });
    }
    Ok((
        pin,
        CorpusPinIdentity {
            path: CORPUS_PIN_RELATIVE_PATH.to_owned(),
            sha256: CORPUS_PIN_SHA256.to_owned(),
            suites,
        },
    ))
}

fn collect_corpus(workspace: &Path, pin: &TestSuitesPin) -> HarnessResult<Vec<CollectedSource>> {
    let mut collected = Vec::with_capacity(EXPECTED_CORPUS_FILES as usize);
    for suite in &pin.suites {
        let suite_name = SuiteName::parse(&suite.name)?;
        let root = workspace.join(&suite.vendored_path);
        let paths = collect_suite_paths(&root)?;
        let executable_paths = validated_executable_paths(&suite.executable_paths)?;
        let path_set = paths.iter().cloned().collect::<BTreeSet<_>>();
        for executable in &executable_paths {
            if !path_set.contains(executable) {
                return Err(error(format!(
                    "suite {} pins missing executable path {executable:?}",
                    suite.name
                )));
            }
        }

        let mut suite_bytes = 0_u64;
        let mut blob_ids = BTreeSet::new();
        let mut git_inventory = Vec::new();
        for relative in paths {
            let absolute = root.join(path_from_posix(&relative)?);
            let raw = fs::read(&absolute).map_err(|source| {
                error(format!(
                    "failed to read corpus file {}: {source}",
                    absolute.display()
                ))
            })?;
            let bytes = u64::try_from(raw.len())
                .map_err(|_| error(format!("corpus file {} is too large", absolute.display())))?;
            suite_bytes = suite_bytes
                .checked_add(bytes)
                .ok_or_else(|| error("upstream suite byte count overflow"))?;
            let mode = if executable_paths.contains(&relative) {
                "100755"
            } else {
                "100644"
            };
            let git_blob_sha1 = git_blob_sha1(&raw);
            blob_ids.insert(git_blob_sha1.clone());
            git_inventory.extend_from_slice(mode.as_bytes());
            git_inventory.extend_from_slice(b" blob ");
            git_inventory.extend_from_slice(git_blob_sha1.as_bytes());
            git_inventory.push(b' ');
            git_inventory.extend_from_slice(bytes.to_string().as_bytes());
            git_inventory.push(b'\t');
            git_inventory.extend_from_slice(relative.as_bytes());
            git_inventory.push(0);
            collected.push(CollectedSource {
                entry: SourceInventoryEntry {
                    suite: suite_name,
                    path: relative,
                    mode: mode.to_owned(),
                    bytes,
                    sha256: sha256_hex(&raw),
                    git_blob_sha1,
                },
                raw,
            });
        }
        let suite_files = u64::try_from(path_set.len())
            .map_err(|_| error("upstream suite file count overflow"))?;
        let unique_blobs = u64::try_from(blob_ids.len())
            .map_err(|_| error("upstream suite blob count overflow"))?;
        let inventory_sha256 = sha256_hex(&git_inventory);
        if suite_files != suite.files
            || suite_bytes != suite.bytes
            || unique_blobs != suite.unique_blobs
            || inventory_sha256 != suite.blob_inventory_sha256
        {
            return Err(error(format!(
                "suite {} does not match pinned inventory: files={suite_files}/{}, bytes={suite_bytes}/{}, unique_blobs={unique_blobs}/{}, inventory={inventory_sha256}/{}",
                suite.name,
                suite.files,
                suite.bytes,
                suite.unique_blobs,
                suite.blob_inventory_sha256
            )));
        }
    }
    Ok(collected)
}

fn collect_suite_paths(root: &Path) -> HarnessResult<Vec<String>> {
    let metadata = fs::symlink_metadata(root).map_err(|source| {
        error(format!(
            "failed to inspect suite root {}: {source}",
            root.display()
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(error(format!(
            "upstream suite root {} must be a real directory",
            root.display()
        )));
    }
    let mut paths = Vec::new();
    visit_suite_directory(root, root, &mut paths)?;
    paths.sort_unstable();
    let unique = paths.iter().collect::<BTreeSet<_>>();
    if unique.len() != paths.len() {
        return Err(error(format!(
            "suite {} contains duplicate normalized paths",
            root.display()
        )));
    }
    Ok(paths)
}

fn visit_suite_directory(
    root: &Path,
    directory: &Path,
    paths: &mut Vec<String>,
) -> HarnessResult<()> {
    let entries = fs::read_dir(directory)
        .map_err(|source| error(format!("failed to read {}: {source}", directory.display())))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| {
            error(format!(
                "failed to enumerate {}: {source}",
                directory.display()
            ))
        })?;
    if entries.is_empty() {
        return Err(error(format!(
            "upstream suite contains empty directory {}",
            directory.display()
        )));
    }
    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|source| error(format!("failed to inspect {}: {source}", path.display())))?;
        if file_type.is_symlink() {
            return Err(error(format!(
                "upstream suite contains symlink {}",
                path.display()
            )));
        }
        if file_type.is_dir() {
            visit_suite_directory(root, &path, paths)?;
        } else if file_type.is_file() {
            paths.push(relative_posix_path(root, &path)?);
        } else {
            return Err(error(format!(
                "upstream suite contains unsupported entry {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn relative_posix_path(root: &Path, path: &Path) -> HarnessResult<String> {
    let relative = path.strip_prefix(root).map_err(|_| {
        error(format!(
            "suite entry {} is outside {}",
            path.display(),
            root.display()
        ))
    })?;
    let mut parts = Vec::new();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(error(format!(
                "suite entry {} has a non-normal path component",
                path.display()
            )));
        };
        let component = component
            .to_str()
            .ok_or_else(|| error(format!("suite entry {} is not UTF-8", path.display())))?;
        if component.is_empty()
            || component.contains('/')
            || component.contains('\\')
            || component.contains('\0')
        {
            return Err(error(format!(
                "suite entry {} cannot be represented canonically",
                path.display()
            )));
        }
        parts.push(component);
    }
    if parts.is_empty() {
        return Err(error("suite file path must not be empty"));
    }
    Ok(parts.join("/"))
}

fn path_from_posix(path: &str) -> HarnessResult<PathBuf> {
    validate_relative_posix_path(path)?;
    Ok(path.split('/').collect())
}

fn validate_relative_posix_path(path: &str) -> HarnessResult<()> {
    if path.is_empty()
        || path.starts_with('/')
        || path.ends_with('/')
        || path.contains('\\')
        || path.contains('\0')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(error(format!("non-canonical relative POSIX path {path:?}")));
    }
    Ok(())
}

fn validated_executable_paths(paths: &[String]) -> HarnessResult<BTreeSet<String>> {
    let mut result = BTreeSet::new();
    for path in paths {
        validate_relative_posix_path(path)?;
        if !result.insert(path.clone()) {
            return Err(error(format!("duplicate executable path {path:?}")));
        }
    }
    Ok(result)
}

fn expand_project_fixture(
    source: u32,
    fixture_path: &str,
    raw: &[u8],
    source_lookup: &BTreeMap<(SuiteName, String), u32>,
) -> HarnessResult<ProjectFixtureExpansion> {
    let (encoding, text) = decode_source(raw);
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|source_error| {
        error(format!(
            "project descriptor {fixture_path:?} is invalid JSON: {source_error}"
        ))
    })?;
    let object = value.as_object().ok_or_else(|| {
        error(format!(
            "project descriptor {fixture_path:?} must contain a JSON object"
        ))
    })?;
    let scenario = required_json_string(object, "scenario", fixture_path)?.to_owned();
    let project_root = required_json_string(object, "projectRoot", fixture_path)?.to_owned();
    let input_files = match object.get("inputFiles") {
        None => ProjectInputFiles::Absent,
        Some(value) => {
            let values = value.as_array().ok_or_else(|| {
                error(format!(
                    "project descriptor {fixture_path:?} field inputFiles must be an array"
                ))
            })?;
            let mut inputs = Vec::with_capacity(values.len());
            for value in values {
                let input = value.as_str().ok_or_else(|| {
                    error(format!(
                        "project descriptor {fixture_path:?} inputFiles entries must be strings"
                    ))
                })?;
                let resolved_backing_path = resolve_project_input(&project_root, input)?;
                let presence = source_lookup
                    .get(&(SuiteName::Projects, resolved_backing_path.clone()))
                    .copied()
                    .map(|source| ProjectInputPresence::Present { source })
                    .unwrap_or(ProjectInputPresence::Missing);
                inputs.push(ProjectInput {
                    path: input.to_owned(),
                    resolved_backing_path,
                    presence,
                });
            }
            ProjectInputFiles::Present { inputs }
        }
    };
    Ok(ProjectFixtureExpansion {
        source,
        encoding,
        scenario,
        project_root,
        input_files,
    })
}

fn required_json_string<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    field: &str,
    fixture_path: &str,
) -> HarnessResult<&'a str> {
    object
        .get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            error(format!(
                "project descriptor {fixture_path:?} field {field:?} must be a string"
            ))
        })
}

fn resolve_project_input(project_root: &str, input: &str) -> HarnessResult<String> {
    const PREFIX: &str = "tests/cases/projects";
    let root = project_root.strip_prefix(PREFIX).ok_or_else(|| {
        error(format!(
            "projectRoot {project_root:?} is outside TypeScript's projects backing suite"
        ))
    })?;
    if !root.is_empty() && !root.starts_with('/') {
        return Err(error(format!(
            "projectRoot {project_root:?} only partially matches {PREFIX:?}"
        )));
    }
    if input.starts_with('/') || input.contains('\\') || input.contains('\0') {
        return Err(error(format!("non-relative project input path {input:?}")));
    }
    let mut parts = Vec::new();
    for part in root.trim_matches('/').split('/').chain(input.split('/')) {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(error(format!(
                        "project input {input:?} escapes projects backing suite"
                    )));
                }
            }
            part => parts.push(part),
        }
    }
    if parts.is_empty() {
        return Err(error(format!(
            "project input {input:?} resolves to a directory root"
        )));
    }
    let path = parts.join("/");
    validate_relative_posix_path(&path)?;
    Ok(path)
}

pub(super) fn decode_source(raw: &[u8]) -> (SourceEncoding, String) {
    if let Some(raw) = raw.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        return (
            SourceEncoding::Utf8Bom,
            String::from_utf8_lossy(raw).into_owned(),
        );
    }
    if let Some(raw) = raw.strip_prefix(&[0xff, 0xfe]) {
        return (SourceEncoding::Utf16Le, decode_utf16(raw, false));
    }
    if let Some(raw) = raw.strip_prefix(&[0xfe, 0xff]) {
        return (SourceEncoding::Utf16Be, decode_utf16(raw, true));
    }
    (
        SourceEncoding::Utf8,
        String::from_utf8_lossy(raw).into_owned(),
    )
}

fn decode_utf16(raw: &[u8], big_endian: bool) -> String {
    let code_units = raw.chunks_exact(2).map(|pair| {
        if big_endian {
            u16::from_be_bytes([pair[0], pair[1]])
        } else {
            u16::from_le_bytes([pair[0], pair[1]])
        }
    });
    char::decode_utf16(code_units)
        .map(|result| result.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

fn is_compiler_fixture_path(path: &str) -> bool {
    path.ends_with(".ts") || path.ends_with(".tsx")
}

fn case_id(suite: SuiteName, fixture_path: &str, variant: &str) -> String {
    format!(
        "typescript-{TYPESCRIPT_VERSION}/{}/{}#{}",
        suite.as_str(),
        percent_encode(fixture_path, true),
        percent_encode(variant, false)
    )
}

fn percent_encode(value: &str, preserve_slash: bool) -> String {
    let mut result = String::with_capacity(value.len());
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'.' | b'_' | b'~')
            || preserve_slash && byte == b'/'
        {
            result.push(char::from(byte));
        } else {
            result.push('%');
            result.push(char::from(HEX[usize::from(byte >> 4)]));
            result.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    result
}

fn source_index(index: usize) -> HarnessResult<u32> {
    source_index_from_usize(index, "source")
}

fn source_index_from_usize(index: usize, description: &str) -> HarnessResult<u32> {
    u32::try_from(index).map_err(|_| error(format!("{description} index {index} exceeds u32")))
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn source_inventory_sha256(sources: &[SourceInventoryEntry]) -> String {
    let mut digest = Sha256::new();
    for source in sources {
        hash_inventory_field(&mut digest, source.suite.as_str().as_bytes());
        hash_inventory_field(&mut digest, source.path.as_bytes());
        hash_inventory_field(&mut digest, source.mode.as_bytes());
        digest.update(source.bytes.to_be_bytes());
        hash_inventory_field(&mut digest, source.sha256.as_bytes());
        hash_inventory_field(&mut digest, source.git_blob_sha1.as_bytes());
    }
    format!("{:x}", digest.finalize())
}

fn hash_inventory_field(digest: &mut Sha256, bytes: &[u8]) {
    digest.update((bytes.len() as u64).to_be_bytes());
    digest.update(bytes);
}

fn git_blob_sha1(raw: &[u8]) -> String {
    let header = format!("blob {}\0", raw.len());
    let mut object = Vec::with_capacity(header.len() + raw.len());
    object.extend_from_slice(header.as_bytes());
    object.extend_from_slice(raw);
    sha1_hex(&object)
}

// Git's corpus identity is SHA-1 by definition. Keeping this small one-shot
// implementation private avoids a production dependency solely for legacy Git
// object IDs; integrity of the checked-in artifact itself uses SHA-256.
fn sha1_hex(bytes: &[u8]) -> String {
    let bit_length = (bytes.len() as u64).wrapping_mul(8);
    let mut padded = Vec::with_capacity((bytes.len() + 72) & !63);
    padded.extend_from_slice(bytes);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_length.to_be_bytes());

    let mut state = [
        0x6745_2301_u32,
        0xefcd_ab89,
        0x98ba_dcfe,
        0x1032_5476,
        0xc3d2_e1f0,
    ];
    for chunk in padded.chunks_exact(64) {
        let mut words = [0_u32; 80];
        for (index, word) in words[..16].iter_mut().enumerate() {
            let offset = index * 4;
            *word = u32::from_be_bytes([
                chunk[offset],
                chunk[offset + 1],
                chunk[offset + 2],
                chunk[offset + 3],
            ]);
        }
        for index in 16..80 {
            words[index] =
                (words[index - 3] ^ words[index - 8] ^ words[index - 14] ^ words[index - 16])
                    .rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = state;
        for (index, word) in words.iter().enumerate() {
            let (function, constant) = match index {
                0..=19 => ((b & c) | ((!b) & d), 0x5a82_7999),
                20..=39 => (b ^ c ^ d, 0x6ed9_eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1b_bcdc),
                _ => (b ^ c ^ d, 0xca62_c1d6),
            };
            let temporary = a
                .rotate_left(5)
                .wrapping_add(function)
                .wrapping_add(e)
                .wrapping_add(constant)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temporary;
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
    }
    state.iter().map(|word| format!("{word:08x}")).collect()
}

fn validate_lower_hex(value: &str, length: usize, description: &str) -> HarnessResult<()> {
    if value.len() != length
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(error(format!(
            "{description} must be {length} lowercase hexadecimal characters, got {value:?}"
        )));
    }
    Ok(())
}

fn error(message: impl Into<String>) -> HarnessError {
    HarnessError::new(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha1_matches_standard_and_git_blob_vectors() {
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            git_blob_sha1(b""),
            "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"
        );
    }

    #[test]
    fn test_decode_source_matches_typescript_bom_dispatch() {
        assert_eq!(
            decode_source(b"\xef\xbb\xbfhello"),
            (SourceEncoding::Utf8Bom, "hello".to_owned())
        );
        assert_eq!(
            decode_source(b"\xff\xfeh\0i\0x"),
            (SourceEncoding::Utf16Le, "hi".to_owned())
        );
        assert_eq!(
            decode_source(b"\xfe\xff\0h\0ix"),
            (SourceEncoding::Utf16Be, "hi".to_owned())
        );
        assert_eq!(
            decode_source(b"a\xffb"),
            (SourceEncoding::Utf8, "a\u{fffd}b".to_owned())
        );
    }

    #[test]
    fn test_project_input_resolution_is_lexical_and_confined() {
        assert_eq!(
            resolve_project_input(
                "tests/cases/projects/ReferenceResolution/src/ts/foo",
                "../../../bar/bar.ts"
            )
            .unwrap(),
            "ReferenceResolution/bar/bar.ts"
        );
        assert!(resolve_project_input("tests/cases/projects/a", "../../escape.ts").is_err());
    }
}

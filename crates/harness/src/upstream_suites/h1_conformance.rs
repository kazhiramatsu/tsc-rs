//! Exact, inventory-only expansion of TypeScript 6.0.3's conformance
//! `CompilerBaselineRunner` universe.
//!
//! This module deliberately stops before config parsing, program creation,
//! diagnostics, emit, and reference-baseline comparison. Every expanded case
//! and every runner observation starts as [`ExecutionState::NotRun`].

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::HarnessResult;

use super::{
    compiler, error, git_blob_sha1, hash_inventory_field, path_from_posix, source_index_from_usize,
    validate_compiler_configuration, validate_compiler_unit, validate_lower_hex,
    validate_ordered_settings, validate_relative_posix_path, CompilerFixtureExpansion,
    ExecutionState, ImplementationSourcePin, UnitContent, MAX_COMPILER_VARIATIONS, SOURCE_COMMIT,
    SOURCE_REPOSITORY, TYPESCRIPT_VERSION,
};

pub const SCHEMA: u32 = 1;
pub const STATUS: &str = "expanded-not-run";
pub const PHASE: &str = "H1.0a-conformance-runner-expansion";
pub const MANIFEST_RELATIVE_PATH: &str =
    "vendor/typescript-6.0.3/conformance-suite-expansion.v1.json";
pub const CONTRACT_RELATIVE_PATH: &str =
    ".github/ci/contracts/h1-conformance-expansion.schema.json";
pub const INDEPENDENT_ORACLE_RELATIVE_PATH: &str = "crates/oracle/h1-conformance-expansion.mjs";
pub const CORPUS_PIN_RELATIVE_PATH: &str = "vendor/typescript-6.0.3/test-suites-pin.v4.json";
pub const CORPUS_PIN_SHA256: &str =
    "9cd0b499d22c8936b78d1bd30d5ab7faa295b23903e838953fddaaffc48d52d4";
pub const TYPESCRIPT_BUNDLE_RELATIVE_PATH: &str = "vendor/typescript-6.0.3/lib/typescript.js";
pub const TYPESCRIPT_BUNDLE_SHA256: &str =
    "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39";

const SUITE_SOURCE_PATH: &str = "tests/cases/conformance";
const SUITE_VENDORED_PATH: &str = "ts-tests/tests/cases/conformance";
const SUITE_GIT_TREE_SHA1: &str = "9d28e54f5b0c7695ca2de6b1a15508dc35b0db98";
const SUITE_BLOB_INVENTORY_SHA256: &str =
    "73c064a14ee9f09ffd60e5d4318285e7cec3fa860a5dab1235cadb42bc8dd72f";
const SUITE_FILES: u64 = 5_908;
const SUITE_BYTES: u64 = 3_825_804;
const SUITE_UNIQUE_BLOBS: u64 = 5_862;
const NOT_ENUMERATED_JS_PATH: &str =
    "parser/ecmascript5/Statements/ReturnStatements/parserReturnStatement4.js";
const OBSERVATION_INDEXES: [u8; 6] = [0, 1, 2, 3, 4, 5];

const EXPECTED_SOURCE_INVENTORY_SHA256: &str =
    "8dd4be94d28c32e953c5931daed512f6f1e4bca13eb0edf550c71b1db4a8c598";

const IMPLEMENTATION_SOURCES: [(&str, &str); 3] = [
    (
        "src/testRunner/compilerRunner.ts",
        "aed00f47656b316f3f20c913e2408a128d4671cb",
    ),
    (
        "src/harness/harnessIO.ts",
        "a06bde1c95182ea1bfad0ddf9af73053501a6dc7",
    ),
    (
        "src/harness/harnessUtils.ts",
        "f768325897167ad793eeff9ced7763e12f9aa154",
    ),
];

const PRODUCER_SOURCE_PATHS: [&str; 3] = [
    "crates/harness/src/upstream_suites.rs",
    "crates/harness/src/upstream_suites/compiler.rs",
    "crates/harness/src/upstream_suites/h1_conformance.rs",
];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConformanceExpansionManifest {
    pub schema: u32,
    pub status: String,
    pub phase: String,
    pub typescript: TypeScriptIdentity,
    pub producer_sources: Vec<PathHash>,
    pub independent_oracle: PathHash,
    pub contract: PathHash,
    pub inputs: ConformanceInputs,
    pub runner_contract: RunnerContract,
    pub source_inventory_sha256: String,
    pub sources: Vec<ConformanceSource>,
    pub not_enumerated_sources: Vec<NotEnumeratedSource>,
    pub fixtures: Vec<CompilerFixtureExpansion>,
    pub cases: Vec<ConformanceCase>,
    pub summary: ConformanceExpansionSummary,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TypeScriptIdentity {
    pub version: String,
    pub source_repository: String,
    pub source_commit: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PathHash {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConformanceInputs {
    pub suite_pin: PathHash,
    pub conformance_suite: ConformanceSuiteIdentity,
    pub typescript_bundle: PathHash,
    pub implementation_sources: Vec<ImplementationSourcePin>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConformanceSuiteIdentity {
    pub name: String,
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
pub struct RunnerContract {
    pub enumeration: String,
    pub emit_enabled: bool,
    pub vary_by: Vec<String>,
    pub variation_limit: u64,
    pub configuration_order: String,
    pub unit_partition: String,
    pub observations: Vec<RunnerObservation>,
    pub reference_baseline_state: ReferenceBaselineState,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerObservation {
    pub name: String,
    pub upstream_method: String,
    pub invocation_gate: String,
    pub initial_execution_state: ExecutionState,
    pub reference_baseline_state: ReferenceBaselineState,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ReferenceBaselineState {
    #[serde(rename = "content-not-vendored-or-compared")]
    ContentNotVendoredOrCompared,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConformanceSource {
    pub path: String,
    pub mode: String,
    pub bytes: u64,
    pub sha256: String,
    pub git_blob_sha1: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NotEnumeratedSource {
    pub source: u32,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConformanceCase {
    pub id: String,
    pub source: u32,
    pub configuration: u32,
    pub observations: Vec<u8>,
    pub initial_execution_state: ExecutionState,
    pub reference_baseline_state: ReferenceBaselineState,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConformanceExpansionSummary {
    pub source_files: u64,
    pub source_bytes: u64,
    pub unique_blobs: u64,
    pub enumerated_fixtures: u64,
    pub not_enumerated_sources: u64,
    pub default_fixtures: u64,
    pub matrix_fixtures: u64,
    pub cases: u64,
    pub normal_units: u64,
    pub virtual_configs: u64,
    pub present_empty_units: u64,
    pub missing_content_units: u64,
    pub link_directives: u64,
    pub document_symlink_directives: u64,
    pub document_symlink_paths: u64,
    pub runner_observations: u64,
    pub case_observations: u64,
    pub not_run_cases: u64,
    pub not_run_case_observations: u64,
    pub execution_results_recorded: u64,
    pub reference_baselines_compared: u64,
}

#[derive(Debug, Deserialize)]
struct TestSuitesPin {
    schema: u32,
    typescript_version: String,
    source_repository: String,
    source_commit: String,
    suites: Vec<ConformanceSuiteIdentity>,
}

struct CollectedSource {
    entry: ConformanceSource,
    raw: Vec<u8>,
}

pub fn generate_manifest(workspace: &Path) -> HarnessResult<ConformanceExpansionManifest> {
    let conformance_suite = read_and_validate_pin(workspace)?;
    let collected = collect_sources(workspace, &conformance_suite)?;
    let sources = collected
        .iter()
        .map(|source| source.entry.clone())
        .collect::<Vec<_>>();
    let source_inventory_sha256 = source_inventory_sha256(&sources);

    let mut not_enumerated_sources = Vec::new();
    let mut fixtures = Vec::with_capacity((SUITE_FILES - 1) as usize);
    let mut cases = Vec::new();
    for (index, source) in collected.iter().enumerate() {
        let source_index = source_index_from_usize(index, "conformance source")?;
        if !is_runner_fixture(&source.entry.path) {
            not_enumerated_sources.push(NotEnumeratedSource {
                source: source_index,
                reason: "extension-does-not-match-/\\.tsx?$/".to_owned(),
            });
            continue;
        }
        let fixture =
            compiler::expand_compiler_fixture(source_index, &source.entry.path, &source.raw)?;
        for (configuration, expanded) in fixture.configurations.iter().enumerate() {
            cases.push(ConformanceCase {
                id: case_id(&source.entry.path, &expanded.variant),
                source: source_index,
                configuration: source_index_from_usize(configuration, "conformance configuration")?,
                observations: OBSERVATION_INDEXES.to_vec(),
                initial_execution_state: ExecutionState::NotRun,
                reference_baseline_state: ReferenceBaselineState::ContentNotVendoredOrCompared,
            });
        }
        fixtures.push(fixture);
    }

    let mut manifest = ConformanceExpansionManifest {
        schema: SCHEMA,
        status: STATUS.to_owned(),
        phase: PHASE.to_owned(),
        typescript: TypeScriptIdentity {
            version: TYPESCRIPT_VERSION.to_owned(),
            source_repository: SOURCE_REPOSITORY.to_owned(),
            source_commit: SOURCE_COMMIT.to_owned(),
        },
        producer_sources: PRODUCER_SOURCE_PATHS
            .iter()
            .map(|path| path_hash(workspace, path))
            .collect::<HarnessResult<_>>()?,
        independent_oracle: path_hash(workspace, INDEPENDENT_ORACLE_RELATIVE_PATH)?,
        contract: path_hash(workspace, CONTRACT_RELATIVE_PATH)?,
        inputs: ConformanceInputs {
            suite_pin: PathHash {
                path: CORPUS_PIN_RELATIVE_PATH.to_owned(),
                sha256: CORPUS_PIN_SHA256.to_owned(),
            },
            conformance_suite,
            typescript_bundle: checked_path_hash(
                workspace,
                TYPESCRIPT_BUNDLE_RELATIVE_PATH,
                TYPESCRIPT_BUNDLE_SHA256,
            )?,
            implementation_sources: IMPLEMENTATION_SOURCES
                .iter()
                .map(|(source_path, git_blob_sha1)| ImplementationSourcePin {
                    source_path: (*source_path).to_owned(),
                    git_blob_sha1: (*git_blob_sha1).to_owned(),
                })
                .collect(),
        },
        runner_contract: expected_runner_contract(),
        source_inventory_sha256,
        sources,
        not_enumerated_sources,
        fixtures,
        cases,
        summary: ConformanceExpansionSummary::default(),
    };
    manifest.summary = derive_summary(&manifest)?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

pub fn render_manifest(manifest: &ConformanceExpansionManifest) -> HarnessResult<Vec<u8>> {
    validate_manifest(manifest)?;
    let mut rendered = serde_json::to_vec_pretty(manifest).map_err(|source| {
        error(format!(
            "failed to serialize H1 conformance expansion manifest: {source}"
        ))
    })?;
    rendered.push(b'\n');
    Ok(rendered)
}

pub fn validate_manifest(
    manifest: &ConformanceExpansionManifest,
) -> HarnessResult<ConformanceExpansionSummary> {
    if manifest.schema != SCHEMA
        || manifest.status != STATUS
        || manifest.phase != PHASE
        || manifest.typescript.version != TYPESCRIPT_VERSION
        || manifest.typescript.source_repository != SOURCE_REPOSITORY
        || manifest.typescript.source_commit != SOURCE_COMMIT
    {
        return Err(error(
            "H1 conformance expansion metadata does not match TypeScript 6.0.3",
        ));
    }
    validate_path_hashes(manifest)?;
    validate_inputs(&manifest.inputs)?;
    if manifest.runner_contract != expected_runner_contract() {
        return Err(error("H1 conformance runner contract changed"));
    }
    validate_sources(manifest)?;
    validate_fixtures_and_cases(manifest)?;

    let derived = derive_summary(manifest)?;
    if manifest.summary != derived {
        return Err(error(format!(
            "H1 conformance expansion summary is stale: recorded={:?}, derived={derived:?}",
            manifest.summary
        )));
    }
    let expected = expected_summary();
    if derived != expected {
        return Err(error(format!(
            "TypeScript 6.0.3 conformance expansion changed: actual={derived:?}, expected={expected:?}"
        )));
    }
    Ok(derived)
}

pub fn check_recorded_manifest(workspace: &Path) -> HarnessResult<ConformanceExpansionSummary> {
    let (parsed, recorded) = read_recorded_manifest(workspace)?;
    let generated = generate_manifest(workspace)?;
    let canonical_generated = render_manifest(&generated)?;
    if recorded != canonical_generated {
        return Err(error(format!(
            "recorded H1 conformance expansion {} is stale; regenerate it with cargo xtask h1-conformance manifest --write",
            workspace.join(MANIFEST_RELATIVE_PATH).display()
        )));
    }
    Ok(parsed.summary)
}

fn read_recorded_manifest(
    workspace: &Path,
) -> HarnessResult<(ConformanceExpansionManifest, Vec<u8>)> {
    let path = workspace.join(MANIFEST_RELATIVE_PATH);
    let recorded = fs::read(&path).map_err(|source| {
        error(format!(
            "failed to read recorded H1 conformance expansion {}: {source}",
            path.display()
        ))
    })?;
    let parsed: ConformanceExpansionManifest =
        serde_json::from_slice(&recorded).map_err(|source| {
            error(format!(
                "recorded H1 conformance expansion {} is invalid JSON: {source}",
                path.display()
            ))
        })?;
    validate_manifest(&parsed)?;
    if recorded != render_manifest(&parsed)? {
        return Err(error(format!(
            "recorded H1 conformance expansion {} is not canonical JSON",
            path.display()
        )));
    }
    Ok((parsed, recorded))
}

fn read_and_validate_pin(workspace: &Path) -> HarnessResult<ConformanceSuiteIdentity> {
    let path = workspace.join(CORPUS_PIN_RELATIVE_PATH);
    let bytes = fs::read(&path).map_err(|source| {
        error(format!(
            "failed to read H1 conformance suite pin {}: {source}",
            path.display()
        ))
    })?;
    let actual = super::sha256_hex(&bytes);
    if actual != CORPUS_PIN_SHA256 {
        return Err(error(format!(
            "H1 conformance suite pin has SHA-256 {actual}, expected {CORPUS_PIN_SHA256}"
        )));
    }
    let pin: TestSuitesPin = serde_json::from_slice(&bytes).map_err(|source| {
        error(format!(
            "H1 conformance suite pin is invalid JSON: {source}"
        ))
    })?;
    if pin.schema != 4
        || pin.typescript_version != TYPESCRIPT_VERSION
        || pin.source_repository != SOURCE_REPOSITORY
        || pin.source_commit != SOURCE_COMMIT
    {
        return Err(error("H1 conformance suite pin metadata changed"));
    }
    let suites = pin
        .suites
        .into_iter()
        .filter(|suite| suite.name == "conformance")
        .collect::<Vec<_>>();
    if suites.len() != 1 || suites[0] != expected_suite_identity() {
        return Err(error(
            "H1 conformance suite pin does not contain the exact frozen suite identity",
        ));
    }
    Ok(suites.into_iter().next().expect("one suite checked"))
}

fn collect_sources(
    workspace: &Path,
    suite: &ConformanceSuiteIdentity,
) -> HarnessResult<Vec<CollectedSource>> {
    let root = workspace.join(SUITE_VENDORED_PATH);
    let paths = super::collect_suite_paths(&root)?;
    let mut collected = Vec::with_capacity(paths.len());
    let mut bytes_total = 0_u64;
    let mut blobs = BTreeSet::new();
    let mut blob_inventory = Vec::new();

    for relative in paths {
        let absolute = root.join(path_from_posix(&relative)?);
        let raw = fs::read(&absolute).map_err(|source| {
            error(format!(
                "failed to read H1 conformance source {}: {source}",
                absolute.display()
            ))
        })?;
        let bytes = u64::try_from(raw.len())
            .map_err(|_| error(format!("conformance source {relative:?} is too large")))?;
        bytes_total = bytes_total
            .checked_add(bytes)
            .ok_or_else(|| error("H1 conformance source byte count overflow"))?;
        let git_blob_sha1 = git_blob_sha1(&raw);
        blobs.insert(git_blob_sha1.clone());
        blob_inventory.extend_from_slice(b"100644 blob ");
        blob_inventory.extend_from_slice(git_blob_sha1.as_bytes());
        blob_inventory.push(b' ');
        blob_inventory.extend_from_slice(bytes.to_string().as_bytes());
        blob_inventory.push(b'\t');
        blob_inventory.extend_from_slice(relative.as_bytes());
        blob_inventory.push(0);
        collected.push(CollectedSource {
            entry: ConformanceSource {
                path: relative,
                mode: "100644".to_owned(),
                bytes,
                sha256: super::sha256_hex(&raw),
                git_blob_sha1,
            },
            raw,
        });
    }

    if collected.len() as u64 != suite.files
        || bytes_total != suite.bytes
        || blobs.len() as u64 != suite.unique_blobs
        || super::sha256_hex(&blob_inventory) != suite.blob_inventory_sha256
    {
        return Err(error(format!(
            "checked-in H1 conformance suite does not match pin v4: files={}, bytes={bytes_total}, unique_blobs={}, inventory={}",
            collected.len(),
            blobs.len(),
            super::sha256_hex(&blob_inventory)
        )));
    }
    Ok(collected)
}

fn validate_path_hashes(manifest: &ConformanceExpansionManifest) -> HarnessResult<()> {
    if manifest.producer_sources.len() != PRODUCER_SOURCE_PATHS.len() {
        return Err(error("H1 conformance producer source set is incomplete"));
    }
    for (pin, expected_path) in manifest.producer_sources.iter().zip(PRODUCER_SOURCE_PATHS) {
        if pin.path != expected_path {
            return Err(error(format!(
                "unexpected H1 conformance producer source {:?}",
                pin.path
            )));
        }
        validate_lower_hex(&pin.sha256, 64, "producer source SHA-256")?;
    }
    if manifest.independent_oracle.path != INDEPENDENT_ORACLE_RELATIVE_PATH {
        return Err(error("H1 conformance independent oracle path changed"));
    }
    validate_lower_hex(
        &manifest.independent_oracle.sha256,
        64,
        "independent oracle SHA-256",
    )?;
    if manifest.contract.path != CONTRACT_RELATIVE_PATH {
        return Err(error("H1 conformance schema path changed"));
    }
    validate_lower_hex(&manifest.contract.sha256, 64, "contract SHA-256")
}

fn validate_inputs(inputs: &ConformanceInputs) -> HarnessResult<()> {
    if inputs.suite_pin.path != CORPUS_PIN_RELATIVE_PATH
        || inputs.suite_pin.sha256 != CORPUS_PIN_SHA256
        || inputs.conformance_suite != expected_suite_identity()
        || inputs.typescript_bundle.path != TYPESCRIPT_BUNDLE_RELATIVE_PATH
        || inputs.typescript_bundle.sha256 != TYPESCRIPT_BUNDLE_SHA256
    {
        return Err(error("H1 conformance expansion input identity changed"));
    }
    if inputs.implementation_sources.len() != IMPLEMENTATION_SOURCES.len() {
        return Err(error(
            "H1 conformance implementation source set is incomplete",
        ));
    }
    for (pin, (path, blob)) in inputs
        .implementation_sources
        .iter()
        .zip(IMPLEMENTATION_SOURCES)
    {
        if pin.source_path != path || pin.git_blob_sha1 != blob {
            return Err(error(format!(
                "H1 conformance implementation source {:?} changed",
                pin.source_path
            )));
        }
        validate_lower_hex(
            &pin.git_blob_sha1,
            40,
            "implementation source Git blob SHA-1",
        )?;
    }
    Ok(())
}

fn validate_sources(manifest: &ConformanceExpansionManifest) -> HarnessResult<()> {
    let inventory = source_inventory_sha256(&manifest.sources);
    if manifest.source_inventory_sha256 != inventory
        || inventory != EXPECTED_SOURCE_INVENTORY_SHA256
    {
        return Err(error(
            "H1 conformance source inventory does not match the frozen identity",
        ));
    }
    let mut previous = None;
    let mut bytes = 0_u64;
    let mut blobs = BTreeSet::new();
    let mut blob_inventory = Vec::new();
    for source in &manifest.sources {
        validate_relative_posix_path(&source.path)?;
        if previous
            .as_ref()
            .is_some_and(|path: &String| path >= &source.path)
        {
            return Err(error(format!(
                "H1 conformance sources are not strictly ordered at {:?}",
                source.path
            )));
        }
        previous = Some(source.path.clone());
        if source.mode != "100644" {
            return Err(error(format!(
                "H1 conformance source {:?} has unexpected mode {:?}",
                source.path, source.mode
            )));
        }
        validate_lower_hex(&source.sha256, 64, "conformance source SHA-256")?;
        validate_lower_hex(
            &source.git_blob_sha1,
            40,
            "conformance source Git blob SHA-1",
        )?;
        bytes = bytes
            .checked_add(source.bytes)
            .ok_or_else(|| error("H1 conformance source byte count overflow"))?;
        blobs.insert(source.git_blob_sha1.as_str());
        blob_inventory.extend_from_slice(source.mode.as_bytes());
        blob_inventory.extend_from_slice(b" blob ");
        blob_inventory.extend_from_slice(source.git_blob_sha1.as_bytes());
        blob_inventory.push(b' ');
        blob_inventory.extend_from_slice(source.bytes.to_string().as_bytes());
        blob_inventory.push(b'\t');
        blob_inventory.extend_from_slice(source.path.as_bytes());
        blob_inventory.push(0);
    }
    if manifest.sources.len() as u64 != SUITE_FILES
        || bytes != SUITE_BYTES
        || blobs.len() as u64 != SUITE_UNIQUE_BLOBS
        || super::sha256_hex(&blob_inventory) != SUITE_BLOB_INVENTORY_SHA256
    {
        return Err(error(
            "H1 conformance source table does not reconstruct suite pin v4",
        ));
    }
    Ok(())
}

fn validate_fixtures_and_cases(manifest: &ConformanceExpansionManifest) -> HarnessResult<()> {
    if manifest.not_enumerated_sources.len() != 1 {
        return Err(error(
            "H1 conformance runner must leave exactly one pinned source unenumerated",
        ));
    }
    let not_enumerated = &manifest.not_enumerated_sources[0];
    let not_enumerated_source = manifest
        .sources
        .get(not_enumerated.source as usize)
        .ok_or_else(|| error("not-enumerated source index is out of range"))?;
    if not_enumerated_source.path != NOT_ENUMERATED_JS_PATH
        || not_enumerated.reason != "extension-does-not-match-/\\.tsx?$/"
    {
        return Err(error(
            "H1 conformance runner's non-enumerated source control changed",
        ));
    }

    let expected_fixture_sources = manifest
        .sources
        .iter()
        .enumerate()
        .filter(|(_, source)| is_runner_fixture(&source.path))
        .map(|(index, _)| index as u32)
        .collect::<Vec<_>>();
    if manifest
        .fixtures
        .iter()
        .map(|fixture| fixture.source)
        .ne(expected_fixture_sources)
    {
        return Err(error(
            "H1 conformance fixtures do not exactly follow runner enumeration order",
        ));
    }

    let mut case_offset = 0_usize;
    let mut case_ids = BTreeSet::new();
    for fixture in &manifest.fixtures {
        let source = manifest
            .sources
            .get(fixture.source as usize)
            .ok_or_else(|| error("H1 conformance fixture source index is out of range"))?;
        validate_lower_hex(&fixture.decoded_sha256, 64, "decoded fixture SHA-256")?;
        validate_ordered_settings(&fixture.settings, "conformance fixture settings")?;
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
                    "H1 conformance fixture {:?} has an empty @link side",
                    source.path
                )));
            }
        }
        if fixture.configurations.is_empty()
            || fixture.configurations.len() > MAX_COMPILER_VARIATIONS
        {
            return Err(error(format!(
                "H1 conformance fixture {:?} has an invalid configuration count",
                source.path
            )));
        }
        for (configuration_index, configuration) in fixture.configurations.iter().enumerate() {
            validate_compiler_configuration(&source.path, configuration)?;
            let case = manifest.cases.get(case_offset).ok_or_else(|| {
                error(format!(
                    "H1 conformance fixture {:?} is missing a case",
                    source.path
                ))
            })?;
            if case.id != case_id(&source.path, &configuration.variant)
                || case.source != fixture.source
                || case.configuration as usize != configuration_index
                || case.observations != OBSERVATION_INDEXES
                || case.initial_execution_state != ExecutionState::NotRun
                || case.reference_baseline_state
                    != ReferenceBaselineState::ContentNotVendoredOrCompared
            {
                return Err(error(format!(
                    "H1 conformance case at offset {case_offset} does not match fixture {:?}",
                    source.path
                )));
            }
            if !case_ids.insert(case.id.as_str()) {
                return Err(error(format!(
                    "duplicate H1 conformance case ID {:?}",
                    case.id
                )));
            }
            case_offset += 1;
        }
    }
    if case_offset != manifest.cases.len() {
        return Err(error(format!(
            "H1 conformance expansion has {} unreferenced cases",
            manifest.cases.len() - case_offset
        )));
    }
    Ok(())
}

fn derive_summary(
    manifest: &ConformanceExpansionManifest,
) -> HarnessResult<ConformanceExpansionSummary> {
    let mut summary = ConformanceExpansionSummary {
        source_files: manifest.sources.len() as u64,
        source_bytes: manifest.sources.iter().try_fold(0_u64, |total, source| {
            total
                .checked_add(source.bytes)
                .ok_or_else(|| error("H1 conformance source byte count overflow"))
        })?,
        unique_blobs: manifest
            .sources
            .iter()
            .map(|source| source.git_blob_sha1.as_str())
            .collect::<BTreeSet<_>>()
            .len() as u64,
        enumerated_fixtures: manifest.fixtures.len() as u64,
        not_enumerated_sources: manifest.not_enumerated_sources.len() as u64,
        cases: manifest.cases.len() as u64,
        runner_observations: manifest.runner_contract.observations.len() as u64,
        case_observations: manifest.cases.iter().try_fold(0_u64, |total, case| {
            total
                .checked_add(case.observations.len() as u64)
                .ok_or_else(|| error("H1 conformance observation count overflow"))
        })?,
        not_run_cases: manifest
            .cases
            .iter()
            .filter(|case| case.initial_execution_state == ExecutionState::NotRun)
            .count() as u64,
        execution_results_recorded: 0,
        reference_baselines_compared: 0,
        ..ConformanceExpansionSummary::default()
    };
    summary.not_run_case_observations = manifest
        .cases
        .iter()
        .filter(|case| case.initial_execution_state == ExecutionState::NotRun)
        .try_fold(0_u64, |total, case| {
            total
                .checked_add(case.observations.len() as u64)
                .ok_or_else(|| error("H1 conformance not-run observation count overflow"))
        })?;

    for fixture in &manifest.fixtures {
        if fixture.configurations.len() == 1 && fixture.configurations[0].variant == "default" {
            summary.default_fixtures += 1;
        } else {
            summary.matrix_fixtures += 1;
        }
        summary.normal_units = summary
            .normal_units
            .checked_add(fixture.normal_units.len() as u64)
            .ok_or_else(|| error("H1 conformance unit count overflow"))?;
        summary.virtual_configs += u64::from(fixture.virtual_config.is_some());
        for unit in fixture
            .normal_units
            .iter()
            .chain(fixture.virtual_config.iter())
        {
            match unit.content {
                UnitContent::Present { utf8_bytes: 0, .. } => summary.present_empty_units += 1,
                UnitContent::Missing => summary.missing_content_units += 1,
                UnitContent::Present { .. } => {}
            }
            if unit
                .file_options
                .iter()
                .any(|setting| setting.name == "symlink")
            {
                summary.document_symlink_directives += 1;
            }
            summary.document_symlink_paths += unit.document_symlinks.len() as u64;
        }
        summary.link_directives += fixture.links.len() as u64;
    }
    Ok(summary)
}

fn expected_summary() -> ConformanceExpansionSummary {
    ConformanceExpansionSummary {
        source_files: 5_908,
        source_bytes: 3_825_804,
        unique_blobs: 5_862,
        enumerated_fixtures: 5_907,
        not_enumerated_sources: 1,
        default_fixtures: 4_809,
        matrix_fixtures: 1_098,
        cases: 7_697,
        normal_units: 8_055,
        virtual_configs: 27,
        present_empty_units: 14,
        missing_content_units: 0,
        link_directives: 0,
        document_symlink_directives: 0,
        document_symlink_paths: 0,
        runner_observations: 6,
        case_observations: 46_182,
        not_run_cases: 7_697,
        not_run_case_observations: 46_182,
        execution_results_recorded: 0,
        reference_baselines_compared: 0,
    }
}

fn expected_suite_identity() -> ConformanceSuiteIdentity {
    ConformanceSuiteIdentity {
        name: "conformance".to_owned(),
        source_path: SUITE_SOURCE_PATH.to_owned(),
        vendored_path: SUITE_VENDORED_PATH.to_owned(),
        git_tree_sha1: SUITE_GIT_TREE_SHA1.to_owned(),
        blob_inventory_sha256: SUITE_BLOB_INVENTORY_SHA256.to_owned(),
        files: SUITE_FILES,
        bytes: SUITE_BYTES,
        unique_blobs: SUITE_UNIQUE_BLOBS,
        executable_paths: Vec::new(),
    }
}

fn expected_runner_contract() -> RunnerContract {
    let observation =
        |name: &str, upstream_method: &str, invocation_gate: &str| RunnerObservation {
            name: name.to_owned(),
            upstream_method: upstream_method.to_owned(),
            invocation_gate: invocation_gate.to_owned(),
            initial_execution_state: ExecutionState::NotRun,
            reference_baseline_state: ReferenceBaselineState::ContentNotVendoredOrCompared,
        };
    RunnerContract {
        enumeration:
            "recursive files matching /\\.tsx?$/ in tests/cases/conformance".to_owned(),
        emit_enabled: true,
        vary_by: compiler::VARY_BY
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
        variation_limit: MAX_COMPILER_VARIATIONS as u64,
        configuration_order:
            "vary_by order, then each comma-separated value order; configuration names sort override keys"
                .to_owned(),
        unit_partition:
            "harnessIO.makeUnitsFromTest with LF splitting, CR stripping, @filename, @link, and virtual tsconfig/jsconfig partitioning"
                .to_owned(),
        observations: vec![
            observation("diagnostics", "verifyDiagnostics", "always"),
            observation(
                "module-resolution-trace",
                "verifyModuleResolution",
                "effective traceResolution",
            ),
            observation(
                "source-map-record",
                "verifySourceMapRecord",
                "effective sourceMap || inlineSourceMap || declarationMap",
            ),
            observation(
                "javascript-output",
                "verifyJavaScriptOutput",
                "runner emit flag && fixture has a non-.d.ts unit",
            ),
            observation(
                "source-map-output",
                "verifySourceMapOutput",
                "delegated Compiler.doSourcemapBaseline gate",
            ),
            observation(
                "types-symbols",
                "verifyTypesAndSymbols",
                "@noTypesAndSymbols is not true",
            ),
        ],
        reference_baseline_state: ReferenceBaselineState::ContentNotVendoredOrCompared,
    }
}

fn source_inventory_sha256(sources: &[ConformanceSource]) -> String {
    let mut digest = Sha256::new();
    for source in sources {
        hash_inventory_field(&mut digest, source.path.as_bytes());
        hash_inventory_field(&mut digest, source.mode.as_bytes());
        digest.update(source.bytes.to_be_bytes());
        hash_inventory_field(&mut digest, source.sha256.as_bytes());
        hash_inventory_field(&mut digest, source.git_blob_sha1.as_bytes());
    }
    format!("{:x}", digest.finalize())
}

fn path_hash(workspace: &Path, relative: &str) -> HarnessResult<PathHash> {
    let bytes = fs::read(workspace.join(relative)).map_err(|source| {
        error(format!(
            "failed to read H1 conformance input {relative}: {source}"
        ))
    })?;
    Ok(PathHash {
        path: relative.to_owned(),
        sha256: super::sha256_hex(&bytes),
    })
}

fn checked_path_hash(
    workspace: &Path,
    relative: &str,
    expected_sha256: &str,
) -> HarnessResult<PathHash> {
    let pin = path_hash(workspace, relative)?;
    if pin.sha256 != expected_sha256 {
        return Err(error(format!(
            "H1 conformance input {relative} has SHA-256 {}, expected {expected_sha256}",
            pin.sha256
        )));
    }
    Ok(pin)
}

fn is_runner_fixture(path: &str) -> bool {
    path.ends_with(".ts") || path.ends_with(".tsx")
}

fn case_id(fixture_path: &str, variant: &str) -> String {
    format!(
        "typescript-{TYPESCRIPT_VERSION}/conformance/{}#{}",
        super::percent_encode(fixture_path, true),
        super::percent_encode(variant, false)
    )
}

#[cfg(test)]
#[path = "../../tests/unit/upstream_suites/h1_conformance_tests.rs"]
mod tests;

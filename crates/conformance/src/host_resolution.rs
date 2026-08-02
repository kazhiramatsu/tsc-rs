//! H0 frozen host-resolution owner registry.
//!
//! `ratchets/host-resolution.v1.json` starts from the exact 241 live A2
//! `host-resolution` occurrences.  Identity and ownership metadata are frozen
//! at bootstrap.  Later H0 slices may only move a row from `open` to `closed`,
//! in the same change that moves its A2 occurrence to a tombstone and records
//! T0--T4 closure evidence.  This preserves the original owner universe after
//! the live exclusion set reaches zero.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::identity::{assign_case_identities, ENCODER_VERSION};
use crate::ratchet::{
    decode_artifact, git_blob_optional, git_root_for, normalize_node_version, pinned_node_version,
    sha256_hex, verify_accepted_pair_history, verify_pair_values, MatchesArtifact,
    OracleInputsArtifact, MATCHES_REL_PATH, ORACLE_INPUTS_REL_PATH,
};
use crate::scope::{
    host_resolution_state, host_resolution_state_from_bytes, HostResolutionScopeRow,
    HostResolutionScopeState, SCOPE_REL_PATH,
};
use crate::{read_golden, ConformanceResult, ExactIdentity};

pub const HOST_RESOLUTION_REL_PATH: &str = "ratchets/host-resolution.v1.json";
const HOST_RESOLUTION_SCHEMA: u32 = 1;
const TYPESCRIPT_VERSION: &str = "6.0.3";
const D2_INVENTORY_REL_PATH: &str = "m8-emitter-inventory.json";
const D2_SOURCE_REL_PATH: &str = "vendor/typescript-6.0.3/lib/_tsc.js";
const REQUEST_PRODUCER_REL_PATH: &str = "crates/oracle/host-resolution-requests.mjs";
const REQUEST_HOST_REL_PATH: &str = "crates/oracle/program-host.mjs";
const REQUEST_TYPESCRIPT_REL_PATH: &str = "vendor/typescript-6.0.3/lib/typescript.js";
const EXPECTED_ROWS: usize = 241;
const EXPECTED_FIXTURES: usize = 30;

type OraclePositionIndex = BTreeMap<ExactIdentity, (Option<u32>, Option<u32>)>;
type OracleCaseCodeIndex = BTreeMap<(String, String), BTreeSet<u32>>;
type ProgramFactIndex = BTreeMap<(String, String), ProgramFact>;
type ResolutionRequestIndex = BTreeMap<String, Vec<ResolutionRequest>>;

const FAMILY_EXPORTS: &str = "package-exports-patterns-and-blocked-subpaths";
const FAMILY_IMPORTS: &str = "package-imports-self-references-and-conditions";
const FAMILY_NODE_MODULES: &str = "node-modules-package-fields-and-types-versions";
const FAMILY_TYPES: &str = "types-type-roots-and-reference-directives";
const FAMILY_MODE: &str = "resolution-mode-and-message-selection";
const FAMILY_CONSUMERS: &str = "host-fed-semantic-and-emit-consumers";
const FAMILY_PROGRAM: &str = "program-discovery-paths-and-references";
const FAMILY_CLI: &str = "config-driver-renderer-and-exit-status";

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RegistryStatus {
    Frozen,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RowStatus {
    Open,
    Closed,
    Lapsed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ModuleResolutionKind {
    Classic,
    Node10,
    Node16,
    NodeNext,
    Bundler,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ResolutionRequestKind {
    Module,
    TypeReference,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RequestResolutionMode {
    CommonJs,
    EsNext,
    Unspecified,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ResolutionAnchorKind {
    ModuleLiteral,
    ModuleAugmentationLiteral,
    ContainingImport,
    TypesVersionsSelfReference,
    JsdocImport,
    ResolvedAliasImport,
    TypeReferenceDirective,
    SyntheticImportHelpers,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
enum CanaryRelation {
    #[serde(rename = "exact-feature-same-mode")]
    ExactFeature,
    #[serde(rename = "closest-available-same-mode")]
    ClosestAvailable,
    #[serde(rename = "intentional-alternate-mode")]
    IntentionalAlternate,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum HostFeature {
    PackageExportsPattern,
    PackageExportsBlockedSubpath,
    PackageExportsTypesVersionCondition,
    PackageExportsConditions,
    PackageImportsPattern,
    PackageImportsSelfReference,
    PackageMain,
    PackageTypesVersions,
    NodeModulesTraversal,
    AtTypesConditionalExports,
    TypeReferenceDirective,
    AlternateResolutionDiagnostic,
    ExternalHelperConsumer,
    UntypedPackageConsumer,
    ResolvedModuleMember,
    ConstEnumModuleBinding,
    RewriteRelativeImport,
    PackageModeDiagnostic,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum TscOwnerRole {
    Primary,
    Dependency,
    Diagnostic,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum BoundaryReadiness {
    SeamOnly,
    Authoritative,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RustBoundaryRole {
    Producer,
    TableConsumer,
    Driver,
    DiagnosticConsumer,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RegistryFile {
    schema: u32,
    status: RegistryStatus,
    typescript_version: String,
    source: SourcePin,
    families: Vec<OwnerFamily>,
    initial_profiles: Vec<InitialProfile>,
    summary: RegistrySummary,
    rows: Vec<RegistryRow>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SourcePin {
    manifest: String,
    reason: String,
    identity_encoder: u32,
    initial_scope_commit: String,
    initial_identity_count: usize,
    initial_fixture_count: usize,
    initial_projection_sha256: String,
    initial_seed_sha256: String,
    request_producer: String,
    request_producer_sha256: String,
    request_host_sha256: String,
    request_typescript_sha256: String,
    request_node_version: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct OwnerFamily {
    id: String,
    phase: String,
    owner: String,
    description: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct InitialProfile {
    id: String,
    workload: String,
    os: String,
    arch: String,
    measurement_backend: String,
    cpu: CpuProfile,
    wall_seconds: f64,
    max_rss_bytes: u64,
    cache_off_smoke: CacheOffProfile,
    ceilings: ResourceCeilings,
    provenance: ProfileProvenance,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CpuProfile {
    policy: String,
    logical_cores: usize,
    cargo_build_jobs: usize,
    rust_test_threads: usize,
    oversubscribed: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CacheOffProfile {
    fixture_limit: usize,
    wall_seconds: f64,
    max_rss_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ResourceCeilings {
    wall_seconds: f64,
    max_rss_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ProfileProvenance {
    producer_commit: String,
    command: String,
    evidence: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RegistrySummary {
    rows: usize,
    open: usize,
    closed: usize,
    lapsed: usize,
    fixtures: usize,
    by_code: BTreeMap<String, usize>,
    by_family: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RegistryRow {
    id: String,
    identity: ExactIdentity,
    line: Option<u32>,
    col: Option<u32>,
    family: String,
    host_feature: HostFeature,
    module_resolution_kind: ModuleResolutionKind,
    resolution_requests: Vec<ResolutionRequest>,
    tsc_owners: Vec<TscOwnerAnchor>,
    owner_evidence: String,
    rust_boundary: RustBoundary,
    canaries: RowCanaries,
    status: RowStatus,
    source_evidence: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    closure_evidence: Option<ClosureEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    closing_commit: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ResolutionRequest {
    kind: ResolutionRequestKind,
    #[serde(alias = "canonicalSource")]
    canonical_source: String,
    specifier: String,
    mode: RequestResolutionMode,
    #[serde(alias = "anchorKind")]
    anchor_kind: ResolutionAnchorKind,
    #[serde(
        default,
        alias = "anchorStart",
        skip_serializing_if = "Option::is_none"
    )]
    anchor_start: Option<u32>,
    synthetic: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct TscOwnerAnchor {
    role: TscOwnerRole,
    declaration: String,
    name: String,
    kind: String,
    lexical_path: String,
    source_range: SourceRange,
    source_slice_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct SourceRange {
    start: SourcePosition,
    end: SourcePosition,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct SourcePosition {
    offset: usize,
    line: usize,
    character: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RustBoundary {
    readiness: BoundaryReadiness,
    seam_anchors: Vec<RustBoundaryAnchor>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    authoritative_anchors: Vec<RustBoundaryAnchor>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RustBoundaryAnchor {
    role: RustBoundaryRole,
    crate_name: String,
    path: String,
    symbol: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RowCanaries {
    emitting: EmittingCanary,
    non_emitting_control: NonEmittingControl,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct EmittingCanary {
    fixture: String,
    matrix_key: String,
    program_sha256: String,
    identity_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct NonEmittingControl {
    fixture: String,
    matrix_key: String,
    program_sha256: String,
    control_feature: HostFeature,
    module_resolution_kind: ModuleResolutionKind,
    relation: CanaryRelation,
    assertion: String,
    forbidden_codes: Vec<u32>,
    evidence: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ClosureEvidence {
    tiers: Vec<String>,
    artifact: String,
    artifact_sha256: String,
    note: String,
}

#[derive(Debug, Deserialize)]
struct D2Inventory {
    schema: u32,
    status: String,
    typescript_version: String,
    source: String,
    source_sha256: String,
    functions: Vec<D2Function>,
}

#[derive(Clone, Debug, Deserialize)]
struct D2Function {
    id: String,
    name: String,
    kind: String,
    lexical_path: String,
    source_range: SourceRange,
    source_slice_sha256: String,
}

struct RegistryValidationContext<'a> {
    workspace: &'a Path,
    inventory: &'a D2Inventory,
    program_facts: &'a ProgramFactIndex,
    resolution_requests: &'a ResolutionRequestIndex,
    oracle_rows: &'a OraclePositionIndex,
    oracle_case_codes: &'a OracleCaseCodeIndex,
    emitting_cases: &'a BTreeSet<(String, String)>,
    closure_authorities: &'a BTreeMap<String, ClosureAuthority>,
    verify_history: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProgramFact {
    program_sha256: String,
    module_resolution_kind: ModuleResolutionKind,
    program_json: String,
}

#[derive(Debug)]
struct ClosureAuthority {
    artifact_sha256: String,
    matches: MatchesArtifact,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestProducerResponse {
    id: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    identities: Option<Vec<RequestProducerIdentity>>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestProducerIdentity {
    id: String,
    requests: Vec<ResolutionRequest>,
}

/// Materialize the reviewed bootstrap registry.  This command intentionally
/// has no update mode: after bootstrap, row closure is a reviewed edit guarded
/// by the trusted-baseline transition checks below.
pub fn draft_host_resolution_registry(workspace: &Path, out: &Path) -> ConformanceResult<()> {
    let scope = host_resolution_state(&workspace.join(SCOPE_REL_PATH))?;
    validate_bootstrap_scope(&scope)?;
    let inventory = read_inventory(workspace)?;
    let inputs = read_oracle_inputs(workspace)?;
    let initial_scope_commit = git_resolve_commit(workspace, "HEAD")?;

    let mut canary_cases = BTreeSet::new();
    for seed in &scope.live {
        canary_cases.insert((
            seed.identity.fixture.clone(),
            seed.identity.matrix_key.clone(),
        ));
        let spec = negative_canary_spec(&seed.identity)?;
        canary_cases.insert((spec.fixture, spec.matrix_key));
    }
    let program_facts = load_program_facts(workspace, &inputs, &canary_cases)?;
    let request_seeds = scope
        .live
        .iter()
        .map(|seed| {
            (
                format!("h0:{}", seed.identity.sha256()),
                seed.identity.clone(),
            )
        })
        .collect::<Vec<_>>();
    let resolution_requests = load_resolution_requests(workspace, &request_seeds, &program_facts)?;

    let mut rows = Vec::with_capacity(scope.live.len());
    for seed in &scope.live {
        rows.push(build_row(
            workspace,
            seed,
            &inventory,
            &program_facts,
            &resolution_requests,
        )?);
    }
    rows.sort_by(|left, right| left.id.cmp(&right.id));

    let registry = RegistryFile {
        schema: HOST_RESOLUTION_SCHEMA,
        status: RegistryStatus::Frozen,
        typescript_version: TYPESCRIPT_VERSION.to_owned(),
        source: SourcePin {
            manifest: SCOPE_REL_PATH.to_owned(),
            reason: "host-resolution".to_owned(),
            identity_encoder: ENCODER_VERSION,
            initial_scope_commit,
            initial_identity_count: rows.len(),
            initial_fixture_count: fixture_count(rows.iter().map(|row| &row.identity)),
            initial_projection_sha256: projection_sha256(rows.iter().map(|row| &row.identity)),
            initial_seed_sha256: row_seed_projection_sha256(&rows),
            request_producer: REQUEST_PRODUCER_REL_PATH.to_owned(),
            request_producer_sha256: sha256_hex(&fs::read(
                workspace.join(REQUEST_PRODUCER_REL_PATH),
            )?),
            request_host_sha256: sha256_hex(&fs::read(workspace.join(REQUEST_HOST_REL_PATH))?),
            request_typescript_sha256: sha256_hex(&fs::read(
                workspace.join(REQUEST_TYPESCRIPT_REL_PATH),
            )?),
            request_node_version: pinned_node_version(workspace)?,
        },
        families: expected_families(),
        initial_profiles: initial_profiles(),
        summary: summarize(&rows),
        rows,
    };
    validate_registry(
        workspace, &registry, &scope, &inventory, &inputs, false, false,
    )?;

    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut bytes = serde_json::to_vec_pretty(&registry)?;
    bytes.push(b'\n');
    fs::write(out, bytes)?;
    println!(
        "host-resolution registry written: rows={} {}",
        registry.rows.len(),
        out.display()
    );
    Ok(())
}

pub fn check_host_resolution_registry(
    workspace: &Path,
    baseline: Option<&str>,
) -> ConformanceResult<()> {
    let path = workspace.join(HOST_RESOLUTION_REL_PATH);
    let bytes = fs::read(&path).map_err(|err| {
        format!(
            "missing H0 host-resolution registry {}: {err}; run `cargo xtask host-resolution draft`",
            path.display()
        )
    })?;
    let registry = parse_registry(&bytes, &path.display().to_string())?;
    let scope = host_resolution_state(&workspace.join(SCOPE_REL_PATH))?;
    let inventory = read_inventory(workspace)?;
    let inputs = read_oracle_inputs(workspace)?;
    let verify_request_producer = match baseline {
        Some(revision) => !baseline_has_host_registry(workspace, revision)?,
        None => true,
    };
    validate_registry(
        workspace,
        &registry,
        &scope,
        &inventory,
        &inputs,
        true,
        verify_request_producer,
    )?;

    if let Some(baseline) = baseline {
        validate_trusted_baseline(workspace, baseline, &registry)?;
    }
    println!(
        "host-resolution registry ok: rows={} open={} closed={} lapsed={} baseline={}",
        registry.summary.rows,
        registry.summary.open,
        registry.summary.closed,
        registry.summary.lapsed,
        baseline.unwrap_or("none")
    );
    Ok(())
}

fn parse_registry(bytes: &[u8], origin: &str) -> ConformanceResult<RegistryFile> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|err| format!("host-resolution registry at {origin} is not valid JSON: {err}"))?;
    let schema = value
        .get("schema")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| format!("host-resolution registry at {origin} lacks a schema number"))?;
    if schema != u64::from(HOST_RESOLUTION_SCHEMA) {
        return Err(format!(
            "host-resolution registry at {origin} has schema {schema}; this tree implements schema {HOST_RESOLUTION_SCHEMA}"
        )
        .into());
    }
    serde_json::from_slice(bytes).map_err(|err| {
        format!("host-resolution registry at {origin} failed to parse: {err}").into()
    })
}

fn validate_bootstrap_scope(scope: &HostResolutionScopeState) -> ConformanceResult<()> {
    if !scope.frozen {
        return Err("H0 bootstrap requires the frozen M8 scope manifest".into());
    }
    if scope.live.len() != EXPECTED_ROWS {
        return Err(format!(
            "H0 bootstrap requires exactly {EXPECTED_ROWS} live host-resolution rows, found {}",
            scope.live.len()
        )
        .into());
    }
    if fixture_count(scope.live.iter().map(|row| &row.identity)) != EXPECTED_FIXTURES {
        return Err(format!(
            "H0 bootstrap requires exactly {EXPECTED_FIXTURES} host-resolution fixtures"
        )
        .into());
    }
    validate_expected_code_counts(scope.live.iter().map(|row| &row.identity))
}

fn validate_registry(
    workspace: &Path,
    registry: &RegistryFile,
    scope: &HostResolutionScopeState,
    inventory: &D2Inventory,
    inputs: &OracleInputsArtifact,
    verify_history: bool,
    verify_request_producer: bool,
) -> ConformanceResult<()> {
    if !scope.frozen {
        return Err("H0 registry requires the current M8 scope manifest to remain frozen".into());
    }
    if registry.schema != HOST_RESOLUTION_SCHEMA
        || registry.status != RegistryStatus::Frozen
        || registry.typescript_version != TYPESCRIPT_VERSION
    {
        return Err(format!(
            "host-resolution registry must be schema {HOST_RESOLUTION_SCHEMA}, frozen, and pin TypeScript {TYPESCRIPT_VERSION}"
        )
        .into());
    }
    validate_source_pin(workspace, &registry.source, &registry.rows)?;
    if verify_history && !git_is_ancestor(workspace, &registry.source.initial_scope_commit, "HEAD")?
    {
        return Err("host-resolution initial scope commit is not reachable from HEAD".into());
    }
    if verify_history {
        validate_initial_scope_history(workspace, &registry.source)?;
    }
    if registry.families != expected_families() {
        return Err("host-resolution registry owner-family declaration drifted".into());
    }
    validate_profiles(workspace, &registry.initial_profiles, verify_history)?;

    let expected_summary = summarize(&registry.rows);
    if registry.summary != expected_summary {
        return Err("host-resolution registry summary is stale".into());
    }
    let expected_family_counts = BTreeMap::from([
        (FAMILY_EXPORTS.to_owned(), 179),
        (FAMILY_IMPORTS.to_owned(), 36),
        (FAMILY_NODE_MODULES.to_owned(), 6),
        (FAMILY_TYPES.to_owned(), 3),
        (FAMILY_MODE.to_owned(), 7),
        (FAMILY_CONSUMERS.to_owned(), 10),
        (FAMILY_PROGRAM.to_owned(), 0),
        (FAMILY_CLI.to_owned(), 0),
    ]);
    if registry.summary.by_family != expected_family_counts {
        return Err(format!(
            "host-resolution registry owner-family census drifted: {:?}",
            registry.summary.by_family
        )
        .into());
    }
    if registry.rows.len() != EXPECTED_ROWS
        || fixture_count(registry.rows.iter().map(|row| &row.identity)) != EXPECTED_FIXTURES
    {
        return Err(format!(
            "host-resolution registry must retain {EXPECTED_ROWS} rows across {EXPECTED_FIXTURES} fixtures"
        )
        .into());
    }
    validate_expected_code_counts(registry.rows.iter().map(|row| &row.identity))?;
    validate_expected_module_resolution_counts(&registry.rows)?;

    let mut prior_id: Option<&str> = None;
    let mut ids = BTreeSet::new();
    let mut identities = BTreeSet::new();
    let live = scope
        .live
        .iter()
        .map(|row| (&row.identity, row))
        .collect::<BTreeMap<_, _>>();
    let tombstones = scope
        .tombstones
        .iter()
        .map(|row| (&row.identity, row))
        .collect::<BTreeMap<_, _>>();
    let mut expected_open = BTreeSet::new();

    let oracle_rows = oracle_identity_index(workspace, &registry.rows)?;
    let oracle_case_codes = oracle_case_code_index(workspace, &registry.rows)?;
    let emitting_cases = registry
        .rows
        .iter()
        .map(|row| {
            (
                row.identity.fixture.clone(),
                row.identity.matrix_key.clone(),
            )
        })
        .collect::<BTreeSet<_>>();
    let canary_cases = registry
        .rows
        .iter()
        .flat_map(|row| {
            [
                (
                    row.canaries.emitting.fixture.clone(),
                    row.canaries.emitting.matrix_key.clone(),
                ),
                (
                    row.canaries.non_emitting_control.fixture.clone(),
                    row.canaries.non_emitting_control.matrix_key.clone(),
                ),
            ]
        })
        .collect::<BTreeSet<_>>();
    let program_facts = load_program_facts(workspace, inputs, &canary_cases)?;
    let request_seeds = registry
        .rows
        .iter()
        .map(|row| (row.id.clone(), row.identity.clone()))
        .collect::<Vec<_>>();
    let resolution_requests = if verify_request_producer {
        load_resolution_requests(workspace, &request_seeds, &program_facts)?
    } else {
        recorded_resolution_requests(&registry.rows)?
    };
    let closure_authorities = load_closure_authorities(workspace, &registry.rows)?;
    let context = RegistryValidationContext {
        workspace,
        inventory,
        program_facts: &program_facts,
        resolution_requests: &resolution_requests,
        oracle_rows: &oracle_rows,
        oracle_case_codes: &oracle_case_codes,
        emitting_cases: &emitting_cases,
        closure_authorities: &closure_authorities,
        verify_history,
    };
    for row in &registry.rows {
        if prior_id.is_some_and(|prior| prior >= row.id.as_str()) {
            return Err("host-resolution registry rows are not strictly sorted by id".into());
        }
        prior_id = Some(&row.id);
        if !ids.insert(row.id.as_str()) {
            return Err(format!("duplicate host-resolution row id {}", row.id).into());
        }
        if !identities.insert(&row.identity) {
            return Err(format!(
                "duplicate host-resolution identity {}",
                row.identity.label()
            )
            .into());
        }
        validate_row(&context, row)?;

        match row.status {
            RowStatus::Open => {
                let seed = live.get(&row.identity).ok_or_else(|| {
                    format!(
                        "open host-resolution row {} is not a live A2 host exclusion",
                        row.id
                    )
                })?;
                if row.line != seed.line
                    || row.col != seed.col
                    || row.source_evidence != seed.evidence
                {
                    return Err(format!(
                        "open host-resolution row {} drifted from its A2 review fields",
                        row.id
                    )
                    .into());
                }
                expected_open.insert(&row.identity);
            }
            RowStatus::Closed => {
                if live.contains_key(&row.identity) {
                    return Err(format!(
                        "closed host-resolution row {} is still a live A2 exclusion",
                        row.id
                    )
                    .into());
                }
                let tombstone = tombstones.get(&row.identity).ok_or_else(|| {
                    format!(
                        "closed host-resolution row {} has no matching A2 tombstone",
                        row.id
                    )
                })?;
                if tombstone.lapsed {
                    return Err(format!(
                        "closed host-resolution row {} points at a lapsed oracle-correction tombstone",
                        row.id
                    )
                    .into());
                }
                if tombstone.resolving_commit.as_deref() != row.closing_commit.as_deref() {
                    return Err(format!(
                        "host-resolution row {} closing commit disagrees with its A2 tombstone",
                        row.id
                    )
                    .into());
                }
            }
            RowStatus::Lapsed => {
                if live.contains_key(&row.identity) {
                    return Err(format!(
                        "lapsed host-resolution row {} is still a live A2 exclusion",
                        row.id
                    )
                    .into());
                }
                let tombstone = tombstones.get(&row.identity).ok_or_else(|| {
                    format!(
                        "lapsed host-resolution row {} has no matching A2 tombstone",
                        row.id
                    )
                })?;
                if !tombstone.lapsed
                    || tombstone.resolving_commit.as_deref() != row.closing_commit.as_deref()
                {
                    return Err(format!(
                        "lapsed host-resolution row {} is not backed by the same oracle-correction tombstone provenance",
                        row.id
                    )
                    .into());
                }
            }
        }
    }
    let live_identities = live.keys().copied().collect::<BTreeSet<_>>();
    if expected_open != live_identities {
        return Err(
            "live A2 host-resolution exclusions are not exactly the registry open rows".into(),
        );
    }
    Ok(())
}

fn validate_source_pin(
    workspace: &Path,
    source: &SourcePin,
    rows: &[RegistryRow],
) -> ConformanceResult<()> {
    if source.manifest != SCOPE_REL_PATH
        || source.reason != "host-resolution"
        || source.identity_encoder != ENCODER_VERSION
        || source.initial_identity_count != EXPECTED_ROWS
        || source.initial_fixture_count != EXPECTED_FIXTURES
        || !valid_sha256(&source.initial_seed_sha256)
        || !valid_commit(&source.initial_scope_commit)
        || source.request_producer != REQUEST_PRODUCER_REL_PATH
        || source.request_producer_sha256
            != sha256_hex(&fs::read(workspace.join(REQUEST_PRODUCER_REL_PATH))?)
        || source.request_host_sha256
            != sha256_hex(&fs::read(workspace.join(REQUEST_HOST_REL_PATH))?)
        || source.request_typescript_sha256
            != sha256_hex(&fs::read(workspace.join(REQUEST_TYPESCRIPT_REL_PATH))?)
        || source.request_node_version != pinned_node_version(workspace)?
    {
        return Err("host-resolution registry has a malformed initial A2 source pin".into());
    }
    let projection = projection_sha256(rows.iter().map(|row| &row.identity));
    if source.initial_projection_sha256 != projection {
        return Err("host-resolution registry initial identity projection hash is stale".into());
    }
    if source.initial_seed_sha256 != row_seed_projection_sha256(rows) {
        return Err("host-resolution registry initial full seed projection hash is stale".into());
    }
    Ok(())
}

fn validate_initial_scope_history(workspace: &Path, source: &SourcePin) -> ConformanceResult<()> {
    let root = git_root_for(workspace)?;
    let scope_rel = workspace_history_rel(&root, workspace, SCOPE_REL_PATH)?;
    let bytes =
        git_blob_optional(&root, &source.initial_scope_commit, &scope_rel)?.ok_or_else(|| {
            format!(
                "host-resolution initial scope commit {} has no {SCOPE_REL_PATH}",
                source.initial_scope_commit
            )
        })?;
    let scope = host_resolution_state_from_bytes(
        &bytes,
        &format!("{}:{scope_rel}", source.initial_scope_commit),
    )?;
    validate_bootstrap_scope(&scope)?;
    if projection_sha256(scope.live.iter().map(|row| &row.identity))
        != source.initial_projection_sha256
        || scope_seed_projection_sha256(&scope) != source.initial_seed_sha256
    {
        return Err(
            "host-resolution initial commit scope blob differs from its frozen source pin".into(),
        );
    }
    Ok(())
}

fn validate_row(
    context: &RegistryValidationContext<'_>,
    row: &RegistryRow,
) -> ConformanceResult<()> {
    let expected_id = format!("h0:{}", row.identity.sha256());
    if row.id != expected_id {
        return Err(format!(
            "host-resolution row id mismatch for {}: expected {expected_id}",
            row.identity.label()
        )
        .into());
    }
    let classification = classify(&row.identity)?;
    if row.family != classification.family
        || row.host_feature != classification.feature
        || row.module_resolution_kind
            != row_module_resolution_kind(&row.identity, context.program_facts)?
    {
        return Err(format!(
            "host-resolution row {} is assigned to the wrong owner family, feature, or mode",
            row.id
        )
        .into());
    }
    if context.resolution_requests.get(&row.id) != Some(&row.resolution_requests) {
        return Err(format!(
            "host-resolution row {} has stale vendored request-resolution evidence",
            row.id
        )
        .into());
    }
    if row.source_evidence.trim().is_empty() {
        return Err(format!("host-resolution row {} has no source evidence", row.id).into());
    }
    validate_tsc_owners(row, context.inventory, &classification)?;
    if row.owner_evidence != owner_evidence(&row.identity, &classification) {
        return Err(format!(
            "host-resolution row {} has stale reviewed owner evidence",
            row.id
        )
        .into());
    }
    validate_rust_boundary(context.workspace, row, &classification)?;
    validate_canaries(
        row,
        context.program_facts,
        context.oracle_rows,
        context.oracle_case_codes,
        context.emitting_cases,
    )?;
    validate_closure(context, row)?;
    Ok(())
}

fn validate_tsc_owners(
    row: &RegistryRow,
    inventory: &D2Inventory,
    classification: &Classification,
) -> ConformanceResult<()> {
    let expected_names = expected_owner_names(&row.identity, classification);
    if expected_names.is_empty()
        || expected_names
            .iter()
            .filter(|(role, _)| *role == TscOwnerRole::Primary)
            .count()
            != 1
    {
        return Err(format!(
            "host-resolution row {} has no unique reviewed primary tsc owner",
            row.id
        )
        .into());
    }
    if row.tsc_owners.len() != expected_names.len() {
        return Err(format!(
            "host-resolution row {} has the wrong vendored owner-chain length",
            row.id
        )
        .into());
    }
    let mut declarations = BTreeSet::new();
    for (anchor, (role, expected_name)) in row.tsc_owners.iter().zip(expected_names) {
        if !declarations.insert(anchor.declaration.as_str()) {
            return Err(format!(
                "host-resolution row {} repeats a vendored owner declaration",
                row.id
            )
            .into());
        }
        if anchor.role != role || anchor.name != expected_name {
            return Err(format!("host-resolution row {} names the wrong tsc owner", row.id).into());
        }
        let function = inventory
            .functions
            .iter()
            .find(|function| function.id == anchor.declaration)
            .ok_or_else(|| {
                format!(
                    "host-resolution row {} references unknown D2 declaration {}",
                    row.id, anchor.declaration
                )
            })?;
        if anchor.name != function.name
            || anchor.kind != function.kind
            || anchor.lexical_path != function.lexical_path
            || anchor.source_range != function.source_range
            || anchor.source_slice_sha256 != function.source_slice_sha256
        {
            return Err(format!(
                "host-resolution row {} has stale D2 owner metadata for {}",
                row.id, anchor.name
            )
            .into());
        }
    }
    Ok(())
}

fn validate_rust_boundary(
    workspace: &Path,
    row: &RegistryRow,
    classification: &Classification,
) -> ConformanceResult<()> {
    let expected = rust_boundary(&row.identity, classification)?;
    if !same_boundary_target(&row.rust_boundary, &expected) {
        return Err(format!(
            "host-resolution row {} has the wrong Rust implementation boundary",
            row.id
        )
        .into());
    }
    match row.rust_boundary.readiness {
        BoundaryReadiness::SeamOnly if !row.rust_boundary.authoritative_anchors.is_empty() => {
            return Err(format!(
                "host-resolution row {} has authoritative anchors but is marked seam-only",
                row.id
            )
            .into())
        }
        BoundaryReadiness::Authoritative if row.rust_boundary.authoritative_anchors.is_empty() => {
            return Err(format!(
                "host-resolution row {} claims authority without implementation anchors",
                row.id
            )
            .into())
        }
        BoundaryReadiness::Authoritative
            if row.rust_boundary.authoritative_anchors == row.rust_boundary.seam_anchors =>
        {
            return Err(format!(
                "host-resolution row {} relabels the prerequisite seam as authoritative",
                row.id
            )
            .into())
        }
        _ => {}
    }
    validate_rust_anchor_set(workspace, row, &row.rust_boundary.seam_anchors, false)?;
    validate_rust_anchor_set(
        workspace,
        row,
        &row.rust_boundary.authoritative_anchors,
        true,
    )?;
    Ok(())
}

fn validate_rust_anchor_set(
    workspace: &Path,
    row: &RegistryRow,
    anchors: &[RustBoundaryAnchor],
    authoritative: bool,
) -> ConformanceResult<()> {
    let mut roles = BTreeSet::new();
    for anchor in anchors {
        if !safe_relative_path(&anchor.path)
            || anchor.crate_name.trim().is_empty()
            || anchor.symbol.trim().is_empty()
        {
            return Err(format!(
                "host-resolution row {} has a malformed Rust boundary anchor",
                row.id
            )
            .into());
        }
        if !roles.insert(anchor.role) {
            return Err(format!(
                "host-resolution row {} repeats Rust boundary role {:?}",
                row.id, anchor.role
            )
            .into());
        }
        let path = workspace.join(&anchor.path);
        let text = fs::read_to_string(&path).map_err(|err| {
            format!(
                "host-resolution row {} Rust boundary {} is unreadable: {err}",
                row.id,
                path.display()
            )
        })?;
        if !text.contains(&anchor.symbol) {
            return Err(format!(
                "host-resolution row {} Rust boundary symbol {:?} is absent from {}",
                row.id,
                anchor.symbol,
                path.display()
            )
            .into());
        }
    }
    if authoritative
        && !anchors.is_empty()
        && roles
            != BTreeSet::from([
                RustBoundaryRole::Producer,
                RustBoundaryRole::TableConsumer,
                RustBoundaryRole::Driver,
                RustBoundaryRole::DiagnosticConsumer,
            ])
    {
        return Err(format!(
            "host-resolution row {} authoritative boundary lacks the complete producer-to-consumer chain",
            row.id
        )
        .into());
    }
    Ok(())
}

fn validate_canaries(
    row: &RegistryRow,
    program_facts: &ProgramFactIndex,
    oracle_rows: &BTreeMap<ExactIdentity, (Option<u32>, Option<u32>)>,
    oracle_case_codes: &BTreeMap<(String, String), BTreeSet<u32>>,
    emitting_cases: &BTreeSet<(String, String)>,
) -> ConformanceResult<()> {
    let emitting = &row.canaries.emitting;
    if emitting.fixture != row.identity.fixture
        || emitting.matrix_key != row.identity.matrix_key
        || emitting.identity_sha256 != row.identity.sha256()
        || program_fact(program_facts, &emitting.fixture, &emitting.matrix_key)?.program_sha256
            != emitting.program_sha256
    {
        return Err(format!("host-resolution row {} has a stale emitting canary", row.id).into());
    }
    let oracle_position = oracle_rows.get(&row.identity).ok_or_else(|| {
        format!(
            "host-resolution row {} emitting identity is absent from committed goldens",
            row.id
        )
    })?;
    if *oracle_position != (row.line, row.col) {
        return Err(format!(
            "host-resolution row {} line/column drifted from its emitting oracle canary",
            row.id
        )
        .into());
    }

    let negative = &row.canaries.non_emitting_control;
    let negative_fact = program_fact(program_facts, &negative.fixture, &negative.matrix_key)?;
    let emitting_fact = program_fact(
        program_facts,
        &row.identity.fixture,
        &row.identity.matrix_key,
    )?;
    if negative.assertion != "oracle-case-excludes-codes"
        || negative.evidence.trim().is_empty()
        || negative.forbidden_codes != [row.identity.code]
        || negative_fact.program_sha256 != negative.program_sha256
        || negative_fact.module_resolution_kind != negative.module_resolution_kind
    {
        return Err(format!(
            "host-resolution row {} has a stale reviewed non-emitting control",
            row.id
        )
        .into());
    }
    match negative.relation {
        CanaryRelation::ExactFeature | CanaryRelation::ClosestAvailable
            if negative_fact.module_resolution_kind == emitting_fact.module_resolution_kind => {}
        CanaryRelation::IntentionalAlternate
            if row.identity.code == 2792
                && emitting_fact.module_resolution_kind == ModuleResolutionKind::Classic
                && negative_fact.module_resolution_kind == ModuleResolutionKind::Bundler => {}
        _ => {
            return Err(format!(
                "host-resolution row {} negative canary violates its typed mode relation",
                row.id
            )
            .into())
        }
    }
    let expected = negative_canary_spec(&row.identity)?;
    if negative.fixture != expected.fixture
        || negative.matrix_key != expected.matrix_key
        || negative.relation != expected.relation
        || negative.evidence != expected.evidence
        || negative.control_feature != row.host_feature
    {
        return Err(format!(
            "host-resolution row {} is not pinned to its reviewed non-emitting control",
            row.id
        )
        .into());
    }
    let case_key = (negative.fixture.clone(), negative.matrix_key.clone());
    if emitting_cases.contains(&case_key) {
        return Err(format!(
            "host-resolution row {} negative canary is itself an H0 emitting case",
            row.id
        )
        .into());
    }
    let codes = oracle_case_codes.get(&case_key).ok_or_else(|| {
        format!(
            "host-resolution row {} negative canary is absent from committed goldens",
            row.id
        )
    })?;
    if negative
        .forbidden_codes
        .iter()
        .any(|code| codes.contains(code))
    {
        return Err(format!(
            "host-resolution row {} negative canary emits a forbidden diagnostic code",
            row.id
        )
        .into());
    }
    Ok(())
}

fn load_closure_authorities(
    workspace: &Path,
    rows: &[RegistryRow],
) -> ConformanceResult<BTreeMap<String, ClosureAuthority>> {
    let commits = rows
        .iter()
        .filter_map(|row| row.closing_commit.as_deref())
        .filter(|commit| valid_commit(commit))
        .collect::<BTreeSet<_>>();
    if commits.is_empty() {
        return Ok(BTreeMap::new());
    }

    // A structurally coherent pair at `closing_commit` is not sufficient: it
    // must also be an inherited member of the one append-only accepted A1
    // history. Keep this conditional so the all-open H0.0 bootstrap adds no
    // second history walk to ordinary CI.
    verify_accepted_pair_history(workspace)?;

    let root = git_root_for(workspace)?;
    let matches_rel = workspace_history_rel(&root, workspace, MATCHES_REL_PATH)?;
    let inputs_rel = workspace_history_rel(&root, workspace, ORACLE_INPUTS_REL_PATH)?;
    let tsc_rel = workspace_history_rel(&root, workspace, D2_SOURCE_REL_PATH)?;
    let mut authorities = BTreeMap::new();
    for commit in commits {
        let matches_bytes = git_blob_optional(&root, commit, &matches_rel)?.ok_or_else(|| {
            format!("H0 closing commit {commit} has no accepted-match artifact {MATCHES_REL_PATH}")
        })?;
        let matches: MatchesArtifact =
            decode_artifact(&matches_bytes, "H0 historical accepted-match artifact")?;
        matches.validate()?;
        let inputs_bytes = git_blob_optional(&root, commit, &inputs_rel)?.ok_or_else(|| {
            format!(
                "H0 closing commit {commit} has no oracle-input artifact {ORACLE_INPUTS_REL_PATH}"
            )
        })?;
        let inputs: OracleInputsArtifact =
            decode_artifact(&inputs_bytes, "H0 historical oracle-input artifact")?;
        inputs.validate()?;
        verify_pair_values(commit, &matches, &inputs, &inputs_bytes)?;
        let tsc_bytes = git_blob_optional(&root, commit, &tsc_rel)?.ok_or_else(|| {
            format!("H0 closing commit {commit} has no vendored {D2_SOURCE_REL_PATH}")
        })?;
        if matches.inputs.tsc_js_sha256 != sha256_hex(&tsc_bytes) {
            return Err(format!(
                "H0 closing commit {commit} accepted sets are not bound to vendored TypeScript"
            )
            .into());
        }
        authorities.insert(
            commit.to_owned(),
            ClosureAuthority {
                artifact_sha256: sha256_hex(&matches_bytes),
                matches,
            },
        );
    }
    Ok(authorities)
}

fn validate_closure(
    context: &RegistryValidationContext<'_>,
    row: &RegistryRow,
) -> ConformanceResult<()> {
    match row.status {
        RowStatus::Open => {
            if row.closure_evidence.is_some() || row.closing_commit.is_some() {
                return Err(format!(
                    "open host-resolution row {} may not claim closure evidence",
                    row.id
                )
                .into());
            }
        }
        RowStatus::Closed => {
            if row.rust_boundary.readiness != BoundaryReadiness::Authoritative {
                return Err(format!(
                    "closed host-resolution row {} still has only a seam-only Rust boundary",
                    row.id
                )
                .into());
            }
            let commit = row.closing_commit.as_deref().ok_or_else(|| {
                format!(
                    "closed host-resolution row {} has no closing commit",
                    row.id
                )
            })?;
            if !valid_commit(commit)
                || (context.verify_history && !git_is_ancestor(context.workspace, commit, "HEAD")?)
            {
                return Err(format!(
                    "closed host-resolution row {} closing commit is not a reachable full SHA",
                    row.id
                )
                .into());
            }
            let evidence = row.closure_evidence.as_ref().ok_or_else(|| {
                format!(
                    "closed host-resolution row {} has no closure evidence",
                    row.id
                )
            })?;
            validate_exact_closure_evidence(context, row, commit, evidence)?;
        }
        RowStatus::Lapsed => match (row.closing_commit.as_deref(), row.closure_evidence.as_ref()) {
            (None, None) => {}
            (Some(commit), Some(evidence)) => {
                if row.rust_boundary.readiness != BoundaryReadiness::Authoritative
                    || !valid_commit(commit)
                    || (context.verify_history
                        && !git_is_ancestor(context.workspace, commit, "HEAD")?)
                {
                    return Err(format!(
                        "lapsed host-resolution row {} has malformed historical closure provenance",
                        row.id
                    )
                    .into());
                }
                validate_exact_closure_evidence(context, row, commit, evidence)?;
            }
            _ => {
                return Err(format!(
                    "lapsed host-resolution row {} must retain both or neither closing fields",
                    row.id
                )
                .into())
            }
        },
    }
    Ok(())
}

fn validate_exact_closure_evidence(
    context: &RegistryValidationContext<'_>,
    row: &RegistryRow,
    commit: &str,
    evidence: &ClosureEvidence,
) -> ConformanceResult<()> {
    if evidence.tiers != ["t0", "t1", "t2", "t3", "t4"]
        || evidence.note.trim().is_empty()
        || evidence.artifact != MATCHES_REL_PATH
        || !valid_sha256(&evidence.artifact_sha256)
    {
        return Err(format!(
            "closed host-resolution row {} lacks exact T0-T4 accepted-set evidence",
            row.id
        )
        .into());
    }
    let authority = context.closure_authorities.get(commit).ok_or_else(|| {
        format!(
            "closed host-resolution row {} has no accepted-set artifact at closing commit {commit}",
            row.id
        )
    })?;
    if authority.artifact_sha256 != evidence.artifact_sha256 {
        return Err(format!(
            "closed host-resolution row {} accepted-set artifact hash is stale",
            row.id
        )
        .into());
    }
    validate_authoritative_anchors_at_commit(context.workspace, row, commit)?;
    let sets = authority
        .matches
        .views
        .get("all")
        .and_then(|fixtures| fixtures.get(&row.identity.fixture))
        .and_then(|cases| cases.get(&row.identity.matrix_key))
        .ok_or_else(|| {
            format!(
                "closed host-resolution row {} has no All-view accepted case at its closing commit",
                row.id
            )
        })?;
    let key = crate::T0Key {
        file: row.identity.file.clone(),
        code: row.identity.code,
        line: row.line,
        col: row.col,
    };
    if !sets.matched.contains(&key)
        || !sets.multiplicity_complete.contains(&key)
        || !sets.t1.contains(&key)
        || !sets.t2.contains(&key)
        || !sets.t3.contains(&key)
        || !sets.t4
    {
        return Err(format!(
            "closed host-resolution row {} is not exact at every accepted T0-T4 tier",
            row.id
        )
        .into());
    }
    Ok(())
}

fn validate_authoritative_anchors_at_commit(
    workspace: &Path,
    row: &RegistryRow,
    commit: &str,
) -> ConformanceResult<()> {
    let root = git_root_for(workspace)?;
    for anchor in &row.rust_boundary.authoritative_anchors {
        let relative = workspace_history_rel(&root, workspace, &anchor.path)?;
        let bytes = git_blob_optional(&root, commit, &relative)?.ok_or_else(|| {
            format!(
                "closed host-resolution row {} authoritative path {} is absent at closing commit {commit}",
                row.id, anchor.path
            )
        })?;
        let text = String::from_utf8(bytes).map_err(|_| {
            format!(
                "closed host-resolution row {} authoritative path {} is not UTF-8 at closing commit",
                row.id, anchor.path
            )
        })?;
        if !text.contains(&anchor.symbol) {
            return Err(format!(
                "closed host-resolution row {} authoritative symbol {:?} is absent at closing commit {commit}",
                row.id, anchor.symbol
            )
            .into());
        }
    }
    Ok(())
}

fn validate_trusted_baseline(
    workspace: &Path,
    baseline: &str,
    current: &RegistryFile,
) -> ConformanceResult<()> {
    let root = git_root_for(workspace)?;
    let baseline_commit = git_resolve_commit(&root, baseline)?;
    if !git_is_ancestor(&root, &baseline_commit, "HEAD")? {
        return Err("host-resolution trusted baseline is not an ancestor of HEAD".into());
    }
    let registry_rel = workspace_history_rel(&root, workspace, HOST_RESOLUTION_REL_PATH)?;
    let Some(bytes) = git_blob_optional(&root, &baseline_commit, &registry_rel)? else {
        if current.rows.iter().any(|row| row.status != RowStatus::Open) {
            return Err("host-resolution registry bootstrap must introduce all rows open".into());
        }
        let scope_rel = workspace_history_rel(&root, workspace, SCOPE_REL_PATH)?;
        let baseline_scope_bytes = git_blob_optional(&root, &baseline_commit, &scope_rel)?
            .ok_or_else(|| format!("trusted baseline {baseline} has no {SCOPE_REL_PATH}"))?;
        let baseline_scope = host_resolution_state_from_bytes(
            &baseline_scope_bytes,
            &format!("{baseline_commit}:{scope_rel}"),
        )?;
        validate_bootstrap_scope(&baseline_scope)?;
        let baseline_projection =
            projection_sha256(baseline_scope.live.iter().map(|row| &row.identity));
        if baseline_projection != current.source.initial_projection_sha256 {
            return Err(
                "host-resolution registry bootstrap differs from the trusted A2 universe".into(),
            );
        }
        if scope_seed_projection_sha256(&baseline_scope) != current.source.initial_seed_sha256 {
            return Err(
                "host-resolution registry bootstrap review fields differ from trusted A2 seed"
                    .into(),
            );
        }
        if !git_is_ancestor(
            &root,
            &current.source.initial_scope_commit,
            &baseline_commit,
        )? {
            return Err(
                "host-resolution initial scope commit is not an ancestor of the trusted baseline"
                    .into(),
            );
        }
        let initial_scope_bytes =
            git_blob_optional(&root, &current.source.initial_scope_commit, &scope_rel)?
                .ok_or_else(|| {
                    format!(
                        "initial scope commit {} has no {SCOPE_REL_PATH}",
                        current.source.initial_scope_commit
                    )
                })?;
        let initial_scope = host_resolution_state_from_bytes(
            &initial_scope_bytes,
            &format!("{}:{scope_rel}", current.source.initial_scope_commit),
        )?;
        validate_bootstrap_scope(&initial_scope)?;
        if scope_seed_projection_sha256(&initial_scope) != current.source.initial_seed_sha256 {
            return Err(
                "host-resolution source pin does not match its initial full A2 seed".into(),
            );
        }
        return Ok(());
    };

    let trusted = parse_registry(&bytes, &format!("{baseline_commit}:{registry_rel}"))?;
    if current.schema != trusted.schema
        || current.status != trusted.status
        || current.typescript_version != trusted.typescript_version
        || current.source != trusted.source
        || current.families != trusted.families
        || current.initial_profiles != trusted.initial_profiles
    {
        return Err(
            "frozen host-resolution registry metadata changed against trusted baseline".into(),
        );
    }
    let trusted_by_id = trusted
        .rows
        .iter()
        .map(|row| (row.id.as_str(), row))
        .collect::<BTreeMap<_, _>>();
    if trusted_by_id.len() != current.rows.len() {
        return Err(
            "host-resolution registry row universe changed against trusted baseline".into(),
        );
    }
    for row in &current.rows {
        let prior = trusted_by_id.get(row.id.as_str()).ok_or_else(|| {
            format!(
                "host-resolution registry added or replaced frozen row {}",
                row.id
            )
        })?;
        if !same_immutable_row(prior, row) {
            return Err(format!(
                "host-resolution row {} immutable owner metadata changed",
                row.id
            )
            .into());
        }
        validate_row_transition(prior, row)?;
    }
    Ok(())
}

fn validate_row_transition(prior: &RegistryRow, row: &RegistryRow) -> ConformanceResult<()> {
    match (prior.status, row.status) {
        (RowStatus::Open, RowStatus::Open | RowStatus::Closed) => {}
        (RowStatus::Open, RowStatus::Lapsed)
            if row.closing_commit.is_none() && row.closure_evidence.is_none() => {}
        (RowStatus::Open, RowStatus::Lapsed) => {
            return Err(format!(
                "open host-resolution row {} cannot lapse with fabricated closure provenance",
                row.id
            )
            .into())
        }
        (RowStatus::Closed, RowStatus::Closed | RowStatus::Lapsed)
            if prior.closing_commit == row.closing_commit
                && prior.closure_evidence == row.closure_evidence => {}
        (RowStatus::Closed, _) => {
            return Err(format!(
                "closed host-resolution row {} lost immutable closure provenance against trusted baseline",
                row.id
            )
            .into())
        }
        (RowStatus::Lapsed, RowStatus::Lapsed)
            if prior.closing_commit == row.closing_commit
                && prior.closure_evidence == row.closure_evidence => {}
        (RowStatus::Lapsed, RowStatus::Closed)
            if match (
                prior.closing_commit.as_ref(),
                prior.closure_evidence.as_ref(),
            ) {
                (Some(_), Some(_)) => {
                    prior.closing_commit == row.closing_commit
                        && prior.closure_evidence == row.closure_evidence
                }
                (None, None) => row.closing_commit.is_some() && row.closure_evidence.is_some(),
                _ => false,
            } => {}
        (RowStatus::Lapsed, _) => {
            return Err(format!(
                "lapsed host-resolution row {} changed against trusted baseline",
                row.id
            )
            .into())
        }
    }
    match (prior.rust_boundary.readiness, row.rust_boundary.readiness) {
        (BoundaryReadiness::SeamOnly, _)
        | (BoundaryReadiness::Authoritative, BoundaryReadiness::Authoritative) => {}
        (BoundaryReadiness::Authoritative, BoundaryReadiness::SeamOnly) => {
            return Err(format!(
                "host-resolution row {} regressed its authoritative Rust boundary",
                row.id
            )
            .into())
        }
    }
    if prior.rust_boundary.readiness == BoundaryReadiness::Authoritative
        && prior.rust_boundary.authoritative_anchors != row.rust_boundary.authoritative_anchors
    {
        return Err(format!(
            "host-resolution row {} changed frozen authoritative Rust anchors",
            row.id
        )
        .into());
    }
    Ok(())
}

fn baseline_has_host_registry(workspace: &Path, baseline: &str) -> ConformanceResult<bool> {
    let root = git_root_for(workspace)?;
    let commit = git_resolve_commit(&root, baseline)?;
    let registry_rel = workspace_history_rel(&root, workspace, HOST_RESOLUTION_REL_PATH)?;
    Ok(git_blob_optional(&root, &commit, &registry_rel)?.is_some())
}

fn same_immutable_row(left: &RegistryRow, right: &RegistryRow) -> bool {
    left.id == right.id
        && left.identity == right.identity
        && left.line == right.line
        && left.col == right.col
        && left.family == right.family
        && left.host_feature == right.host_feature
        && left.module_resolution_kind == right.module_resolution_kind
        && left.resolution_requests == right.resolution_requests
        && left.tsc_owners == right.tsc_owners
        && left.owner_evidence == right.owner_evidence
        && same_boundary_target(&left.rust_boundary, &right.rust_boundary)
        && left.canaries == right.canaries
        && left.source_evidence == right.source_evidence
}

fn build_row(
    workspace: &Path,
    seed: &HostResolutionScopeRow,
    inventory: &D2Inventory,
    program_facts: &ProgramFactIndex,
    resolution_requests: &ResolutionRequestIndex,
) -> ConformanceResult<RegistryRow> {
    let classification = classify(&seed.identity)?;
    let owners = expected_owner_names(&seed.identity, &classification)
        .into_iter()
        .map(|(role, name)| owner_anchor(inventory, role, name))
        .collect::<ConformanceResult<Vec<_>>>()?;
    let negative = negative_canary_spec(&seed.identity)?;
    let negative_fact = program_fact(program_facts, &negative.fixture, &negative.matrix_key)?;
    let canaries = RowCanaries {
        emitting: EmittingCanary {
            fixture: seed.identity.fixture.clone(),
            matrix_key: seed.identity.matrix_key.clone(),
            program_sha256: program_fact(
                program_facts,
                &seed.identity.fixture,
                &seed.identity.matrix_key,
            )?
            .program_sha256
            .clone(),
            identity_sha256: seed.identity.sha256(),
        },
        non_emitting_control: NonEmittingControl {
            fixture: negative.fixture,
            matrix_key: negative.matrix_key,
            program_sha256: negative_fact.program_sha256.clone(),
            control_feature: classification.feature,
            module_resolution_kind: negative_fact.module_resolution_kind,
            relation: negative.relation,
            assertion: "oracle-case-excludes-codes".to_owned(),
            forbidden_codes: vec![seed.identity.code],
            evidence: negative.evidence,
        },
    };
    let row = RegistryRow {
        id: format!("h0:{}", seed.identity.sha256()),
        identity: seed.identity.clone(),
        line: seed.line,
        col: seed.col,
        family: classification.family.to_owned(),
        host_feature: classification.feature,
        module_resolution_kind: row_module_resolution_kind(&seed.identity, program_facts)?,
        resolution_requests: resolution_requests
            .get(&format!("h0:{}", seed.identity.sha256()))
            .cloned()
            .ok_or_else(|| format!("H0 request producer omitted {}", seed.identity.label()))?,
        tsc_owners: owners,
        owner_evidence: owner_evidence(&seed.identity, &classification),
        rust_boundary: rust_boundary(&seed.identity, &classification)?,
        canaries,
        status: RowStatus::Open,
        source_evidence: seed.evidence.clone(),
        closure_evidence: None,
        closing_commit: None,
    };
    validate_rust_boundary(workspace, &row, &classification)?;
    Ok(row)
}

#[derive(Clone, Copy)]
struct Classification {
    family: &'static str,
    feature: HostFeature,
}

struct NegativeCanarySpec {
    fixture: String,
    matrix_key: String,
    relation: CanaryRelation,
    evidence: String,
}

fn classify(identity: &ExactIdentity) -> ConformanceResult<Classification> {
    let fixture = identity.fixture.as_str();
    let classification = match fixture {
        "conformance/classes/members/privateNames/privateNameEmitHelpers.ts"
        | "conformance/classes/members/privateNames/privateNameStaticEmitHelpers.ts" => {
            Classification {
                family: FAMILY_CONSUMERS,
                feature: HostFeature::ExternalHelperConsumer,
            }
        }
        "conformance/declarationEmit/typesVersionsDeclarationEmit.multiFileBackReferenceToSelf.ts" => {
            Classification {
                family: FAMILY_NODE_MODULES,
                feature: HostFeature::PackageTypesVersions,
            }
        }
        "conformance/externalModules/rewriteRelativeImportExtensions/nodeModulesTsFiles.ts" => {
            Classification {
                family: FAMILY_NODE_MODULES,
                feature: HostFeature::NodeModulesTraversal,
            }
        }
        "conformance/externalModules/rewriteRelativeImportExtensions/packageJsonImportsErrors.ts" => {
            Classification {
                family: FAMILY_IMPORTS,
                feature: HostFeature::RewriteRelativeImport,
            }
        }
        "conformance/externalModules/verbatimModuleSyntaxAmbientConstEnum.ts" => Classification {
            family: FAMILY_CONSUMERS,
            feature: HostFeature::ConstEnumModuleBinding,
        },
        "conformance/jsdoc/importTag17.ts" => Classification {
            family: FAMILY_TYPES,
            feature: HostFeature::AtTypesConditionalExports,
        },
        "conformance/node/allowJs/nodeModulesAllowJsPackagePatternExportsExclude.ts"
        | "conformance/node/nodeModulesPackagePatternExportsExclude.ts"
        | "conformance/node/nodeModulesExportsDoubleAsterisk.ts"
        | "conformance/node/nodeModulesExportsSpecifierGenerationPattern.ts" => Classification {
            family: FAMILY_EXPORTS,
            feature: HostFeature::PackageExportsPattern,
        },
        "conformance/node/nodeModulesExportsBlocksSpecifierResolution.ts"
        | "conformance/node/nodeModulesExportsSourceTs.ts"
        | "conformance/node/nodeModulesExportsSpecifierGenerationDirectory.ts" => {
            Classification {
                family: FAMILY_EXPORTS,
                feature: HostFeature::PackageExportsBlockedSubpath,
            }
        }
        "conformance/node/nodeModulesExportsBlocksTypesVersions.ts"
            if identity.start == Some(222) => Classification {
                family: FAMILY_EXPORTS,
                feature: HostFeature::PackageExportsTypesVersionCondition,
            },
        "conformance/node/nodeModulesExportsBlocksTypesVersions.ts" => Classification {
            family: FAMILY_EXPORTS,
            feature: HostFeature::PackageExportsBlockedSubpath,
        },
        "conformance/moduleResolution/bundler/bundlerCommonJS.ts"
        | "conformance/moduleResolution/conditionalExportsResolutionFallbackNull.ts"
        | "conformance/node/nodeModulesExportsSpecifierGenerationConditions.ts" => {
            Classification {
                family: FAMILY_IMPORTS,
                feature: HostFeature::PackageExportsConditions,
            }
        }
        "conformance/node/nodeModulesImportResolutionIntoExport.ts"
        | "conformance/node/nodeModulesImportResolutionNoCycle.ts" => Classification {
            family: FAMILY_IMPORTS,
            feature: HostFeature::PackageImportsSelfReference,
        },
        "conformance/node/nodeModulesPackageImportsRootWildcardNode16.ts" => Classification {
            family: FAMILY_IMPORTS,
            feature: HostFeature::PackageImportsPattern,
        },
        "conformance/moduleResolution/packageJsonMain_isNonRecursive.ts" => Classification {
            family: FAMILY_NODE_MODULES,
            feature: HostFeature::PackageMain,
        },
        "conformance/node/nodeModulesNoDirectoryModule.ts" => Classification {
            family: FAMILY_NODE_MODULES,
            feature: HostFeature::PackageModeDiagnostic,
        },
        "conformance/moduleResolution/node10AlternateResult_noResolution.ts" => Classification {
            family: FAMILY_MODE,
            feature: HostFeature::AlternateResolutionDiagnostic,
        },
        "conformance/moduleResolution/resolutionModeImportType1.ts"
        | "conformance/moduleResolution/resolutionModeTypeOnlyImport1.ts" => Classification {
            family: FAMILY_MODE,
            feature: HostFeature::AlternateResolutionDiagnostic,
        },
        "conformance/moduleResolution/untypedModuleImport_allowJs.ts"
        | "conformance/salsa/namespaceAssignmentToRequireAlias.ts"
        | "conformance/moduleResolution/untypedModuleImport_withAugmentation.ts" => {
            Classification {
                family: FAMILY_CONSUMERS,
                feature: HostFeature::UntypedPackageConsumer,
            }
        }
        "conformance/typings/typingsLookup3.ts" => Classification {
            family: FAMILY_TYPES,
            feature: HostFeature::TypeReferenceDirective,
        },
        other => {
            return Err(format!(
                "unclassified H0 diagnostic {} fixture {other}",
                identity.code
            )
            .into());
        }
    };
    Ok(classification)
}

fn row_module_resolution_kind(
    identity: &ExactIdentity,
    program_facts: &ProgramFactIndex,
) -> ConformanceResult<ModuleResolutionKind> {
    Ok(
        program_fact(program_facts, &identity.fixture, &identity.matrix_key)?
            .module_resolution_kind,
    )
}

fn expected_owner_names(
    identity: &ExactIdentity,
    classification: &Classification,
) -> Vec<(TscOwnerRole, &'static str)> {
    let fixture = identity.fixture.as_str();
    let mut owners = match fixture {
        "conformance/node/allowJs/nodeModulesAllowJsPackagePatternExportsExclude.ts"
        | "conformance/node/nodeModulesPackagePatternExportsExclude.ts" => vec![
            (TscOwnerRole::Primary, "loadModuleFromTargetExportOrImport"),
            (TscOwnerRole::Dependency, "loadModuleFromExportsOrImports"),
            (TscOwnerRole::Diagnostic, "resolveExternalModule"),
        ],
        "conformance/node/nodeModulesExportsBlocksSpecifierResolution.ts"
        | "conformance/node/nodeModulesExportsSourceTs.ts" => vec![
            (TscOwnerRole::Primary, "loadModuleFromExports"),
            (TscOwnerRole::Diagnostic, "resolveExternalModule"),
        ],
        "conformance/node/nodeModulesExportsBlocksTypesVersions.ts"
            if classification.feature == HostFeature::PackageExportsTypesVersionCondition =>
        {
            vec![
                (TscOwnerRole::Primary, "loadModuleFromTargetExportOrImport"),
                (TscOwnerRole::Diagnostic, "resolveExternalModule"),
            ]
        }
        "conformance/node/nodeModulesExportsBlocksTypesVersions.ts" => vec![
            (TscOwnerRole::Primary, "loadModuleFromExports"),
            (TscOwnerRole::Diagnostic, "resolveExternalModule"),
        ],
        "conformance/node/nodeModulesExportsDoubleAsterisk.ts"
        | "conformance/node/nodeModulesExportsSpecifierGenerationPattern.ts" => vec![
            (TscOwnerRole::Primary, "loadModuleFromExportsOrImports"),
            (TscOwnerRole::Diagnostic, "resolveExternalModule"),
        ],
        "conformance/node/nodeModulesExportsSpecifierGenerationDirectory.ts" => vec![
            (TscOwnerRole::Primary, "loadModuleFromExportsOrImports"),
            (TscOwnerRole::Dependency, "loadModuleFromTargetExportOrImport"),
            (TscOwnerRole::Diagnostic, "resolveExternalModule"),
        ],
        "conformance/externalModules/rewriteRelativeImportExtensions/packageJsonImportsErrors.ts" => vec![
            (TscOwnerRole::Primary, "loadModuleFromImports"),
            (TscOwnerRole::Dependency, "loadModuleFromExportsOrImports"),
            (TscOwnerRole::Dependency, "loadModuleFromTargetExportOrImport"),
            (TscOwnerRole::Diagnostic, "resolveExternalModule"),
        ],
        "conformance/moduleResolution/bundler/bundlerCommonJS.ts"
        | "conformance/moduleResolution/conditionalExportsResolutionFallbackNull.ts" => vec![
            (TscOwnerRole::Primary, "loadModuleFromTargetExportOrImport"),
            (TscOwnerRole::Diagnostic, "resolveExternalModule"),
        ],
        "conformance/node/nodeModulesExportsSpecifierGenerationConditions.ts" => vec![
            (TscOwnerRole::Primary, "loadModuleFromExportsOrImports"),
            (TscOwnerRole::Diagnostic, "resolveExternalModule"),
        ],
        "conformance/node/nodeModulesImportResolutionIntoExport.ts"
        | "conformance/node/nodeModulesImportResolutionNoCycle.ts" => vec![
            (TscOwnerRole::Primary, "loadModuleFromTargetExportOrImport"),
            (TscOwnerRole::Dependency, "loadModuleFromImports"),
            (TscOwnerRole::Dependency, "loadModuleFromSelfNameReference"),
            (TscOwnerRole::Dependency, "loadModuleFromExports"),
            (TscOwnerRole::Dependency, "nodeModuleNameResolverWorker"),
            (TscOwnerRole::Diagnostic, "resolveExternalModule"),
        ],
        "conformance/node/nodeModulesPackageImportsRootWildcardNode16.ts" => vec![
            (TscOwnerRole::Primary, "loadModuleFromImports"),
            (TscOwnerRole::Diagnostic, "resolveExternalModule"),
        ],
        "conformance/declarationEmit/typesVersionsDeclarationEmit.multiFileBackReferenceToSelf.ts" => vec![
            (TscOwnerRole::Primary, "loadNodeModuleFromDirectoryWorker"),
            (TscOwnerRole::Diagnostic, "reportNonExportedMember"),
        ],
        "conformance/externalModules/rewriteRelativeImportExtensions/nodeModulesTsFiles.ts" => vec![
            (TscOwnerRole::Primary, "loadModuleFromSpecificNodeModulesDirectory"),
            (TscOwnerRole::Diagnostic, "resolveExternalModule"),
        ],
        "conformance/moduleResolution/packageJsonMain_isNonRecursive.ts" => vec![
            (TscOwnerRole::Primary, "loadNodeModuleFromDirectoryWorker"),
            (TscOwnerRole::Diagnostic, "resolveExternalModule"),
        ],
        "conformance/node/nodeModulesNoDirectoryModule.ts" => vec![
            (TscOwnerRole::Primary, "loadModuleFromSpecificNodeModulesDirectory"),
            (TscOwnerRole::Diagnostic, "checkImportDeclaration"),
        ],
        "conformance/jsdoc/importTag17.ts" => vec![
            (TscOwnerRole::Primary, "loadModuleFromImmediateNodeModulesDirectory"),
            (TscOwnerRole::Dependency, "getModeForUsageLocation"),
            (TscOwnerRole::Dependency, "loadModuleFromTargetExportOrImport"),
            (TscOwnerRole::Diagnostic, "reportRelationError"),
        ],
        "conformance/typings/typingsLookup3.ts" => vec![
            (TscOwnerRole::Primary, "resolveTypeReferenceDirective"),
            (TscOwnerRole::Diagnostic, "processTypeReferenceDirectiveWorker"),
        ],
        "conformance/moduleResolution/node10AlternateResult_noResolution.ts" => vec![
            (TscOwnerRole::Primary, "createModuleNotFoundChain"),
            (TscOwnerRole::Dependency, "nodeModuleNameResolverWorker"),
            (TscOwnerRole::Diagnostic, "resolveExternalModule"),
        ],
        "conformance/moduleResolution/resolutionModeImportType1.ts"
        | "conformance/moduleResolution/resolutionModeTypeOnlyImport1.ts" => vec![
            (TscOwnerRole::Primary, "resolveExternalModuleName"),
        ],
        "conformance/classes/members/privateNames/privateNameEmitHelpers.ts"
        | "conformance/classes/members/privateNames/privateNameStaticEmitHelpers.ts" => vec![
            (TscOwnerRole::Primary, "checkExternalEmitHelpers"),
        ],
        "conformance/externalModules/verbatimModuleSyntaxAmbientConstEnum.ts" => vec![
            (TscOwnerRole::Primary, "checkAliasSymbol"),
        ],
        "conformance/moduleResolution/untypedModuleImport_allowJs.ts"
        | "conformance/salsa/namespaceAssignmentToRequireAlias.ts" => vec![
            (TscOwnerRole::Primary, "reportNonexistentProperty"),
            (TscOwnerRole::Dependency, "resolveExternalModule"),
        ],
        "conformance/moduleResolution/untypedModuleImport_withAugmentation.ts" => vec![
            (TscOwnerRole::Primary, "resolveExternalModule"),
        ],
        _ => Vec::new(),
    };
    if owners.is_empty() {
        return owners;
    }
    let primary = owners.remove(0);
    let mut complete = vec![primary];
    if identity.code == 2688 {
        complete.push((TscOwnerRole::Dependency, "getModeForFileReference"));
        complete.push((
            TscOwnerRole::Dependency,
            "getModeForTypeReferenceDirectiveInFile",
        ));
        complete.push((TscOwnerRole::Dependency, "createModeAwareCacheKey"));
        complete.push((TscOwnerRole::Dependency, "createModeAwareCache"));
    } else {
        complete.push((TscOwnerRole::Dependency, "resolveModuleName"));
        complete.push((TscOwnerRole::Dependency, "getModeForUsageLocationWorker"));
        complete.push((TscOwnerRole::Dependency, "createModeAwareCacheKey"));
        complete.push((TscOwnerRole::Dependency, "createModeAwareCache"));
    }
    complete.extend(owners);
    complete
}

fn owner_evidence(identity: &ExactIdentity, classification: &Classification) -> String {
    if matches!(
        identity.fixture.as_str(),
        "conformance/moduleResolution/resolutionModeImportType1.ts"
            | "conformance/moduleResolution/resolutionModeTypeOnlyImport1.ts"
    ) {
        return "TypeScript 6.0.3 resolveExternalModuleName selects TS2792 directly from the effective Classic module-resolution kind; the Bundler matrix sibling is a canary, not a Node resolver call on this diagnostic path."
            .to_owned();
    }
    let chain = expected_owner_names(identity, classification)
        .into_iter()
        .map(|(role, name)| format!("{role:?}:{name}"))
        .collect::<Vec<_>>()
        .join(" -> ");
    format!("Reviewed TypeScript 6.0.3 declaration owner chain: {chain}.")
}

fn rust_boundary(
    identity: &ExactIdentity,
    classification: &Classification,
) -> ConformanceResult<RustBoundary> {
    let type_reference = classification.feature == HostFeature::TypeReferenceDirective;
    let mut anchors = vec![
        RustBoundaryAnchor {
            role: RustBoundaryRole::Producer,
            crate_name: "tsc-program".to_owned(),
            path: "crates/program/src/prepared.rs".to_owned(),
            symbol: if type_reference {
                "add_type_reference_resolution"
            } else {
                "add_module_resolution"
            }
            .to_owned(),
        },
        RustBoundaryAnchor {
            role: RustBoundaryRole::TableConsumer,
            crate_name: "tsc-program".to_owned(),
            path: "crates/program/src/prepared.rs".to_owned(),
            symbol: if type_reference {
                "require_type_reference"
            } else {
                "require_module"
            }
            .to_owned(),
        },
        RustBoundaryAnchor {
            role: RustBoundaryRole::Driver,
            crate_name: "tsc-compiler".to_owned(),
            path: "crates/compiler/src/lib.rs".to_owned(),
            symbol: "pub fn run(self)".to_owned(),
        },
    ];
    let (crate_name, path, symbol) = rust_diagnostic_consumer(identity)?;
    anchors.push(RustBoundaryAnchor {
        role: RustBoundaryRole::DiagnosticConsumer,
        crate_name: crate_name.to_owned(),
        path: path.to_owned(),
        symbol: symbol.to_owned(),
    });
    Ok(RustBoundary {
        readiness: BoundaryReadiness::SeamOnly,
        seam_anchors: anchors,
        authoritative_anchors: Vec::new(),
    })
}

fn rust_diagnostic_consumer(
    identity: &ExactIdentity,
) -> ConformanceResult<(&'static str, &'static str, &'static str)> {
    let anchor = match identity.code {
        2305 => (
            "tsc-checker",
            "crates/checker/src/modules.rs",
            "fn report_non_exported_member",
        ),
        2322 => (
            "tsc-checker",
            "crates/checker/src/engine.rs",
            "fn report_relation_error",
        ),
        2339 => (
            "tsc-checker",
            "crates/checker/src/access.rs",
            "fn report_nonexistent_property",
        ),
        2688 => (
            "tsc-compiler",
            "crates/compiler/src/lib.rs",
            "for diagnostic in preparation.program()",
        ),
        2748 => (
            "tsc-checker",
            "crates/checker/src/modules.rs",
            "fn check_alias_symbol",
        ),
        2807 => (
            "tsc-checker",
            "crates/checker/src/modules.rs",
            "fn check_external_emit_helpers",
        ),
        2882 => (
            "tsc-checker",
            "crates/checker/src/modules.rs",
            "fn check_import_declaration",
        ),
        2307 | 2665 | 2792 | 2877 => (
            "tsc-checker",
            "crates/checker/src/modules.rs",
            "fn resolve_external_module",
        ),
        other => return Err(format!("unreviewed H0 diagnostic consumer for code {other}").into()),
    };
    Ok(anchor)
}

fn same_boundary_target(left: &RustBoundary, right: &RustBoundary) -> bool {
    left.seam_anchors == right.seam_anchors
}

fn negative_canary_spec(identity: &ExactIdentity) -> ConformanceResult<NegativeCanarySpec> {
    use CanaryRelation::{
        ClosestAvailable as Closest, ExactFeature as Exact, IntentionalAlternate as Alternate,
    };

    let fixture = identity.fixture.as_str();
    let matrix = identity.matrix_key.as_str();
    let (control, control_matrix, relation, evidence) = match fixture {
        "conformance/classes/members/privateNames/privateNameEmitHelpers.ts"
        | "conformance/classes/members/privateNames/privateNameStaticEmitHelpers.ts" => (
            "conformance/es2020/modules/exportAsNamespace_missingEmitHelpers.ts",
            "",
            Closest,
            "Closest corpus control for the external-helper consumer under the same Node10 kind; it exercises helper lookup without TS2807 (the corpus has no successful sibling with the same private-helper ABI).",
        ),
        "conformance/declarationEmit/typesVersionsDeclarationEmit.multiFileBackReferenceToSelf.ts" => (
            "conformance/declarationEmit/typesVersionsDeclarationEmit.multiFileBackReferenceToUnmapped.ts",
            "",
            Exact,
            "Same typesVersions declaration back-reference surface with an unmapped, non-self target and no TS2305.",
        ),
        "conformance/externalModules/rewriteRelativeImportExtensions/nodeModulesTsFiles.ts" => (
            "conformance/node/nodeModulesPackageExports.ts",
            matrix,
            Closest,
            "Closest same-kind node_modules package traversal control without TS2877; no rewrite-plus-node_modules success sibling exists in the corpus.",
        ),
        "conformance/externalModules/rewriteRelativeImportExtensions/packageJsonImportsErrors.ts" => (
            "conformance/externalModules/rewriteRelativeImportExtensions/nonTSExtensions.ts",
            matrix,
            Exact,
            "Same rewriteRelativeImportExtensions surface and resolver kind with safe rewrites and no TS2877.",
        ),
        "conformance/externalModules/verbatimModuleSyntaxAmbientConstEnum.ts" => (
            "conformance/externalModules/verbatimModuleSyntaxConstEnumUsage.ts",
            "",
            Exact,
            "Same verbatim-module binding surface using the regular const-enum control without TS2748.",
        ),
        "conformance/jsdoc/importTag17.ts" => (
            "conformance/moduleResolution/resolvesWithoutExportsDiagnostic1.ts",
            "moduleResolution=node16",
            Exact,
            "Node16 @types conditional-exports resolution control that reaches the same package feature without TS2322.",
        ),
        "conformance/node/allowJs/nodeModulesAllowJsPackagePatternExportsExclude.ts" => (
            "conformance/node/allowJs/nodeModulesAllowJsPackagePatternExports.ts",
            matrix,
            Exact,
            "Same allowJs package-pattern surface without the './exclude' carve-out.",
        ),
        "conformance/node/nodeModulesPackagePatternExportsExclude.ts" => (
            "conformance/node/nodeModulesPackagePatternExports.ts",
            matrix,
            Exact,
            "Same package-pattern surface without the './exclude' carve-out.",
        ),
        "conformance/node/nodeModulesExportsBlocksSpecifierResolution.ts"
        | "conformance/node/nodeModulesExportsSourceTs.ts" => (
            "conformance/node/nodeModulesPackageExports.ts",
            matrix,
            Exact,
            "Same resolver kind with valid explicit package-export targets and subpaths.",
        ),
        "conformance/node/nodeModulesExportsBlocksTypesVersions.ts" => (
            "conformance/node/nodeModulesTypesVersionPackageExports.ts",
            matrix,
            Exact,
            "Same package exports plus types-version condition surface with successful version selection.",
        ),
        "conformance/node/nodeModulesExportsDoubleAsterisk.ts" => (
            "conformance/node/nodeModulesPackagePatternExportsTrailers.ts",
            matrix,
            Exact,
            "Same wildcard/trailer exports surface using a legal pattern target.",
        ),
        "conformance/node/nodeModulesExportsSpecifierGenerationPattern.ts" => (
            "conformance/node/nodeModulesPackagePatternExports.ts",
            matrix,
            Exact,
            "Same package-pattern export resolution with a successful generated specifier.",
        ),
        "conformance/node/nodeModulesExportsSpecifierGenerationDirectory.ts" => (
            "conformance/node/nodeModulesDeclarationEmitWithPackageExports.ts",
            matrix,
            Exact,
            "Same declaration/specifier-generation surface with explicit successful export entries.",
        ),
        "conformance/node/nodeModulesExportsSpecifierGenerationConditions.ts" => (
            "conformance/node/nodeModulesConditionalPackageExports.ts",
            matrix,
            Exact,
            "Same conditional package-exports selection with a successful matching branch.",
        ),
        "conformance/moduleResolution/bundler/bundlerCommonJS.ts" => (
            "conformance/moduleResolution/bundler/bundlerConditionsExcludesNode.ts",
            "module=preserve",
            Exact,
            "Same Bundler conditional-exports selection with a successful non-node branch.",
        ),
        "conformance/moduleResolution/conditionalExportsResolutionFallbackNull.ts" => (
            "conformance/moduleResolution/conditionalExportsResolutionFallback.ts",
            matrix,
            Exact,
            "Same explicit resolver mode with a non-null successful conditional fallback.",
        ),
        "conformance/node/nodeModulesImportResolutionIntoExport.ts"
        | "conformance/node/nodeModulesImportResolutionNoCycle.ts" => (
            "conformance/node/nodePackageSelfName.ts",
            matrix,
            Exact,
            "Same self-name package resolution surface without an imports/exports cycle failure.",
        ),
        "conformance/node/nodeModulesPackageImportsRootWildcardNode16.ts" => (
            "conformance/node/nodeModulesPackageImports.ts",
            "module=node16",
            Closest,
            "Closest Node16 package-imports success control; the corpus's exact root-wildcard success sibling is NodeNext only.",
        ),
        "conformance/moduleResolution/packageJsonMain_isNonRecursive.ts" => (
            "conformance/moduleResolution/packageJsonMain.ts",
            "",
            Exact,
            "Same package.json main lookup with a successful non-recursive target.",
        ),
        "conformance/node/nodeModulesNoDirectoryModule.ts" => (
            "conformance/node/nodeModulesPackageExports.ts",
            "module=node16",
            Closest,
            "Closest Node16 package-subpath success control; this is the corpus's only noUncheckedSideEffectImports fixture, so an exact TS2882-negative sibling does not exist.",
        ),
        "conformance/moduleResolution/node10AlternateResult_noResolution.ts" => (
            "conformance/moduleResolution/node10Alternateresult_noTypes.ts",
            "",
            Exact,
            "Same Node10 alternate-result branch with a package target that does not emit TS2307.",
        ),
        "conformance/moduleResolution/resolutionModeImportType1.ts"
        | "conformance/moduleResolution/resolutionModeTypeOnlyImport1.ts" => (
            fixture,
            "moduleResolution=bundler",
            Alternate,
            "Intentional same-source Classic-to-Bundler contrast: Bundler resolves the request and therefore omits TS2792.",
        ),
        "conformance/moduleResolution/untypedModuleImport_allowJs.ts"
        | "conformance/salsa/namespaceAssignmentToRequireAlias.ts" => (
            "conformance/moduleResolution/untypedModuleImport.ts",
            "",
            Exact,
            "Same Node10 untyped-package consumer with no frozen host-dependent member error.",
        ),
        "conformance/moduleResolution/untypedModuleImport_withAugmentation.ts" => (
            "conformance/moduleResolution/untypedModuleImport_vsAmbient.ts",
            "",
            Exact,
            "Same untyped package plus ambient-module contrast without TS2665.",
        ),
        "conformance/typings/typingsLookup3.ts" => (
            "conformance/typings/typingsLookup1.ts",
            "",
            Exact,
            "Adjacent successful type-reference lookup without TS2688.",
        ),
        other => {
            return Err(format!("H0 row has no reviewed negative canary mapping: {other}").into())
        }
    };
    Ok(NegativeCanarySpec {
        fixture: control.to_owned(),
        matrix_key: control_matrix.to_owned(),
        relation,
        evidence: evidence.to_owned(),
    })
}

fn owner_anchor(
    inventory: &D2Inventory,
    role: TscOwnerRole,
    name: &str,
) -> ConformanceResult<TscOwnerAnchor> {
    let mut matches = inventory
        .functions
        .iter()
        .filter(|function| function.name == name);
    let function = matches
        .next()
        .ok_or_else(|| format!("D2 inventory has no declaration named {name}"))?;
    if matches.next().is_some() {
        return Err(format!("D2 inventory declaration name {name} is ambiguous").into());
    }
    Ok(TscOwnerAnchor {
        role,
        declaration: function.id.clone(),
        name: function.name.clone(),
        kind: function.kind.clone(),
        lexical_path: function.lexical_path.clone(),
        source_range: function.source_range.clone(),
        source_slice_sha256: function.source_slice_sha256.clone(),
    })
}

fn read_inventory(workspace: &Path) -> ConformanceResult<D2Inventory> {
    let path = workspace.join(D2_INVENTORY_REL_PATH);
    let bytes = fs::read(&path)?;
    let inventory: D2Inventory = serde_json::from_slice(&bytes)?;
    if inventory.schema != 2
        || inventory.status != "draft/report-only"
        || inventory.typescript_version != TYPESCRIPT_VERSION
        || inventory.source != D2_SOURCE_REL_PATH
        || inventory.source_sha256 != sha256_hex(&fs::read(workspace.join(D2_SOURCE_REL_PATH))?)
    {
        return Err("H0 owner registry requires the fresh TypeScript 6.0.3 D2 inventory".into());
    }
    let source = fs::read_to_string(workspace.join(D2_SOURCE_REL_PATH))?;
    let lines = source.split_inclusive('\n').collect::<Vec<_>>();
    for function in inventory
        .functions
        .iter()
        .filter(|function| is_registry_owner_name(&function.name))
    {
        let start = function.source_range.start.line;
        let end = function.source_range.end.line;
        if start == 0 || end < start || start > lines.len() {
            return Err(
                format!("D2 declaration {} has an invalid source span", function.id).into(),
            );
        }
        // The inventory generator uses Array#slice, whose end clamps at the
        // source-line count.  A SourceFile declaration may end at the virtual
        // line immediately after a trailing newline; selected function owners
        // normally end in-bounds, but mirror the producer exactly here.
        let actual = sha256_hex(lines[start - 1..end.min(lines.len())].concat().as_bytes());
        if actual != function.source_slice_sha256 {
            return Err(format!("D2 declaration {} source hash is stale", function.id).into());
        }
    }
    Ok(inventory)
}

fn is_registry_owner_name(name: &str) -> bool {
    matches!(
        name,
        "resolveModuleName"
            | "getModeForUsageLocationWorker"
            | "getModeForFileReference"
            | "getModeForTypeReferenceDirectiveInFile"
            | "createModeAwareCacheKey"
            | "createModeAwareCache"
            | "loadModuleFromExports"
            | "loadModuleFromImports"
            | "loadModuleFromSelfNameReference"
            | "loadModuleFromExportsOrImports"
            | "loadModuleFromTargetExportOrImport"
            | "loadNodeModuleFromDirectoryWorker"
            | "loadModuleFromSpecificNodeModulesDirectory"
            | "loadModuleFromImmediateNodeModulesDirectory"
            | "resolveTypeReferenceDirective"
            | "processTypeReferenceDirectiveWorker"
            | "resolveExternalModuleName"
            | "resolveExternalModule"
            | "createModuleNotFoundChain"
            | "reportNonExportedMember"
            | "checkExternalEmitHelpers"
            | "checkAliasSymbol"
            | "reportRelationError"
            | "reportNonexistentProperty"
            | "checkImportDeclaration"
            | "getModeForUsageLocation"
            | "nodeModuleNameResolverWorker"
    )
}

fn read_oracle_inputs(workspace: &Path) -> ConformanceResult<OracleInputsArtifact> {
    let bytes = fs::read(workspace.join(ORACLE_INPUTS_REL_PATH))?;
    let json = zstd::stream::decode_all(bytes.as_slice())?;
    let inputs: OracleInputsArtifact = serde_json::from_slice(&json)?;
    if inputs.schema != 1 {
        return Err("H0 host canaries require oracle-inputs schema 1".into());
    }
    Ok(inputs)
}

fn case_program_sha256(
    inputs: &OracleInputsArtifact,
    fixture: &str,
    matrix_key: &str,
) -> ConformanceResult<String> {
    let fixture_pins = inputs
        .fixtures
        .get(fixture)
        .ok_or_else(|| format!("H0 canary fixture {fixture} is absent from oracle inputs"))?;
    let case = fixture_pins
        .cases
        .get(matrix_key)
        .ok_or_else(|| format!("H0 canary fixture {fixture} has no matrix case {matrix_key:?}"))?;
    if !valid_sha256(&case.program_sha256) {
        return Err(
            format!("H0 canary {fixture} [{matrix_key}] has an invalid program pin").into(),
        );
    }
    Ok(case.program_sha256.clone())
}

fn load_program_facts(
    workspace: &Path,
    inputs: &OracleInputsArtifact,
    cases: &BTreeSet<(String, String)>,
) -> ConformanceResult<ProgramFactIndex> {
    let mut by_fixture = BTreeMap::<String, BTreeSet<String>>::new();
    for (fixture, matrix_key) in cases {
        if !safe_relative_path(fixture) || !inputs.fixtures.contains_key(fixture) {
            return Err(
                format!("H0 canary fixture {fixture:?} is not a pinned safe corpus path").into(),
            );
        }
        by_fixture
            .entry(fixture.clone())
            .or_default()
            .insert(matrix_key.clone());
    }

    let corpus_root = workspace.join("ts-tests/tests/cases");
    let lib_dir = workspace.join("vendor/typescript-6.0.3/lib");
    let mut facts = ProgramFactIndex::new();
    for (fixture, wanted_matrices) in by_fixture {
        let path = corpus_root.join(&fixture);
        let programs = tsc_harness::expand_fixture_file(&path, &lib_dir).map_err(|err| {
            format!(
                "failed to expand H0 canary fixture {} from ProgramJson: {err}",
                path.display()
            )
        })?;
        for program in programs {
            if !wanted_matrices.contains(&program.matrix_key) {
                continue;
            }
            let program_json = program.to_json();
            let program_sha256 = sha256_hex(program_json.as_bytes());
            let pinned = case_program_sha256(inputs, &fixture, &program.matrix_key)?;
            if program_sha256 != pinned {
                return Err(format!(
                    "H0 canary {fixture} [{}] expanded ProgramJson hash drifted from oracle inputs",
                    program.matrix_key
                )
                .into());
            }
            let computed =
                tsc_harness::compiler_options_from_program(&program).emit_module_resolution_kind();
            let module_resolution_kind = match computed {
                1 => ModuleResolutionKind::Classic,
                2 => ModuleResolutionKind::Node10,
                3 => ModuleResolutionKind::Node16,
                99 => ModuleResolutionKind::NodeNext,
                100 => ModuleResolutionKind::Bundler,
                other => {
                    return Err(format!(
                        "H0 canary {fixture} [{}] computed unsupported TypeScript 6.0.3 module resolution kind {other}",
                        program.matrix_key
                    )
                    .into())
                }
            };
            let key = (fixture.clone(), program.matrix_key.clone());
            if facts
                .insert(
                    key.clone(),
                    ProgramFact {
                        program_sha256,
                        module_resolution_kind,
                        program_json,
                    },
                )
                .is_some()
            {
                return Err(format!(
                    "H0 canary fixture {} repeats matrix case {:?}",
                    key.0, key.1
                )
                .into());
            }
        }
        if let Some(missing) = wanted_matrices
            .iter()
            .find(|matrix| !facts.contains_key(&(fixture.clone(), (*matrix).clone())))
        {
            return Err(format!(
                "H0 canary fixture {fixture} has no expanded ProgramJson matrix case {missing:?}"
            )
            .into());
        }
    }
    Ok(facts)
}

fn program_fact<'a>(
    facts: &'a ProgramFactIndex,
    fixture: &str,
    matrix_key: &str,
) -> ConformanceResult<&'a ProgramFact> {
    facts
        .get(&(fixture.to_owned(), matrix_key.to_owned()))
        .ok_or_else(|| {
            format!("H0 canary program fact is missing for {fixture} [{matrix_key}]").into()
        })
}

fn load_resolution_requests(
    workspace: &Path,
    seeds: &[(String, ExactIdentity)],
    program_facts: &ProgramFactIndex,
) -> ConformanceResult<ResolutionRequestIndex> {
    let seed_by_id = seeds
        .iter()
        .map(|(id, identity)| (id.as_str(), identity))
        .collect::<BTreeMap<_, _>>();
    if seed_by_id.len() != seeds.len() {
        return Err("H0 request producer input repeats a registry row id".into());
    }
    let mut by_case = BTreeMap::<(String, String), Vec<(&str, &ExactIdentity)>>::new();
    for (id, identity) in seeds {
        by_case
            .entry((identity.fixture.clone(), identity.matrix_key.clone()))
            .or_default()
            .push((id, identity));
    }

    let mut requests = Vec::with_capacity(by_case.len() + 1);
    requests.push(serde_json::json!({
        "id": "version-probe",
        "versionProbe": true,
    }));
    let mut case_ids = BTreeMap::new();
    for (index, ((fixture, matrix_key), identities)) in by_case.iter().enumerate() {
        let case_id = format!("case-{index}");
        let program: serde_json::Value =
            serde_json::from_str(&program_fact(program_facts, fixture, matrix_key)?.program_json)?;
        requests.push(serde_json::json!({
            "id": case_id,
            "programJson": program,
            "identities": identities
                .iter()
                .map(|(id, identity)| serde_json::json!({ "id": id, "identity": identity }))
                .collect::<Vec<_>>(),
        }));
        case_ids.insert(case_id, (fixture, matrix_key));
    }

    let script = workspace.join(REQUEST_PRODUCER_REL_PATH);
    let mut child = Command::new("node")
        .arg("--single-threaded")
        .arg(&script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| {
            format!(
                "failed to launch H0 request producer {}: {err}",
                script.display()
            )
        })?;
    let stdin = child
        .stdin
        .take()
        .ok_or("H0 request producer did not expose its piped stdin")?;
    let mut input = Vec::new();
    for request in &requests {
        input.extend(serde_json::to_vec(request)?);
        input.push(b'\n');
    }
    // Drain stdout/stderr while feeding the batch. Waiting to read until the
    // entire ProgramJson stream has been written can deadlock once either OS
    // pipe fills: Node then waits for its response pipe while Rust waits for
    // its request pipe. A scoped writer keeps the producer single-threaded
    // while allowing `wait_with_output` to drain both output pipes.
    let (output, write_result) = std::thread::scope(|scope| {
        let writer = scope.spawn(move || {
            let mut stdin = stdin;
            stdin.write_all(&input)
        });
        let output = child.wait_with_output();
        (output, writer.join())
    });
    let output = output?;
    let write_result = write_result.map_err(|_| "H0 request producer stdin writer panicked")?;
    if !output.status.success() {
        return Err(format!(
            "H0 request producer failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    write_result.map_err(|err| format!("H0 request producer stdin write failed: {err}"))?;

    let mut launched_version = None;
    let mut resolved = ResolutionRequestIndex::new();
    let stdout = String::from_utf8(output.stdout)?;
    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let response: RequestProducerResponse = serde_json::from_str(line).map_err(|err| {
            format!("H0 request producer returned invalid JSONL response: {err}: {line}")
        })?;
        if let Some(error) = response.error {
            return Err(format!("H0 request producer {} failed: {error}", response.id).into());
        }
        if response.id == "version-probe" {
            if response.identities.is_some()
                || launched_version
                    .replace(response.version.ok_or(
                        "H0 request producer version probe omitted the launched Node version",
                    )?)
                    .is_some()
            {
                return Err(
                    "H0 request producer returned a duplicate malformed version probe".into(),
                );
            }
            continue;
        }
        if response.version.is_some() || !case_ids.contains_key(&response.id) {
            return Err(format!(
                "H0 request producer returned unexpected response id {}",
                response.id
            )
            .into());
        }
        let identities = response.identities.ok_or_else(|| {
            format!(
                "H0 request producer case {} omitted identity results",
                response.id
            )
        })?;
        for identity in identities {
            let seed = seed_by_id.get(identity.id.as_str()).ok_or_else(|| {
                format!(
                    "H0 request producer returned unknown registry row {}",
                    identity.id
                )
            })?;
            validate_resolution_requests(&identity.id, seed, &identity.requests)?;
            if resolved
                .insert(identity.id.clone(), identity.requests)
                .is_some()
            {
                return Err(
                    format!("H0 request producer repeated registry row {}", identity.id).into(),
                );
            }
        }
    }

    let launched = normalize_node_version(
        launched_version
            .as_deref()
            .ok_or("H0 request producer returned no Node version probe")?,
    );
    let pinned = pinned_node_version(workspace)?;
    if launched != pinned {
        return Err(format!(
            "H0 request producer launched Node v{launched}, but .node-version pins v{pinned}"
        )
        .into());
    }
    let expected_ids = seeds.iter().map(|(id, _)| id).collect::<BTreeSet<_>>();
    let actual_ids = resolved.keys().collect::<BTreeSet<_>>();
    if actual_ids != expected_ids {
        return Err("H0 request producer did not return the exact frozen row universe".into());
    }
    validate_expected_request_mode_counts(&resolved)?;
    Ok(resolved)
}

fn validate_resolution_requests(
    id: &str,
    identity: &ExactIdentity,
    requests: &[ResolutionRequest],
) -> ConformanceResult<()> {
    let expected_len = if identity.code == 2305 { 2 } else { 1 };
    if requests.len() != expected_len {
        return Err(format!("H0 registry row {id} has an invalid resolution request chain").into());
    }
    for request in requests {
        if !request.canonical_source.starts_with('/')
            || request.canonical_source.contains('\\')
            || request.specifier.is_empty()
            || (request.synthetic
                != (request.anchor_kind == ResolutionAnchorKind::SyntheticImportHelpers))
            || (request.kind == ResolutionRequestKind::TypeReference
                && (request.mode != RequestResolutionMode::Unspecified
                    || request.anchor_kind != ResolutionAnchorKind::TypeReferenceDirective))
            || (identity.code == 2688) != (request.kind == ResolutionRequestKind::TypeReference)
        {
            return Err(format!(
                "H0 registry row {id} has a malformed vendored resolution request"
            )
            .into());
        }
    }
    Ok(())
}

fn validate_expected_request_mode_counts(
    requests: &ResolutionRequestIndex,
) -> ConformanceResult<()> {
    let mut actual = BTreeMap::new();
    for row_requests in requests.values() {
        *actual.entry(row_requests[0].mode).or_insert(0usize) += 1;
    }
    let expected = BTreeMap::from([
        (RequestResolutionMode::CommonJs, 100),
        (RequestResolutionMode::EsNext, 137),
        (RequestResolutionMode::Unspecified, 4),
    ]);
    if actual != expected {
        return Err(format!(
            "H0 TypeScript 6.0.3 request-resolution mode census drifted: {actual:?}"
        )
        .into());
    }
    Ok(())
}

fn recorded_resolution_requests(rows: &[RegistryRow]) -> ConformanceResult<ResolutionRequestIndex> {
    let mut requests = ResolutionRequestIndex::new();
    for row in rows {
        validate_resolution_requests(&row.id, &row.identity, &row.resolution_requests)?;
        if requests
            .insert(row.id.clone(), row.resolution_requests.clone())
            .is_some()
        {
            return Err(format!("H0 registry repeats request evidence for row {}", row.id).into());
        }
    }
    validate_expected_request_mode_counts(&requests)?;
    Ok(requests)
}

fn oracle_identity_index(
    workspace: &Path,
    rows: &[RegistryRow],
) -> ConformanceResult<OraclePositionIndex> {
    let cases = rows
        .iter()
        .map(|row| {
            (
                row.identity.fixture.clone(),
                row.identity.matrix_key.clone(),
            )
        })
        .collect::<BTreeSet<_>>();
    let mut by_fixture = BTreeMap::<String, BTreeSet<String>>::new();
    for (fixture, matrix) in cases {
        by_fixture.entry(fixture).or_default().insert(matrix);
    }
    let mut index = BTreeMap::new();
    for (fixture, matrices) in by_fixture {
        let golden = read_golden(&workspace.join("goldens"), &fixture)?;
        for matrix in matrices {
            let case = golden
                .cases
                .iter()
                .find(|case| case.matrix_key == matrix)
                .ok_or_else(|| format!("H0 emitting canary {fixture} has no matrix {matrix:?}"))?;
            let identities = assign_case_identities(&fixture, &matrix, &case.oracle)?;
            for (identity, diagnostic) in identities.into_iter().zip(&case.oracle) {
                if index
                    .insert(identity.clone(), (diagnostic.line, diagnostic.col))
                    .is_some()
                {
                    return Err(format!(
                        "duplicate oracle identity while indexing H0 canaries: {}",
                        identity.label()
                    )
                    .into());
                }
            }
        }
    }
    Ok(index)
}

fn oracle_case_code_index(
    workspace: &Path,
    rows: &[RegistryRow],
) -> ConformanceResult<OracleCaseCodeIndex> {
    let cases = rows
        .iter()
        .map(|row| {
            (
                row.canaries.non_emitting_control.fixture.clone(),
                row.canaries.non_emitting_control.matrix_key.clone(),
            )
        })
        .collect::<BTreeSet<_>>();
    let mut by_fixture = BTreeMap::<String, BTreeSet<String>>::new();
    for (fixture, matrix) in cases {
        by_fixture.entry(fixture).or_default().insert(matrix);
    }
    let mut index = BTreeMap::new();
    for (fixture, matrices) in by_fixture {
        let golden = read_golden(&workspace.join("goldens"), &fixture)?;
        for matrix in matrices {
            let case = golden
                .cases
                .iter()
                .find(|case| case.matrix_key == matrix)
                .ok_or_else(|| {
                    format!("H0 non-emitting canary {fixture} has no matrix {matrix:?}")
                })?;
            index.insert(
                (fixture.clone(), matrix),
                case.oracle
                    .iter()
                    .map(|diagnostic| diagnostic.code)
                    .collect(),
            );
        }
    }
    Ok(index)
}

fn expected_families() -> Vec<OwnerFamily> {
    [
        (
            FAMILY_EXPORTS,
            "H0.2",
            "MemoryCompilerHost module resolver",
            "Package exports patterns, precedence, blocked and null subpaths.",
        ),
        (
            FAMILY_IMPORTS,
            "H0.2/H0.3",
            "MemoryCompilerHost module resolver",
            "Package imports, self-name references, condition selection, and residual rewrite consumers.",
        ),
        (
            FAMILY_NODE_MODULES,
            "H0.2/H0.3",
            "MemoryCompilerHost module resolver",
            "node_modules traversal, package main/types, typesVersions, and residual module-member consumers.",
        ),
        (
            FAMILY_TYPES,
            "H0.2/H0.3",
            "MemoryCompilerHost type-reference resolver",
            "@types, typeRoots, and type-reference directives.",
        ),
        (
            FAMILY_MODE,
            "H0.2/H0.3",
            "resolution-mode diagnostic selector",
            "Classic/Node/Bundler alternate and package-mode message selection.",
        ),
        (
            FAMILY_CONSUMERS,
            "H0.3",
            "checker host-fact consumers",
            "External helpers, untyped packages, module members, const enums, and rewrite consumers.",
        ),
        (
            FAMILY_PROGRAM,
            "H0.4",
            "program and filesystem host",
            "Program discovery, canonical paths, case collisions, and references.",
        ),
        (
            FAMILY_CLI,
            "H0.5",
            "no-emit command driver",
            "Config, option, batch-driver, renderer, and exit-status owners.",
        ),
    ]
    .into_iter()
    .map(|(id, phase, owner, description)| OwnerFamily {
        id: id.to_owned(),
        phase: phase.to_owned(),
        owner: owner.to_owned(),
        description: description.to_owned(),
    })
    .collect()
}

fn initial_profiles() -> Vec<InitialProfile> {
    vec![
        InitialProfile {
            id: "dev-macos-arm64".to_owned(),
            workload: "full conformance performance producer, cache-on plus 8-fixture cache-off smoke".to_owned(),
            os: "macos".to_owned(),
            arch: "aarch64".to_owned(),
            measurement_backend: "bsd-time-l".to_owned(),
            cpu: CpuProfile {
                policy: "Apple Silicon, at least 8 logical cores, no oversubscription".to_owned(),
                logical_cores: 10,
                cargo_build_jobs: 2,
                rust_test_threads: 2,
                oversubscribed: false,
            },
            wall_seconds: 23.068379041,
            max_rss_bytes: 2_452_488_192,
            cache_off_smoke: CacheOffProfile {
                fixture_limit: 8,
                wall_seconds: 1.6145694590000002,
                max_rss_bytes: 429_015_040,
            },
            ceilings: ResourceCeilings {
                wall_seconds: 60.0,
                max_rss_bytes: 8_589_934_592,
            },
            provenance: ProfileProvenance {
                producer_commit: "a9c1fd84c8299a8fc6ef8bb669fb3033842a22fd".to_owned(),
                command: "cargo xtask perf conformance --runner-profile dev-macos-arm64".to_owned(),
                evidence: "m8 performance fingerprint 78f1ac7e8e870134620660ffb0e94f71780ee4e82a82259f1cc40d3a51fbb4b9".to_owned(),
            },
        },
        InitialProfile {
            id: "github-ubuntu-x64-standard".to_owned(),
            workload: "full conformance performance producer, cache-on plus 8-fixture cache-off smoke".to_owned(),
            os: "linux".to_owned(),
            arch: "x86_64".to_owned(),
            measurement_backend: "gnu-time-v".to_owned(),
            cpu: CpuProfile {
                policy: "GitHub-hosted ubuntu standard runner, at least 4 logical cores".to_owned(),
                logical_cores: 4,
                cargo_build_jobs: 2,
                rust_test_threads: 2,
                oversubscribed: false,
            },
            wall_seconds: 49.517,
            max_rss_bytes: 2_465_595_392,
            cache_off_smoke: CacheOffProfile {
                fixture_limit: 8,
                wall_seconds: 3.244,
                max_rss_bytes: 167_243_776,
            },
            ceilings: ResourceCeilings {
                wall_seconds: 60.0,
                max_rss_bytes: 8_589_934_592,
            },
            provenance: ProfileProvenance {
                producer_commit: "2fb19c2dd9cfe4dbba2a26021206daf54ad64e8c".to_owned(),
                command: "cargo xtask perf conformance --runner-profile github-ubuntu-x64-standard".to_owned(),
                evidence: "GitHub Actions run 30732562355 (2026-08-02)".to_owned(),
            },
        },
    ]
}

fn validate_profiles(
    workspace: &Path,
    profiles: &[InitialProfile],
    verify_history: bool,
) -> ConformanceResult<()> {
    if profiles != initial_profiles() {
        return Err("H0 initial CPU/wall/RSS profiles drifted from the reviewed bootstrap".into());
    }
    for profile in profiles {
        if !profile.wall_seconds.is_finite()
            || profile.wall_seconds <= 0.0
            || !profile.cache_off_smoke.wall_seconds.is_finite()
            || profile.cache_off_smoke.wall_seconds <= 0.0
            || profile.max_rss_bytes == 0
            || profile.cache_off_smoke.max_rss_bytes == 0
            || profile.ceilings.wall_seconds < profile.wall_seconds
            || profile.ceilings.max_rss_bytes < profile.max_rss_bytes
            || profile.cpu.logical_cores == 0
            || profile.cpu.cargo_build_jobs > profile.cpu.logical_cores
            || profile.cpu.rust_test_threads > profile.cpu.logical_cores
            || profile.cpu.oversubscribed
            || !valid_commit(&profile.provenance.producer_commit)
        {
            return Err(format!("malformed H0 initial resource profile {}", profile.id).into());
        }
        if verify_history
            && !git_is_ancestor(workspace, &profile.provenance.producer_commit, "HEAD")?
        {
            return Err(format!(
                "H0 resource profile {} producer commit is not reachable from HEAD",
                profile.id
            )
            .into());
        }
    }
    Ok(())
}

fn summarize(rows: &[RegistryRow]) -> RegistrySummary {
    let mut by_code = BTreeMap::new();
    let mut by_family = expected_families()
        .into_iter()
        .map(|family| (family.id, 0usize))
        .collect::<BTreeMap<_, _>>();
    let mut fixtures = BTreeSet::new();
    let mut open = 0;
    let mut lapsed = 0;
    for row in rows {
        fixtures.insert(row.identity.fixture.as_str());
        *by_code.entry(row.identity.code.to_string()).or_insert(0) += 1;
        *by_family.entry(row.family.clone()).or_insert(0) += 1;
        open += usize::from(row.status == RowStatus::Open);
        lapsed += usize::from(row.status == RowStatus::Lapsed);
    }
    RegistrySummary {
        rows: rows.len(),
        open,
        closed: rows.len() - open - lapsed,
        lapsed,
        fixtures: fixtures.len(),
        by_code,
        by_family,
    }
}

fn validate_expected_code_counts<'a>(
    identities: impl Iterator<Item = &'a ExactIdentity>,
) -> ConformanceResult<()> {
    let mut actual = BTreeMap::new();
    for identity in identities {
        *actual.entry(identity.code).or_insert(0usize) += 1;
    }
    let expected = BTreeMap::from([
        (2305, 1),
        (2307, 214),
        (2322, 2),
        (2339, 3),
        (2665, 1),
        (2688, 1),
        (2748, 2),
        (2792, 6),
        (2807, 4),
        (2877, 6),
        (2882, 1),
    ]);
    if actual != expected {
        return Err(format!("H0 host-resolution code census drifted: {actual:?}").into());
    }
    Ok(())
}

fn validate_expected_module_resolution_counts(rows: &[RegistryRow]) -> ConformanceResult<()> {
    let mut actual = BTreeMap::new();
    for row in rows {
        *actual.entry(row.module_resolution_kind).or_insert(0usize) += 1;
    }
    let expected = BTreeMap::from([
        (ModuleResolutionKind::Classic, 6),
        (ModuleResolutionKind::Node10, 1),
        (ModuleResolutionKind::Node16, 164),
        (ModuleResolutionKind::NodeNext, 55),
        (ModuleResolutionKind::Bundler, 15),
    ]);
    if actual != expected {
        return Err(format!(
            "H0 TypeScript 6.0.3 computed module-resolution census drifted: {actual:?}"
        )
        .into());
    }
    Ok(())
}

fn projection_sha256<'a>(identities: impl Iterator<Item = &'a ExactIdentity>) -> String {
    let mut identities = identities.collect::<Vec<_>>();
    identities.sort();
    let mut bytes = Vec::new();
    for identity in identities {
        bytes.extend(identity.canonical_bytes());
        bytes.push(b'\n');
    }
    sha256_hex(&bytes)
}

fn row_seed_projection_sha256(rows: &[RegistryRow]) -> String {
    seed_projection_sha256(rows.iter().map(|row| {
        (
            &row.identity,
            row.line,
            row.col,
            row.source_evidence.as_str(),
        )
    }))
}

fn scope_seed_projection_sha256(scope: &HostResolutionScopeState) -> String {
    seed_projection_sha256(
        scope
            .live
            .iter()
            .map(|row| (&row.identity, row.line, row.col, row.evidence.as_str())),
    )
}

fn seed_projection_sha256<'a>(
    rows: impl Iterator<Item = (&'a ExactIdentity, Option<u32>, Option<u32>, &'a str)>,
) -> String {
    let mut rows = rows.collect::<Vec<_>>();
    rows.sort_by(|left, right| left.0.cmp(right.0));
    let mut bytes = Vec::new();
    for (identity, line, col, evidence) in rows {
        bytes.extend(identity.canonical_bytes());
        bytes.push(0);
        bytes.extend(
            line.map_or_else(|| "null".to_owned(), |value| value.to_string())
                .as_bytes(),
        );
        bytes.push(0);
        bytes.extend(
            col.map_or_else(|| "null".to_owned(), |value| value.to_string())
                .as_bytes(),
        );
        bytes.push(0);
        bytes.extend(evidence.len().to_string().as_bytes());
        bytes.push(b':');
        bytes.extend(evidence.as_bytes());
        bytes.push(b'\n');
    }
    sha256_hex(&bytes)
}

fn fixture_count<'a>(identities: impl Iterator<Item = &'a ExactIdentity>) -> usize {
    identities
        .map(|identity| identity.fixture.as_str())
        .collect::<BTreeSet<_>>()
        .len()
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn safe_relative_path(value: &str) -> bool {
    let path = Path::new(value);
    !value.is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

fn git_resolve_commit(workspace: &Path, revision: &str) -> ConformanceResult<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(["rev-parse", "--verify", &format!("{revision}^{{commit}}")])
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "cannot resolve host-resolution baseline {revision}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn git_is_ancestor(workspace: &Path, ancestor: &str, descendant: &str) -> ConformanceResult<bool> {
    let status = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .status()?;
    match status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err("git merge-base failed while checking an H0 closing commit".into()),
    }
}

fn workspace_history_rel(
    git_root: &Path,
    workspace: &Path,
    rel: &str,
) -> ConformanceResult<String> {
    let git_root = fs::canonicalize(git_root)?;
    let workspace = fs::canonicalize(workspace)?;
    let path = workspace.join(rel);
    let relative = path.strip_prefix(&git_root).map_err(|_| {
        format!(
            "workspace path {} is outside git root {}",
            path.display(),
            git_root.display()
        )
    })?;
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_git::{git_test, init_repo};
    use std::path::PathBuf;

    fn workspace() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn committed_registry(workspace: &Path) -> RegistryFile {
        let path = workspace.join(HOST_RESOLUTION_REL_PATH);
        parse_registry(&fs::read(&path).unwrap(), &path.display().to_string()).unwrap()
    }

    fn commit_bytes(root: &Path, rel: &str, bytes: &[u8], message: &str) -> String {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
        git_test(root, &["add", rel]);
        git_test(root, &["commit", "-q", "-m", message]);
        String::from_utf8(
            Command::new("git")
                .arg("-C")
                .arg(root)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_owned()
    }

    #[test]
    fn expected_owner_families_are_complete_and_ordered() {
        let families = expected_families();
        assert_eq!(families.len(), 8);
        assert_eq!(families[0].id, FAMILY_EXPORTS);
        assert_eq!(families[7].id, FAMILY_CLI);
        assert_eq!(
            families
                .iter()
                .map(|family| family.id.as_str())
                .collect::<BTreeSet<_>>()
                .len(),
            8
        );
    }

    #[test]
    fn row_id_uses_the_exact_occurrence_identity() {
        let mut identity = ExactIdentity {
            fixture: "conformance/example.ts".to_owned(),
            matrix_key: String::new(),
            pass: "semantic".to_owned(),
            file: Some("/a.ts".to_owned()),
            start: Some(1),
            length: Some(2),
            code: 2307,
            category: "Error".to_owned(),
            chain_sha256: "a".repeat(64),
            related_sha256: "b".repeat(64),
            occurrence: 0,
        };
        let first = format!("h0:{}", identity.sha256());
        identity.occurrence = 1;
        assert_ne!(first, format!("h0:{}", identity.sha256()));
    }

    #[test]
    fn closure_shape_is_fail_closed() {
        assert!(safe_relative_path("ratchets/h0/evidence.json"));
        assert!(!safe_relative_path("../evidence.json"));
        assert!(!safe_relative_path("/tmp/evidence.json"));
        assert!(valid_commit(&"a".repeat(40)));
        assert!(!valid_commit(&"A".repeat(40)));
    }

    #[test]
    fn committed_registry_passes_full_owner_and_canary_validation() {
        let workspace = workspace();
        let scope = host_resolution_state(&workspace.join(SCOPE_REL_PATH)).unwrap();
        let inventory = read_inventory(&workspace).unwrap();
        let inputs = read_oracle_inputs(&workspace).unwrap();
        validate_registry(
            &workspace,
            &committed_registry(&workspace),
            &scope,
            &inventory,
            &inputs,
            false,
            false,
        )
        .unwrap();
    }

    #[test]
    fn strict_schema_rejects_unreviewed_fields() {
        let workspace = workspace();
        let bytes = fs::read(workspace.join(HOST_RESOLUTION_REL_PATH)).unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        value["unreviewed"] = serde_json::json!(true);
        let error = parse_registry(&serde_json::to_vec(&value).unwrap(), "mutation")
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown field `unreviewed`"), "{error}");

        let mut nested: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        nested["rows"][0]["identity"]["unreviewed"] = serde_json::json!(true);
        let error = parse_registry(&serde_json::to_vec(&nested).unwrap(), "nested-mutation")
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown field `unreviewed`"), "{error}");
    }

    #[test]
    fn committed_mode_and_canary_censuses_are_exact() {
        let workspace = workspace();
        let registry = committed_registry(&workspace);
        validate_expected_module_resolution_counts(&registry.rows).unwrap();
        validate_expected_request_mode_counts(
            &recorded_resolution_requests(&registry.rows).unwrap(),
        )
        .unwrap();
        let relations = registry
            .rows
            .iter()
            .fold(BTreeMap::new(), |mut counts, row| {
                *counts
                    .entry(row.canaries.non_emitting_control.relation)
                    .or_insert(0usize) += 1;
                counts
            });
        assert_eq!(
            relations,
            BTreeMap::from([
                (CanaryRelation::ExactFeature, 226),
                (CanaryRelation::ClosestAvailable, 9),
                (CanaryRelation::IntentionalAlternate, 6),
            ])
        );
    }

    #[test]
    fn registry_rejects_a_draft_current_scope() {
        let workspace = workspace();
        let mut scope = host_resolution_state(&workspace.join(SCOPE_REL_PATH)).unwrap();
        scope.frozen = false;
        let error = validate_registry(
            &workspace,
            &committed_registry(&workspace),
            &scope,
            &read_inventory(&workspace).unwrap(),
            &read_oracle_inputs(&workspace).unwrap(),
            false,
            false,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("current M8 scope manifest"), "{error}");
    }

    #[test]
    fn trusted_baseline_must_be_on_the_head_ancestry() {
        let workspace = workspace();
        let repo = init_repo("h0-sideways-baseline");
        commit_bytes(&repo, "seed", b"seed\n", "seed");
        git_test(&repo, &["branch", "side"]);
        commit_bytes(&repo, "main", b"main\n", "main");
        git_test(&repo, &["checkout", "-q", "side"]);
        let side = commit_bytes(&repo, "side", b"side\n", "side");
        git_test(&repo, &["checkout", "-q", "main"]);

        let error = validate_trusted_baseline(&repo, &side, &committed_registry(&workspace))
            .unwrap_err()
            .to_string();
        assert!(error.contains("not an ancestor of HEAD"), "{error}");
    }

    #[test]
    fn initial_scope_history_and_closing_anchors_are_commit_local() {
        let workspace = workspace();
        let registry = committed_registry(&workspace);
        let repo = init_repo("h0-commit-local-provenance");
        let workspace_root = git_root_for(&workspace).unwrap();
        let scope_rel = workspace_history_rel(&workspace_root, &workspace, SCOPE_REL_PATH).unwrap();
        let scope_bytes = git_blob_optional(
            &workspace_root,
            &registry.source.initial_scope_commit,
            &scope_rel,
        )
        .unwrap()
        .expect("initial frozen scope blob");
        let commit = commit_bytes(&repo, SCOPE_REL_PATH, &scope_bytes, "scope");

        let mut source = registry.source.clone();
        source.initial_scope_commit = commit.clone();
        source.initial_seed_sha256 = "0".repeat(64);
        let error = validate_initial_scope_history(&repo, &source)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("differs from its frozen source pin"),
            "{error}"
        );

        let mut row = registry.rows[0].clone();
        row.rust_boundary.authoritative_anchors = vec![RustBoundaryAnchor {
            role: RustBoundaryRole::Producer,
            crate_name: "test".to_owned(),
            path: SCOPE_REL_PATH.to_owned(),
            symbol: "symbol-that-was-added-after-closing".to_owned(),
        }];
        let error = validate_authoritative_anchors_at_commit(&repo, &row, &commit)
            .unwrap_err()
            .to_string();
        assert!(error.contains("is absent at closing commit"), "{error}");
    }

    #[test]
    fn lapsed_transition_state_machine_is_fail_closed_and_reactivatable() {
        let workspace = workspace();
        let open = committed_registry(&workspace)
            .rows
            .into_iter()
            .find(|row| row.status == RowStatus::Open)
            .expect("committed registry retains an open row");

        let mut lapsed = open.clone();
        lapsed.status = RowStatus::Lapsed;
        validate_row_transition(&open, &lapsed).unwrap();

        let fake_evidence = ClosureEvidence {
            tiers: ["t0", "t1", "t2", "t3", "t4"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            artifact: MATCHES_REL_PATH.to_owned(),
            artifact_sha256: "a".repeat(64),
            note: "test evidence".to_owned(),
        };
        let mut fabricated_lapse = lapsed.clone();
        fabricated_lapse.closing_commit = Some("b".repeat(40));
        fabricated_lapse.closure_evidence = Some(fake_evidence.clone());
        let error = validate_row_transition(&open, &fabricated_lapse)
            .unwrap_err()
            .to_string();
        assert!(error.contains("fabricated closure provenance"), "{error}");

        let mut reactivated = lapsed.clone();
        reactivated.status = RowStatus::Closed;
        reactivated.closing_commit = Some("b".repeat(40));
        reactivated.closure_evidence = Some(fake_evidence.clone());
        reactivated.rust_boundary.readiness = BoundaryReadiness::Authoritative;
        reactivated.rust_boundary.authoritative_anchors =
            reactivated.rust_boundary.seam_anchors.clone();
        reactivated.rust_boundary.authoritative_anchors[0].symbol =
            "try_add_module_resolution".to_owned();
        validate_row_transition(&lapsed, &reactivated).unwrap();

        let mut closed = reactivated.clone();
        let mut closed_lapse = closed.clone();
        closed_lapse.status = RowStatus::Lapsed;
        validate_row_transition(&closed, &closed_lapse).unwrap();
        closed.closure_evidence.as_mut().unwrap().note = "changed".to_owned();
        let error = validate_row_transition(&closed, &closed_lapse)
            .unwrap_err()
            .to_string();
        assert!(error.contains("immutable closure provenance"), "{error}");

        let error = validate_row_transition(&lapsed, &open)
            .unwrap_err()
            .to_string();
        assert!(error.contains("lapsed host-resolution row"), "{error}");
    }

    #[test]
    fn full_validator_rejects_owner_canary_closure_and_universe_drift() {
        let workspace = workspace();
        let scope = host_resolution_state(&workspace.join(SCOPE_REL_PATH)).unwrap();
        let inventory = read_inventory(&workspace).unwrap();
        let inputs = read_oracle_inputs(&workspace).unwrap();
        let registry = committed_registry(&workspace);

        let mut stale_owner = registry.clone();
        stale_owner.rows[0].tsc_owners[0].source_slice_sha256 = "0".repeat(64);
        let error = validate_registry(
            &workspace,
            &stale_owner,
            &scope,
            &inventory,
            &inputs,
            false,
            false,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("stale D2 owner metadata"), "{error}");

        let mut stale_canary = registry.clone();
        stale_canary.rows[0]
            .canaries
            .non_emitting_control
            .forbidden_codes = vec![9999];
        let error = validate_registry(
            &workspace,
            &stale_canary,
            &scope,
            &inventory,
            &inputs,
            false,
            false,
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("stale reviewed non-emitting control"),
            "{error}"
        );

        let mut false_closure = registry.clone();
        let open_index = false_closure
            .rows
            .iter()
            .position(|row| row.status == RowStatus::Open)
            .expect("committed registry retains an open row");
        false_closure.rows[open_index].status = RowStatus::Closed;
        false_closure.rows[open_index].rust_boundary.readiness = BoundaryReadiness::Authoritative;
        false_closure.rows[open_index]
            .rust_boundary
            .authoritative_anchors = false_closure.rows[open_index]
            .rust_boundary
            .seam_anchors
            .clone();
        false_closure.rows[open_index]
            .rust_boundary
            .authoritative_anchors[0]
            .symbol = "try_add_module_resolution".to_owned();
        false_closure.summary = summarize(&false_closure.rows);
        let error = validate_registry(
            &workspace,
            &false_closure,
            &scope,
            &inventory,
            &inputs,
            false,
            false,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("has no closing commit"), "{error}");

        let mut missing = registry.clone();
        missing.rows.pop();
        missing.summary = summarize(&missing.rows);
        let error = validate_registry(
            &workspace, &missing, &scope, &inventory, &inputs, false, false,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("projection hash is stale"), "{error}");
    }
}

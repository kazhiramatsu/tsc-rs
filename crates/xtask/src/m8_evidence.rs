use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tsc_checker::{check_program_with_libs_at, InputFile};
use tsc_diagnostics::{
    format_diagnostics_with_context, Diagnostic, FormatDiagnosticsHost, MessageChain, RelatedInfo,
};

const CONFIG_REL: &str = "m8-evidence.json";
const INVENTORY_REL: &str = "m8-emitter-inventory.json";
const ARTIFACT_SCHEMA: u32 = 2;
const PRODUCER_VERSION: &str = "m8-evidence-v2";
const CI_RECEIPT_FILE: &str = "ci-conformance-receipt.json";
const CI_ALL_FILE: &str = "ci-conformance-all.json";
const CI_TWO_XXX_FILE: &str = "ci-conformance-2xxx.json";
const CI_SYNTACTIC_FILE: &str = "ci-conformance-syntactic.json";
const CI_OUTPUT_KINDS: [&str; 4] = [
    "summary-all",
    "summary-2xxx",
    "summary-syntactic",
    "families-report",
];
const CACHE_OFF_SMOKE_LIMIT: usize = 8;

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct EvidenceConfig {
    schema: u32,
    artifact_dir: String,
    runtime_coverage: RuntimeConfig,
    workspace_tests: WorkspaceTestsConfig,
    conformance_runner: ConformanceRunnerConfig,
    fuzzer: FuzzerConfig,
    performance: PerformanceConfig,
}

/// Reviewed CPU policy for the local workspace-test pipeline. Hosted
/// Actions never executes workspace tests (the fixed boundary runs only
/// `cargo xtask acceptance`), so this ceiling governs local gate runs;
/// `TSRS_CI_TEST_WORKERS` may still clamp below it and the machine's
/// available parallelism always bounds it.
#[derive(Clone, Debug, Deserialize)]
struct WorkspaceTestsConfig {
    max_workers: usize,
}

/// Reviewed CPU policy for the conformance grading pipeline's checker
/// workers. Grading order and every compared surface stay sequential; the
/// ceiling only bounds how many checker executions run ahead.
/// `TSRS_CONFORMANCE_WORKERS` may clamp below it and available
/// parallelism always bounds it.
#[derive(Clone, Debug, Deserialize)]
struct ConformanceRunnerConfig {
    max_workers: usize,
}

#[derive(Clone, Debug, Deserialize)]
struct RuntimeConfig {
    artifact: String,
    max_workers: usize,
    programs_per_process: usize,
    max_lib_cache_buckets: usize,
    diagnostic_canary_programs: usize,
    #[serde(default)]
    zero_hit_reviews: Vec<ZeroHitReview>,
}

#[derive(Clone, Debug, Deserialize)]
struct ZeroHitReview {
    declaration: String,
    evidence: String,
}

#[derive(Clone, Debug, Deserialize)]
struct FuzzerConfig {
    artifact: String,
    seed: u64,
    cases: usize,
}

#[derive(Clone, Debug, Deserialize)]
struct PerformanceConfig {
    artifact: String,
    default_runner_profile: String,
    runners: Vec<RunnerProfile>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct RunnerProfile {
    id: String,
    os: String,
    arch: String,
    measurement_backend: String,
    cpu_policy: String,
    minimum_logical_cores: usize,
    minimum_memory_bytes: u64,
    ceiling_wall_seconds: f64,
    ceiling_rss_bytes: u64,
}

#[derive(Clone, Debug, Deserialize)]
struct Inventory {
    schema: u32,
    status: String,
    source_sha256: String,
    band: String,
    summary: InventorySummary,
    functions: Vec<InventoryFunction>,
}

#[derive(Clone, Debug, Deserialize)]
struct InventorySummary {
    emitter_declarations: usize,
}

#[derive(Clone, Debug, Deserialize)]
struct InventoryFunction {
    id: String,
    direct_emitter: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct InputEntry {
    path: String,
    sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Fingerprint {
    sha256: String,
    inputs: Vec<InputEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ArtifactHeader {
    schema: u32,
    producer_version: String,
    kind: String,
    producer_commit: String,
    command: String,
    started_unix_ms: u128,
    finished_unix_ms: u128,
    exit_status: i32,
    fingerprint: Fingerprint,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RuntimeArtifact {
    header: ArtifactHeader,
    inventory_sha256: String,
    raw_counts: BTreeMap<String, u64>,
    zero_hit_reviews: Vec<ZeroHitReviewArtifact>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ZeroHitReviewArtifact {
    declaration: String,
    evidence: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FuzzerArtifact {
    header: ArtifactHeader,
    seed: u64,
    requested_cases: usize,
    cases: Vec<FuzzCaseObservation>,
    reducer: ReducerObservation,
    dedupe: DedupeObservation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FuzzCaseObservation {
    case: usize,
    source_sha256: String,
    program_sha256: String,
    compared_tiers: Vec<String>,
    oracle_sha256: String,
    tsrs_sha256: String,
    /// Exact normalized T4 bytes. Optional only for deserializing stale
    /// pre-A3 artifacts; current verification requires both fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    oracle_rendered: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tsrs_rendered: Option<String>,
    divergence_signature: Option<String>,
    source: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ReducerObservation {
    exercised: bool,
    /// True only when the generated differential corpus is already exact
    /// and a deterministic one-sided mutation is used to keep testing the
    /// reducer/deduper machinery.
    #[serde(default)]
    mutation_canary: bool,
    original_signature: Option<String>,
    reduced_signature: Option<String>,
    original_bytes: usize,
    reduced_bytes: usize,
    reduced_source: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct DedupeObservation {
    exercised: bool,
    observed_signatures: Vec<String>,
    unique_signatures: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PerformanceArtifact {
    header: ArtifactHeader,
    runner: RunnerProfile,
    observed_os: String,
    observed_arch: String,
    logical_cores: usize,
    memory_bytes: u64,
    wall_seconds: f64,
    max_rss_bytes: u64,
    child_stdout_sha256: String,
    child_stderr_sha256: String,
    cache_off_smoke: CacheOffObservation,
    ci_conformance: CiConformanceBinding,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CacheOffObservation {
    fixture_limit: usize,
    wall_seconds: f64,
    max_rss_bytes: u64,
    exit_status: i32,
    child_stdout_sha256: String,
    child_stderr_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct BoundOutput {
    kind: String,
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CiConformanceBinding {
    receipt: BoundOutput,
    /// Fixed order: all, 2xxx, syntactic, families.
    outputs: Vec<BoundOutput>,
}

struct TimedObservation {
    wall_seconds: f64,
    max_rss_bytes: u64,
    exit_status: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

struct CiConformancePaths {
    receipt: PathBuf,
    all: PathBuf,
    two_xxx: PathBuf,
    syntactic: PathBuf,
    families: PathBuf,
}

struct ProducedPerformance {
    evidence: ProducedEvidence,
    binding: CiConformanceBinding,
}

pub(crate) struct ProducedEvidence {
    receipt_token: crate::ci_conformance_receipt::ReceiptToken,
    invocation: crate::ci_conformance_receipt::Invocation,
    paths: CiConformancePaths,
    binding: CiConformanceBinding,
}

impl ProducedEvidence {
    pub(crate) fn consume_ci_conformance(
        self,
    ) -> Result<tsc_conformance::CiConformanceSummaries, Box<dyn Error>> {
        if git_head(&self.invocation.workspace)? != self.invocation.head
            || sha256_file(&std::env::current_exe()?)? != self.invocation.producer_executable_sha256
            || performance_fingerprint(&self.invocation.workspace)?.sha256
                != self.invocation.fingerprint_sha256
        {
            return Err(
                "CI conformance inputs changed between B4 production and receipt consumption"
                    .into(),
            );
        }
        let consumed =
            crate::ci_conformance_receipt::consume(self.receipt_token, &self.invocation)?;
        if consumed.outputs.len() != crate::ci_conformance_receipt::OUTPUT_ROLES.len()
            || consumed.receipt.bindings.len() != consumed.outputs.len()
            || consumed.outputs.len() != self.binding.outputs.len()
        {
            return Err("CI conformance receipt has an incomplete output binding set".into());
        }
        for ((observed, role), expected) in consumed
            .outputs
            .iter()
            .zip(crate::ci_conformance_receipt::OUTPUT_ROLES)
            .zip(&self.binding.outputs)
        {
            if observed.binding.role != role
                || observed.binding.path != expected.path
                || observed.binding.bytes != expected.bytes
                || observed.binding.sha256 != expected.sha256
            {
                return Err(format!(
                    "CI conformance receipt binding {role:?} differs from the published evidence binding"
                )
                .into());
            }
        }

        let all = tsc_conformance::decode_ci_conformance_summary(&consumed.outputs[0].bytes)?;
        let two_xxx = tsc_conformance::decode_ci_conformance_summary(&consumed.outputs[1].bytes)?;
        let syntactic = tsc_conformance::decode_ci_conformance_summary(&consumed.outputs[2].bytes)?;
        let all_summary = all.as_summary();
        let two_xxx_summary = two_xxx.as_summary();
        let syntactic_summary = syntactic.as_summary();
        if all_summary.band != "all"
            || two_xxx_summary.band != "2xxx"
            || syntactic_summary.band != "syntactic"
            || all_summary.fixtures_total == 0
            || all_summary.cases_total == 0
            || all_summary.fixtures_total != two_xxx_summary.fixtures_total
            || all_summary.fixtures_total != syntactic_summary.fixtures_total
            || all_summary.cases_total != two_xxx_summary.cases_total
            || all_summary.cases_total != syntactic_summary.cases_total
        {
            return Err(
                "CI conformance receipt summaries violate the fixed full-corpus view contract"
                    .into(),
            );
        }
        super::print_conformance_summary(all_summary, &self.paths.all);
        super::print_conformance_summary(two_xxx_summary, &self.paths.two_xxx);
        super::print_conformance_summary(syntactic_summary, &self.paths.syntactic);
        Ok(tsc_conformance::CiConformanceSummaries {
            all,
            two_xxx,
            syntactic,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ManifestArtifact {
    kind: String,
    path: String,
    sha256: String,
    fingerprint_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EvidenceManifest {
    schema: u32,
    producer_version: String,
    producer_commit: String,
    artifacts: Vec<ManifestArtifact>,
    ci_conformance: CiConformanceBinding,
}

struct RuntimeValidation {
    ready: bool,
    fresh: bool,
    executed: usize,
    zero_hit: usize,
    reviewed: usize,
}

pub(crate) fn evidence(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    match args.next().as_deref() {
        Some("produce") => {
            let mut all = false;
            for arg in args {
                match arg.as_str() {
                    "--all" => all = true,
                    _ => return Err(format!("unexpected m8 evidence argument: {arg}").into()),
                }
            }
            if !all {
                return Err("m8 evidence produce requires --all".into());
            }
            produce_all().map(drop)
        }
        Some("fingerprint") => {
            if args.next().as_deref() != Some("--kind")
                || args.next().as_deref() != Some("runtime")
                || args.next().is_some()
            {
                return Err("m8 evidence fingerprint requires --kind runtime".into());
            }
            let workspace = super::find_workspace_root()?;
            println!("{}", runtime_fingerprint(&workspace)?.sha256);
            Ok(())
        }
        Some(other) => Err(format!("unknown m8 evidence command: {other}").into()),
        None => Err("missing m8 evidence command (produce/fingerprint)".into()),
    }
}

pub(crate) fn produce_all() -> Result<ProducedEvidence, Box<dyn Error>> {
    let workspace = super::find_workspace_root()?;
    ensure_relevant_tree_clean(&workspace)?;
    let config = read_config(&workspace)?;
    let runtime_path =
        resolve_artifact_path(&workspace, &config, &config.runtime_coverage.artifact)?;
    let fuzz_path = resolve_artifact_path(&workspace, &config, &config.fuzzer.artifact)?;
    let perf_path = resolve_artifact_path(&workspace, &config, &config.performance.artifact)?;
    let manifest_path = artifact_dir(&workspace, &config)?.join("manifest.json");
    let ci_paths = ci_conformance_paths(&workspace, &config)?;

    // A failed B3/B4 attempt must not leave a previously-published success
    // discoverable by a later readiness invocation. The content-addressed B2
    // runtime artifact is deliberately retained: it has its own strict reuse
    // validator and is the only evidence file restored by Actions cache.
    // gate-tax 4: the performance artifact and its CI conformance outputs
    // are content-addressed the same way as B2 — `reuse_performance` below
    // either proves the standing set against a freshly recomputed exact
    // fingerprint or the miss path invalidates them before producing, so
    // readiness can never discover a half-produced or unproven B4 set.
    invalidate_published_files(&workspace, [&manifest_path, &fuzz_path])?;

    if runtime_artifact_is_current(&workspace, &runtime_path)? {
        println!(
            "runtime emitter coverage: reused current verified artifact={}",
            runtime_path.display()
        );
    } else {
        produce_runtime(&workspace, &config, &runtime_path, false)?;
    }
    produce_fuzz(
        &workspace,
        &config,
        config.fuzzer.seed,
        config.fuzzer.cases,
        &fuzz_path,
    )?;
    let performance = match reuse_performance(&workspace, &config, &perf_path)? {
        Some(reused) => reused,
        None => {
            invalidate_published_files(
                &workspace,
                [
                    &perf_path,
                    &ci_paths.receipt,
                    &ci_paths.all,
                    &ci_paths.two_xxx,
                    &ci_paths.syntactic,
                    &ci_paths.families,
                ],
            )?;
            produce_performance(&workspace, &config, None, &perf_path)?
        }
    };

    let artifacts = [
        ("runtime-coverage", runtime_path),
        ("differential-fuzzer", fuzz_path),
        ("performance", perf_path),
    ]
    .into_iter()
    .map(|(kind, path)| {
        let fingerprint_sha256 = match kind {
            "runtime-coverage" => {
                read_json::<RuntimeArtifact>(&path)?
                    .header
                    .fingerprint
                    .sha256
            }
            "differential-fuzzer" => {
                read_json::<FuzzerArtifact>(&path)?
                    .header
                    .fingerprint
                    .sha256
            }
            "performance" => {
                read_json::<PerformanceArtifact>(&path)?
                    .header
                    .fingerprint
                    .sha256
            }
            _ => unreachable!(),
        };
        Ok(ManifestArtifact {
            kind: kind.to_owned(),
            path: workspace_relative(&workspace, &path)?,
            sha256: sha256_file(&path)?,
            fingerprint_sha256,
        })
    })
    .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let manifest = EvidenceManifest {
        schema: ARTIFACT_SCHEMA,
        producer_version: PRODUCER_VERSION.to_owned(),
        producer_commit: git_head(&workspace)?,
        artifacts,
        ci_conformance: performance.binding.clone(),
    };
    write_json(&workspace, &manifest_path, &manifest)?;
    println!("M8 evidence manifest written: {}", manifest_path.display());
    Ok(performance.evidence)
}

pub(crate) fn coverage_emitters(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let workspace = super::find_workspace_root()?;
    let config = read_config(&workspace)?;
    let mut corpus = false;
    let mut artifact = None;
    let mut allow_unreviewed = false;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--corpus" => corpus = true,
            "--allow-unreviewed" => allow_unreviewed = true,
            "--artifact" => {
                artifact = Some(PathBuf::from(
                    args.next().ok_or("missing value after --artifact")?,
                ))
            }
            _ => return Err(format!("unexpected coverage emitters argument: {arg}").into()),
        }
    }
    if !corpus {
        return Err("coverage emitters requires --corpus".into());
    }
    let artifact = artifact.unwrap_or(resolve_artifact_path(
        &workspace,
        &config,
        &config.runtime_coverage.artifact,
    )?);
    produce_runtime(&workspace, &config, &artifact, allow_unreviewed)
}

fn produce_runtime(
    workspace: &Path,
    config: &EvidenceConfig,
    artifact_path: &Path,
    allow_unreviewed: bool,
) -> Result<(), Box<dyn Error>> {
    let started_unix_ms = now_unix_ms()?;
    let started = Instant::now();
    let fingerprint = runtime_fingerprint(workspace)?;
    let inventory_path = workspace.join(INVENTORY_REL);
    let inventory: Inventory = read_json(&inventory_path)?;
    validate_inventory(workspace, &inventory)?;

    let out_dir = artifact_dir(workspace, config)?;
    fs::create_dir_all(&out_dir)?;
    let instrumented = out_dir.join("instrumented-tsc.cjs");
    let instrument = Command::new("node")
        .arg(workspace.join("crates/oracle/coverage-instrument.mjs"))
        .arg(workspace.join("vendor/typescript-6.0.3/lib/_tsc.js"))
        .arg(&inventory_path)
        .arg(&instrumented)
        .output()?;
    if !instrument.status.success() {
        return Err(format!(
            "coverage instrumenter failed: {}",
            String::from_utf8_lossy(&instrument.stderr)
        )
        .into());
    }

    let programs_root = out_dir.join("runtime-programs");
    if programs_root.exists() {
        fs::remove_dir_all(&programs_root)?;
    }
    fs::create_dir_all(&programs_root)?;
    let programs = expand_corpus_programs(workspace, &programs_root)?;
    verify_coverage_driver_diagnostics(
        workspace,
        &instrumented,
        &programs[..programs
            .len()
            .min(config.runtime_coverage.diagnostic_canary_programs)],
        config.runtime_coverage.max_lib_cache_buckets,
    )?;
    let worker_count = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(config.runtime_coverage.max_workers)
        .min(programs.len());
    let mut shards = vec![Vec::new(); worker_count];
    for (index, program) in programs.iter().enumerate() {
        shards[index % worker_count].push(program.canonicalize()?);
    }
    let process_batches = shards
        .iter()
        .map(|shard| {
            shard
                .len()
                .div_ceil(config.runtime_coverage.programs_per_process)
        })
        .sum::<usize>();
    let driver = workspace.join("crates/oracle/coverage-driver.mjs");
    let instrumented = instrumented.canonicalize()?;
    let handles = shards
        .into_iter()
        .map(|shard| {
            let driver = driver.clone();
            let instrumented = instrumented.clone();
            let programs_per_process = config.runtime_coverage.programs_per_process;
            let max_lib_cache_buckets = config.runtime_coverage.max_lib_cache_buckets;
            std::thread::spawn(move || {
                run_coverage_worker(
                    &driver,
                    &instrumented,
                    &shard,
                    programs_per_process,
                    max_lib_cache_buckets,
                )
            })
        })
        .collect::<Vec<_>>();
    let mut raw_counts = BTreeMap::<String, u64>::new();
    for handle in handles {
        let counts = handle
            .join()
            .map_err(|_| "coverage worker thread panicked")?
            .map_err(|error| format!("coverage worker failed: {error}"))?;
        for (id, count) in counts {
            if count > 0 {
                raw_counts.insert(id, 1);
            }
        }
    }
    let direct = inventory
        .functions
        .iter()
        .filter(|function| function.direct_emitter)
        .map(|function| function.id.as_str())
        .collect::<BTreeSet<_>>();
    if raw_counts.keys().any(|id| !direct.contains(id.as_str())) {
        return Err("coverage artifact contains a non-emitter declaration identity".into());
    }
    let reviews = config
        .runtime_coverage
        .zero_hit_reviews
        .iter()
        .map(|review| (review.declaration.as_str(), review))
        .collect::<BTreeMap<_, _>>();
    let zero_hit = direct
        .iter()
        .filter(|id| raw_counts.get(**id).copied().unwrap_or(0) == 0)
        .copied()
        .collect::<Vec<_>>();
    let missing_reviews = zero_hit
        .iter()
        .filter(|id| {
            reviews
                .get(**id)
                .is_none_or(|review| review.evidence.trim().is_empty())
        })
        .copied()
        .collect::<Vec<_>>();
    let extra_reviews = reviews
        .keys()
        .filter(|id| !zero_hit.contains(id))
        .copied()
        .collect::<Vec<_>>();
    let zero_hit_reviews = zero_hit
        .iter()
        .filter_map(|id| reviews.get(id).copied())
        .map(|review| ZeroHitReviewArtifact {
            declaration: review.declaration.clone(),
            evidence: review.evidence.clone(),
        })
        .collect::<Vec<_>>();
    let artifact = RuntimeArtifact {
        header: artifact_header(
            workspace,
            "runtime-coverage",
            "cargo xtask coverage emitters --corpus",
            started_unix_ms,
            fingerprint,
            0,
        )?,
        inventory_sha256: sha256_file(&inventory_path)?,
        raw_counts,
        zero_hit_reviews,
    };
    write_json(workspace, artifact_path, &artifact)?;
    println!(
        "runtime emitter coverage: programs={} workers={} process-batches={} executed={}/{} zero-hit={} elapsed={:.2}s artifact={}",
        programs.len(),
        worker_count,
        process_batches,
        direct.len() - zero_hit.len(),
        direct.len(),
        zero_hit.len(),
        started.elapsed().as_secs_f64(),
        artifact_path.display()
    );
    if !missing_reviews.is_empty() || !extra_reviews.is_empty() {
        println!(
            "runtime zero-hit review diff: missing={} extra={}{}{}",
            missing_reviews.len(),
            extra_reviews.len(),
            missing_reviews
                .first()
                .map(|id| format!("; first missing {id}"))
                .unwrap_or_default(),
            extra_reviews
                .first()
                .map(|id| format!("; first extra {id}"))
                .unwrap_or_default()
        );
        if !allow_unreviewed {
            return Err(
                "runtime coverage observations were written, but zero-hit reviews are incomplete"
                    .into(),
            );
        }
    }
    Ok(())
}

fn runtime_artifact_is_current(
    workspace: &Path,
    artifact_path: &Path,
) -> Result<bool, Box<dyn Error>> {
    if !artifact_path.is_file() {
        return Ok(false);
    }
    let artifact: RuntimeArtifact = match read_json(artifact_path) {
        Ok(artifact) => artifact,
        Err(_) => return Ok(false),
    };
    let inventory_path = workspace.join(INVENTORY_REL);
    let inventory: Inventory = read_json(&inventory_path)?;
    validate_inventory(workspace, &inventory)?;
    let direct_emitter_ids = inventory
        .functions
        .iter()
        .filter(|function| function.direct_emitter)
        .map(|function| function.id.as_str())
        .collect::<BTreeSet<_>>();
    let current = runtime_fingerprint(workspace)?;
    let inventory_hash = sha256_file(&inventory_path)?;
    Ok(validate_runtime_artifact(&artifact, &current, &inventory_hash, &direct_emitter_ids).ready)
}

fn validate_runtime_artifact(
    artifact: &RuntimeArtifact,
    current: &Fingerprint,
    inventory_hash: &str,
    direct_emitter_ids: &BTreeSet<&str>,
) -> RuntimeValidation {
    let executed = artifact
        .raw_counts
        .iter()
        .filter(|(id, count)| direct_emitter_ids.contains(id.as_str()) && **count > 0)
        .count();
    let zero_hit_ids = direct_emitter_ids
        .iter()
        .filter(|id| artifact.raw_counts.get(**id).copied().unwrap_or(0) == 0)
        .copied()
        .collect::<BTreeSet<_>>();
    let reviewed_ids = artifact
        .zero_hit_reviews
        .iter()
        .map(|review| review.declaration.as_str())
        .collect::<BTreeSet<_>>();
    let fresh = artifact.header.fingerprint == *current;
    let ready = artifact.header.schema == ARTIFACT_SCHEMA
        && artifact.header.producer_version == PRODUCER_VERSION
        && artifact.header.kind == "runtime-coverage"
        && artifact.header.command == "cargo xtask coverage emitters --corpus"
        // Runtime coverage is intentionally content-addressed and may be
        // restored from a different commit. Its exact fingerprint, rather
        // than HEAD equality, owns reuse; still reject malformed provenance.
        && is_full_lower_hex_commit(&artifact.header.producer_commit)
        && artifact.header.exit_status == 0
        && artifact.header.finished_unix_ms >= artifact.header.started_unix_ms
        && fresh
        && artifact.inventory_sha256 == inventory_hash
        && executed > 0
        && artifact
            .raw_counts
            .keys()
            .all(|id| direct_emitter_ids.contains(id.as_str()))
        && artifact.raw_counts.values().all(|count| *count == 1)
        && artifact.zero_hit_reviews.len() == reviewed_ids.len()
        && artifact
            .zero_hit_reviews
            .iter()
            .all(|review| !review.evidence.trim().is_empty())
        && reviewed_ids == zero_hit_ids;
    RuntimeValidation {
        ready,
        fresh,
        executed,
        zero_hit: zero_hit_ids.len(),
        reviewed: reviewed_ids.len(),
    }
}

fn artifact_header_matches(
    header: &ArtifactHeader,
    kind: &str,
    command: &str,
    current_fingerprint: &Fingerprint,
    current_commit: Option<&str>,
) -> bool {
    header.schema == ARTIFACT_SCHEMA
        && header.producer_version == PRODUCER_VERSION
        && header.kind == kind
        && header.command == command
        && is_full_lower_hex_commit(&header.producer_commit)
        && current_commit.is_none_or(|commit| header.producer_commit == commit)
        && header.exit_status == 0
        && header.finished_unix_ms >= header.started_unix_ms
        && header.fingerprint == *current_fingerprint
}

fn is_full_lower_hex_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn run_coverage_worker(
    driver: &Path,
    instrumented: &Path,
    programs: &[PathBuf],
    programs_per_process: usize,
    max_lib_cache_buckets: usize,
) -> Result<BTreeMap<String, u64>, String> {
    let mut combined = BTreeMap::new();
    for batch in programs.chunks(programs_per_process) {
        for (id, count) in run_coverage_process(driver, instrumented, batch, max_lib_cache_buckets)?
        {
            if count > 0 {
                combined.insert(id, 1);
            }
        }
    }
    Ok(combined)
}

fn verify_coverage_driver_diagnostics(
    workspace: &Path,
    instrumented: &Path,
    programs: &[PathBuf],
    max_lib_cache_buckets: usize,
) -> Result<(), Box<dyn Error>> {
    let requests = programs
        .iter()
        .chain(programs.iter())
        .enumerate()
        .map(|(index, program)| -> Result<Value, std::io::Error> {
            Ok(json!({
                "id": index,
                "programJsonPath": program.canonicalize()?.display().to_string()
            }))
        })
        .collect::<Result<Vec<_>, std::io::Error>>()?;
    let mut oracle = Command::new("node");
    oracle
        .arg("--single-threaded")
        .arg(workspace.join("crates/oracle/driver.mjs"));
    let oracle_responses = run_node_jsonl(&mut oracle, &requests)?;

    let mut coverage_requests = requests
        .iter()
        .cloned()
        .map(|mut request| {
            request["returnDiagnostics"] = Value::Bool(true);
            request
        })
        .collect::<Vec<_>>();
    coverage_requests.push(json!({"finish": true}));
    let mut coverage = Command::new("node");
    coverage
        .arg("--single-threaded")
        .arg(workspace.join("crates/oracle/coverage-driver.mjs"))
        .arg(instrumented)
        .arg(max_lib_cache_buckets.to_string());
    let coverage_responses = run_node_jsonl(&mut coverage, &coverage_requests)?;
    let expected_responses = programs.len() * 2;
    if oracle_responses.len() != expected_responses
        || coverage_responses.len() != expected_responses + 1
        || oracle_responses != coverage_responses[..expected_responses]
        || coverage_responses
            .last()
            .and_then(|value| value["schema"].as_u64())
            != Some(1)
    {
        return Err(
            "coverage driver diagnostic canary differs from the uncached oracle driver".into(),
        );
    }
    println!(
        "runtime emitter coverage diagnostic canary: programs={} passes=2 exact=true",
        programs.len(),
    );
    Ok(())
}

fn run_node_jsonl(command: &mut Command, requests: &[Value]) -> Result<Vec<Value>, Box<dyn Error>> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    {
        let stdin = child.stdin.as_mut().ok_or("Node JSONL stdin unavailable")?;
        for request in requests {
            serde_json::to_writer(&mut *stdin, request)?;
            writeln!(stdin)?;
        }
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(format!(
            "Node JSONL process failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    String::from_utf8(output.stdout)?
        .lines()
        .map(|line| Ok(serde_json::from_str(line)?))
        .collect()
}

fn run_coverage_process(
    driver: &Path,
    instrumented: &Path,
    programs: &[PathBuf],
    max_lib_cache_buckets: usize,
) -> Result<BTreeMap<String, u64>, String> {
    let mut child = Command::new("node")
        .arg("--single-threaded")
        .arg(driver)
        .arg(instrumented)
        .arg(max_lib_cache_buckets.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("spawn: {error}"))?;
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| "stdin unavailable".to_owned())?;
        for program in programs {
            writeln!(
                stdin,
                "{}",
                serde_json::to_string(&json!({
                    "programJsonPath": program.display().to_string()
                }))
                .map_err(|error| format!("request serialization: {error}"))?
            )
            .map_err(|error| format!("request write: {error}"))?;
        }
        writeln!(
            stdin,
            "{}",
            serde_json::to_string(&json!({"finish": true}))
                .map_err(|error| format!("finish serialization: {error}"))?
        )
        .map_err(|error| format!("finish write: {error}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("wait: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "exit {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let response: Value =
        serde_json::from_slice(&output.stdout).map_err(|error| format!("response: {error}"))?;
    if response["schema"].as_u64() != Some(1) {
        return Err("unknown response schema".to_owned());
    }
    response["counts"]
        .as_object()
        .ok_or_else(|| "response lacks counts".to_owned())?
        .iter()
        .map(|(id, count)| {
            count
                .as_u64()
                .map(|count| (id.clone(), count))
                .ok_or_else(|| format!("counter for {id} is not an integer"))
        })
        .collect()
}

pub(crate) fn fuzz_run(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let workspace = super::find_workspace_root()?;
    let config = read_config(&workspace)?;
    let mut seed = None;
    let mut cases = None;
    let mut artifact = None;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--seed" => seed = Some(args.next().ok_or("missing value after --seed")?.parse()?),
            "--cases" => cases = Some(args.next().ok_or("missing value after --cases")?.parse()?),
            "--artifact" => {
                artifact = Some(PathBuf::from(
                    args.next().ok_or("missing value after --artifact")?,
                ))
            }
            _ => return Err(format!("unexpected fuzz run argument: {arg}").into()),
        }
    }
    let seed = seed.ok_or("fuzz run requires --seed")?;
    let cases = cases.ok_or("fuzz run requires --cases")?;
    let artifact = artifact.ok_or("fuzz run requires --artifact")?;
    produce_fuzz(&workspace, &config, seed, cases, &artifact)
}

pub(crate) fn fuzz_replay(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let args = args.collect::<Vec<_>>();
    if args.len() != 1 {
        return Err("fuzz replay requires one artifact path".into());
    }
    let workspace = super::find_workspace_root()?;
    let catalog = super::workspace_catalog::WorkspaceCatalog::discover(&workspace)?;
    let fuzz_package = catalog.require_package("fuzz")?;
    fuzz_package.require_default_run_target()?;
    let status = Command::new("cargo")
        .current_dir(&workspace)
        .arg("run")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(fuzz_package.manifest_path())
        .arg("--")
        .arg("replay")
        .args(args)
        .status()?;
    if !status.success() {
        return Err(format!("dedicated fuzz producer failed with status {status:?}").into());
    }
    Ok(())
}

pub(crate) fn fuzz_reduce(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let args = args.collect::<Vec<_>>();
    if args.len() != 1 {
        return Err("fuzz reduce requires one artifact path".into());
    }
    Err("fuzz reduce is fail-closed until the M9.1d real-replay reducer lands".into())
}

fn produce_fuzz(
    workspace: &Path,
    config: &EvidenceConfig,
    seed: u64,
    cases: usize,
    artifact_path: &Path,
) -> Result<(), Box<dyn Error>> {
    if cases == 0 {
        return Err("fuzz cases must be non-zero".into());
    }
    let started_unix_ms = now_unix_ms()?;
    let fingerprint = fuzz_fingerprint(workspace, seed, cases)?;
    let vendor_lib_dir = workspace.join("vendor/typescript-6.0.3/lib");
    let out_dir = artifact_dir(workspace, config)?.join("fuzz-programs");
    if out_dir.exists() {
        fs::remove_dir_all(&out_dir)?;
    }
    fs::create_dir_all(&out_dir)?;
    let pool = verified_fuzzer_oracle_pool(workspace)?;
    let mut observations = Vec::with_capacity(cases);
    let mut first_divergence = None;
    for case in 0..cases {
        let source = generated_source(seed, case);
        let program = tsc_harness::expand_fixture_text("main.ts", &source, &vendor_lib_dir)?
            .into_iter()
            .next()
            .ok_or("generated fixture expanded to no programs")?;
        let case_dir = out_dir.join(format!("case-{case:05}"));
        let paths = tsc_harness::write_program_jsons(std::slice::from_ref(&program), &case_dir)?;
        let comparison = compare_program(&program, &paths[0], &vendor_lib_dir, &pool)?;
        if first_divergence.is_none() && comparison.signature.is_some() {
            first_divergence = Some((source.clone(), comparison.signature.clone().unwrap()));
        }
        observations.push(FuzzCaseObservation {
            case,
            source_sha256: sha256_bytes(source.as_bytes()),
            program_sha256: sha256_file(&paths[0])?,
            compared_tiers: vec![
                "t0".to_owned(),
                "t1".to_owned(),
                "t2".to_owned(),
                "t3".to_owned(),
                "t4".to_owned(),
            ],
            oracle_sha256: comparison.oracle_sha256,
            tsrs_sha256: comparison.tsrs_sha256,
            oracle_rendered: Some(comparison.oracle_rendered),
            tsrs_rendered: Some(comparison.tsrs_rendered),
            divergence_signature: comparison.signature,
            source,
        });
    }
    let (divergent_source, signature, mutation_canary) = if let Some((source, signature)) =
        first_divergence
    {
        (source, signature, false)
    } else {
        // Exact parity is a valid fuzzer outcome. Keep proving the
        // reducer and signature deduper by mutating only this separate
        // canary comparison; generated case observations above remain
        // unmodified oracle/tsrs comparisons.
        let source = "const fuzzReducerCanaryA = 0;\nconst fuzzReducerCanaryB = 0;\n".to_owned();
        let program = tsc_harness::expand_fixture_text("main.ts", &source, &vendor_lib_dir)?
            .into_iter()
            .next()
            .ok_or("fuzzer mutation canary expanded to no programs")?;
        let paths = tsc_harness::write_program_jsons(
            std::slice::from_ref(&program),
            &out_dir.join("mutation-canary"),
        )?;
        let comparison = compare_program_with_mutation_canary(
            &program,
            &paths[0],
            &vendor_lib_dir,
            &pool,
            true,
        )?;
        let signature = comparison
            .signature
            .ok_or("fuzzer mutation canary did not create a divergence")?;
        (source, signature, true)
    };
    let reduced_source = reduce_source_preserving_signature(
        &divergent_source,
        &signature,
        &vendor_lib_dir,
        &out_dir.join("reducer"),
        &pool,
        mutation_canary,
    )?;
    let reduced_program =
        tsc_harness::expand_fixture_text("main.ts", &reduced_source, &vendor_lib_dir)?
            .into_iter()
            .next()
            .ok_or("reduced fixture expanded to no programs")?;
    let reduced_paths = tsc_harness::write_program_jsons(
        std::slice::from_ref(&reduced_program),
        &out_dir.join("reduced"),
    )?;
    let reduced_comparison = compare_program_with_mutation_canary(
        &reduced_program,
        &reduced_paths[0],
        &vendor_lib_dir,
        &pool,
        mutation_canary,
    )?;
    let reducer = ReducerObservation {
        exercised: reduced_comparison.signature.as_deref() == Some(signature.as_str()),
        mutation_canary,
        original_signature: Some(signature.clone()),
        reduced_signature: reduced_comparison.signature.clone(),
        original_bytes: divergent_source.len(),
        reduced_bytes: reduced_source.len(),
        reduced_source: Some(reduced_source),
    };
    let observed_signatures = vec![signature.clone(), signature];
    let unique_signatures = observed_signatures
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let dedupe = DedupeObservation {
        exercised: observed_signatures.len() == 2 && unique_signatures.len() == 1,
        observed_signatures,
        unique_signatures,
    };
    let artifact = FuzzerArtifact {
        header: artifact_header(
            workspace,
            "differential-fuzzer",
            &format!(
                "cargo xtask fuzz run --seed {seed} --cases {cases} --artifact {}",
                artifact_path.display()
            ),
            started_unix_ms,
            fingerprint,
            0,
        )?,
        seed,
        requested_cases: cases,
        cases: observations,
        reducer,
        dedupe,
    };
    verify_fuzzer_raw(&artifact)?;
    write_json(workspace, artifact_path, &artifact)?;
    println!(
        "fuzz smoke: generated={} compared={} divergences={} reducer={} reducer-mode={} dedupe={} artifact={}",
        artifact.cases.len(),
        artifact.cases.len(),
        artifact
            .cases
            .iter()
            .filter(|case| case.divergence_signature.is_some())
            .count(),
        artifact.reducer.exercised,
        if artifact.reducer.mutation_canary {
            "mutation-canary"
        } else {
            "natural-divergence"
        },
        artifact.dedupe.exercised,
        artifact_path.display()
    );
    Ok(())
}

fn verified_fuzzer_oracle_pool(workspace: &Path) -> Result<tsc_oracle::OraclePool, Box<dyn Error>> {
    // The fuzzer needs only the explicit A3 renderer response. A
    // renderer-only pool avoids eagerly launching an unused normal
    // oracle worker, and the launch probe verifies the actual single
    // lazy worker against the workspace Node pin.
    let pool = tsc_oracle::OraclePool::new_render_only();
    tsc_conformance::ratchet::verify_launched_render_node(workspace, &pool)?;
    Ok(pool)
}

struct ProgramComparison {
    oracle_sha256: String,
    tsrs_sha256: String,
    oracle_rendered: String,
    tsrs_rendered: String,
    signature: Option<String>,
}

fn compare_program(
    program: &tsc_harness::ProgramJson,
    path: &Path,
    vendor_lib_dir: &Path,
    pool: &tsc_oracle::OraclePool,
) -> Result<ProgramComparison, Box<dyn Error>> {
    compare_program_with_mutation_canary(program, path, vendor_lib_dir, pool, false)
}

fn compare_program_with_mutation_canary(
    program: &tsc_harness::ProgramJson,
    path: &Path,
    vendor_lib_dir: &Path,
    pool: &tsc_oracle::OraclePool,
    inject_mutation_canary: bool,
) -> Result<ProgramComparison, Box<dyn Error>> {
    let mut file_texts = BTreeMap::new();
    let libs = program
        .libs
        .iter()
        .map(|name| {
            let text = fs::read_to_string(vendor_lib_dir.join(name))?;
            file_texts.insert(name.clone(), text.clone());
            Ok(InputFile::new(name.clone(), text))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let files = program
        .files
        .iter()
        .map(|file| {
            let text = String::from_utf8(BASE64.decode(&file.text_b64)?)?;
            file_texts.insert(file.name.clone(), text.clone());
            Ok(InputFile::new(file.name.clone(), text))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let result = check_program_with_libs_at(
        &libs,
        &files,
        &tsc_harness::compiler_options_from_program(program),
        &program.cwd,
    );
    let oracle = pool.diagnostics_with_rendering(path)?;
    let oracle_values = oracle
        .diagnostics
        .iter()
        .map(oracle_value)
        .collect::<Vec<_>>();
    let tsrs_rendered = format_diagnostics_with_context(
        &result.diagnostics,
        &FormatDiagnosticsHost::new(&program.cwd, &file_texts),
    )?;
    let mut tsrs_values = result
        .diagnostics
        .iter()
        .map(tsrs_value)
        .collect::<Vec<_>>();
    if inject_mutation_canary {
        tsrs_values.push(json!({
            "file": "/__fuzzer_mutation_canary__.ts",
            "start": 0,
            "length": 1,
            "code": 99999,
            "category": "Error",
            "head": "Differential fuzzer mutation canary",
            "chain": {
                "text": "Differential fuzzer mutation canary",
                "code": 99999,
                "category": "Error",
                "next": [],
            },
            "related": [],
        }));
    }
    let tiers = [
        (
            "t0",
            project_tier(&oracle_values, 0),
            project_tier(&tsrs_values, 0),
        ),
        (
            "t1",
            project_tier(&oracle_values, 1),
            project_tier(&tsrs_values, 1),
        ),
        (
            "t2",
            project_tier(&oracle_values, 2),
            project_tier(&tsrs_values, 2),
        ),
        (
            "t3",
            project_tier(&oracle_values, 3),
            project_tier(&tsrs_values, 3),
        ),
    ];
    let signature = if let Some((tier, oracle, tsrs)) = tiers
        .iter()
        .find(|(_, oracle, tsrs)| !tier_multisets_equal(oracle, tsrs))
    {
        Some(divergence_signature(tier, oracle, tsrs)?)
    } else if oracle.rendered != tsrs_rendered {
        Some(t4_divergence_signature(
            &oracle_values,
            &tsrs_values,
            &oracle.rendered,
            &tsrs_rendered,
        )?)
    } else {
        None
    };
    Ok(ProgramComparison {
        oracle_sha256: sha256_bytes(oracle.rendered.as_bytes()),
        tsrs_sha256: sha256_bytes(tsrs_rendered.as_bytes()),
        oracle_rendered: oracle.rendered,
        tsrs_rendered,
        signature,
    })
}

fn oracle_value(diagnostic: &tsc_oracle::OracleDiag) -> Value {
    json!({
        "file": diagnostic.file,
        "start": diagnostic.start,
        "length": diagnostic.length,
        "code": diagnostic.code,
        "category": diagnostic.category,
        "head": diagnostic.chain.text,
        "chain": diagnostic.chain,
        "related": diagnostic.related,
    })
}

fn tsrs_value(diagnostic: &Diagnostic) -> Value {
    json!({
        "file": diagnostic.file_name,
        "start": diagnostic.start,
        "length": diagnostic.length,
        "code": diagnostic.code(),
        "category": diagnostic.category().name(),
        "head": diagnostic.message.text,
        "chain": message_chain_value(&diagnostic.message),
        "related": diagnostic.related.iter().map(related_value).collect::<Vec<_>>(),
    })
}

fn message_chain_value(chain: &MessageChain) -> Value {
    json!({
        "text": chain.text,
        "code": chain.code,
        "category": chain.category.name(),
        "next": chain.next.iter().map(message_chain_value).collect::<Vec<_>>(),
    })
}

fn related_value(related: &RelatedInfo) -> Value {
    json!({
        "file": related.file_name,
        "start": related.start,
        "length": related.length,
        "code": related.message.code,
        "category": related.message.category.name(),
        "chain": message_chain_value(&related.message),
    })
}

fn project_tier(values: &[Value], tier: usize) -> Vec<Value> {
    values
        .iter()
        .map(|value| match tier {
            0 => json!({
                "file": value["file"],
                "start": value["start"],
                "code": value["code"],
            }),
            1 => json!({
                "file": value["file"],
                "start": value["start"],
                "code": value["code"],
                "category": value["category"],
            }),
            2 => json!({
                "file": value["file"],
                "start": value["start"],
                "length": value["length"],
                "code": value["code"],
                "category": value["category"],
                "head": value["head"],
            }),
            3 => value.clone(),
            _ => unreachable!(),
        })
        .collect()
}

fn tier_multisets_equal(oracle: &[Value], tsrs: &[Value]) -> bool {
    canonical_rows(oracle) == canonical_rows(tsrs)
}

fn canonical_rows(values: &[Value]) -> Vec<String> {
    let mut rows = values
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()
        .expect("serde_json::Value serializes");
    rows.sort();
    rows
}

fn t4_divergence_signature(
    oracle_values: &[Value],
    tsrs_values: &[Value],
    oracle_rendered: &str,
    tsrs_rendered: &str,
) -> Result<String, Box<dyn Error>> {
    let renderer_class =
        renderer_divergence_class(oracle_values, tsrs_values, oracle_rendered, tsrs_rendered);
    let oracle_rows = canonical_rows(oracle_values);
    let tsrs_rows = canonical_rows(tsrs_values);
    let one_sided = multiset_symmetric_rows(&oracle_rows, &tsrs_rows)?
        .into_iter()
        .map(|value| {
            format!(
                "{}:{}",
                value["code"].as_u64().unwrap_or(0),
                value["head"].as_str().unwrap_or("")
            )
        })
        .collect::<Vec<_>>();
    let side = match (
        multiset_has_difference(&oracle_rows, &tsrs_rows),
        multiset_has_difference(&tsrs_rows, &oracle_rows),
    ) {
        (true, true) => "both",
        (true, false) => "oracle-only",
        (false, true) => "tsrs-only",
        (false, false) => "both",
    };
    let first_key = first_affected_diagnostic_key(oracle_values, tsrs_values);
    let canonical = format!(
        "schema=1;tier=t4;pass=aggregate;side={side};renderer={renderer_class};key={first_key};rows={}",
        one_sided.join("|")
    );
    Ok(format!(
        "fuzzsig:{}:{}",
        sha256_bytes(canonical.as_bytes()),
        canonical
    ))
}

fn renderer_divergence_class(
    oracle_values: &[Value],
    tsrs_values: &[Value],
    oracle_rendered: &str,
    tsrs_rendered: &str,
) -> &'static str {
    let oracle_rows = oracle_values
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()
        .expect("serde_json::Value serializes");
    let tsrs_rows = tsrs_values
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()
        .expect("serde_json::Value serializes");
    let oracle_multiset = {
        let mut rows = oracle_rows.clone();
        rows.sort();
        rows
    };
    let tsrs_multiset = {
        let mut rows = tsrs_rows.clone();
        rows.sort();
        rows
    };
    if oracle_multiset == tsrs_multiset && oracle_rows != tsrs_rows {
        return "order";
    }
    let oracle_set = oracle_multiset.iter().collect::<BTreeSet<_>>();
    let tsrs_set = tsrs_multiset.iter().collect::<BTreeSet<_>>();
    if oracle_set == tsrs_set && oracle_multiset != tsrs_multiset {
        return "dedupe";
    }
    if normalize_rendered_paths(oracle_rendered) == normalize_rendered_paths(tsrs_rendered) {
        return "path";
    }
    if normalize_rendered_newlines(oracle_rendered) == normalize_rendered_newlines(tsrs_rendered) {
        return "newline";
    }
    "text"
}

fn multiset_has_difference(left: &[String], right: &[String]) -> bool {
    let mut right_counts = BTreeMap::<&str, usize>::new();
    for row in right {
        *right_counts.entry(row).or_default() += 1;
    }
    for row in left {
        let count = right_counts.entry(row).or_default();
        if *count == 0 {
            return true;
        }
        *count -= 1;
    }
    false
}

fn multiset_symmetric_rows(
    oracle: &[String],
    tsrs: &[String],
) -> Result<Vec<Value>, Box<dyn Error>> {
    let mut counts = BTreeMap::<&str, i64>::new();
    for row in oracle {
        *counts.entry(row).or_default() += 1;
    }
    for row in tsrs {
        *counts.entry(row).or_default() -= 1;
    }
    let mut values = Vec::new();
    for (row, count) in counts {
        for _ in 0..count.unsigned_abs() {
            values.push(serde_json::from_str(row)?);
        }
    }
    Ok(values)
}

fn first_affected_diagnostic_key(oracle: &[Value], tsrs: &[Value]) -> String {
    oracle
        .iter()
        .zip(tsrs)
        .find(|(oracle, tsrs)| oracle != tsrs)
        .map(|(oracle, _)| oracle)
        .or_else(|| oracle.get(tsrs.len()))
        .or_else(|| tsrs.get(oracle.len()))
        .or_else(|| oracle.first())
        .or_else(|| tsrs.first())
        .map(|value| {
            format!(
                "{}:{}",
                value["code"].as_u64().unwrap_or(0),
                value["head"].as_str().unwrap_or("")
            )
        })
        .unwrap_or_else(|| "none".to_owned())
}

fn normalize_rendered_paths(rendered: &str) -> String {
    let mut normalized = String::with_capacity(rendered.len());
    for line in rendered.split_inclusive('\n') {
        let (body, newline) = line
            .strip_suffix('\n')
            .map_or((line, ""), |body| (body, "\n"));
        normalized.push_str(&normalize_rendered_location_line(body));
        normalized.push_str(newline);
    }
    normalized
}

fn normalize_rendered_location_line(line: &str) -> String {
    let (location, suffix) = line
        .split_once(" - ")
        .map_or((line, ""), |(location, suffix)| (location, suffix));
    let Some((before_column, column)) = location.rsplit_once(':') else {
        return line.to_owned();
    };
    let Some((path, row)) = before_column.rsplit_once(':') else {
        return line.to_owned();
    };
    if !row.bytes().all(|byte| byte.is_ascii_digit())
        || !column.bytes().all(|byte| byte.is_ascii_digit())
        || row.is_empty()
        || column.is_empty()
    {
        return line.to_owned();
    }
    let indent_len = path.len() - path.trim_start_matches(' ').len();
    let indent = &path[..indent_len];
    if suffix.is_empty() {
        format!("{indent}<path>:{row}:{column}")
    } else {
        format!("{indent}<path>:{row}:{column} - {suffix}")
    }
}

fn normalize_rendered_newlines(rendered: &str) -> String {
    rendered.replace("\r\n", "\n").replace('\r', "\n")
}

fn divergence_signature(
    tier: &str,
    oracle: &[Value],
    tsrs: &[Value],
) -> Result<String, Box<dyn Error>> {
    let oracle_bytes = oracle
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<BTreeSet<_>, _>>()?;
    let tsrs_bytes = tsrs
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<BTreeSet<_>, _>>()?;
    let side = match (
        oracle_bytes.difference(&tsrs_bytes).next().is_some(),
        tsrs_bytes.difference(&oracle_bytes).next().is_some(),
    ) {
        (true, true) => "both",
        (true, false) => "oracle-only",
        (false, true) => "tsrs-only",
        (false, false) => "none",
    };
    let one_sided = oracle_bytes
        .symmetric_difference(&tsrs_bytes)
        .map(|record| {
            let value: Value = serde_json::from_str(record)?;
            Ok(format!(
                "{}:{}",
                value["code"].as_u64().unwrap_or(0),
                value["head"].as_str().unwrap_or("")
            ))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let canonical = format!(
        "schema=1;tier={tier};pass=aggregate;side={side};rows={}",
        one_sided.join("|")
    );
    Ok(format!(
        "fuzzsig:{}:{}",
        sha256_bytes(canonical.as_bytes()),
        canonical
    ))
}

fn generated_source(seed: u64, case: usize) -> String {
    let n = seed.wrapping_add(case as u64);
    let name = format!("v{}", n % 17);
    match case % 8 {
        0 => format!("let {name}: number = \"x\";\n"),
        1 => format!("function f{case}(x: string): number {{ return x; }}\n"),
        2 => format!(
            "type C{case}<T> = T extends string ? 1 : 2;\nlet {name}: C{case}<string> = 2;\n"
        ),
        3 => format!(
            "type M{case}<T> = {{ [K in keyof T]: T[K] }};\nlet {name}: M{case}<{{a:number}}> = {{a:\"x\"}};\n"
        ),
        4 => format!(
            "interface I{case} {{ a: number }}\nconst {name}: I{case} = {{ a: \"x\" }};\n"
        ),
        5 => format!(
            "function g{case}<T>(x: T, y: T): T {{ return x; }}\ng{case}(1, \"x\");\n"
        ),
        6 => format!(
            "// @allowJS: true\n\
             // @checkJs: true\n\
             // @filename: /main.js\n\
             /**\n\
              * @typedef {{Object}} SatisfiesTarget{case}\n\
              * @property {{number}} required{case}\n\
              */\n\
             const satisfiesValue{case} = /** @satisfies {{SatisfiesTarget{case}}} */ ({{}});\n"
        ),
        _ => format!(
            "declare function o{case}(a: number): void;\ndeclare function o{case}(a: string): void;\no{case}(true);\n"
        ),
    }
}

fn reduce_source_preserving_signature(
    source: &str,
    signature: &str,
    vendor_lib_dir: &Path,
    out_dir: &Path,
    pool: &tsc_oracle::OraclePool,
    mutation_canary: bool,
) -> Result<String, Box<dyn Error>> {
    let mut current = source.to_owned();
    let lines = source.lines().map(str::to_owned).collect::<Vec<_>>();
    for index in (0..lines.len()).rev() {
        let candidate = lines
            .iter()
            .enumerate()
            .filter(|(line, _)| *line != index)
            .map(|(_, line)| line.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if candidate.trim().is_empty() {
            continue;
        }
        let program = match tsc_harness::expand_fixture_text(
            "main.ts",
            &(candidate.clone() + "\n"),
            vendor_lib_dir,
        ) {
            Ok(programs) => match programs.into_iter().next() {
                Some(program) => program,
                None => continue,
            },
            Err(_) => continue,
        };
        let paths = tsc_harness::write_program_jsons(std::slice::from_ref(&program), out_dir)?;
        let comparison = compare_program_with_mutation_canary(
            &program,
            &paths[0],
            vendor_lib_dir,
            pool,
            mutation_canary,
        )?;
        if comparison.signature.as_deref() == Some(signature) {
            current = candidate + "\n";
        }
    }
    Ok(current)
}

fn verify_fuzzer_raw(artifact: &FuzzerArtifact) -> Result<(), Box<dyn Error>> {
    let natural_signatures = artifact
        .cases
        .iter()
        .filter_map(|case| case.divergence_signature.as_ref())
        .collect::<BTreeSet<_>>();
    let reducer_source_is_valid = if artifact.reducer.mutation_canary {
        natural_signatures.is_empty()
    } else {
        artifact
            .reducer
            .original_signature
            .as_ref()
            .is_some_and(|signature| natural_signatures.contains(signature))
    };
    if artifact.header.schema != ARTIFACT_SCHEMA
        || artifact.requested_cases == 0
        || artifact.cases.len() != artifact.requested_cases
        || artifact
            .cases
            .iter()
            .any(|case| case.compared_tiers != ["t0", "t1", "t2", "t3", "t4"])
        || artifact.cases.iter().any(|case| {
            let (Some(oracle), Some(tsrs)) = (&case.oracle_rendered, &case.tsrs_rendered) else {
                return true;
            };
            sha256_bytes(oracle.as_bytes()) != case.oracle_sha256
                || sha256_bytes(tsrs.as_bytes()) != case.tsrs_sha256
        })
        || !artifact.reducer.exercised
        || !reducer_source_is_valid
        || artifact.reducer.original_signature != artifact.reducer.reduced_signature
        || !artifact.dedupe.exercised
        || artifact.dedupe.observed_signatures.len() <= artifact.dedupe.unique_signatures.len()
    {
        return Err(
            "fuzzer raw observations do not prove generation/comparison/reducer/dedupe".into(),
        );
    }
    Ok(())
}

fn produce_ci_conformance_outputs(
    workspace: &Path,
    paths: &CiConformancePaths,
) -> Result<tsc_conformance::CiConformanceSummaries, Box<dyn Error>> {
    let summaries = tsc_conformance::run_ci_conformance(
        workspace,
        [&paths.all, &paths.two_xxx, &paths.syntactic],
        &paths.families,
        super::conformance_checker_workers(workspace)?,
        |summary| {
            let path = match summary.band.as_str() {
                "all" => &paths.all,
                "2xxx" => &paths.two_xxx,
                "syntactic" => &paths.syntactic,
                _ => &paths.syntactic,
            };
            super::print_conformance_summary(summary, path);
        },
    )?;
    // `run_ci_conformance` streams every summary directly to its receipt-bound
    // path. Do not serialize them again here: on hosted x64 runners these
    // machine-consumed summaries are large enough for the duplicate pretty
    // encoding to dominate the fused producer's wall time.
    Ok(summaries)
}

/// Hidden B4 child entrypoint. It performs only the fixed full-corpus CI
/// conformance producer; the parent owns timing, cache-off smoke, receipt
/// publication, and the move-only reuse token.
pub(crate) fn perf_ci_conformance_child(
    args: impl Iterator<Item = String>,
) -> Result<(), Box<dyn Error>> {
    let workspace = super::find_workspace_root()?;
    let config = read_config(&workspace)?;
    let paths = ci_conformance_paths(&workspace, &config)?;

    // Invalidate discoverable success and generated data before even
    // validating child arguments/cache mode. The child has no publication
    // guard, so only the timing parent can mint the replacement receipt.
    invalidate_published_files(
        &workspace,
        [
            &paths.receipt,
            &paths.all,
            &paths.two_xxx,
            &paths.syntactic,
            &paths.families,
        ],
    )?;
    if let Some(argument) = args.into_iter().next() {
        return Err(format!("unexpected perf ci-conformance-child argument: {argument}").into());
    }
    if std::env::var_os("TSRS_LIB_BUNDLE_CACHE").is_some_and(|value| value == "0") {
        return Err(
            "cache-off execution is forbidden from producing CI conformance outputs".into(),
        );
    }

    produce_ci_conformance_outputs(&workspace, &paths)?;
    Ok(())
}

pub(crate) fn perf_conformance(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let workspace = super::find_workspace_root()?;
    let config = read_config(&workspace)?;
    let mut artifact = None;
    let mut runner = None;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--artifact" => {
                artifact = Some(PathBuf::from(
                    args.next().ok_or("missing value after --artifact")?,
                ))
            }
            "--runner-profile" => {
                runner = Some(args.next().ok_or("missing value after --runner-profile")?)
            }
            _ => return Err(format!("unexpected perf conformance argument: {arg}").into()),
        }
    }
    let artifact = artifact.ok_or("perf conformance requires --artifact")?;
    produce_performance(&workspace, &config, runner.as_deref(), &artifact).map(drop)
}

/// The pure decision core of `reuse_performance`: every recorded term of the
/// standing artifact is compared against the CURRENT runner policy and the
/// freshly recomputed total fingerprint. Content-addressed like B2 — the
/// exact fingerprint (which includes the producer executable) owns reuse and
/// the mint commit stays as provenance, so `artifact_header_matches` runs
/// with no HEAD requirement.
fn performance_reuse_miss(
    artifact: &PerformanceArtifact,
    runner: &RunnerProfile,
    fingerprint: &Fingerprint,
    command: &str,
) -> Option<&'static str> {
    if artifact.runner != *runner {
        return Some("runner profile changed");
    }
    if artifact.observed_os != std::env::consts::OS
        || artifact.observed_arch != std::env::consts::ARCH
    {
        return Some("platform changed");
    }
    if !artifact_header_matches(&artifact.header, "performance", command, fingerprint, None) {
        return Some("fingerprint or header mismatch");
    }
    if artifact.wall_seconds < 0.0
        || artifact.wall_seconds > runner.ceiling_wall_seconds
        || artifact.max_rss_bytes == 0
        || artifact.max_rss_bytes > runner.ceiling_rss_bytes
        || artifact.cache_off_smoke.exit_status != 0
        || artifact.cache_off_smoke.fixture_limit != CACHE_OFF_SMOKE_LIMIT
    {
        return Some("recorded observation violates the current runner policy");
    }
    None
}

/// gate-tax 4: the PerformanceArtifact is the cross-run receipt for the B4
/// conformance/performance execution. Reuse is licensed exactly like the B2
/// runtime artifact — a freshly recomputed total fingerprint (all compiler
/// and xtask sources, corpus, ratchet/scope anchors, and the producer
/// executable) must equal the recorded one, every CI conformance output must
/// byte-verify, and the recorded observation must satisfy the CURRENT runner
/// profile's ceilings. On a hit the verified outputs are re-bound through
/// the unchanged in-process move-only receipt flow; only the timed child
/// executions are skipped. Any validation failure prints the miss reason and
/// returns None, and the caller invalidates and produces in full.
fn reuse_performance(
    workspace: &Path,
    config: &EvidenceConfig,
    artifact_path: &Path,
) -> Result<Option<ProducedPerformance>, Box<dyn Error>> {
    fn miss(reason: &str) -> Result<Option<ProducedPerformance>, Box<dyn Error>> {
        println!("b4 conformance: full run ({reason})");
        Ok(None)
    }
    let bytes = match fs::read(artifact_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return miss("performance artifact absent");
        }
        Err(error) => return Err(error.into()),
    };
    let Ok(mut artifact) = serde_json::from_slice::<PerformanceArtifact>(&bytes) else {
        return miss("performance artifact invalid");
    };
    let profile_id = std::env::var("TSRS_M8_RUNNER_PROFILE")
        .ok()
        .unwrap_or_else(|| config.performance.default_runner_profile.clone());
    let Some(runner) = config
        .performance
        .runners
        .iter()
        .find(|profile| profile.id == profile_id)
    else {
        return miss("runner profile unknown");
    };
    let fingerprint = performance_fingerprint(workspace)?;
    let command = format!(
        "cargo xtask perf conformance --artifact {} --runner-profile {}",
        artifact_path.display(),
        runner.id
    );
    if let Some(reason) = performance_reuse_miss(&artifact, runner, &fingerprint, &command) {
        return miss(reason);
    }
    if verify_ci_conformance_binding(workspace, config, &artifact.ci_conformance).is_err() {
        return miss("CI conformance outputs failed byte verification");
    }
    // Rebind the verified outputs through the unchanged move-only flow: a
    // fresh in-process receipt replaces the mint run's, and the artifact's
    // binding tracks it so the manifest/artifact binding equality holds.
    let ci_paths = ci_conformance_paths(workspace, config)?;
    let invocation = ci_conformance_invocation(workspace, &ci_paths, &fingerprint)?;
    let publication_guard = crate::ci_conformance_receipt::begin(&invocation)?;
    let receipt_token = crate::ci_conformance_receipt::publish(publication_guard, &invocation)?;
    let binding = bind_ci_conformance(workspace, &ci_paths)?;
    verify_ci_conformance_binding(workspace, config, &binding)?;
    artifact.ci_conformance = binding.clone();
    write_json(workspace, artifact_path, &artifact)?;
    println!(
        "b4 conformance: receipt hit — reused artifact {} (wall={:.3}/{:.3}s rss={}/{} profile={})",
        artifact_path.display(),
        artifact.wall_seconds,
        runner.ceiling_wall_seconds,
        artifact.max_rss_bytes,
        runner.ceiling_rss_bytes,
        runner.id
    );
    Ok(Some(ProducedPerformance {
        evidence: ProducedEvidence {
            receipt_token,
            invocation,
            paths: ci_paths,
            binding: binding.clone(),
        },
        binding,
    }))
}

fn produce_performance(
    workspace: &Path,
    config: &EvidenceConfig,
    selected_runner: Option<&str>,
    artifact_path: &Path,
) -> Result<ProducedPerformance, Box<dyn Error>> {
    let started_unix_ms = now_unix_ms()?;
    let fingerprint = performance_fingerprint(workspace)?;
    let profile_id = selected_runner
        .map(str::to_owned)
        .or_else(|| std::env::var("TSRS_M8_RUNNER_PROFILE").ok())
        .unwrap_or_else(|| config.performance.default_runner_profile.clone());
    let runner = config
        .performance
        .runners
        .iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| format!("unknown M8 runner profile {profile_id:?}"))?
        .clone();
    let observed_os = std::env::consts::OS.to_owned();
    let observed_arch = std::env::consts::ARCH.to_owned();
    if runner.os != observed_os || runner.arch != observed_arch {
        return Err(format!(
            "runner profile {} requires {}/{}, observed {}/{}",
            runner.id, runner.os, runner.arch, observed_os, observed_arch
        )
        .into());
    }
    let logical_cores = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let memory_bytes = system_memory_bytes()?;
    if logical_cores < runner.minimum_logical_cores || memory_bytes < runner.minimum_memory_bytes {
        return Err(format!(
            "runner profile {} resource policy failed: cores={logical_cores}/{} memory={memory_bytes}/{}",
            runner.id, runner.minimum_logical_cores, runner.minimum_memory_bytes
        )
            .into());
    }
    let ci_paths = ci_conformance_paths(workspace, config)?;
    // The A5 report keeps its long-standing readiness path. Create both
    // receipt/output parents before `begin`: publication validates every
    // parent component and therefore intentionally refuses an implicit,
    // unchecked directory creation after the producer has started.
    ensure_workspace_directory(workspace, &artifact_dir(workspace, config)?)?;
    let families_parent = ci_paths
        .families
        .parent()
        .ok_or("CI families report path has no parent")?;
    ensure_workspace_directory(workspace, families_parent)?;
    let cache_off_out = artifact_dir(workspace, config)?.join("performance-cache-off-smoke.json");
    let artifact_path = artifact_path.to_owned();
    invalidate_published_files(
        workspace,
        [
            &artifact_path,
            &ci_paths.all,
            &ci_paths.two_xxx,
            &ci_paths.syntactic,
            &ci_paths.families,
            &cache_off_out,
        ],
    )?;
    let invocation = ci_conformance_invocation(workspace, &ci_paths, &fingerprint)?;
    let publication_guard = crate::ci_conformance_receipt::begin(&invocation)?;
    let full = timed_conformance(
        workspace,
        &runner,
        &["perf".to_owned(), "ci-conformance-child".to_owned()],
        false,
    )?;
    if full.exit_status != 0 {
        return Err(format!(
            "performance conformance failed: {}",
            String::from_utf8_lossy(&full.stderr)
        )
        .into());
    }
    let cache_off = timed_conformance(
        workspace,
        &runner,
        &[
            "conformance".to_owned(),
            "--limit".to_owned(),
            CACHE_OFF_SMOKE_LIMIT.to_string(),
            "--out-json".to_owned(),
            cache_off_out.display().to_string(),
        ],
        true,
    )?;
    if cache_off.exit_status != 0 {
        return Err(format!(
            "cache-off performance smoke failed: {}",
            String::from_utf8_lossy(&cache_off.stderr)
        )
        .into());
    }
    if full.wall_seconds > runner.ceiling_wall_seconds
        || full.max_rss_bytes > runner.ceiling_rss_bytes
    {
        return Err(format!(
            "performance observation exceeds its reviewed ceiling: wall={:.3}/{:.3}s rss={}/{}",
            full.wall_seconds,
            runner.ceiling_wall_seconds,
            full.max_rss_bytes,
            runner.ceiling_rss_bytes
        )
        .into());
    }
    let finished_fingerprint = performance_fingerprint(workspace)?;
    if finished_fingerprint != fingerprint {
        return Err(
            "performance inputs changed while the CI conformance observation was running".into(),
        );
    }
    if git_head(workspace)? != invocation.head {
        return Err("repository HEAD changed while producing CI conformance evidence".into());
    }

    // Publish the receipt only after the full child, cache-off smoke, resource
    // ceiling, and before/after input fingerprint have all passed. The token
    // never crosses the process boundary and is returned to the eventual
    // merge-gate consumer as a move-only capability.
    let receipt_token = crate::ci_conformance_receipt::publish(publication_guard, &invocation)?;
    let binding = bind_ci_conformance(workspace, &ci_paths)?;
    verify_ci_conformance_binding(workspace, config, &binding)?;
    let artifact = PerformanceArtifact {
        header: artifact_header(
            workspace,
            "performance",
            &format!(
                "cargo xtask perf conformance --artifact {} --runner-profile {}",
                artifact_path.display(),
                runner.id
            ),
            started_unix_ms,
            fingerprint,
            0,
        )?,
        runner: runner.clone(),
        observed_os,
        observed_arch,
        logical_cores,
        memory_bytes,
        wall_seconds: full.wall_seconds,
        max_rss_bytes: full.max_rss_bytes,
        child_stdout_sha256: sha256_bytes(&full.stdout),
        child_stderr_sha256: sha256_bytes(&full.stderr),
        cache_off_smoke: CacheOffObservation {
            fixture_limit: CACHE_OFF_SMOKE_LIMIT,
            wall_seconds: cache_off.wall_seconds,
            max_rss_bytes: cache_off.max_rss_bytes,
            exit_status: cache_off.exit_status,
            child_stdout_sha256: sha256_bytes(&cache_off.stdout),
            child_stderr_sha256: sha256_bytes(&cache_off.stderr),
        },
        ci_conformance: binding.clone(),
    };
    write_json(workspace, &artifact_path, &artifact)?;
    println!(
        "performance conformance: wall={:.3}/{:.3}s rss={}/{} cache-off-smoke={:.3}s/{}B profile={} artifact={}",
        full.wall_seconds,
        runner.ceiling_wall_seconds,
        full.max_rss_bytes,
        runner.ceiling_rss_bytes,
        cache_off.wall_seconds,
        cache_off.max_rss_bytes,
        runner.id,
        artifact_path.display()
    );
    Ok(ProducedPerformance {
        evidence: ProducedEvidence {
            receipt_token,
            invocation,
            paths: ci_paths,
            binding: binding.clone(),
        },
        binding,
    })
}

fn timed_conformance(
    workspace: &Path,
    runner: &RunnerProfile,
    arguments: &[String],
    cache_off: bool,
) -> Result<TimedObservation, Box<dyn Error>> {
    let executable = std::env::current_exe()?;
    let mut command = Command::new("/usr/bin/time");
    match runner.measurement_backend.as_str() {
        "bsd-time-l" => {
            command.arg("-l");
        }
        "gnu-time-v" => {
            command.arg("-v");
        }
        other => {
            return Err(format!("unsupported performance backend {other:?}").into());
        }
    }
    command
        .current_dir(workspace)
        .arg(executable)
        .args(arguments);
    if cache_off {
        command.env("TSRS_LIB_BUNDLE_CACHE", "0");
    } else {
        command.env_remove("TSRS_LIB_BUNDLE_CACHE");
    }
    let start = Instant::now();
    let output = command.output()?;
    let wall_seconds = start.elapsed().as_secs_f64();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let max_rss_bytes = parse_max_rss(&stderr, &runner.measurement_backend)?;
    Ok(TimedObservation {
        wall_seconds,
        max_rss_bytes,
        exit_status: output.status.code().unwrap_or(-1),
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

fn parse_max_rss(stderr: &str, backend: &str) -> Result<u64, Box<dyn Error>> {
    match backend {
        "bsd-time-l" => stderr
            .lines()
            .find_map(|line| {
                line.trim()
                    .strip_suffix("  maximum resident set size")
                    .or_else(|| line.trim().strip_suffix(" maximum resident set size"))
                    .and_then(|value| value.trim().parse::<u64>().ok())
            })
            .ok_or_else(|| "BSD time output lacks maximum resident set size".into()),
        "gnu-time-v" => stderr
            .lines()
            .find_map(|line| {
                line.trim()
                    .strip_prefix("Maximum resident set size (kbytes):")
                    .and_then(|value| value.trim().parse::<u64>().ok())
                    .map(|kilobytes| kilobytes * 1024)
            })
            .ok_or_else(|| "GNU time output lacks maximum resident set size".into()),
        _ => Err("unknown RSS backend".into()),
    }
}

pub(crate) struct VerifiedEvidence {
    pub(crate) runtime_ready: bool,
    pub(crate) runtime_detail: String,
    pub(crate) fuzzer_ready: bool,
    pub(crate) fuzzer_detail: String,
    pub(crate) performance_ready: bool,
    pub(crate) performance_detail: String,
}

pub(crate) fn verify_for_readiness(
    workspace: &Path,
    inventory_hash: &str,
    direct_emitter_ids: &BTreeSet<&str>,
) -> Result<VerifiedEvidence, Box<dyn Error>> {
    let config = read_config(workspace)?;
    let current_head = git_head(workspace)?;
    let manifest_path = artifact_dir(workspace, &config)?.join("manifest.json");
    let manifest: EvidenceManifest = match read_json(&manifest_path) {
        Ok(manifest) => manifest,
        Err(error) => {
            let detail = format!("missing/unreadable evidence manifest: {error}");
            return Ok(VerifiedEvidence {
                runtime_ready: false,
                runtime_detail: detail.clone(),
                fuzzer_ready: false,
                fuzzer_detail: detail.clone(),
                performance_ready: false,
                performance_detail: detail,
            });
        }
    };
    if manifest.schema != ARTIFACT_SCHEMA || manifest.producer_version != PRODUCER_VERSION {
        return Err("M8 evidence manifest has an unsupported schema/producer".into());
    }
    if manifest.producer_commit != current_head {
        return Err(format!(
            "stale M8 evidence manifest: producer HEAD {} != current HEAD {current_head}",
            manifest.producer_commit
        )
        .into());
    }

    let expected_paths = BTreeMap::from([
        (
            "runtime-coverage",
            resolve_artifact_path(workspace, &config, &config.runtime_coverage.artifact)?,
        ),
        (
            "differential-fuzzer",
            resolve_artifact_path(workspace, &config, &config.fuzzer.artifact)?,
        ),
        (
            "performance",
            resolve_artifact_path(workspace, &config, &config.performance.artifact)?,
        ),
    ]);
    if manifest.artifacts.len() != expected_paths.len() {
        return Err(format!(
            "M8 evidence manifest must contain exactly {} artifacts, found {}",
            expected_paths.len(),
            manifest.artifacts.len()
        )
        .into());
    }
    let mut entries = BTreeMap::<&str, &ManifestArtifact>::new();
    for artifact in &manifest.artifacts {
        let expected_path = expected_paths
            .get(artifact.kind.as_str())
            .ok_or_else(|| format!("M8 evidence manifest has unknown kind {:?}", artifact.kind))?;
        if entries.insert(artifact.kind.as_str(), artifact).is_some() {
            return Err(format!(
                "M8 evidence manifest repeats artifact kind {:?}",
                artifact.kind
            )
            .into());
        }
        let secured = secure_workspace_path(workspace, &artifact.path)?;
        if secured != *expected_path
            || artifact.path != workspace_relative(workspace, expected_path)?
        {
            return Err(format!(
                "M8 evidence manifest kind {:?} uses unexpected path {}",
                artifact.kind, artifact.path
            )
            .into());
        }
        if !is_sha256(&artifact.sha256) || !is_sha256(&artifact.fingerprint_sha256) {
            return Err(format!(
                "M8 evidence manifest kind {:?} has a malformed digest",
                artifact.kind
            )
            .into());
        }
    }
    verify_ci_conformance_binding(workspace, &config, &manifest.ci_conformance)?;
    let entry = |kind: &str| entries.get(kind).copied();

    let (runtime_ready, runtime_detail) = if let Some(entry) = entry("runtime-coverage") {
        let path = secure_workspace_path(workspace, &entry.path)?;
        let artifact: RuntimeArtifact = read_json(&path)?;
        let current = runtime_fingerprint(workspace)?;
        let validation =
            validate_runtime_artifact(&artifact, &current, inventory_hash, direct_emitter_ids);
        let ready = validation.ready
            && entry.sha256 == sha256_file(&path)?
            && entry.fingerprint_sha256 == artifact.header.fingerprint.sha256
            && entry.kind == artifact.header.kind;
        (
            ready,
            format!(
                "fresh={} accounted={}/{} executed={} zero-hit={} reviewed={}",
                validation.fresh,
                validation.executed + validation.reviewed,
                direct_emitter_ids.len(),
                validation.executed,
                validation.zero_hit,
                validation.reviewed
            ),
        )
    } else {
        (false, "manifest lacks runtime-coverage".to_owned())
    };

    let (fuzzer_ready, fuzzer_detail) = if let Some(entry) = entry("differential-fuzzer") {
        let path = secure_workspace_path(workspace, &entry.path)?;
        let artifact: FuzzerArtifact = read_json(&path)?;
        let current = fuzz_fingerprint(workspace, artifact.seed, artifact.requested_cases)?;
        let raw_valid = verify_fuzzer_raw(&artifact).is_ok();
        let expected_command = format!(
            "cargo xtask fuzz run --seed {} --cases {} --artifact {}",
            artifact.seed,
            artifact.requested_cases,
            path.display()
        );
        let ready = artifact_header_matches(
            &artifact.header,
            "differential-fuzzer",
            &expected_command,
            &current,
            Some(&current_head),
        ) && entry.sha256 == sha256_file(&path)?
            && entry.fingerprint_sha256 == artifact.header.fingerprint.sha256
            && raw_valid;
        (
            ready,
            format!(
                "fresh={} generated={} compared={} reducer-smoke={} signature-dedupe={}",
                artifact.header.fingerprint == current,
                artifact.cases.len(),
                artifact.cases.len(),
                artifact.reducer.exercised,
                artifact.dedupe.exercised
            ),
        )
    } else {
        (false, "manifest lacks differential-fuzzer".to_owned())
    };

    let (performance_ready, performance_detail) = if let Some(entry) = entry("performance") {
        let path = secure_workspace_path(workspace, &entry.path)?;
        let artifact: PerformanceArtifact = read_json(&path)?;
        let current = performance_fingerprint(workspace)?;
        let configured_runner = config
            .performance
            .runners
            .iter()
            .find(|runner| runner.id == artifact.runner.id);
        let expected_command = format!(
            "cargo xtask perf conformance --artifact {} --runner-profile {}",
            path.display(),
            artifact.runner.id
        );
        // gate-tax 4: the performance artifact is content-addressed like the
        // B2 runtime artifact — its exact fingerprint, not HEAD equality,
        // owns reuse across commits; the mint commit stays as provenance.
        let ready = artifact_header_matches(
            &artifact.header,
            "performance",
            &expected_command,
            &current,
            None,
        ) && entry.sha256 == sha256_file(&path)?
            && entry.fingerprint_sha256 == artifact.header.fingerprint.sha256
            && artifact.ci_conformance == manifest.ci_conformance
            && verify_ci_conformance_binding(workspace, &config, &artifact.ci_conformance).is_ok()
            && configured_runner == Some(&artifact.runner)
            && artifact.observed_os == std::env::consts::OS
            && artifact.observed_arch == std::env::consts::ARCH
            && artifact.logical_cores >= artifact.runner.minimum_logical_cores
            && artifact.memory_bytes >= artifact.runner.minimum_memory_bytes
            && artifact.wall_seconds >= 0.0
            && artifact.wall_seconds <= artifact.runner.ceiling_wall_seconds
            && artifact.runner.ceiling_wall_seconds <= 60.0
            && artifact.max_rss_bytes <= artifact.runner.ceiling_rss_bytes
            && artifact.max_rss_bytes > 0
            && is_sha256(&artifact.child_stdout_sha256)
            && is_sha256(&artifact.child_stderr_sha256)
            && artifact.cache_off_smoke.exit_status == 0
            && artifact.cache_off_smoke.fixture_limit == CACHE_OFF_SMOKE_LIMIT
            && artifact.cache_off_smoke.wall_seconds >= 0.0
            && artifact.cache_off_smoke.max_rss_bytes > 0
            && is_sha256(&artifact.cache_off_smoke.child_stdout_sha256)
            && is_sha256(&artifact.cache_off_smoke.child_stderr_sha256);
        (
            ready,
            format!(
                "fresh={} wall={:.3}/{:.3}s rss={}/{} cache-off-smoke={}@{} fixtures profile={}",
                artifact.header.fingerprint == current,
                artifact.wall_seconds,
                artifact.runner.ceiling_wall_seconds,
                artifact.max_rss_bytes,
                artifact.runner.ceiling_rss_bytes,
                artifact.cache_off_smoke.exit_status == 0,
                artifact.cache_off_smoke.fixture_limit,
                artifact.runner.id
            ),
        )
    } else {
        (false, "manifest lacks performance".to_owned())
    };
    Ok(VerifiedEvidence {
        runtime_ready,
        runtime_detail,
        fuzzer_ready,
        fuzzer_detail,
        performance_ready,
        performance_detail,
    })
}

fn read_config(workspace: &Path) -> Result<EvidenceConfig, Box<dyn Error>> {
    let config: EvidenceConfig = read_json(&workspace.join(CONFIG_REL))?;
    if config.schema != 2 {
        return Err("m8-evidence.json must be schema 2".into());
    }
    if config.runtime_coverage.max_workers == 0 {
        return Err("m8-evidence.json runtime_coverage.max_workers must be at least 1".into());
    }
    if config.runtime_coverage.programs_per_process == 0 {
        return Err(
            "m8-evidence.json runtime_coverage.programs_per_process must be at least 1".into(),
        );
    }
    if config.runtime_coverage.max_lib_cache_buckets == 0 {
        return Err(
            "m8-evidence.json runtime_coverage.max_lib_cache_buckets must be at least 1".into(),
        );
    }
    if config.runtime_coverage.diagnostic_canary_programs == 0 {
        return Err(
            "m8-evidence.json runtime_coverage.diagnostic_canary_programs must be at least 1"
                .into(),
        );
    }
    if config.workspace_tests.max_workers == 0 {
        return Err("m8-evidence.json workspace_tests.max_workers must be at least 1".into());
    }
    if config.conformance_runner.max_workers == 0 {
        return Err("m8-evidence.json conformance_runner.max_workers must be at least 1".into());
    }
    Ok(config)
}

/// The reviewed local workspace-test worker ceiling from `m8-evidence.json`.
/// tsrs-native: gate-policy accessor; no tsc counterpart.
pub(crate) fn workspace_test_worker_ceiling(workspace: &Path) -> Result<usize, Box<dyn Error>> {
    Ok(read_config(workspace)?.workspace_tests.max_workers)
}

fn artifact_dir(workspace: &Path, config: &EvidenceConfig) -> Result<PathBuf, Box<dyn Error>> {
    secure_workspace_path(workspace, &config.artifact_dir)
}

fn ci_conformance_paths(
    workspace: &Path,
    config: &EvidenceConfig,
) -> Result<CiConformancePaths, Box<dyn Error>> {
    let directory = artifact_dir(workspace, config)?;
    Ok(CiConformancePaths {
        receipt: directory.join(CI_RECEIPT_FILE),
        all: directory.join(CI_ALL_FILE),
        two_xxx: directory.join(CI_TWO_XXX_FILE),
        syntactic: directory.join(CI_SYNTACTIC_FILE),
        // M8 readiness and the standalone families command share this
        // normative path. Binding that exact file avoids copying or reopening
        // an unbound report between receipt consumption and readiness.
        families: workspace.join("target/families/report.json"),
    })
}

fn ci_conformance_invocation(
    workspace: &Path,
    paths: &CiConformancePaths,
    fingerprint: &Fingerprint,
) -> Result<crate::ci_conformance_receipt::Invocation, Box<dyn Error>> {
    let canonical_workspace = fs::canonicalize(workspace)?;
    let head = git_head(&canonical_workspace)?;
    let executable = std::env::current_exe()?;
    let producer_executable_sha256 = sha256_file(&executable)?;
    let nonce = crate::ci_conformance_receipt::fresh_nonce()?;
    let output_paths = [
        &paths.all,
        &paths.two_xxx,
        &paths.syntactic,
        &paths.families,
    ];
    let outputs = crate::ci_conformance_receipt::OUTPUT_ROLES
        .into_iter()
        .zip(output_paths)
        .map(|(role, path)| {
            Ok(crate::ci_conformance_receipt::OutputSpec {
                role: role.to_owned(),
                path: PathBuf::from(workspace_relative(workspace, path)?),
            })
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    Ok(crate::ci_conformance_receipt::Invocation {
        workspace: canonical_workspace,
        receipt_path: PathBuf::from(workspace_relative(workspace, &paths.receipt)?),
        nonce,
        head,
        producer_executable_sha256,
        fingerprint_sha256: fingerprint.sha256.clone(),
        started_unix_ms: now_unix_ms()?,
        outputs,
    })
}

fn bind_output(workspace: &Path, kind: &str, path: &Path) -> Result<BoundOutput, Box<dyn Error>> {
    let path = secure_workspace_output_path(workspace, path, true)?;
    let bytes = fs::read(&path)?;
    Ok(BoundOutput {
        kind: kind.to_owned(),
        path: workspace_relative(workspace, &path)?,
        bytes: bytes.len() as u64,
        sha256: sha256_bytes(&bytes),
    })
}

fn bind_ci_conformance(
    workspace: &Path,
    paths: &CiConformancePaths,
) -> Result<CiConformanceBinding, Box<dyn Error>> {
    Ok(CiConformanceBinding {
        receipt: bind_output(workspace, "receipt", &paths.receipt)?,
        outputs: vec![
            bind_output(workspace, CI_OUTPUT_KINDS[0], &paths.all)?,
            bind_output(workspace, CI_OUTPUT_KINDS[1], &paths.two_xxx)?,
            bind_output(workspace, CI_OUTPUT_KINDS[2], &paths.syntactic)?,
            bind_output(workspace, CI_OUTPUT_KINDS[3], &paths.families)?,
        ],
    })
}

fn verify_bound_output(
    workspace: &Path,
    observed: &BoundOutput,
    expected_kind: &str,
    expected_path: &Path,
) -> Result<(), Box<dyn Error>> {
    if observed.kind != expected_kind
        || observed.path != workspace_relative(workspace, expected_path)?
        || !is_sha256(&observed.sha256)
    {
        return Err(format!(
            "invalid CI conformance binding for {expected_kind}: kind={:?} path={:?}",
            observed.kind, observed.path
        )
        .into());
    }
    let secured = secure_workspace_output_path(workspace, &workspace.join(&observed.path), true)?;
    let expected = secure_workspace_output_path(workspace, expected_path, true)?;
    if secured != expected {
        return Err(format!(
            "CI conformance binding for {expected_kind} resolved to an unexpected path"
        )
        .into());
    }
    let bytes = fs::read(&secured)?;
    if observed.bytes != bytes.len() as u64 || observed.sha256 != sha256_bytes(&bytes) {
        return Err(format!(
            "CI conformance binding for {expected_kind} failed byte/digest verification"
        )
        .into());
    }
    Ok(())
}

fn verify_ci_conformance_binding(
    workspace: &Path,
    config: &EvidenceConfig,
    binding: &CiConformanceBinding,
) -> Result<(), Box<dyn Error>> {
    let paths = ci_conformance_paths(workspace, config)?;
    verify_bound_output(workspace, &binding.receipt, "receipt", &paths.receipt)?;
    if binding.outputs.len() != CI_OUTPUT_KINDS.len() {
        return Err(format!(
            "CI conformance binding requires {} ordered outputs, found {}",
            CI_OUTPUT_KINDS.len(),
            binding.outputs.len()
        )
        .into());
    }
    for ((observed, expected_kind), expected_path) in binding
        .outputs
        .iter()
        .zip(CI_OUTPUT_KINDS)
        .zip([paths.all, paths.two_xxx, paths.syntactic, paths.families])
    {
        verify_bound_output(workspace, observed, expected_kind, &expected_path)?;
    }
    Ok(())
}

fn resolve_artifact_path(
    workspace: &Path,
    config: &EvidenceConfig,
    path: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let base = artifact_dir(workspace, config)?;
    let candidate = Path::new(path);
    let path = if candidate.is_absolute() {
        candidate.to_owned()
    } else {
        base.join(candidate)
    };
    if !path.starts_with(workspace) {
        return Err(format!("M8 evidence artifact escapes workspace: {}", path.display()).into());
    }
    Ok(path)
}

fn canonical_workspace_candidate(
    workspace: &Path,
    path: &Path,
) -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
    let canonical_workspace = workspace.canonicalize()?;
    let relative = path
        .strip_prefix(workspace)
        .or_else(|_| path.strip_prefix(&canonical_workspace))
        .map_err(|_| format!("M8 evidence path escapes workspace: {}", path.display()))?
        .to_owned();
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "M8 evidence path must be a lexical workspace path: {}",
            path.display()
        )
        .into());
    }
    Ok((canonical_workspace, relative))
}

/// Resolves a configured workspace output without following a symlink in
/// any existing component. A missing suffix is permitted only for a future
/// output; a consumed output must already be a regular file.
fn secure_workspace_output_path(
    workspace: &Path,
    path: &Path,
    require_file: bool,
) -> Result<PathBuf, Box<dyn Error>> {
    let (canonical_workspace, relative) = canonical_workspace_candidate(workspace, path)?;
    if relative.as_os_str().is_empty() {
        return Err("M8 evidence output cannot be the workspace root".into());
    }
    let components = relative
        .components()
        .map(|component| match component {
            Component::Normal(name) => name.to_owned(),
            _ => unreachable!("components were validated above"),
        })
        .collect::<Vec<_>>();
    let mut current = canonical_workspace;
    let mut missing = false;
    for (index, component) in components.iter().enumerate() {
        current.push(component);
        if missing {
            continue;
        }
        let last = index + 1 == components.len();
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(format!(
                        "M8 evidence path contains a symlink: {}",
                        current.display()
                    )
                    .into());
                }
                if !last && !metadata.is_dir() {
                    return Err(format!(
                        "M8 evidence parent is not a directory: {}",
                        current.display()
                    )
                    .into());
                }
                if last && !metadata.is_file() {
                    return Err(format!(
                        "M8 evidence output is not a regular file: {}",
                        current.display()
                    )
                    .into());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if require_file {
                    return Err(
                        format!("M8 evidence output is missing: {}", current.display()).into(),
                    );
                }
                missing = true;
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(current)
}

/// Creates a workspace-internal directory one component at a time, checking
/// each existing or newly-created component before descending into it.
fn ensure_workspace_directory(workspace: &Path, path: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let (canonical_workspace, relative) = canonical_workspace_candidate(workspace, path)?;
    let mut current = canonical_workspace;
    for component in relative.components() {
        let Component::Normal(name) = component else {
            unreachable!("components were validated above")
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(format!(
                        "M8 evidence directory is not a real directory: {}",
                        current.display()
                    )
                    .into());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match fs::create_dir(&current) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error.into()),
                }
                let metadata = fs::symlink_metadata(&current)?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(format!(
                        "M8 evidence directory changed during creation: {}",
                        current.display()
                    )
                    .into());
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(current)
}

fn secure_workspace_path(workspace: &Path, path: &str) -> Result<PathBuf, Box<dyn Error>> {
    let candidate = Path::new(path);
    if candidate.is_absolute() || candidate.components().any(|part| part.as_os_str() == "..") {
        return Err(format!("M8 evidence path must be workspace-relative: {path}").into());
    }
    Ok(workspace.join(candidate))
}

fn runtime_fingerprint(workspace: &Path) -> Result<Fingerprint, Box<dyn Error>> {
    fingerprint(
        workspace,
        &[
            CONFIG_REL,
            INVENTORY_REL,
            ".node-version",
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            "crates/oracle/coverage-instrument.mjs",
            "crates/oracle/coverage-driver.mjs",
            "crates/oracle/driver.mjs",
            "crates/oracle/program-host.mjs",
            "crates/oracle/emitter-inventory.mjs",
            "crates/harness/src",
            "crates/harness/Cargo.toml",
            "crates/xtask/Cargo.toml",
            "crates/xtask/src/m8_evidence.rs",
            "ts-tests/tests/cases/conformance",
            "vendor/typescript-6.0.3/lib/_tsc.js",
            "vendor/typescript-6.0.3/lib",
        ],
        &[],
        false,
    )
}

fn fuzz_fingerprint(
    workspace: &Path,
    seed: u64,
    cases: usize,
) -> Result<Fingerprint, Box<dyn Error>> {
    fingerprint(
        workspace,
        &[
            CONFIG_REL,
            ".node-version",
            "Cargo.lock",
            "rust-toolchain.toml",
            "crates/checker/src",
            "crates/conformance/src",
            "crates/fuzz/src",
            "crates/harness/src",
            "crates/oracle/driver.mjs",
            "crates/oracle/program-host.mjs",
            "crates/oracle/src",
            "crates/xtask/src/m8_evidence.rs",
            "vendor/typescript-6.0.3/lib/_tsc.js",
            "vendor/typescript-6.0.3/lib/typescript.js",
            "vendor/typescript-6.0.3/lib",
        ],
        &[format!("seed={seed}"), format!("cases={cases}")],
        true,
    )
}

fn performance_fingerprint(workspace: &Path) -> Result<Fingerprint, Box<dyn Error>> {
    fingerprint(
        workspace,
        &[
            CONFIG_REL,
            ".node-version",
            ".cargo/config.toml",
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            "ratchet.toml",
            "m8-scope.json",
            "diag-families.json",
            "ratchets/oracle-inputs.v1.json.zst",
            "ratchets/conformance-matches.v1.json.zst",
            "pins/recovery.json",
            "crates/binder/src",
            "crates/checker/src",
            "crates/compiler/src",
            "crates/conformance/src",
            "crates/diagnostics/src",
            "crates/harness/src",
            "crates/program/src",
            "crates/syntax/src",
            "crates/types/src",
            "crates/xtask/src",
            "goldens",
            "ts-tests/tests/cases/conformance",
            "vendor/typescript-6.0.3/lib",
        ],
        &[
            "command=perf ci-conformance-child".to_owned(),
            "full-corpus=true".to_owned(),
            "views=all,2xxx,syntactic".to_owned(),
            "families-report=true".to_owned(),
            "lib-bundle-cache=enabled".to_owned(),
        ],
        true,
    )
}

fn fingerprint(
    workspace: &Path,
    relative_inputs: &[&str],
    arguments: &[String],
    include_executable: bool,
) -> Result<Fingerprint, Box<dyn Error>> {
    let mut files = Vec::new();
    for relative in relative_inputs {
        collect_files(workspace, &workspace.join(relative), &mut files)?;
    }
    files.sort();
    files.dedup();
    let mut inputs = files
        .iter()
        .map(|path| {
            Ok(InputEntry {
                path: workspace_relative(workspace, path)?,
                sha256: sha256_file(path)?,
            })
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    if include_executable {
        let executable = std::env::current_exe()?;
        inputs.push(InputEntry {
            path: "<producer-executable>".to_owned(),
            sha256: sha256_file(&executable)?,
        });
    }
    for argument in arguments {
        inputs.push(InputEntry {
            path: format!("<argument:{argument}>"),
            sha256: sha256_bytes(argument.as_bytes()),
        });
    }
    inputs.sort_by(|left, right| left.path.cmp(&right.path));
    let sha256 = sha256_bytes(&serde_json::to_vec(&inputs)?);
    Ok(Fingerprint { sha256, inputs })
}

fn collect_files(
    workspace: &Path,
    path: &Path,
    out: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn Error>> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        format!(
            "fingerprint input is missing or unreadable: {}: {error}",
            path.strip_prefix(workspace).unwrap_or(path).display()
        )
    })?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "fingerprint input symlink is forbidden: {}",
            path.strip_prefix(workspace).unwrap_or(path).display()
        )
        .into());
    }
    if metadata.is_file() {
        if !path.starts_with(workspace) {
            return Err(format!("fingerprint input escaped workspace: {}", path.display()).into());
        }
        out.push(path.to_owned());
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(format!(
            "unsupported fingerprint input: {}",
            path.strip_prefix(workspace).unwrap_or(path).display()
        )
        .into());
    }
    let mut entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let child = entry.path();
        collect_files(workspace, &child, out)?;
    }
    Ok(())
}

fn artifact_header(
    workspace: &Path,
    kind: &str,
    command: &str,
    started_unix_ms: u128,
    fingerprint: Fingerprint,
    exit_status: i32,
) -> Result<ArtifactHeader, Box<dyn Error>> {
    Ok(ArtifactHeader {
        schema: ARTIFACT_SCHEMA,
        producer_version: PRODUCER_VERSION.to_owned(),
        kind: kind.to_owned(),
        producer_commit: git_head(workspace)?,
        command: command.to_owned(),
        started_unix_ms,
        finished_unix_ms: now_unix_ms()?,
        exit_status,
        fingerprint,
    })
}

fn validate_inventory(workspace: &Path, inventory: &Inventory) -> Result<(), Box<dyn Error>> {
    if inventory.schema != 2
        || inventory.status != "draft/report-only"
        || inventory.band != "all"
        || inventory.summary.emitter_declarations
            != inventory
                .functions
                .iter()
                .filter(|function| function.direct_emitter)
                .count()
        || inventory.source_sha256
            != sha256_file(&workspace.join("vendor/typescript-6.0.3/lib/_tsc.js"))?
    {
        return Err("runtime coverage requires a fresh schema-2 all-band D2 inventory".into());
    }
    Ok(())
}

fn expand_corpus_programs(
    workspace: &Path,
    out_root: &Path,
) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let root = workspace.join("ts-tests/tests/cases/conformance");
    let vendor = workspace.join("vendor/typescript-6.0.3/lib");
    let mut fixtures = Vec::new();
    collect_ts_fixtures(&root, &mut fixtures)?;
    fixtures.sort();
    let mut paths = Vec::new();
    for (index, fixture) in fixtures.iter().enumerate() {
        let programs = tsc_harness::expand_fixture_file(fixture, &vendor)?;
        paths.extend(tsc_harness::write_program_jsons(
            &programs,
            &out_root.join(index.to_string()),
        )?);
    }
    Ok(paths)
}

fn collect_ts_fixtures(path: &Path, out: &mut Vec<PathBuf>) -> Result<(), Box<dyn Error>> {
    let mut entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let child = entry.path();
        if child.is_dir() {
            collect_ts_fixtures(&child, out)?;
        } else if child.extension().and_then(|extension| extension.to_str()) == Some("ts") {
            out.push(child);
        }
    }
    Ok(())
}

fn ensure_relevant_tree_clean(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let git_root = git_root(workspace)?;
    let relative = workspace_git_pathspec(&git_root, workspace)?;
    let output = Command::new("git")
        .arg("-C")
        .arg(&git_root)
        .args(["status", "--porcelain", "--"])
        .arg(&relative)
        .output()?;
    if !output.status.success() {
        return Err("failed to inspect relevant working-tree cleanliness".into());
    }
    if !output.stdout.is_empty() {
        return Err(
            "M8 evidence production requires a clean workspace tree; commit the reviewed inputs \
             first"
                .into(),
        );
    }
    Ok(())
}

fn workspace_git_pathspec(git_root: &Path, workspace: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let relative = workspace.strip_prefix(git_root)?;
    if relative.as_os_str().is_empty() {
        Ok(PathBuf::from("."))
    } else {
        Ok(relative.to_owned())
    }
}

fn git_root(workspace: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(["rev-parse", "--show-toplevel"])
        .output()?;
    if !output.status.success() {
        return Err("failed to resolve git root".into());
    }
    Ok(PathBuf::from(String::from_utf8(output.stdout)?.trim()))
}

fn git_head(workspace: &Path) -> Result<String, Box<dyn Error>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !output.status.success() {
        return Err("failed to resolve producer commit".into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn system_memory_bytes() -> Result<u64, Box<dyn Error>> {
    match std::env::consts::OS {
        "macos" => {
            let output = Command::new("sysctl").args(["-n", "hw.memsize"]).output()?;
            if !output.status.success() {
                return Err("sysctl hw.memsize failed".into());
            }
            Ok(String::from_utf8(output.stdout)?.trim().parse()?)
        }
        "linux" => {
            let text = fs::read_to_string("/proc/meminfo")?;
            let kilobytes = text
                .lines()
                .find_map(|line| line.strip_prefix("MemTotal:"))
                .and_then(|value| value.split_whitespace().next())
                .ok_or("/proc/meminfo lacks MemTotal")?
                .parse::<u64>()?;
            Ok(kilobytes * 1024)
        }
        other => Err(format!("unsupported performance OS {other:?}").into()),
    }
}

fn workspace_relative(workspace: &Path, path: &Path) -> Result<String, Box<dyn Error>> {
    let canonical_workspace = workspace.canonicalize()?;
    let relative = path
        .strip_prefix(workspace)
        .or_else(|_| path.strip_prefix(&canonical_workspace))?;
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, Box<dyn Error>> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn write_json<T: Serialize>(
    workspace: &Path,
    path: &Path,
    value: &T,
) -> Result<(), Box<dyn Error>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    atomic_write(workspace, path, &bytes)
}

fn atomic_write(workspace: &Path, path: &Path, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    let canonical_workspace = workspace.canonicalize()?;
    let internal = path.starts_with(workspace) || path.starts_with(&canonical_workspace);
    let path = if internal {
        secure_workspace_output_path(workspace, path, false)?
    } else {
        // Explicit standalone CLI artifact paths retain their historical
        // behavior. All configured/manifest/receipt-bound evidence is
        // workspace-internal and takes the symlink-safe branch above.
        path.to_owned()
    };
    let parent = path
        .parent()
        .ok_or_else(|| format!("artifact path has no parent: {}", path.display()))?;
    if internal {
        ensure_workspace_directory(workspace, parent)?;
    } else {
        fs::create_dir_all(parent)?;
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("artifact path has no UTF-8 file name: {}", path.display()))?;
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        counter
    ));
    let result = (|| -> Result<(), Box<dyn Error>> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, &path)?;
        if internal {
            let published = secure_workspace_output_path(workspace, &path, true)?;
            if fs::read(&published)? != bytes {
                return Err("workspace evidence changed during atomic publication".into());
            }
            fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn invalidate_published_files<'a>(
    workspace: &Path,
    paths: impl IntoIterator<Item = &'a PathBuf>,
) -> Result<(), Box<dyn Error>> {
    let canonical_workspace = workspace.canonicalize()?;
    for path in paths {
        let internal = path.starts_with(workspace) || path.starts_with(&canonical_workspace);
        let secured = if internal {
            secure_workspace_output_path(workspace, path, false)?
        } else {
            // This can only be an explicit standalone `--artifact` path; the
            // fixed CI outputs and manifest are always workspace-internal.
            path.to_owned()
        };
        match fs::symlink_metadata(&secured) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(format!(
                    "refusing to invalidate non-regular evidence {}",
                    secured.display()
                )
                .into())
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        }
        match fs::remove_file(&secured) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "failed to invalidate published evidence {}: {error}",
                    secured.display()
                )
                .into());
            }
        }
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, Box<dyn Error>> {
    Ok(sha256_bytes(&fs::read(path)?))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn now_unix_ms() -> Result<u128, Box<dyn Error>> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())
}

#[cfg(test)]
#[path = "../tests/unit/m8_evidence/tests.rs"]
mod tests;

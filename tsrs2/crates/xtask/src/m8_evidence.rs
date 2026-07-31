use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tsrs2_checker::{check_program_with_libs_at, InputFile};
use tsrs2_diags::{
    format_diagnostics_with_context, Diagnostic, FormatDiagnosticsHost, MessageChain, RelatedInfo,
};

const CONFIG_REL: &str = "m8-evidence.json";
const INVENTORY_REL: &str = "m8-emitter-inventory.json";
const ARTIFACT_SCHEMA: u32 = 1;
const PRODUCER_VERSION: &str = "m8-evidence-v1";

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct EvidenceConfig {
    schema: u32,
    artifact_dir: String,
    runtime_coverage: RuntimeConfig,
    fuzzer: FuzzerConfig,
    performance: PerformanceConfig,
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

#[derive(Clone, Debug, Deserialize, Serialize)]
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
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CacheOffObservation {
    fixture_limit: usize,
    wall_seconds: f64,
    max_rss_bytes: u64,
    exit_status: i32,
    child_stdout_sha256: String,
    child_stderr_sha256: String,
}

struct TimedObservation {
    wall_seconds: f64,
    max_rss_bytes: u64,
    exit_status: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ManifestArtifact {
    kind: String,
    path: String,
    sha256: String,
    fingerprint_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct EvidenceManifest {
    schema: u32,
    producer_version: String,
    producer_commit: String,
    artifacts: Vec<ManifestArtifact>,
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
            produce_all()
        }
        Some("fingerprint") => {
            if args.next().as_deref() != Some("--kind")
                || args.next().as_deref() != Some("runtime")
                || args.next().is_some()
            {
                return Err("m8 evidence fingerprint requires --kind runtime".into());
            }
            let workspace = super::find_tsrs2_root()?;
            println!("{}", runtime_fingerprint(&workspace)?.sha256);
            Ok(())
        }
        Some(other) => Err(format!("unknown m8 evidence command: {other}").into()),
        None => Err("missing m8 evidence command (produce/fingerprint)".into()),
    }
}

pub(crate) fn produce_all() -> Result<(), Box<dyn Error>> {
    let workspace = super::find_tsrs2_root()?;
    ensure_relevant_tree_clean(&workspace)?;
    let config = read_config(&workspace)?;
    let runtime_path =
        resolve_artifact_path(&workspace, &config, &config.runtime_coverage.artifact)?;
    let fuzz_path = resolve_artifact_path(&workspace, &config, &config.fuzzer.artifact)?;
    let perf_path = resolve_artifact_path(&workspace, &config, &config.performance.artifact)?;

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
    produce_performance(&workspace, &config, None, &perf_path)?;

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
    };
    let manifest_path = artifact_dir(&workspace, &config)?.join("manifest.json");
    write_json(&manifest_path, &manifest)?;
    println!("M8 evidence manifest written: {}", manifest_path.display());
    Ok(())
}

pub(crate) fn coverage_emitters(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let workspace = super::find_tsrs2_root()?;
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
    write_json(artifact_path, &artifact)?;
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
    let workspace = super::find_tsrs2_root()?;
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
    let workspace = super::find_tsrs2_root()?;
    let status = Command::new("cargo")
        .current_dir(workspace)
        .arg("run")
        .arg("--quiet")
        .arg("-p")
        .arg("tsrs2-fuzz")
        .arg("--bin")
        .arg("tsrs2-fuzz-producer")
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
        let program = tsrs2_harness::expand_fixture_text("main.ts", &source, &vendor_lib_dir)?
            .into_iter()
            .next()
            .ok_or("generated fixture expanded to no programs")?;
        let case_dir = out_dir.join(format!("case-{case:05}"));
        let paths = tsrs2_harness::write_program_jsons(std::slice::from_ref(&program), &case_dir)?;
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
        let program = tsrs2_harness::expand_fixture_text("main.ts", &source, &vendor_lib_dir)?
            .into_iter()
            .next()
            .ok_or("fuzzer mutation canary expanded to no programs")?;
        let paths = tsrs2_harness::write_program_jsons(
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
        tsrs2_harness::expand_fixture_text("main.ts", &reduced_source, &vendor_lib_dir)?
            .into_iter()
            .next()
            .ok_or("reduced fixture expanded to no programs")?;
    let reduced_paths = tsrs2_harness::write_program_jsons(
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
    write_json(artifact_path, &artifact)?;
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

fn verified_fuzzer_oracle_pool(
    workspace: &Path,
) -> Result<tsrs2_oracle::OraclePool, Box<dyn Error>> {
    // The fuzzer needs only the explicit A3 renderer response. A
    // renderer-only pool avoids eagerly launching an unused normal
    // oracle worker, and the launch probe verifies the actual single
    // lazy worker against the workspace Node pin.
    let pool = tsrs2_oracle::OraclePool::new_render_only();
    tsrs2_conformance::ratchet::verify_launched_render_node(workspace, &pool)?;
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
    program: &tsrs2_harness::ProgramJson,
    path: &Path,
    vendor_lib_dir: &Path,
    pool: &tsrs2_oracle::OraclePool,
) -> Result<ProgramComparison, Box<dyn Error>> {
    compare_program_with_mutation_canary(program, path, vendor_lib_dir, pool, false)
}

fn compare_program_with_mutation_canary(
    program: &tsrs2_harness::ProgramJson,
    path: &Path,
    vendor_lib_dir: &Path,
    pool: &tsrs2_oracle::OraclePool,
    inject_mutation_canary: bool,
) -> Result<ProgramComparison, Box<dyn Error>> {
    let mut file_texts = BTreeMap::new();
    let libs = program
        .libs
        .iter()
        .map(|name| {
            let text = fs::read_to_string(vendor_lib_dir.join(name))?;
            file_texts.insert(name.clone(), text.clone());
            Ok(InputFile {
                name: name.clone(),
                text,
            })
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let files = program
        .files
        .iter()
        .map(|file| {
            let text = String::from_utf8(BASE64.decode(&file.text_b64)?)?;
            file_texts.insert(file.name.clone(), text.clone());
            Ok(InputFile {
                name: file.name.clone(),
                text,
            })
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let result = check_program_with_libs_at(
        &libs,
        &files,
        &tsrs2_harness::compiler_options_from_program(program),
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

fn oracle_value(diagnostic: &tsrs2_oracle::OracleDiag) -> Value {
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
    pool: &tsrs2_oracle::OraclePool,
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
        let program = match tsrs2_harness::expand_fixture_text(
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
        let paths = tsrs2_harness::write_program_jsons(std::slice::from_ref(&program), out_dir)?;
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

pub(crate) fn perf_conformance(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let workspace = super::find_tsrs2_root()?;
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
    produce_performance(&workspace, &config, runner.as_deref(), &artifact)
}

fn produce_performance(
    workspace: &Path,
    config: &EvidenceConfig,
    selected_runner: Option<&str>,
    artifact_path: &Path,
) -> Result<(), Box<dyn Error>> {
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
    let conformance_out = artifact_dir(workspace, config)?.join("performance-conformance.json");
    let full = timed_conformance(
        &runner,
        &[
            "conformance".to_owned(),
            "--out-json".to_owned(),
            conformance_out.display().to_string(),
        ],
        false,
    )?;
    if full.exit_status != 0 {
        return Err(format!(
            "performance conformance failed: {}",
            String::from_utf8_lossy(&full.stderr)
        )
        .into());
    }
    let smoke_limit = 8usize;
    let cache_off_out = artifact_dir(workspace, config)?.join("performance-cache-off-smoke.json");
    let cache_off = timed_conformance(
        &runner,
        &[
            "conformance".to_owned(),
            "--limit".to_owned(),
            smoke_limit.to_string(),
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
            fixture_limit: smoke_limit,
            wall_seconds: cache_off.wall_seconds,
            max_rss_bytes: cache_off.max_rss_bytes,
            exit_status: cache_off.exit_status,
            child_stdout_sha256: sha256_bytes(&cache_off.stdout),
            child_stderr_sha256: sha256_bytes(&cache_off.stderr),
        },
    };
    write_json(artifact_path, &artifact)?;
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
    if full.wall_seconds > runner.ceiling_wall_seconds
        || full.max_rss_bytes > runner.ceiling_rss_bytes
    {
        return Err("performance observation exceeds its reviewed ceiling".into());
    }
    Ok(())
}

fn timed_conformance(
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
    command.arg(executable).args(arguments);
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
    let entry = |kind: &str| {
        manifest
            .artifacts
            .iter()
            .find(|artifact| artifact.kind == kind)
    };

    let (runtime_ready, runtime_detail) = if let Some(entry) = entry("runtime-coverage") {
        let path = workspace.join(&entry.path);
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
        let path = workspace.join(&entry.path);
        let artifact: FuzzerArtifact = read_json(&path)?;
        let current = fuzz_fingerprint(workspace, artifact.seed, artifact.requested_cases)?;
        let raw_valid = verify_fuzzer_raw(&artifact).is_ok();
        let ready = artifact.header.fingerprint == current
            && entry.sha256 == sha256_file(&path)?
            && entry.fingerprint_sha256 == artifact.header.fingerprint.sha256
            && artifact.header.exit_status == 0
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
        let path = workspace.join(&entry.path);
        let artifact: PerformanceArtifact = read_json(&path)?;
        let current = performance_fingerprint(workspace)?;
        let ready = artifact.header.fingerprint == current
            && entry.sha256 == sha256_file(&path)?
            && entry.fingerprint_sha256 == artifact.header.fingerprint.sha256
            && artifact.header.exit_status == 0
            && artifact.wall_seconds <= artifact.runner.ceiling_wall_seconds
            && artifact.runner.ceiling_wall_seconds <= 60.0
            && artifact.max_rss_bytes <= artifact.runner.ceiling_rss_bytes
            && artifact.max_rss_bytes > 0
            && artifact.cache_off_smoke.exit_status == 0
            && artifact.cache_off_smoke.fixture_limit > 0
            && artifact.cache_off_smoke.max_rss_bytes > 0;
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
    Ok(config)
}

fn artifact_dir(workspace: &Path, config: &EvidenceConfig) -> Result<PathBuf, Box<dyn Error>> {
    secure_workspace_path(workspace, &config.artifact_dir)
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
            "Cargo.lock",
            "ratchet.toml",
            "m8-scope.json",
            "crates/checker/src",
            "crates/conformance/src",
            "crates/harness/src",
            "crates/xtask/src",
            "goldens",
            "ts-tests/tests/cases/conformance",
            "vendor/typescript-6.0.3/lib",
        ],
        &["command=conformance".to_owned()],
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
    if path.is_file() {
        out.push(path.to_owned());
        return Ok(());
    }
    if !path.is_dir() {
        return Err(format!(
            "fingerprint input is missing: {}",
            path.strip_prefix(workspace).unwrap_or(path).display()
        )
        .into());
    }
    let mut entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let child = entry.path();
        if child.is_dir() {
            collect_files(workspace, &child, out)?;
        } else if child.is_file() {
            out.push(child);
        }
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
        let programs = tsrs2_harness::expand_fixture_file(fixture, &vendor)?;
        paths.extend(tsrs2_harness::write_program_jsons(
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
    let relative = workspace.strip_prefix(&git_root)?;
    let output = Command::new("git")
        .arg("-C")
        .arg(&git_root)
        .args(["status", "--porcelain", "--"])
        .arg(relative)
        .output()?;
    if !output.status.success() {
        return Err("failed to inspect relevant working-tree cleanliness".into());
    }
    if !output.stdout.is_empty() {
        return Err(
            "M8 evidence production requires a clean tsrs2 tree; commit the reviewed inputs first"
                .into(),
        );
    }
    Ok(())
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
    Ok(path
        .strip_prefix(workspace)?
        .to_string_lossy()
        .replace('\\', "/"))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, Box<dyn Error>> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    fs::write(path, bytes)?;
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
mod tests {
    use super::*;

    #[test]
    fn evidence_paths_cannot_escape_the_workspace() {
        let workspace = Path::new("/workspace/tsrs2");
        assert_eq!(
            secure_workspace_path(workspace, "target/m8/evidence")
                .unwrap()
                .to_string_lossy(),
            "/workspace/tsrs2/target/m8/evidence"
        );
        assert!(secure_workspace_path(workspace, "../outside").is_err());
        assert!(secure_workspace_path(workspace, "/tmp/outside").is_err());
    }

    #[test]
    fn divergence_signature_ignores_positions_in_canonical_rows() {
        let oracle = vec![json!({"code":2322,"head":"Type mismatch","start":1})];
        let tsrs = vec![json!({"code":2322,"head":"Type mismatch","start":99})];
        let signature = divergence_signature("t2", &oracle, &tsrs).unwrap();
        assert!(signature.contains("2322:Type mismatch"));
        assert!(!signature.contains("start"));
    }

    #[test]
    fn t4_renderer_classifier_pins_precedence_and_stable_signature() {
        let a = json!({"code":2322,"head":"Type mismatch","start":1});
        let b = json!({"code":2345,"head":"Bad argument","start":9});
        assert_eq!(
            renderer_divergence_class(
                &[a.clone(), b.clone()],
                &[b.clone(), a.clone()],
                "oracle",
                "tsrs",
            ),
            "order"
        );
        assert_eq!(
            renderer_divergence_class(
                &[a.clone(), a.clone()],
                std::slice::from_ref(&a),
                "oracle",
                "tsrs",
            ),
            "dedupe"
        );
        let oracle_path = "/oracle/main.ts:1:1 - error TS2322: Type mismatch\n";
        let tsrs_path = "C:/work/main.ts:1:1 - error TS2322: Type mismatch\n";
        assert_eq!(
            renderer_divergence_class(
                std::slice::from_ref(&a),
                std::slice::from_ref(&a),
                oracle_path,
                tsrs_path,
            ),
            "path"
        );
        assert_eq!(
            renderer_divergence_class(
                std::slice::from_ref(&a),
                std::slice::from_ref(&a),
                "error TS2322: x\r\n",
                "error TS2322: x\n",
            ),
            "newline"
        );
        assert_eq!(
            renderer_divergence_class(
                std::slice::from_ref(&a),
                std::slice::from_ref(&a),
                "error TS2322: x\n",
                "error TS2322: y\n",
            ),
            "text"
        );

        let signature = t4_divergence_signature(
            std::slice::from_ref(&a),
            std::slice::from_ref(&a),
            oracle_path,
            tsrs_path,
        )
        .unwrap();
        assert!(signature.contains("tier=t4"));
        assert!(signature.contains("renderer=path"));
        assert!(signature.contains("key=2322:Type mismatch"));
        assert!(!signature.contains("/oracle"));
        assert!(!signature.contains("start"));
    }

    #[test]
    fn fuzzer_pool_is_pinned_and_has_no_normal_oracle_worker() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let pool = verified_fuzzer_oracle_pool(&workspace).unwrap();

        assert!(
            pool.node_version().is_err(),
            "fuzzer must not eagerly launch or retain a normal oracle worker"
        );
        assert!(pool.render_node_version().is_ok());
    }

    #[test]
    fn rss_parsers_pin_backend_units() {
        assert_eq!(
            parse_max_rss("  123456  maximum resident set size\n", "bsd-time-l").unwrap(),
            123456
        );
        assert_eq!(
            parse_max_rss("Maximum resident set size (kbytes): 1024\n", "gnu-time-v").unwrap(),
            1024 * 1024
        );
    }

    #[test]
    fn fuzzer_raw_rejects_uncompared_and_unreduced_artifacts() {
        let header = ArtifactHeader {
            schema: ARTIFACT_SCHEMA,
            producer_version: PRODUCER_VERSION.to_owned(),
            kind: "differential-fuzzer".to_owned(),
            producer_commit: "0".repeat(40),
            command: "fuzz".to_owned(),
            started_unix_ms: 1,
            finished_unix_ms: 2,
            exit_status: 0,
            fingerprint: Fingerprint {
                sha256: "a".repeat(64),
                inputs: Vec::new(),
            },
        };
        let mut artifact = FuzzerArtifact {
            header,
            seed: 1,
            requested_cases: 1,
            cases: Vec::new(),
            reducer: ReducerObservation {
                exercised: false,
                mutation_canary: false,
                original_signature: None,
                reduced_signature: None,
                original_bytes: 0,
                reduced_bytes: 0,
                reduced_source: None,
            },
            dedupe: DedupeObservation {
                exercised: false,
                observed_signatures: Vec::new(),
                unique_signatures: Vec::new(),
            },
        };
        assert!(verify_fuzzer_raw(&artifact).is_err());
        artifact.requested_cases = 0;
        assert!(verify_fuzzer_raw(&artifact).is_err());
    }

    #[test]
    fn m9_reduce_is_fail_closed_before_reading_an_artifact() {
        let error = fuzz_reduce(["/tmp/does-not-need-to-exist.json".to_owned()].into_iter())
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            "fuzz reduce is fail-closed until the M9.1d real-replay reducer lands"
        );
    }

    #[test]
    fn runtime_reuse_requires_fresh_inputs_and_exact_zero_hit_reviews() {
        let fingerprint = Fingerprint {
            sha256: "a".repeat(64),
            inputs: Vec::new(),
        };
        let mut artifact = RuntimeArtifact {
            header: ArtifactHeader {
                schema: ARTIFACT_SCHEMA,
                producer_version: PRODUCER_VERSION.to_owned(),
                kind: "runtime-coverage".to_owned(),
                producer_commit: "0".repeat(40),
                command: "cargo xtask coverage emitters --corpus".to_owned(),
                started_unix_ms: 1,
                finished_unix_ms: 2,
                exit_status: 0,
                fingerprint: fingerprint.clone(),
            },
            inventory_sha256: "inventory".to_owned(),
            raw_counts: BTreeMap::from([("hit".to_owned(), 1)]),
            zero_hit_reviews: vec![ZeroHitReviewArtifact {
                declaration: "zero".to_owned(),
                evidence: "reviewed exact declaration".to_owned(),
            }],
        };
        let direct = BTreeSet::from(["hit", "zero"]);
        assert!(validate_runtime_artifact(&artifact, &fingerprint, "inventory", &direct).ready);

        artifact.zero_hit_reviews[0].declaration = "hit".to_owned();
        assert!(!validate_runtime_artifact(&artifact, &fingerprint, "inventory", &direct).ready);
        artifact.zero_hit_reviews[0].declaration = "zero".to_owned();
        artifact.header.fingerprint.sha256 = "b".repeat(64);
        assert!(!validate_runtime_artifact(&artifact, &fingerprint, "inventory", &direct).ready);
    }
}

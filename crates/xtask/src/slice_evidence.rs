//! Immutable before/after evidence for one terminal semantic slice.
//!
//! The command deliberately executes the existing xtask commands in
//! child processes. That keeps one implementation of every gate while
//! giving each step an independent exit status and a complete log.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{find_workspace_root, sha256_file};

const SCHEMA: u32 = 1;
const MANIFEST_NAME: &str = "manifest.json";
const INPUT_PATHS: &[&str] = &[
    "Cargo.lock",
    "STAGE",
    "diag-families.json",
    "m8-scope.json",
    "ratchet.toml",
    "ratchets/conformance-matches.v1.json.zst",
    "ratchets/oracle-inputs.v1.json.zst",
];

pub(crate) fn run(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    match args.next().as_deref() {
        Some("snapshot") => snapshot(parse_snapshot_args(args)?),
        Some("verify") => verify(parse_verify_args(args)?),
        Some(other) => Err(format!("unknown slice-evidence command: {other}\n{}", usage()).into()),
        None => Err(usage().into()),
    }
}

fn usage() -> &'static str {
    "usage:\n  cargo xtask slice-evidence snapshot --slice <name> --targets <csv> \
     --band <all|2xxx|syntactic> --out-dir <new-directory>\n  cargo xtask slice-evidence \
     verify --before-dir <snapshot-directory> --out-dir <new-directory> \
     [--baseline <trusted-ref>]"
}

#[derive(Debug, Eq, PartialEq)]
struct SnapshotArgs {
    slice: String,
    targets: Vec<String>,
    band: String,
    out_dir: PathBuf,
}

fn parse_snapshot_args(args: impl Iterator<Item = String>) -> Result<SnapshotArgs, Box<dyn Error>> {
    let mut slice = None;
    let mut targets = None;
    let mut band = None;
    let mut out_dir = None;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        let value = match arg.as_str() {
            "--slice" | "--targets" | "--band" | "--out-dir" => args
                .next()
                .ok_or_else(|| format!("missing value after {arg}"))?,
            _ => return Err(format!("unexpected slice-evidence snapshot argument: {arg}").into()),
        };
        match arg.as_str() {
            "--slice" => set_once(&mut slice, value, "--slice")?,
            "--targets" => set_once(&mut targets, parse_targets(&value)?, "--targets")?,
            "--band" => set_once(&mut band, parse_band(&value)?, "--band")?,
            "--out-dir" => set_once(&mut out_dir, PathBuf::from(value), "--out-dir")?,
            _ => unreachable!(),
        }
    }

    let slice = slice.ok_or("missing required --slice")?;
    validate_slice_name(&slice)?;
    Ok(SnapshotArgs {
        slice,
        targets: targets.ok_or("missing required --targets")?,
        band: band.ok_or("missing required --band")?,
        out_dir: out_dir.ok_or("missing required --out-dir")?,
    })
}

#[derive(Debug, Eq, PartialEq)]
struct VerifyArgs {
    before_dir: PathBuf,
    out_dir: PathBuf,
    baseline: String,
}

fn parse_verify_args(args: impl Iterator<Item = String>) -> Result<VerifyArgs, Box<dyn Error>> {
    let mut before_dir = None;
    let mut out_dir = None;
    let mut baseline = None;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        let value = match arg.as_str() {
            "--before-dir" | "--out-dir" | "--baseline" => args
                .next()
                .ok_or_else(|| format!("missing value after {arg}"))?,
            _ => return Err(format!("unexpected slice-evidence verify argument: {arg}").into()),
        };
        match arg.as_str() {
            "--before-dir" => set_once(&mut before_dir, PathBuf::from(value), "--before-dir")?,
            "--out-dir" => set_once(&mut out_dir, PathBuf::from(value), "--out-dir")?,
            "--baseline" => set_once(&mut baseline, value, "--baseline")?,
            _ => unreachable!(),
        }
    }
    Ok(VerifyArgs {
        before_dir: before_dir.ok_or("missing required --before-dir")?,
        out_dir: out_dir.ok_or("missing required --out-dir")?,
        baseline: baseline.unwrap_or_else(|| "origin/main".to_owned()),
    })
}

fn set_once<T>(slot: &mut Option<T>, value: T, flag: &str) -> Result<(), Box<dyn Error>> {
    if slot.replace(value).is_some() {
        return Err(format!("{flag} may be supplied only once").into());
    }
    Ok(())
}

fn parse_targets(value: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let mut targets = value
        .split(',')
        .map(str::trim)
        .filter(|target| !target.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    targets.sort();
    targets.dedup();
    if targets.is_empty() {
        return Err("--targets must name at least one fixture".into());
    }
    for target in &targets {
        validate_relative_path(Path::new(target), "target fixture")?;
    }
    Ok(targets)
}

fn parse_band(value: &str) -> Result<String, Box<dyn Error>> {
    match value {
        "all" | "2xxx" | "syntactic" => Ok(value.to_owned()),
        _ => Err(format!("unknown conformance band: {value}").into()),
    }
}

fn validate_slice_name(value: &str) -> Result<(), Box<dyn Error>> {
    if value.is_empty()
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
    {
        return Err("--slice must contain only ASCII letters, digits, '.', '_' or '-'".into());
    }
    Ok(())
}

fn validate_relative_path(path: &Path, label: &str) -> Result<(), Box<dyn Error>> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!("{label} must be a non-empty relative path without '..'").into());
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct EvidenceManifest {
    schema: u32,
    manifest_type: String,
    status: String,
    slice: String,
    band: String,
    targets: Vec<String>,
    git_commit: String,
    git_worktree_dirty: bool,
    git_worktree_sha256: String,
    created_unix_seconds: u64,
    baseline: Option<String>,
    before: Option<BeforeReference>,
    input_hashes: BTreeMap<String, String>,
    steps: Vec<StepRecord>,
    observations: Vec<ObservationRecord>,
    diffs: Vec<DiffRecord>,
    review: ReviewRecord,
    failure: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct BeforeReference {
    manifest: String,
    manifest_sha256: String,
    git_commit: String,
    git_worktree_dirty: bool,
    git_worktree_sha256: String,
    input_hashes: BTreeMap<String, String>,
    steps: Vec<StepRecord>,
    observations: Vec<ObservationRecord>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StepRecord {
    label: String,
    command: Vec<String>,
    exit_code: Option<i32>,
    log: String,
    log_sha256: String,
    spawn_error: Option<String>,
}

impl StepRecord {
    fn succeeded(&self) -> bool {
        self.exit_code == Some(0) && self.spawn_error.is_none()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ObservationRecord {
    label: String,
    report: String,
    report_sha256: String,
    metrics: ObservationMetrics,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ObservationMetrics {
    band: String,
    fixtures_total: usize,
    cases_total: usize,
    oracle_diagnostics: usize,
    tsrs_diagnostics: usize,
    matched_t0_diagnostics: usize,
    false_positive_diagnostics: usize,
    false_negative_diagnostics: usize,
    supported_oracle_diagnostics: usize,
    supported_tsrs_diagnostics: usize,
    supported_matched_t0_diagnostics: usize,
    supported_false_negative_diagnostics: usize,
    oracle_universe_sha256: String,
    supported_oracle_universe_sha256: String,
}

#[derive(Deserialize)]
struct ObservationInput {
    band: String,
    fixtures_total: usize,
    cases_total: usize,
    oracle_diagnostics: usize,
    tsrs_diagnostics: usize,
    matched_t0_diagnostics: usize,
    false_positive_diagnostics: usize,
    false_negative_diagnostics: usize,
    supported_oracle_diagnostics: usize,
    supported_tsrs_diagnostics: usize,
    supported_matched_t0_diagnostics: usize,
    supported_false_negative_diagnostics: usize,
    shadow_tier_identities: UniverseInput,
    supported_shadow_tier_identities: UniverseInput,
}

#[derive(Deserialize)]
struct UniverseInput {
    oracle_universe_sha256: String,
}

impl From<ObservationInput> for ObservationMetrics {
    fn from(input: ObservationInput) -> Self {
        Self {
            band: input.band,
            fixtures_total: input.fixtures_total,
            cases_total: input.cases_total,
            oracle_diagnostics: input.oracle_diagnostics,
            tsrs_diagnostics: input.tsrs_diagnostics,
            matched_t0_diagnostics: input.matched_t0_diagnostics,
            false_positive_diagnostics: input.false_positive_diagnostics,
            false_negative_diagnostics: input.false_negative_diagnostics,
            supported_oracle_diagnostics: input.supported_oracle_diagnostics,
            supported_tsrs_diagnostics: input.supported_tsrs_diagnostics,
            supported_matched_t0_diagnostics: input.supported_matched_t0_diagnostics,
            supported_false_negative_diagnostics: input.supported_false_negative_diagnostics,
            oracle_universe_sha256: input.shadow_tier_identities.oracle_universe_sha256,
            supported_oracle_universe_sha256: input
                .supported_shadow_tier_identities
                .oracle_universe_sha256,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct DiffRecord {
    label: String,
    report: String,
    report_sha256: String,
    supported_oracle_universe_unchanged: bool,
    all_corpus: TierCounts,
    supported: TierCounts,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct TierCounts {
    t1_lost: usize,
    t1_gained: usize,
    t2_lost: usize,
    t2_gained: usize,
    t3_lost: usize,
    t3_gained: usize,
}

impl TierCounts {
    fn has_losses(&self) -> bool {
        self.t1_lost != 0 || self.t2_lost != 0 || self.t3_lost != 0
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct ReviewRecord {
    gains_outside_target: BTreeMap<String, ScopeTierCounts>,
    required: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct ScopeTierCounts {
    all_corpus: TierCounts,
    supported: TierCounts,
}

impl ScopeTierCounts {
    fn has_gains(&self) -> bool {
        let has_gains = |counts: &TierCounts| {
            counts.t1_gained != 0 || counts.t2_gained != 0 || counts.t3_gained != 0
        };
        has_gains(&self.all_corpus) || has_gains(&self.supported)
    }
}

fn snapshot(args: SnapshotArgs) -> Result<(), Box<dyn Error>> {
    let workspace = find_workspace_root()?;
    validate_target_files(&workspace, &args.targets)?;
    let out_dir = create_immutable_dir(&args.out_dir, &workspace)?;

    let mut manifest = new_manifest("before", &args.slice, &args.band, &args.targets, &workspace)?;
    persist_manifest(&out_dir, &manifest)?;

    for specification in observation_specs("before", &args.band, &args.targets) {
        let report_path = out_dir.join(&specification.report);
        let command = conformance_command(&specification, &report_path);
        let step = if args.band == "all" && specification.label == "all-before" {
            reuse_logged(
                &out_dir,
                &specification.label,
                &out_dir.join("band-before.json"),
                &report_path,
            )?
        } else {
            run_logged(&workspace, &out_dir, &specification.label, &command)?
        };
        let succeeded = step.succeeded();
        manifest.steps.push(step);
        persist_manifest(&out_dir, &manifest)?;
        if !succeeded {
            return fail_manifest(
                &out_dir,
                &mut manifest,
                format!("{} observation failed", specification.label),
            );
        }
        manifest
            .observations
            .push(read_observation(&specification.label, &report_path)?);
        persist_manifest(&out_dir, &manifest)?;
    }
    if let Err(error) = ensure_inputs_unchanged(&workspace, &manifest.input_hashes) {
        return fail_manifest(&out_dir, &mut manifest, error.to_string());
    }
    if let Err(error) = ensure_worktree_unchanged(
        &workspace,
        &manifest.git_commit,
        &manifest.git_worktree_sha256,
    ) {
        return fail_manifest(&out_dir, &mut manifest, error.to_string());
    }

    manifest.status = "complete".to_owned();
    persist_manifest(&out_dir, &manifest)?;
    println!(
        "slice evidence snapshot complete: {}",
        out_dir.join(MANIFEST_NAME).display()
    );
    Ok(())
}

fn verify(args: VerifyArgs) -> Result<(), Box<dyn Error>> {
    let workspace = find_workspace_root()?;
    let before_dir = fs::canonicalize(&args.before_dir).map_err(|error| {
        format!(
            "cannot resolve before directory {}: {error}",
            args.before_dir.display()
        )
    })?;
    ensure_outside_worktree(&before_dir, &workspace, "before directory")?;
    let before_manifest_path = before_dir.join(MANIFEST_NAME);
    let before_bytes = fs::read(&before_manifest_path).map_err(|error| {
        format!(
            "cannot read before manifest {}: {error}",
            before_manifest_path.display()
        )
    })?;
    let before: EvidenceManifest = serde_json::from_slice(&before_bytes)
        .map_err(|error| format!("invalid before manifest: {error}"))?;
    validate_before_manifest(&before_dir, &before)?;
    validate_target_files(&workspace, &before.targets)?;
    let out_dir = create_immutable_dir(&args.out_dir, &workspace)?;

    let mut manifest = new_manifest(
        "after",
        &before.slice,
        &before.band,
        &before.targets,
        &workspace,
    )?;
    manifest.baseline = Some(args.baseline.clone());
    manifest.before = Some(BeforeReference {
        manifest: before_manifest_path.display().to_string(),
        manifest_sha256: sha256_bytes(&before_bytes),
        git_commit: before.git_commit.clone(),
        git_worktree_dirty: before.git_worktree_dirty,
        git_worktree_sha256: before.git_worktree_sha256.clone(),
        input_hashes: before.input_hashes.clone(),
        steps: before.steps.clone(),
        observations: before.observations.clone(),
    });
    persist_manifest(&out_dir, &manifest)?;

    for specification in observation_specs("after", &before.band, &before.targets) {
        let report_path = out_dir.join(&specification.report);
        let command = conformance_command(&specification, &report_path);
        let step = if before.band == "all" && specification.label == "all-after" {
            reuse_logged(
                &out_dir,
                &specification.label,
                &out_dir.join("band-after.json"),
                &report_path,
            )?
        } else {
            run_logged(&workspace, &out_dir, &specification.label, &command)?
        };
        let succeeded = step.succeeded();
        manifest.steps.push(step);
        persist_manifest(&out_dir, &manifest)?;
        if !succeeded {
            return fail_manifest(
                &out_dir,
                &mut manifest,
                format!("{} observation failed", specification.label),
            );
        }
        let observation = read_observation(&specification.label, &report_path)?;
        let false_positives = observation.metrics.false_positive_diagnostics;
        manifest.observations.push(observation);
        persist_manifest(&out_dir, &manifest)?;
        if false_positives != 0 {
            return fail_manifest(
                &out_dir,
                &mut manifest,
                format!(
                    "{} produced {} false-positive diagnostics",
                    specification.label, false_positives
                ),
            );
        }
    }

    for label in ["target", "band", "all"] {
        let before_report = report_for(&before_dir, label, "before");
        let after_report = report_for(&out_dir, label, "after");
        let diff_report = out_dir.join(format!("{label}-diff.json"));
        let command = vec![
            "conformance-diff".to_owned(),
            before_report.display().to_string(),
            after_report.display().to_string(),
            "--out-json".to_owned(),
            diff_report.display().to_string(),
        ];
        let step = run_logged(&workspace, &out_dir, &format!("{label}-diff"), &command)?;
        let succeeded = step.succeeded();
        manifest.steps.push(step);
        persist_manifest(&out_dir, &manifest)?;
        if !succeeded {
            return fail_manifest(
                &out_dir,
                &mut manifest,
                format!("{label} conformance diff failed"),
            );
        }
        let report = tsc_conformance::conformance_diff(&before_report, &after_report)?;
        let diff = diff_record(label, &diff_report, &report)?;
        if !diff.supported_oracle_universe_unchanged {
            return fail_manifest(
                &out_dir,
                &mut manifest,
                format!("{label} supported oracle universe changed"),
            );
        }
        if diff.all_corpus.has_losses() || diff.supported.has_losses() {
            return fail_manifest(
                &out_dir,
                &mut manifest,
                format!("{label} lost accepted shadow-tier identities"),
            );
        }
        manifest.diffs.push(diff);
        persist_manifest(&out_dir, &manifest)?;
    }

    manifest.review = gains_outside_target(&out_dir, &manifest.diffs)?;
    persist_manifest(&out_dir, &manifest)?;

    let stage = fs::read_to_string(workspace.join("STAGE"))?;
    let repository_gates = [
        (
            "ratchet-check",
            vec![
                "ratchet".to_owned(),
                "check".to_owned(),
                "--baseline".to_owned(),
                args.baseline.clone(),
            ],
        ),
        (
            "scope-audit",
            vec![
                "scope".to_owned(),
                "audit".to_owned(),
                "--baseline".to_owned(),
                args.baseline.clone(),
            ],
        ),
        (
            "families-check",
            vec![
                "families".to_owned(),
                "check".to_owned(),
                "--baseline".to_owned(),
                args.baseline.clone(),
            ],
        ),
        (
            "ledger-check",
            vec!["ledger".to_owned(), "check".to_owned()],
        ),
        (
            "escapes",
            vec![
                "escapes".to_owned(),
                "--stale".to_owned(),
                stage.trim().to_owned(),
            ],
        ),
        (
            "invariants",
            vec![
                "invariants".to_owned(),
                "--suite".to_owned(),
                "all".to_owned(),
            ],
        ),
    ];
    for (label, command) in repository_gates {
        let step = run_logged(&workspace, &out_dir, label, &command)?;
        let succeeded = step.succeeded();
        manifest.steps.push(step);
        persist_manifest(&out_dir, &manifest)?;
        if !succeeded {
            return fail_manifest(&out_dir, &mut manifest, format!("{label} failed"));
        }
    }
    if let Err(error) = ensure_inputs_unchanged(&workspace, &manifest.input_hashes) {
        return fail_manifest(&out_dir, &mut manifest, error.to_string());
    }
    if let Err(error) = ensure_worktree_unchanged(
        &workspace,
        &manifest.git_commit,
        &manifest.git_worktree_sha256,
    ) {
        return fail_manifest(&out_dir, &mut manifest, error.to_string());
    }

    manifest.status = if manifest.review.required {
        "review-required".to_owned()
    } else {
        "complete".to_owned()
    };
    persist_manifest(&out_dir, &manifest)?;
    println!(
        "slice evidence verification {}: {}",
        manifest.status,
        out_dir.join(MANIFEST_NAME).display()
    );
    Ok(())
}

fn new_manifest(
    manifest_type: &str,
    slice: &str,
    band: &str,
    targets: &[String],
    workspace: &Path,
) -> Result<EvidenceManifest, Box<dyn Error>> {
    let git_state = git_state(workspace)?;
    Ok(EvidenceManifest {
        schema: SCHEMA,
        manifest_type: manifest_type.to_owned(),
        status: "running".to_owned(),
        slice: slice.to_owned(),
        band: band.to_owned(),
        targets: targets.to_vec(),
        git_commit: git_state.commit,
        git_worktree_dirty: git_state.dirty,
        git_worktree_sha256: git_state.sha256,
        created_unix_seconds: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        baseline: None,
        before: None,
        input_hashes: input_hashes(workspace)?,
        steps: Vec::new(),
        observations: Vec::new(),
        diffs: Vec::new(),
        review: ReviewRecord::default(),
        failure: None,
    })
}

struct GitState {
    commit: String,
    dirty: bool,
    sha256: String,
}

fn git_state(workspace: &Path) -> Result<GitState, Box<dyn Error>> {
    let root = git_output(workspace, ["rev-parse", "--show-toplevel"])?;
    let root = PathBuf::from(String::from_utf8(root)?.trim());
    let commit = String::from_utf8(git_output(&root, ["rev-parse", "HEAD"])?)?
        .trim()
        .to_owned();
    let status = git_output(
        &root,
        ["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    let diff = git_output(&root, ["diff", "--binary", "--no-ext-diff", "HEAD"])?;
    let untracked = git_output(&root, ["ls-files", "--others", "--exclude-standard", "-z"])?;

    let mut hasher = Sha256::new();
    hasher.update(b"tsrs2-slice-evidence-worktree-v1\0");
    hash_chunk(&mut hasher, &diff);
    for relative in untracked
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        hash_chunk(&mut hasher, relative);
        let relative_path = String::from_utf8(relative.to_vec())
            .map_err(|_| "slice-evidence requires UTF-8 Git worktree paths")?;
        hash_chunk(&mut hasher, &fs::read(root.join(relative_path))?);
    }
    Ok(GitState {
        commit,
        dirty: !status.is_empty(),
        sha256: format!("{:x}", hasher.finalize()),
    })
}

fn git_output<I, S>(directory: &Path, args: I) -> Result<Vec<u8>, Box<dyn Error>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "git command failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(output.stdout)
}

fn hash_chunk(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn input_hashes(workspace: &Path) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    INPUT_PATHS
        .iter()
        .map(|relative| {
            Ok((
                (*relative).to_owned(),
                sha256_file(&workspace.join(relative))?,
            ))
        })
        .collect()
}

fn ensure_inputs_unchanged(
    workspace: &Path,
    expected: &BTreeMap<String, String>,
) -> Result<(), Box<dyn Error>> {
    let actual = input_hashes(workspace)?;
    if &actual == expected {
        return Ok(());
    }
    let changed = expected
        .keys()
        .chain(actual.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|path| expected.get(*path) != actual.get(*path))
        .cloned()
        .collect::<Vec<_>>();
    Err(format!(
        "slice-evidence is report-only, but controlled inputs changed while it ran: {}",
        changed.join(", ")
    )
    .into())
}

fn ensure_worktree_unchanged(
    workspace: &Path,
    expected_commit: &str,
    expected_sha256: &str,
) -> Result<(), Box<dyn Error>> {
    let actual = git_state(workspace)?;
    if actual.commit == expected_commit && actual.sha256 == expected_sha256 {
        Ok(())
    } else {
        Err(format!(
            "slice-evidence is report-only, but the Git worktree changed while it ran: \
             expected commit={expected_commit} sha256={expected_sha256}, got commit={} sha256={}",
            actual.commit, actual.sha256
        )
        .into())
    }
}

fn validate_target_files(workspace: &Path, targets: &[String]) -> Result<(), Box<dyn Error>> {
    let corpus = fs::canonicalize(workspace.join("ts-tests/tests/cases"))?;
    for target in targets {
        validate_relative_path(Path::new(target), "target fixture")?;
        let path = workspace.join(target);
        if !path.is_file() {
            return Err(format!("target fixture does not exist: {}", path.display()).into());
        }
        let canonical = fs::canonicalize(&path)?;
        if !canonical.starts_with(&corpus) {
            return Err(format!(
                "target fixture must be under {}: {}",
                corpus.display(),
                path.display()
            )
            .into());
        }
    }
    Ok(())
}

fn create_immutable_dir(path: &Path, workspace: &Path) -> Result<PathBuf, Box<dyn Error>> {
    if path.as_os_str().is_empty() {
        return Err("evidence directory must not be empty".into());
    }
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    ensure_outside_worktree(&absolute, workspace, "evidence directory")?;
    if absolute.exists() {
        return Err(format!(
            "evidence directory already exists (refusing to overwrite): {}",
            absolute.display()
        )
        .into());
    }
    fs::create_dir_all(&absolute)?;
    let canonical = fs::canonicalize(absolute)?;
    ensure_outside_worktree(&canonical, workspace, "evidence directory")?;
    Ok(canonical)
}

fn ensure_outside_worktree(
    path: &Path,
    workspace: &Path,
    label: &str,
) -> Result<(), Box<dyn Error>> {
    let root = String::from_utf8(git_output(workspace, ["rev-parse", "--show-toplevel"])?)?;
    let root = fs::canonicalize(root.trim())?;
    if path.starts_with(&root) {
        return Err(format!(
            "{label} must be outside the Git worktree so evidence cannot dirty or be committed \
             with the implementation: {}",
            path.display()
        )
        .into());
    }
    Ok(())
}

struct ObservationSpec {
    label: String,
    report: String,
    band: String,
    targets: Vec<String>,
}

fn observation_specs(suffix: &str, active_band: &str, targets: &[String]) -> [ObservationSpec; 3] {
    [
        ObservationSpec {
            label: format!("target-{suffix}"),
            report: format!("target-{suffix}.json"),
            band: active_band.to_owned(),
            targets: targets.to_vec(),
        },
        ObservationSpec {
            label: format!("band-{suffix}"),
            report: format!("band-{suffix}.json"),
            band: active_band.to_owned(),
            targets: Vec::new(),
        },
        ObservationSpec {
            label: format!("all-{suffix}"),
            report: format!("all-{suffix}.json"),
            band: "all".to_owned(),
            targets: Vec::new(),
        },
    ]
}

fn conformance_command(specification: &ObservationSpec, report_path: &Path) -> Vec<String> {
    let mut command = vec![
        "conformance".to_owned(),
        "--band".to_owned(),
        specification.band.clone(),
        "--out-json".to_owned(),
        report_path.display().to_string(),
    ];
    if !specification.targets.is_empty() {
        command.push("--files".to_owned());
        command.push(specification.targets.join(","));
    }
    command
}

fn run_logged(
    workspace: &Path,
    evidence_dir: &Path,
    label: &str,
    args: &[String],
) -> Result<StepRecord, Box<dyn Error>> {
    let log_name = format!("{label}.log");
    validate_relative_path(Path::new(&log_name), "log name")?;
    let log_path = evidence_dir.join(&log_name);
    let executable = std::env::current_exe()?;
    let display_command = std::iter::once("cargo".to_owned())
        .chain(std::iter::once("xtask".to_owned()))
        .chain(args.iter().cloned())
        .collect::<Vec<_>>();

    let result = Command::new(&executable)
        .args(args)
        .current_dir(workspace)
        .output();
    let (exit_code, spawn_error, stdout, stderr) = match result {
        Ok(output) => (output.status.code(), None, output.stdout, output.stderr),
        Err(error) => (
            None,
            Some(error.to_string()),
            Vec::new(),
            error.to_string().into_bytes(),
        ),
    };
    let mut log = Vec::new();
    log.extend_from_slice(format!("command: {}\n", display_command.join(" ")).as_bytes());
    log.extend_from_slice(format!("exit_code: {exit_code:?}\n\n[stdout]\n").as_bytes());
    log.extend_from_slice(&stdout);
    log.extend_from_slice(b"\n[stderr]\n");
    log.extend_from_slice(&stderr);
    if !log.ends_with(b"\n") {
        log.push(b'\n');
    }
    fs::write(&log_path, &log)?;
    println!(
        "slice-evidence {label}: exit={exit_code:?} log={}",
        log_path.display()
    );

    Ok(StepRecord {
        label: label.to_owned(),
        command: display_command,
        exit_code,
        log: log_name,
        log_sha256: sha256_bytes(&log),
        spawn_error,
    })
}

fn reuse_logged(
    evidence_dir: &Path,
    label: &str,
    source: &Path,
    destination: &Path,
) -> Result<StepRecord, Box<dyn Error>> {
    let log_name = format!("{label}.log");
    let command = vec![
        "internal-copy".to_owned(),
        source.display().to_string(),
        destination.display().to_string(),
    ];
    let copied = fs::copy(source, destination);
    let (exit_code, spawn_error) = match copied {
        Ok(_) => (Some(0), None),
        Err(error) => (None, Some(error.to_string())),
    };
    let log = format!(
        "command: {}\nexit_code: {exit_code:?}\n{}",
        command.join(" "),
        spawn_error
            .as_deref()
            .map(|error| format!("error: {error}\n"))
            .unwrap_or_default()
    )
    .into_bytes();
    fs::write(evidence_dir.join(&log_name), &log)?;
    println!(
        "slice-evidence {label}: reused {} as {}",
        source.display(),
        destination.display()
    );
    Ok(StepRecord {
        label: label.to_owned(),
        command,
        exit_code,
        log: log_name,
        log_sha256: sha256_bytes(&log),
        spawn_error,
    })
}

fn read_observation(label: &str, path: &Path) -> Result<ObservationRecord, Box<dyn Error>> {
    let input: ObservationInput = serde_json::from_slice(&fs::read(path)?)
        .map_err(|error| format!("invalid conformance report {}: {error}", path.display()))?;
    Ok(ObservationRecord {
        label: label.to_owned(),
        report: file_name(path)?,
        report_sha256: sha256_file(path)?,
        metrics: input.into(),
    })
}

fn file_name(path: &Path) -> Result<String, Box<dyn Error>> {
    path.file_name()
        .and_then(OsStr::to_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("path has no UTF-8 file name: {}", path.display()).into())
}

fn persist_manifest(dir: &Path, manifest: &EvidenceManifest) -> Result<(), Box<dyn Error>> {
    let path = dir.join(MANIFEST_NAME);
    let temporary = dir.join(".manifest.json.tmp");
    let mut bytes = serde_json::to_vec_pretty(manifest)?;
    bytes.push(b'\n');
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn fail_manifest<T>(
    dir: &Path,
    manifest: &mut EvidenceManifest,
    message: String,
) -> Result<T, Box<dyn Error>> {
    manifest.status = "failed".to_owned();
    manifest.failure = Some(message.clone());
    persist_manifest(dir, manifest)?;
    Err(format!(
        "{message}; evidence preserved at {}",
        dir.join(MANIFEST_NAME).display()
    )
    .into())
}

fn validate_before_manifest(dir: &Path, manifest: &EvidenceManifest) -> Result<(), Box<dyn Error>> {
    if manifest.schema != SCHEMA {
        return Err(format!(
            "unsupported slice-evidence schema {}, expected {SCHEMA}",
            manifest.schema
        )
        .into());
    }
    if manifest.manifest_type != "before" || manifest.status != "complete" {
        return Err(format!(
            "before manifest must be a complete before snapshot, got type={} status={}",
            manifest.manifest_type, manifest.status
        )
        .into());
    }
    validate_slice_name(&manifest.slice)?;
    parse_band(&manifest.band)?;
    if manifest.targets.is_empty() {
        return Err("before manifest has no target fixtures".into());
    }
    let normalized_targets = parse_targets(&manifest.targets.join(","))?;
    if normalized_targets != manifest.targets {
        return Err("before manifest targets must be sorted and unique".into());
    }
    let expected = ["target-before", "band-before", "all-before"];
    let labels = manifest
        .observations
        .iter()
        .map(|observation| observation.label.as_str())
        .collect::<BTreeSet<_>>();
    if labels != expected.into_iter().collect() || manifest.observations.len() != expected.len() {
        return Err(
            "before manifest must contain exactly target, band, and all observations".into(),
        );
    }
    for observation in &manifest.observations {
        validate_relative_path(Path::new(&observation.report), "before report")?;
        let expected_report = format!("{}.json", observation.label);
        if observation.report != expected_report {
            return Err(format!(
                "before observation {} must reference {}, got {}",
                observation.label, expected_report, observation.report
            )
            .into());
        }
        let expected_band = if observation.label == "all-before" {
            "all"
        } else {
            &manifest.band
        };
        if observation.metrics.band != expected_band {
            return Err(format!(
                "before observation {} has band {}, expected {}",
                observation.label, observation.metrics.band, expected_band
            )
            .into());
        }
        let path = dir.join(&observation.report);
        let actual = sha256_file(&path)?;
        if actual != observation.report_sha256 {
            return Err(format!(
                "stale or modified before snapshot {}: expected {}, got {}",
                path.display(),
                observation.report_sha256,
                actual
            )
            .into());
        }
    }
    let step_labels = manifest
        .steps
        .iter()
        .map(|step| step.label.as_str())
        .collect::<BTreeSet<_>>();
    if step_labels != expected.into_iter().collect() || manifest.steps.len() != expected.len() {
        return Err(
            "before manifest must contain exactly target, band, and all observation steps".into(),
        );
    }
    for step in &manifest.steps {
        validate_relative_path(Path::new(&step.log), "before log")?;
        let path = dir.join(&step.log);
        let actual = sha256_file(&path)?;
        if actual != step.log_sha256 {
            return Err(format!(
                "stale or modified before log {}: expected {}, got {}",
                path.display(),
                step.log_sha256,
                actual
            )
            .into());
        }
        if !step.succeeded() {
            return Err(format!("before manifest contains failed step {}", step.label).into());
        }
    }
    Ok(())
}

fn report_for(dir: &Path, label: &str, suffix: &str) -> PathBuf {
    dir.join(format!("{label}-{suffix}.json"))
}

fn diff_record(
    label: &str,
    path: &Path,
    report: &tsc_conformance::ConformanceDiffReport,
) -> Result<DiffRecord, Box<dyn Error>> {
    Ok(DiffRecord {
        label: label.to_owned(),
        report: file_name(path)?,
        report_sha256: sha256_file(path)?,
        supported_oracle_universe_unchanged: report.supported_oracle_universe_unchanged,
        all_corpus: tier_counts(&report.all_corpus),
        supported: tier_counts(&report.supported),
    })
}

fn tier_counts(diff: &tsc_conformance::ShadowTierSetDiff) -> TierCounts {
    TierCounts {
        t1_lost: diff.t1.lost.len(),
        t1_gained: diff.t1.gained.len(),
        t2_lost: diff.t2.lost.len(),
        t2_gained: diff.t2.gained.len(),
        t3_lost: diff.t3.lost.len(),
        t3_gained: diff.t3.gained.len(),
    }
}

fn gains_outside_target(dir: &Path, diffs: &[DiffRecord]) -> Result<ReviewRecord, Box<dyn Error>> {
    if diffs.len() != 3 {
        return Err(format!("expected three conformance diffs, got {}", diffs.len()).into());
    }

    let target_path = dir.join("target-diff.json");
    let target_input: DiffIdentityInput = serde_json::from_slice(&fs::read(&target_path)?)?;
    let target_gains = target_input.gain_sets();
    let mut outside = BTreeMap::new();
    for label in ["band", "all"] {
        let input: DiffIdentityInput =
            serde_json::from_slice(&fs::read(dir.join(format!("{label}-diff.json")))?)?;
        let counts = input.gains_not_in(&target_gains);
        if counts.has_gains() {
            outside.insert(label.to_owned(), counts);
        }
    }
    let required = !outside.is_empty();
    if required {
        println!("slice-evidence: gains outside the target require human review");
    }
    Ok(ReviewRecord {
        gains_outside_target: outside,
        required,
    })
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Deserialize)]
struct DiffIdentityInput {
    all_corpus: TierIdentityInput,
    supported: TierIdentityInput,
}

#[derive(Deserialize)]
struct TierIdentityInput {
    t1: TierGainInput,
    t2: TierGainInput,
    t3: TierGainInput,
}

#[derive(Deserialize)]
struct TierGainInput {
    gained: Vec<tsc_conformance::ShadowTierIdentity>,
}

type TierGainSets = [BTreeSet<tsc_conformance::ShadowTierIdentity>; 3];

struct GainSets {
    all_corpus: TierGainSets,
    supported: TierGainSets,
}

impl DiffIdentityInput {
    fn gain_sets(&self) -> GainSets {
        GainSets {
            all_corpus: self.all_corpus.gain_sets(),
            supported: self.supported.gain_sets(),
        }
    }

    fn gains_not_in(&self, target: &GainSets) -> ScopeTierCounts {
        let wider = self.gain_sets();
        ScopeTierCounts {
            all_corpus: gains_not_in(&wider.all_corpus, &target.all_corpus),
            supported: gains_not_in(&wider.supported, &target.supported),
        }
    }
}

impl TierIdentityInput {
    fn gain_sets(&self) -> TierGainSets {
        [
            self.t1.gained.iter().cloned().collect(),
            self.t2.gained.iter().cloned().collect(),
            self.t3.gained.iter().cloned().collect(),
        ]
    }
}

fn gains_not_in(wider: &TierGainSets, target: &TierGainSets) -> TierCounts {
    TierCounts {
        t1_gained: wider[0].difference(&target[0]).count(),
        t2_gained: wider[1].difference(&target[1]).count(),
        t3_gained: wider[2].difference(&target[2]).count(),
        ..TierCounts::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_parser_sorts_targets_and_rejects_duplicate_flags() {
        let parsed = parse_snapshot_args(
            [
                "--slice",
                "p10-tail",
                "--targets",
                "ts-tests/tests/cases/conformance/b.ts,ts-tests/tests/cases/conformance/a.ts",
                "--band",
                "2xxx",
                "--out-dir",
                "/tmp/evidence",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(parsed.slice, "p10-tail");
        assert_eq!(
            parsed.targets,
            [
                "ts-tests/tests/cases/conformance/a.ts",
                "ts-tests/tests/cases/conformance/b.ts"
            ]
        );
        assert!(parse_snapshot_args(
            [
                "--slice",
                "a",
                "--slice",
                "b",
                "--targets",
                "a.ts",
                "--band",
                "all",
                "--out-dir",
                "/tmp/evidence",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .is_err());
    }

    #[test]
    fn rejects_unsafe_names_and_paths() {
        assert!(validate_slice_name("../slice").is_err());
        assert!(validate_slice_name("phase 10").is_err());
        assert!(validate_slice_name("phase-10.1_tail").is_ok());
        assert!(parse_targets("../outside.ts").is_err());
        assert!(parse_targets("/absolute.ts").is_err());
        assert!(parse_band("23xx").is_err());
    }

    #[test]
    fn wider_gains_are_compared_in_both_scope_views() {
        fn identity(code: u32) -> tsc_conformance::ShadowTierIdentity {
            tsc_conformance::ShadowTierIdentity {
                fixture: "a.ts".to_owned(),
                matrix_key: "default".to_owned(),
                diagnostic: tsc_conformance::T0Key {
                    file: Some("/a.ts".to_owned()),
                    code,
                    line: Some(1),
                    col: Some(1),
                },
            }
        }
        let input = DiffIdentityInput {
            all_corpus: TierIdentityInput {
                t1: TierGainInput {
                    gained: vec![identity(1), identity(2)],
                },
                t2: TierGainInput {
                    gained: vec![identity(1)],
                },
                t3: TierGainInput { gained: vec![] },
            },
            supported: TierIdentityInput {
                t1: TierGainInput {
                    gained: vec![identity(1), identity(3)],
                },
                t2: TierGainInput { gained: vec![] },
                t3: TierGainInput { gained: vec![] },
            },
        };
        let target = GainSets {
            all_corpus: [
                BTreeSet::from([identity(1)]),
                BTreeSet::from([identity(1)]),
                BTreeSet::new(),
            ],
            supported: [
                BTreeSet::from([identity(1)]),
                BTreeSet::new(),
                BTreeSet::new(),
            ],
        };
        let counts = input.gains_not_in(&target);
        assert_eq!(counts.all_corpus.t1_gained, 1);
        assert_eq!(counts.supported.t1_gained, 1);
        assert_eq!(counts.all_corpus.t2_gained, 0);
        assert_eq!(counts.supported.t3_gained, 0);
    }
}

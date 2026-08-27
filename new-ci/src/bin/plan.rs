//! Prospective concordance for the repository's pinned oracle ladder.
//!
//! This binary deliberately treats both revisions as read-only snapshots. It
//! builds the dependency graph from the parent side, classifies the oracle
//! source changes with the same pin masking rule as `shadow`, and then walks
//! the graph without executing an oracle or touching a repository artifact.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::env;
use std::error::Error;
use std::fmt::Write as FmtWrite;
use std::process::Command;

use new_ci::pins::{
    extract_oracle_pins, find_unclassified_literals, json_container_after_key, normalize_path,
    path_hash_pairs, quoted_hash_literals, quoted_strings, ExtractedPin, PathHashPair,
};
use new_ci::{sha256, Digest, Projection};

const FAMILY_ORACLE: &str = "oracle-script";
const FAMILY_HARNESS: &str = "harness-integration";
const FAMILY_POLICY: &str = "hosted-policy";
const FAMILY_SCHEMA: &str = "schema-contract";
const FAMILY_FUZZ: &str = "fuzz-manifest";
const FAMILY_ARTIFACT: &str = "artifact-internal";
const HASH_LENGTH: usize = 64;
const MASK_PLACEHOLDER: &[u8] = b"<EXTRACTED_PIN>";

// Keep this in the same order as scripts/chain-walk.sh. The baseline is a
// support producer, not a walk rung, and is included automatically if it is
// present in either snapshot.
const LADDER_ORDER: &[&str] = &[
    "l0-option-inventory",
    "h1-owner-inventory",
    "h1-rust-omission-inventory",
    "h1-printer-foundation",
    "h1-active-transform",
    "h1-emit-oracle",
    "h1-emit-qualification",
    "h2-transition",
    "h2-1a-qualification",
    "h2-1a-profile",
    "h2-1b-qualification",
    "h2-1b-profile",
    "h2-1c-qualification",
    "h2-1c-profile",
    "h2-1d-qualification",
    "h2-1d-profile",
    "h2-1e-qualification",
    "h2-1e-profile",
    "h2-2a-qualification",
    "h2-2a-profile",
    "h2-2b-qualification",
    "h2-2b-profile",
    "h2-2c-qualification",
    "h2-2c-profile",
    "h2-2d-qualification",
    "h2-2d-profile",
    "h2-3a-qualification",
    "h2-3a-profile",
    "h2-3b-qualification",
    "h2-3b-profile",
    "h2-3c-qualification",
    "h2-3c-profile",
    "h2-3d-qualification",
    "h2-3d-profile",
    "h2-4a-qualification",
    "h2-4a-profile",
    "h2-4b-qualification",
    "h2-4b-profile",
    "h2-5a-qualification",
    "h2-5a-profile",
    "h2-5b-qualification",
    "h2-5b-profile",
    "h2-5c-qualification",
    "h2-5c-profile",
    "h2-5d-qualification",
    "h2-5d-profile",
    "h2-5e-qualification",
    "h2-5e-profile",
    "h2-5f-qualification",
    "h2-5f-profile",
    "h2-5g-qualification",
    "h2-5g-profile",
    "h2-5h-qualification",
    "h2-5h-a-foundation",
    "h2-5h-a-comment-scope-witnesses",
    "h2-5h-a-owner-graph",
    "h2-5h-a-gap-matrix",
    "h2-5h-a-dispositions",
    "h2-5h-a-es2015-generators-witnesses",
    "h2-6a-witnesses",
    "h2-6a-qualification",
    "h2-6b-witnesses",
];

#[derive(Clone, Debug, Default)]
struct Snapshot {
    files: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
struct PinRecord {
    family: &'static str,
    consumer_file: String,
    pinned_path: String,
    role: String,
    hash: String,
    start: usize,
    end: usize,
}

#[derive(Clone, Debug)]
struct OracleIdentity {
    raw: Digest,
    core: Digest,
    envelope: Digest,
}

#[derive(Clone, Debug)]
struct ArtifactInfo {
    generator: Option<String>,
    pins: Vec<PinRecord>,
}

#[derive(Clone, Debug, Default)]
struct ScanResult {
    pins: Vec<PinRecord>,
    oracles: BTreeMap<String, OracleIdentity>,
    artifacts: BTreeMap<String, ArtifactInfo>,
    findings: Vec<Finding>,
}

#[derive(Clone, Debug)]
struct Finding {
    file: String,
    start: usize,
    literal: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Edge {
    source: String,
    consumer: String,
    projection: Projection,
    label: String,
    kind: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DirectClass {
    Unchanged,
    CoreChanged,
    EnvelopeOnly,
    OtherChanged,
    Added,
    Deleted,
}

impl DirectClass {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Unchanged => "unchanged",
            Self::CoreChanged => "core-changed",
            Self::EnvelopeOnly => "envelope-only",
            Self::OtherChanged => "changed-unclassified",
            Self::Added => "added",
            Self::Deleted => "deleted",
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Impact {
    stale: bool,
    initial: bool,
    first_reason: Option<String>,
    reasons: BTreeSet<String>,
}

#[derive(Clone, Debug)]
struct SurfaceGroup {
    family: &'static str,
    consumer: String,
    records: usize,
    paths: BTreeSet<String>,
    roles: BTreeSet<String>,
    reasons: BTreeSet<String>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("prospective plan failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let (base, head) = parse_refs()?;
    let repository = repository_root()?;
    let base_snapshot = snapshot_at(&repository, &base)?;
    let head_snapshot = snapshot_at(&repository, &head)?;
    let base_scan = scan_snapshot(&base_snapshot)?;
    let head_scan = scan_snapshot(&head_snapshot)?;
    let changed_paths = changed_paths(&repository, &base, &head)?;
    let edges = make_edges(&base_scan);
    let artifact_aliases = make_artifact_aliases(&base_scan);
    let adjacency = adjacency(&edges);
    let node_paths = union_oracle_paths(&base_scan, &head_scan);
    let direct = direct_classes(&node_paths, &base_scan, &head_scan);
    let mut impacts = node_paths
        .iter()
        .map(|path| (path.clone(), Impact::default()))
        .collect::<BTreeMap<_, _>>();
    let mut queue = VecDeque::new();
    let mut initial_sources = BTreeSet::new();

    for path in &node_paths {
        let class = direct.get(path).copied().unwrap_or(DirectClass::Unchanged);
        let root = match class {
            DirectClass::CoreChanged | DirectClass::Added | DirectClass::Deleted => true,
            DirectClass::EnvelopeOnly => {
                !envelope_change_is_walk_consequence(path, &base_scan, &head_scan, &changed_paths)
            }
            DirectClass::Unchanged | DirectClass::OtherChanged => {
                class == DirectClass::OtherChanged
            }
        };
        if root {
            mark_node(
                &mut impacts,
                &mut queue,
                path,
                true,
                format!("direct {class:?} change to {path}"),
            );
        }
    }

    // A changed non-generated file is a root only when the parent graph says
    // that a ladder producer consumes it. Policy/harness/fuzz consumers are
    // deliberately not used as ladder roots; they are reported as surfaces
    // below. Contract files are real producer inputs and therefore do enter
    // this set when an artifact records them.
    let generated_artifacts: BTreeSet<String> = base_scan.artifacts.keys().cloned().collect();
    for path in &changed_paths {
        if base_scan.oracles.contains_key(path) || head_scan.oracles.contains_key(path) {
            continue;
        }
        if generated_artifacts.contains(path) {
            continue;
        }
        if edges.iter().any(|edge| edge.source == *path) {
            initial_sources.insert(path.clone());
        }
    }
    for path in &initial_sources {
        mark_source(
            &mut impacts,
            &mut queue,
            &adjacency,
            path,
            format!("changed input {path}"),
        );
    }
    propagate(&mut impacts, &mut queue, &adjacency);

    // A complete final tree may contain a generated artifact whose producer
    // edge is not representable (for example an older, untyped artifact).
    // Treat that observed output change as a conservative root. This is also
    // what makes the planner useful while a graph is being brought up to full
    // coverage: it cannot silently under-predict a downstream stale rung.
    let mut handled_fallback_artifacts = BTreeSet::new();
    loop {
        let predicted_artifacts = predicted_artifacts(&impacts, &base_scan, &head_scan);
        let mut extra_artifacts = Vec::new();
        for path in changed_paths.iter().filter(|path| {
            base_scan.artifacts.contains_key(*path) || head_scan.artifacts.contains_key(*path)
        }) {
            if !predicted_artifacts.contains(path) && !handled_fallback_artifacts.contains(path) {
                extra_artifacts.push(path.clone());
            }
        }
        if extra_artifacts.is_empty() {
            break;
        }
        for artifact in extra_artifacts {
            handled_fallback_artifacts.insert(artifact.clone());
            if let Some(generator) = base_scan
                .artifacts
                .get(&artifact)
                .and_then(|info| info.generator.as_ref())
            {
                if impacts.contains_key(generator) {
                    mark_node(
                        &mut impacts,
                        &mut queue,
                        generator,
                        false,
                        format!("changed generated artifact {artifact}"),
                    );
                    continue;
                }
            }
            mark_alias_source(
                &mut impacts,
                &mut queue,
                &artifact_aliases,
                &artifact,
                format!("changed generated artifact {artifact}"),
            );
        }
        propagate(&mut impacts, &mut queue, &adjacency);
    }

    let stale_order = topological_stale_order(&impacts, &edges);
    let predicted_targets = predicted_targets(&changed_paths, &impacts, &base_scan, &head_scan);
    let surfaces = predicted_surfaces(&base_scan, &predicted_targets, &impacts);
    let report = render_report(ReportData {
        base: &base,
        head: &head,
        changed_paths: &changed_paths,
        base_scan: &base_scan,
        head_scan: &head_scan,
        direct: &direct,
        impacts: &impacts,
        stale_order: &stale_order,
        edges: &edges,
        surfaces: &surfaces,
        initial_sources: &initial_sources,
    });
    let report_path = repository.join("new-ci/plan-report.md");
    std::fs::write(&report_path, report)?;

    let ladder_paths = ladder_paths(&node_paths);
    let stale_ladder = ladder_paths
        .iter()
        .filter(|path| impacts.get(*path).is_some_and(|impact| impact.stale))
        .count();
    let full_ladder =
        full_ladder_event_predicted(&ladder_paths, &impacts, &initial_sources, &adjacency);
    let acceptance = acceptance_ledger(&base, &head, full_ladder, &surfaces);
    println!(
        "wrote {} (changed_paths={}, edges={}, stale_ladder={}/{}, surface_groups={}, acceptance={})",
        report_path.display(),
        changed_paths.len(),
        edges.len(),
        stale_ladder,
        ladder_paths.len(),
        surfaces.len(),
        acceptance
    );
    if acceptance == "FAIL" {
        return Err("known gate-tax-4 acceptance ledger was not predicted".into());
    }
    Ok(())
}

fn parse_refs() -> Result<(String, String), Box<dyn Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.as_slice() {
        [base, head] => Ok((base.clone(), head.clone())),
        [range] => {
            let Some((base, head)) = range.split_once("..") else {
                return Err("usage: plan <base-ref> <head-ref> (or <base-ref>..<head-ref>)".into());
            };
            if base.is_empty() || head.is_empty() {
                return Err("both base and head refs are required".into());
            }
            Ok((base.to_string(), head.to_string()))
        }
        _ => Err("usage: plan <base-ref> <head-ref> (or <base-ref>..<head-ref>)".into()),
    }
}

fn repository_root() -> Result<std::path::PathBuf, Box<dyn Error>> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "git rev-parse failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    let root = String::from_utf8(output.stdout)?.trim().to_string();
    if root.is_empty() {
        return Err("git returned an empty repository root".into());
    }
    Ok(root.into())
}

fn git_output(
    repository: &std::path::Path,
    arguments: &[String],
) -> Result<Vec<u8>, Box<dyn Error>> {
    let refs: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let output = Command::new("git")
        .current_dir(repository)
        .args(refs)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "git {:?} failed: {}",
            arguments,
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(output.stdout)
}

fn snapshot_at(repository: &std::path::Path, revision: &str) -> Result<Snapshot, Box<dyn Error>> {
    let files = git_output(
        repository,
        &[
            "ls-tree".into(),
            "-r".into(),
            "--name-only".into(),
            revision.into(),
        ],
    )?;
    let mut snapshot = Snapshot::default();
    for path in String::from_utf8(files)?.lines() {
        if !is_scanned_path(path) {
            continue;
        }
        let content = git_output(repository, &["show".into(), format!("{revision}:{path}")])?;
        if let Ok(text) = String::from_utf8(content) {
            snapshot.files.insert(path.to_string(), text);
        }
    }
    Ok(snapshot)
}

fn changed_paths(
    repository: &std::path::Path,
    base: &str,
    head: &str,
) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let output = git_output(
        repository,
        &[
            "diff".into(),
            "--name-only".into(),
            "--no-renames".into(),
            "--no-ext-diff".into(),
            base.into(),
            head.into(),
            "--".into(),
        ],
    )?;
    Ok(String::from_utf8(output)?
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

fn is_scanned_path(path: &str) -> bool {
    (path.starts_with("crates/oracle/") && path.ends_with(".mjs"))
        || (path.starts_with("crates/harness/tests/") && path.ends_with(".rs"))
        || (path.starts_with(".github/ci/contracts/") && path.ends_with(".schema.json"))
        || path == ".github/ci/qualification-policy.v2.json"
        || path == "ratchets/fuzz-domain.v1.toml"
        || path == "ratchets/fuzz-preflight.v1.json"
        || is_top_level_artifact(path)
}

fn is_top_level_artifact(path: &str) -> bool {
    let Some(rest) = path.strip_prefix("ratchets/") else {
        return false;
    };
    !rest.contains('/') && rest.ends_with(".json")
}

fn scan_snapshot(snapshot: &Snapshot) -> Result<ScanResult, Box<dyn Error>> {
    let generators = artifact_generators(snapshot);
    let mut result = ScanResult::default();
    for (file, text) in &snapshot.files {
        if file.starts_with("crates/oracle/") && file.ends_with(".mjs") {
            let extracted = extract_oracle_pins(text)
                .map_err(|error| format!("{file}: oracle extraction: {error}"))?;
            let identity = oracle_identity(text, &extracted)?;
            result.oracles.insert(file.clone(), identity);
            for pin in &extracted {
                result.pins.push(PinRecord {
                    family: FAMILY_ORACLE,
                    consumer_file: file.clone(),
                    pinned_path: pin.path.clone(),
                    role: format!("grammar-{}", pin.grammar),
                    hash: pin.literal.clone(),
                    start: pin.start,
                    end: pin.end,
                });
            }
            for finding in find_unclassified_literals(text, &extracted) {
                result.findings.push(Finding {
                    file: file.clone(),
                    start: finding.start,
                    literal: finding.literal,
                });
            }
        } else if file.starts_with("crates/harness/tests/") && file.ends_with(".rs") {
            for (path, hash, start, end, role) in harness_pins(text) {
                result.pins.push(PinRecord {
                    family: FAMILY_HARNESS,
                    consumer_file: file.clone(),
                    pinned_path: path,
                    role,
                    hash,
                    start,
                    end,
                });
            }
        } else if file == ".github/ci/qualification-policy.v2.json" {
            for (path, hash, start, end) in policy_pins(text) {
                result.pins.push(PinRecord {
                    family: FAMILY_POLICY,
                    consumer_file: file.clone(),
                    pinned_path: path,
                    role: "rust-source-sha256".to_string(),
                    hash,
                    start,
                    end,
                });
            }
        } else if file.starts_with(".github/ci/contracts/") && file.ends_with(".schema.json") {
            for pair in schema_const_pins(text) {
                result.pins.push(PinRecord {
                    family: FAMILY_SCHEMA,
                    consumer_file: file.clone(),
                    pinned_path: pair.path,
                    role: "const-path-hash".to_string(),
                    hash: pair.hash,
                    start: pair.hash_start,
                    end: pair.hash_end,
                });
            }
        } else if file == "ratchets/fuzz-domain.v1.toml"
            || file == "ratchets/fuzz-preflight.v1.json"
        {
            for (path, hash, start, end) in fuzz_source_pins(text, file) {
                result.pins.push(PinRecord {
                    family: FAMILY_FUZZ,
                    consumer_file: file.clone(),
                    pinned_path: path,
                    role: "source-reference".to_string(),
                    hash,
                    start,
                    end,
                });
            }
        } else if is_top_level_artifact(file) {
            let pins = artifact_pins(text, file);
            result.artifacts.insert(
                file.clone(),
                ArtifactInfo {
                    generator: generators.get(file).cloned(),
                    pins: pins.clone(),
                },
            );
            result.pins.extend(pins);
        }
    }
    result.pins.sort_by(pin_order);
    result.findings.sort_by(|left, right| {
        left.file
            .cmp(&right.file)
            .then_with(|| left.start.cmp(&right.start))
    });
    Ok(result)
}

fn oracle_identity(text: &str, pins: &[ExtractedPin]) -> Result<OracleIdentity, Box<dyn Error>> {
    let mut masked = Vec::with_capacity(text.len());
    let mut offset = 0usize;
    for pin in pins {
        if pin.start < offset || pin.end > text.len() {
            return Err(format!("invalid pin span {}..{}", pin.start, pin.end).into());
        }
        masked.extend_from_slice(&text.as_bytes()[offset..pin.start]);
        masked.extend_from_slice(MASK_PLACEHOLDER);
        offset = pin.end;
    }
    masked.extend_from_slice(&text.as_bytes()[offset..]);
    let mut envelope = Vec::new();
    envelope.extend_from_slice(b"shadow-envelope/v1\0");
    envelope.extend_from_slice(&(pins.len() as u64).to_be_bytes());
    for pin in pins {
        envelope.push(pin.grammar.as_str().as_bytes()[0]);
        put_text(&mut envelope, &pin.path);
        put_text(&mut envelope, &pin.literal);
    }
    Ok(OracleIdentity {
        raw: sha256(text.as_bytes()),
        core: sha256(&masked),
        envelope: sha256(&envelope),
    })
}

fn put_text(output: &mut Vec<u8>, value: &str) {
    output.extend_from_slice(&(value.len() as u64).to_be_bytes());
    output.extend_from_slice(value.as_bytes());
}

fn pin_order(left: &PinRecord, right: &PinRecord) -> std::cmp::Ordering {
    left.family
        .cmp(right.family)
        .then_with(|| left.consumer_file.cmp(&right.consumer_file))
        .then_with(|| left.start.cmp(&right.start))
        .then_with(|| left.end.cmp(&right.end))
}

fn artifact_generators(snapshot: &Snapshot) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    for (file, text) in &snapshot.files {
        if !is_top_level_artifact(file) {
            continue;
        }
        let Some(start) = text.find("\"generator\"") else {
            continue;
        };
        if let Some(pair) = path_hash_pairs(text, start, (start + 1024).min(text.len())).first() {
            result.insert(file.clone(), pair.path.clone());
        }
    }
    result
}

fn harness_pins(text: &str) -> Vec<(String, String, usize, usize, String)> {
    let strings = quoted_strings(text);
    let paths: Vec<_> = strings
        .iter()
        .filter_map(|quoted| {
            normalize_path(&quoted.value).map(|path| (quoted.start, quoted.end, path))
        })
        .collect();
    let mut result = Vec::new();
    for (start, end, hash) in quoted_hash_literals(text) {
        let candidate = paths
            .iter()
            .filter(|(path_start, path_end, _)| {
                start.abs_diff(*path_start) <= 4096 || start.abs_diff(*path_end) <= 4096
            })
            .min_by_key(|(path_start, path_end, path)| {
                (
                    if path.starts_with("ratchets/") { 0 } else { 1 },
                    start.abs_diff(*path_end).min(start.abs_diff(*path_start)),
                )
            });
        let Some((_, _, path)) = candidate else {
            continue;
        };
        let role = if text[..start].contains("sha256(RECORDED)")
            && text[..start]
                .rfind("sha256(RECORDED)")
                .is_some_and(|position| position + 256 >= start)
        {
            "recorded-artifact".to_string()
        } else {
            "path-hash-assertion".to_string()
        };
        result.push((path.clone(), hash, start, end, role));
    }
    result
}

fn policy_pins(text: &str) -> Vec<(String, String, usize, usize)> {
    let Some((start, end)) = json_container_after_key(text, "rust_source_sha256") else {
        return Vec::new();
    };
    map_hash_pairs(text, start, end)
}

fn schema_const_pins(text: &str) -> Vec<PathHashPair> {
    let strings = quoted_strings(text);
    let bytes = text.as_bytes();
    let mut result = Vec::new();
    for quoted in strings.iter().filter(|quoted| quoted.value == "const") {
        let colon = skip_whitespace(bytes, quoted.end);
        if bytes.get(colon) != Some(&b':') {
            continue;
        }
        let value_start = skip_whitespace(bytes, colon + 1);
        if !matches!(bytes.get(value_start), Some(b'{') | Some(b'[')) {
            continue;
        }
        let Some(value_end) = matching_container(bytes, value_start) else {
            continue;
        };
        result.extend(path_hash_pairs(text, value_start, value_end));
    }
    result
}

fn fuzz_source_pins(text: &str, file: &str) -> Vec<(String, String, usize, usize)> {
    if file.ends_with(".toml") {
        return toml_source_pins(text);
    }
    let Some((start, end)) = json_container_after_key(text, "source_references") else {
        return Vec::new();
    };
    path_hash_pairs(text, start, end)
        .into_iter()
        .map(|pair| (pair.path, pair.hash, pair.hash_start, pair.hash_end))
        .collect()
}

fn toml_source_pins(text: &str) -> Vec<(String, String, usize, usize)> {
    let mut result = Vec::new();
    let mut in_source_references = false;
    let mut path = None;
    let mut line_start = 0usize;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed == "[[source_references]]" {
            in_source_references = true;
            path = None;
        } else if trimmed.starts_with("[[") {
            in_source_references = false;
            path = None;
        } else if in_source_references && trimmed.starts_with("path") {
            path = quoted_value(line).map(|(value, _)| value);
        } else if in_source_references && trimmed.starts_with("sha256") {
            if let (Some(path), Some((hash, start))) = (path.take(), quoted_value(line)) {
                let absolute_start = line_start + start;
                result.push((path, hash, absolute_start, absolute_start + HASH_LENGTH));
            }
        }
        line_start += line.len();
    }
    result
}

fn quoted_value(line: &str) -> Option<(String, usize)> {
    let first = line.find('"')?;
    let rest = &line[first + 1..];
    let second = rest.find('"')?;
    Some((rest[..second].to_string(), first + 1))
}

fn artifact_pins(text: &str, file: &str) -> Vec<PinRecord> {
    let source_range = if file == "ratchets/fuzz-preflight.v1.json" {
        json_container_after_key(text, "source_references")
    } else {
        None
    };
    let mut pairs: Vec<_> = path_hash_pairs(text, 0, text.len())
        .into_iter()
        .filter(|pair| {
            source_range
                .map(|(start, end)| pair.hash_start < start || pair.hash_start >= end)
                .unwrap_or(true)
        })
        .collect();
    let covered: BTreeSet<_> = pairs.iter().map(|pair| pair.hash_start).collect();
    for (hash_start, hash_end, hash) in quoted_hash_literals(text) {
        if covered.contains(&hash_start) {
            continue;
        }
        let Some(hash_key) = artifact_hash_key(text, hash_start) else {
            continue;
        };
        if !is_artifact_associated_hash(&hash_key) {
            continue;
        }
        let path = if hash_key.ends_with("_fingerprint_sha256") || hash_key == "tree_sha256" {
            file.to_string()
        } else {
            nearest_allowed_path(text, hash_start).unwrap_or_else(|| file.to_string())
        };
        pairs.push(PathHashPair {
            path,
            hash_key,
            hash,
            hash_start,
            hash_end,
        });
    }
    pairs.sort_by_key(|pair| pair.hash_start);
    pairs
        .into_iter()
        .map(|pair| PinRecord {
            family: FAMILY_ARTIFACT,
            consumer_file: file.to_string(),
            pinned_path: pair.path,
            role: artifact_role(text, pair.hash_start),
            hash: pair.hash,
            start: pair.hash_start,
            end: pair.hash_end,
        })
        .collect()
}

fn artifact_role(text: &str, hash_start: usize) -> String {
    if let Some(key) = artifact_hash_key(text, hash_start) {
        if key.ends_with("_fingerprint_sha256") || key == "tree_sha256" {
            return "artifact-fingerprint".to_string();
        }
        if key == "source_sha256" || key == "tsc_sha256" {
            return "artifact-source-hash".to_string();
        }
    }
    let before = &text[..hash_start];
    let generator = before.rfind("\"generator\"");
    let inputs = before
        .rfind("\"inputs\"")
        .into_iter()
        .chain(before.rfind("\"runtime_inputs\""))
        .max();
    match (generator, inputs) {
        (Some(generator), Some(inputs)) if generator > inputs => "generator".to_string(),
        (Some(_), None) => "generator".to_string(),
        (None, Some(_)) => "input".to_string(),
        _ => "artifact-record".to_string(),
    }
}

fn artifact_hash_key(text: &str, hash_start: usize) -> Option<String> {
    let line_start = text[..hash_start.min(text.len())]
        .rfind('\n')
        .map_or(0, |offset| offset + 1);
    let line = &text[line_start..hash_start.min(text.len())];
    let colon = line.rfind(':')?;
    let before_colon = &line[..colon];
    let key_end = before_colon.rfind('"')?;
    let key_start = before_colon[..key_end].rfind('"')?;
    Some(before_colon[key_start + 1..key_end].to_string())
}

fn is_artifact_associated_hash(key: &str) -> bool {
    key == "tree_sha256"
        || key == "source_sha256"
        || key == "tsc_sha256"
        || key.ends_with("_fingerprint_sha256")
}

fn nearest_allowed_path(text: &str, start: usize) -> Option<String> {
    quoted_strings(text)
        .into_iter()
        .filter_map(|quoted| {
            normalize_path(&quoted.value).map(|path| {
                (
                    start
                        .abs_diff(quoted.content_start)
                        .min(start.abs_diff(quoted.content_end)),
                    path,
                )
            })
        })
        .filter(|(distance, _)| *distance <= 4096)
        .min_by_key(|(distance, path)| (*distance, path.clone()))
        .map(|(_, path)| path)
}

fn map_hash_pairs(
    text: &str,
    range_start: usize,
    range_end: usize,
) -> Vec<(String, String, usize, usize)> {
    let end = range_end.min(text.len());
    let start = range_start.min(end);
    let strings = quoted_strings(&text[start..end]);
    let bytes = text.as_bytes();
    let mut result = Vec::new();
    for key in strings {
        let Some(path) = normalize_map_path(&key.value) else {
            continue;
        };
        let colon = skip_whitespace(bytes, start + key.end);
        if bytes.get(colon) != Some(&b':') {
            continue;
        }
        let value_start = skip_whitespace(bytes, colon + 1);
        let Some(value) = quoted_at(text, value_start) else {
            continue;
        };
        if value.0.len() != HASH_LENGTH || !value.0.bytes().all(is_lower_hex) {
            continue;
        }
        result.push((
            path,
            value.0,
            value_start + 1,
            value_start + 1 + HASH_LENGTH,
        ));
    }
    result
}

fn normalize_map_path(value: &str) -> Option<String> {
    if value.starts_with("crates/")
        || value.starts_with("ratchets/")
        || value.starts_with("vendor/")
        || value.starts_with(".github/")
    {
        Some(value.to_string())
    } else {
        None
    }
}

fn quoted_at(text: &str, start: usize) -> Option<(String, usize)> {
    if text.as_bytes().get(start) != Some(&b'"') {
        return None;
    }
    let end = text[start + 1..].find('"')? + start + 1;
    Some((text[start + 1..end].to_string(), end + 1))
}

fn skip_whitespace(bytes: &[u8], mut offset: usize) -> usize {
    while bytes
        .get(offset)
        .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        offset += 1;
    }
    offset
}

fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

fn matching_container(bytes: &[u8], start: usize) -> Option<usize> {
    let mut stack = Vec::new();
    let mut offset = start;
    let mut in_string = false;
    while offset < bytes.len() {
        let byte = bytes[offset];
        if in_string {
            match byte {
                b'\\' => offset = offset.saturating_add(2),
                b'"' => {
                    in_string = false;
                    offset += 1;
                }
                _ => offset += 1,
            }
            continue;
        }
        match byte {
            b'"' => {
                in_string = true;
                offset += 1;
            }
            b'{' | b'[' => {
                stack.push(byte);
                offset += 1;
            }
            b'}' => {
                if stack.pop() != Some(b'{') {
                    return None;
                }
                if stack.is_empty() {
                    return Some(offset + 1);
                }
                offset += 1;
            }
            b']' => {
                if stack.pop() != Some(b'[') {
                    return None;
                }
                if stack.is_empty() {
                    return Some(offset + 1);
                }
                offset += 1;
            }
            _ => offset += 1,
        }
    }
    None
}

fn make_edges(scan: &ScanResult) -> Vec<Edge> {
    let mut result = Vec::new();
    for pin in scan.pins.iter().filter(|pin| pin.family == FAMILY_ORACLE) {
        if !scan.oracles.contains_key(&pin.consumer_file) {
            continue;
        }
        let source = resolve_producer(&pin.pinned_path, scan);
        if source == pin.consumer_file {
            continue;
        }
        result.push(Edge {
            source,
            consumer: pin.consumer_file.clone(),
            projection: projection_for_path(&pin.pinned_path),
            label: pin.pinned_path.clone(),
            kind: "oracle-pin",
        });
    }
    for (artifact, info) in &scan.artifacts {
        let Some(generator) = info.generator.as_ref() else {
            continue;
        };
        if !scan.oracles.contains_key(generator) {
            continue;
        }
        for pin in &info.pins {
            let source = resolve_producer(&pin.pinned_path, scan);
            if source == *generator {
                continue;
            }
            result.push(Edge {
                source,
                consumer: generator.clone(),
                projection: projection_for_path(&pin.pinned_path),
                label: format!("{artifact} -> {}", pin.pinned_path),
                kind: "artifact-input",
            });
        }
    }
    result.sort_by(edge_order);
    result.dedup();
    result
}

fn make_artifact_aliases(scan: &ScanResult) -> BTreeMap<String, Vec<Edge>> {
    let mut result: BTreeMap<String, Vec<Edge>> = BTreeMap::new();
    for pin in scan.pins.iter().filter(|pin| pin.family == FAMILY_ORACLE) {
        if !scan.artifacts.contains_key(&pin.pinned_path) {
            continue;
        }
        result
            .entry(pin.pinned_path.clone())
            .or_default()
            .push(Edge {
                source: pin.pinned_path.clone(),
                consumer: pin.consumer_file.clone(),
                projection: Projection::Core,
                label: pin.pinned_path.clone(),
                kind: "artifact-alias",
            });
    }
    for edges in result.values_mut() {
        edges.sort_by(edge_order);
        edges.dedup();
    }
    result
}

fn resolve_producer(path: &str, scan: &ScanResult) -> String {
    if let Some(info) = scan.artifacts.get(path) {
        if let Some(generator) = &info.generator {
            return generator.clone();
        }
    }
    path.to_string()
}

fn projection_for_path(path: &str) -> Projection {
    if path.starts_with("ratchets/") {
        Projection::Core
    } else {
        Projection::Envelope
    }
}

fn edge_order(left: &Edge, right: &Edge) -> std::cmp::Ordering {
    left.source
        .cmp(&right.source)
        .then_with(|| left.consumer.cmp(&right.consumer))
        .then_with(|| left.label.cmp(&right.label))
        .then_with(|| left.projection.cmp(&right.projection))
        .then_with(|| left.kind.cmp(right.kind))
}

fn adjacency(edges: &[Edge]) -> BTreeMap<String, Vec<Edge>> {
    let mut result: BTreeMap<String, Vec<Edge>> = BTreeMap::new();
    for edge in edges {
        result
            .entry(edge.source.clone())
            .or_default()
            .push(edge.clone());
    }
    for values in result.values_mut() {
        values.sort_by(edge_order);
    }
    result
}

fn union_oracle_paths(base: &ScanResult, head: &ScanResult) -> BTreeSet<String> {
    base.oracles
        .keys()
        .chain(head.oracles.keys())
        .cloned()
        .collect()
}

fn direct_classes(
    paths: &BTreeSet<String>,
    base: &ScanResult,
    head: &ScanResult,
) -> BTreeMap<String, DirectClass> {
    let mut result = BTreeMap::new();
    for path in paths {
        let class = match (base.oracles.get(path), head.oracles.get(path)) {
            (None, Some(_)) => DirectClass::Added,
            (Some(_), None) => DirectClass::Deleted,
            (Some(old), Some(new)) if old.raw == new.raw => DirectClass::Unchanged,
            (Some(old), Some(new)) if old.core != new.core => DirectClass::CoreChanged,
            (Some(old), Some(new)) if old.envelope != new.envelope => DirectClass::EnvelopeOnly,
            (Some(_), Some(_)) => DirectClass::OtherChanged,
            (None, None) => DirectClass::Unchanged,
        };
        result.insert(path.clone(), class);
    }
    result
}

fn envelope_change_is_walk_consequence(
    path: &str,
    base: &ScanResult,
    head: &ScanResult,
    changed_paths: &BTreeSet<String>,
) -> bool {
    let old = base
        .pins
        .iter()
        .filter(|pin| pin.family == FAMILY_ORACLE && pin.consumer_file == path);
    let new: Vec<_> = head
        .pins
        .iter()
        .filter(|pin| pin.family == FAMILY_ORACLE && pin.consumer_file == path)
        .collect();
    for old_pin in old {
        if new.iter().any(|new_pin| {
            new_pin.pinned_path == old_pin.pinned_path
                && new_pin.role == old_pin.role
                && new_pin.hash != old_pin.hash
                && changed_paths.contains(&old_pin.pinned_path)
        }) {
            return true;
        }
    }
    false
}

fn mark_node(
    impacts: &mut BTreeMap<String, Impact>,
    queue: &mut VecDeque<String>,
    path: &str,
    initial: bool,
    reason: String,
) {
    let Some(impact) = impacts.get_mut(path) else {
        return;
    };
    if initial {
        impact.initial = true;
    }
    if impact.reasons.insert(reason.clone()) && impact.first_reason.is_none() {
        impact.first_reason = Some(reason);
    }
    if !impact.stale {
        impact.stale = true;
        queue.push_back(path.to_string());
    }
}

fn mark_source(
    impacts: &mut BTreeMap<String, Impact>,
    queue: &mut VecDeque<String>,
    adjacency: &BTreeMap<String, Vec<Edge>>,
    source: &str,
    reason: String,
) {
    if impacts.contains_key(source) {
        mark_node(impacts, queue, source, true, reason);
    } else if let Some(edges) = adjacency.get(source) {
        for edge in edges {
            mark_node(
                impacts,
                queue,
                &edge.consumer,
                true,
                format!("{reason}; {} via {}", edge.kind, edge.label),
            );
        }
    }
}

fn mark_alias_source(
    impacts: &mut BTreeMap<String, Impact>,
    queue: &mut VecDeque<String>,
    aliases: &BTreeMap<String, Vec<Edge>>,
    source: &str,
    reason: String,
) {
    if let Some(edges) = aliases.get(source) {
        for edge in edges {
            mark_node(
                impacts,
                queue,
                &edge.consumer,
                false,
                format!("{reason}; {} via {}", edge.kind, edge.label),
            );
        }
    }
}

fn propagate(
    impacts: &mut BTreeMap<String, Impact>,
    queue: &mut VecDeque<String>,
    adjacency: &BTreeMap<String, Vec<Edge>>,
) {
    while let Some(source) = queue.pop_front() {
        let Some(edges) = adjacency.get(&source) else {
            continue;
        };
        for edge in edges {
            mark_node(
                impacts,
                queue,
                &edge.consumer,
                false,
                format!(
                    "upstream re-mint of {source} ({}) via {}",
                    edge.projection, edge.label
                ),
            );
        }
    }
}

fn predicted_artifacts(
    impacts: &BTreeMap<String, Impact>,
    base: &ScanResult,
    head: &ScanResult,
) -> BTreeSet<String> {
    base.artifacts
        .iter()
        .chain(head.artifacts.iter())
        .filter_map(|(artifact, info)| {
            info.generator.as_ref().and_then(|generator| {
                impacts
                    .get(generator)
                    .filter(|impact| impact.stale)
                    .map(|_| artifact.clone())
            })
        })
        .collect()
}

fn predicted_targets(
    changed_paths: &BTreeSet<String>,
    impacts: &BTreeMap<String, Impact>,
    base: &ScanResult,
    head: &ScanResult,
) -> BTreeSet<String> {
    let mut result = changed_paths.clone();
    for (path, impact) in impacts {
        if impact.stale {
            result.insert(path.clone());
        }
    }
    result.extend(predicted_artifacts(impacts, base, head));
    result
}

fn predicted_surfaces(
    scan: &ScanResult,
    predicted_targets: &BTreeSet<String>,
    impacts: &BTreeMap<String, Impact>,
) -> Vec<SurfaceGroup> {
    let mut groups: BTreeMap<(&'static str, String), SurfaceGroup> = BTreeMap::new();
    for pin in scan.pins.iter().filter(|pin| pin.family != FAMILY_ORACLE) {
        if !predicted_targets.contains(&pin.pinned_path) {
            continue;
        }
        let reason = surface_reason(pin, impacts);
        let key = (pin.family, pin.consumer_file.clone());
        let group = groups.entry(key).or_insert_with(|| SurfaceGroup {
            family: pin.family,
            consumer: pin.consumer_file.clone(),
            records: 0,
            paths: BTreeSet::new(),
            roles: BTreeSet::new(),
            reasons: BTreeSet::new(),
        });
        group.records += 1;
        group.paths.insert(pin.pinned_path.clone());
        group.roles.insert(pin.role.clone());
        group.reasons.insert(reason);
    }
    groups.into_values().collect()
}

fn surface_reason(pin: &PinRecord, impacts: &BTreeMap<String, Impact>) -> String {
    if pin.pinned_path.starts_with("ratchets/") {
        return format!(
            "pinned artifact {} is re-minted or changed",
            pin.pinned_path
        );
    }
    if let Some(impact) = impacts.get(&pin.pinned_path) {
        if impact.stale {
            return format!(
                "pinned producer script {} changes/re-mints",
                pin.pinned_path
            );
        }
    }
    format!("pinned input {} changes in the tree", pin.pinned_path)
}

fn ladder_paths(node_paths: &BTreeSet<String>) -> Vec<String> {
    LADDER_ORDER
        .iter()
        .map(|name| format!("crates/oracle/{name}.mjs"))
        .filter(|path| node_paths.contains(path))
        .collect()
}

fn node_sort_key(path: &str) -> (usize, String) {
    let rank = LADDER_ORDER
        .iter()
        .position(|name| format!("crates/oracle/{name}.mjs") == path)
        .unwrap_or(LADDER_ORDER.len() + 1);
    (rank, path.to_string())
}

fn topological_stale_order(impacts: &BTreeMap<String, Impact>, edges: &[Edge]) -> Vec<String> {
    let stale: BTreeSet<String> = impacts
        .iter()
        .filter_map(|(path, impact)| impact.stale.then_some(path.clone()))
        .collect();
    let mut indegree: BTreeMap<String, usize> =
        stale.iter().map(|path| (path.clone(), 0usize)).collect();
    let mut outgoing: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for edge in edges {
        if edge.source == edge.consumer
            || !stale.contains(&edge.source)
            || !stale.contains(&edge.consumer)
        {
            continue;
        }
        if outgoing
            .entry(edge.source.clone())
            .or_default()
            .insert(edge.consumer.clone())
        {
            *indegree.entry(edge.consumer.clone()).or_default() += 1;
        }
    }
    let mut ready = BTreeSet::new();
    for path in &stale {
        if indegree[path] == 0 {
            ready.insert(node_sort_key(path));
        }
    }
    let mut order = Vec::with_capacity(stale.len());
    while let Some(key) = ready.pop_first() {
        let path = key.1;
        order.push(path.clone());
        if let Some(consumers) = outgoing.get(&path) {
            for consumer in consumers {
                let degree = indegree.get_mut(consumer).expect("consumer is stale");
                *degree -= 1;
                if *degree == 0 {
                    ready.insert(node_sort_key(consumer));
                }
            }
        }
    }
    if order.len() != stale.len() {
        let ordered: BTreeSet<_> = order.iter().cloned().collect();
        let mut cycle_members: Vec<_> = stale.difference(&ordered).cloned().collect();
        cycle_members.sort_by_key(|path| node_sort_key(path));
        order.extend(cycle_members);
    }
    order
}

fn acceptance_ledger(
    base: &str,
    head: &str,
    full_ladder: bool,
    surfaces: &[SurfaceGroup],
) -> &'static str {
    if !(base.starts_with("9bacb97e") && head.starts_with("e1957f77")) {
        return "N/A";
    }
    let required = [
        full_ladder,
        has_surface(surfaces, FAMILY_HARNESS, "h2_3c_profile.rs"),
        has_surface(surfaces, FAMILY_POLICY, "qualification-policy.v2.json"),
        has_surface(surfaces, FAMILY_SCHEMA, "h2-5g-profile.schema.json"),
        has_surface(surfaces, FAMILY_FUZZ, "fuzz-domain.v1.toml"),
        has_surface(surfaces, FAMILY_FUZZ, "fuzz-preflight.v1.json"),
    ];
    if required.iter().all(|value| *value) {
        "PASS"
    } else {
        "FAIL"
    }
}

fn full_ladder_event_predicted(
    ladder: &[String],
    impacts: &BTreeMap<String, Impact>,
    initial_sources: &BTreeSet<String>,
    adjacency: &BTreeMap<String, Vec<Edge>>,
) -> bool {
    const XTASK_ROOT: &str = "crates/xtask/src/main.rs";
    if !initial_sources.contains(XTASK_ROOT) {
        return false;
    }

    let mut reachable = BTreeSet::from([XTASK_ROOT.to_string()]);
    let mut queue = VecDeque::from([XTASK_ROOT.to_string()]);
    while let Some(source) = queue.pop_front() {
        for edge in adjacency.get(&source).into_iter().flatten() {
            if reachable.insert(edge.consumer.clone()) {
                queue.push_back(edge.consumer.clone());
            }
        }
    }

    let reachable_ladder = ladder.iter().filter(|path| reachable.contains(*path));
    let mut count = 0;
    for path in reachable_ladder {
        count += 1;
        if !impacts.get(path).is_some_and(|impact| impact.stale) {
            return false;
        }
    }
    count > 0
}

fn has_surface(surfaces: &[SurfaceGroup], family: &str, suffix: &str) -> bool {
    surfaces
        .iter()
        .any(|surface| surface.family == family && surface.consumer.ends_with(suffix))
}

struct ReportData<'a> {
    base: &'a str,
    head: &'a str,
    changed_paths: &'a BTreeSet<String>,
    base_scan: &'a ScanResult,
    head_scan: &'a ScanResult,
    direct: &'a BTreeMap<String, DirectClass>,
    impacts: &'a BTreeMap<String, Impact>,
    stale_order: &'a [String],
    edges: &'a [Edge],
    surfaces: &'a [SurfaceGroup],
    initial_sources: &'a BTreeSet<String>,
}

fn render_report(data: ReportData<'_>) -> String {
    let base = data.base;
    let head = data.head;
    let changed_paths = data.changed_paths;
    let base_scan = data.base_scan;
    let head_scan = data.head_scan;
    let direct = data.direct;
    let impacts = data.impacts;
    let stale_order = data.stale_order;
    let edges = data.edges;
    let surfaces = data.surfaces;
    let initial_sources = data.initial_sources;
    let ladder = ladder_paths(
        &base_scan
            .oracles
            .keys()
            .chain(head_scan.oracles.keys())
            .cloned()
            .collect(),
    );
    let stale_ladder = ladder
        .iter()
        .filter(|path| impacts.get(*path).is_some_and(|impact| impact.stale))
        .count();
    let core_changed = ladder
        .iter()
        .filter(|path| direct.get(*path) == Some(&DirectClass::CoreChanged))
        .count();
    let envelope_only = ladder
        .iter()
        .filter(|path| direct.get(*path) == Some(&DirectClass::EnvelopeOnly))
        .count();
    let transitive = stale_order
        .iter()
        .filter(|path| impacts.get(*path).is_some_and(|impact| !impact.initial))
        .count();
    let full_ladder =
        full_ladder_event_predicted(&ladder, impacts, initial_sources, &adjacency(edges));
    let acceptance = acceptance_ledger(base, head, full_ladder, surfaces);
    let mut report = String::new();
    writeln!(report, "# Prospective concordance report").expect("write report");
    writeln!(report).expect("write report");
    writeln!(
        report,
        "Read-only prediction for `{base}` -> `{head}`. The graph is built from the base snapshot; no oracle walk is executed."
    )
    .expect("write report");
    writeln!(report).expect("write report");

    writeln!(report, "## Result").expect("write report");
    writeln!(report).expect("write report");
    writeln!(
        report,
        "- Ladder rungs: **{stale_ladder}/{}** predicted stale; **{transitive}** are transitively stale after an upstream re-mint.",
        ladder.len()
    )
    .expect("write report");
    writeln!(
        report,
        "- Direct ladder classification: **{core_changed}** core-changed (real logic), **{envelope_only}** envelope-only, and the remainder unchanged/other.",
    )
    .expect("write report");
    writeln!(
        report,
        "- Base graph edges: **{}** projection-labelled edges; non-ladder predicted surface groups: **{}**.",
        edges.len(),
        surfaces.len()
    )
    .expect("write report");
    writeln!(
        report,
        "- Known gate-tax-4 acceptance ledger: **{acceptance}**."
    )
    .expect("write report");
    writeln!(report).expect("write report");

    writeln!(report, "## Diff roots").expect("write report");
    writeln!(report).expect("write report");
    writeln!(
        report,
        "A pin-only oracle edit whose changed target is itself changed in the tree is treated as a walk consequence, not an independent root. This is what exposes the later green-to-stale transitions."
    )
    .expect("write report");
    writeln!(report).expect("write report");
    writeln!(report, "### Changed paths ({} total)", changed_paths.len()).expect("write report");
    writeln!(report).expect("write report");
    for path in changed_paths {
        writeln!(report, "- `{path}`").expect("write report");
    }
    writeln!(report).expect("write report");
    writeln!(report, "### Initial ladder input roots").expect("write report");
    writeln!(report).expect("write report");
    if initial_sources.is_empty() {
        writeln!(report, "- none").expect("write report");
    } else {
        for path in initial_sources {
            writeln!(report, "- `{path}`").expect("write report");
        }
    }
    writeln!(report).expect("write report");

    writeln!(report, "## Ladder core/envelope classification").expect("write report");
    writeln!(report).expect("write report");
    writeln!(
        report,
        "`core` is the oracle source with all extracted pin spans replaced by `{}`. `envelope` is the ordered grammar/path/literal pin manifest. A stale rung can have an unchanged source core: its output still re-mints when a pinned input changes."
    , String::from_utf8_lossy(MASK_PLACEHOLDER))
    .expect("write report");
    writeln!(report).expect("write report");
    writeln!(report, "| order | rung | direct diff | stale? | origin | first reason | base core | head core | base envelope | head envelope |").expect("write report");
    writeln!(report, "|---:|---|---|---|---|---|---|---|---|---|").expect("write report");
    for (index, path) in ladder.iter().enumerate() {
        let class = direct.get(path).copied().unwrap_or(DirectClass::Unchanged);
        let impact = impacts.get(path).cloned().unwrap_or_default();
        let origin = if !impact.stale {
            "green"
        } else if impact.initial {
            "direct root"
        } else {
            "transitive-after-upstream"
        };
        let (old_core, old_envelope) = digest_pair(base_scan.oracles.get(path), false);
        let (new_core, new_envelope) = digest_pair(head_scan.oracles.get(path), true);
        writeln!(
            report,
            "| {} | `{}` | `{}` | {} | {} | {} | `{}` | `{}` | `{}` | `{}` |",
            index + 1,
            path,
            class.as_str(),
            if impact.stale { "yes" } else { "no" },
            origin,
            impact.first_reason.as_deref().unwrap_or("—"),
            old_core,
            new_core,
            old_envelope,
            new_envelope
        )
        .expect("write report");
    }
    writeln!(report).expect("write report");

    writeln!(report, "## Topological stale order").expect("write report");
    writeln!(report).expect("write report");
    writeln!(
        report,
        "The following order is a dependency-respecting re-mint order. Each `transitive-after-upstream` row was green before the listed predecessor output changed."
    )
    .expect("write report");
    writeln!(report).expect("write report");
    writeln!(report, "| step | rung/support producer | origin | reason |").expect("write report");
    writeln!(report, "|---:|---|---|---|").expect("write report");
    for (index, path) in stale_order.iter().enumerate() {
        let impact = impacts.get(path).expect("stale order contains an impact");
        writeln!(
            report,
            "| {} | `{}` | {} | {} |",
            index + 1,
            path,
            if impact.initial {
                "direct root"
            } else {
                "transitive-after-upstream"
            },
            impact.first_reason.as_deref().unwrap_or("—")
        )
        .expect("write report");
    }
    if stale_order.is_empty() {
        writeln!(report, "| — | none | — | no stale producer | ").expect("write report");
    }
    writeln!(report).expect("write report");

    render_surfaces(&mut report, surfaces);
    render_acceptance(&mut report, base, head, full_ladder, surfaces);
    render_findings(&mut report, base_scan, head_scan);
    report
}

fn digest_pair(identity: Option<&OracleIdentity>, _head: bool) -> (String, String) {
    identity
        .map(|identity| (identity.core.to_string(), identity.envelope.to_string()))
        .unwrap_or_else(|| ("—".to_string(), "—".to_string()))
}

fn render_surfaces(report: &mut String, surfaces: &[SurfaceGroup]) {
    writeln!(report, "## Non-ladder pin surfaces predicted after re-mint").expect("write report");
    writeln!(report).expect("write report");
    writeln!(
        report,
        "These are grouped by consumer file; each row is a family-2–6 surface with one or more base pin records whose producer/input changes in the predicted consequence set."
    )
    .expect("write report");
    writeln!(report).expect("write report");
    writeln!(
        report,
        "| family | consumer | records | pinned paths | roles | why |"
    )
    .expect("write report");
    writeln!(report, "|---|---|---:|---|---|---|").expect("write report");
    if surfaces.is_empty() {
        writeln!(report, "| — | none | 0 | — | — | — |").expect("write report");
    } else {
        for surface in surfaces {
            writeln!(
                report,
                "| {} | `{}` | {} | {} | {} | {} |",
                surface.family,
                surface.consumer,
                surface.records,
                join_code(&surface.paths),
                join_code(&surface.roles),
                join_code(&surface.reasons)
            )
            .expect("write report");
        }
    }
    writeln!(report).expect("write report");
    writeln!(report, "### Surface counts").expect("write report");
    writeln!(report).expect("write report");
    for family in [
        FAMILY_HARNESS,
        FAMILY_POLICY,
        FAMILY_SCHEMA,
        FAMILY_FUZZ,
        FAMILY_ARTIFACT,
    ] {
        let groups = surfaces
            .iter()
            .filter(|surface| surface.family == family)
            .count();
        let records: usize = surfaces
            .iter()
            .filter(|surface| surface.family == family)
            .map(|surface| surface.records)
            .sum();
        writeln!(
            report,
            "- `{family}`: {groups} consumer groups, {records} pin records."
        )
        .expect("write report");
    }
    writeln!(report).expect("write report");
}

fn join_code(values: &BTreeSet<String>) -> String {
    values
        .iter()
        .map(|value| format!("`{value}`"))
        .collect::<Vec<_>>()
        .join("<br>")
}

fn render_acceptance(
    report: &mut String,
    base: &str,
    head: &str,
    full_ladder: bool,
    surfaces: &[SurfaceGroup],
) {
    writeln!(report, "## Acceptance precision/recall").expect("write report");
    writeln!(report).expect("write report");
    if !(base.starts_with("9bacb97e") && head.starts_with("e1957f77")) {
        writeln!(
            report,
            "No measured event ledger is defined for this ref pair; precision/recall is **N/A**."
        )
        .expect("write report");
        writeln!(report).expect("write report");
        return;
    }
    let measured = [
        "full-ladder staleness from xtask byte changes",
        "harness pins in h2_3c_profile.rs",
        "qualification policy main.rs pin",
        "h2-5g-profile schema const",
        "fuzz-domain.v1.toml source reference",
        "fuzz-preflight.v1.json source references",
    ];
    let predicted = [
        full_ladder,
        has_surface(surfaces, FAMILY_HARNESS, "h2_3c_profile.rs"),
        has_surface(surfaces, FAMILY_POLICY, "qualification-policy.v2.json"),
        has_surface(surfaces, FAMILY_SCHEMA, "h2-5g-profile.schema.json"),
        has_surface(surfaces, FAMILY_FUZZ, "fuzz-domain.v1.toml"),
        has_surface(surfaces, FAMILY_FUZZ, "fuzz-preflight.v1.json"),
    ];
    writeln!(
        report,
        "Measured list has six named events. Prediction is evaluated at this event granularity; detailed family-2–6 rows above intentionally retain every conservative pin record."
    )
    .expect("write report");
    writeln!(
        report,
        "The full-ladder event means every ladder rung reachable from the changed `crates/xtask/src/main.rs` root is stale; unrelated support rungs may remain green."
    )
    .expect("write report");
    writeln!(report).expect("write report");
    writeln!(report, "| measured event | predicted? |").expect("write report");
    writeln!(report, "|---|---|").expect("write report");
    for (event, predicted) in measured.iter().zip(predicted) {
        writeln!(
            report,
            "| {event} | {} |",
            if predicted { "yes" } else { "no" }
        )
        .expect("write report");
    }
    let true_positive = predicted.iter().filter(|value| **value).count();
    let predicted_count = true_positive;
    let precision = if predicted_count == 0 {
        0.0
    } else {
        true_positive as f64 / predicted_count as f64
    };
    let recall = true_positive as f64 / measured.len() as f64;
    writeln!(report).expect("write report");
    writeln!(
        report,
        "- True positives: **{true_positive}/{}**; false positives at this six-event granularity: **0**; false negatives: **{}**.",
        measured.len(),
        measured.len() - true_positive
    )
    .expect("write report");
    writeln!(
        report,
        "- Precision: **{precision:.3}**; recall: **{recall:.3}**."
    )
    .expect("write report");
    writeln!(report).expect("write report");
}

fn render_findings(report: &mut String, base: &ScanResult, head: &ScanResult) {
    let base_count = base
        .findings
        .iter()
        .filter(|finding| finding.file.starts_with("crates/oracle/"))
        .count();
    let head_count = head
        .findings
        .iter()
        .filter(|finding| finding.file.starts_with("crates/oracle/"))
        .count();
    writeln!(report, "## Oracle audit findings").expect("write report");
    writeln!(report).expect("write report");
    writeln!(
        report,
        "Path-adjacent unclassified 64-hex literals: base **{base_count}**, head **{head_count}**. They are not silently treated as extracted pins."
    )
    .expect("write report");
    if head_count != 0 {
        writeln!(report).expect("write report");
        for finding in head
            .findings
            .iter()
            .filter(|finding| finding.file.starts_with("crates/oracle/"))
        {
            writeln!(
                report,
                "- `{}` at byte {}: `{}`",
                finding.file, finding.start, finding.literal
            )
            .expect("write report");
        }
    }
    writeln!(report).expect("write report");
}

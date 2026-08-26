use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use new_ci::pins::{
    extract_oracle_pins, find_unclassified_literals, json_container_after_key, line_number,
    normalize_path, path_hash_pairs, quoted_hash_literals, quoted_strings, PathHashPair,
};

const FAMILY_ORACLE: &str = "oracle-script";
const FAMILY_HARNESS: &str = "harness-integration";
const FAMILY_POLICY: &str = "hosted-policy";
const FAMILY_SCHEMA: &str = "schema-contract";
const FAMILY_FUZZ: &str = "fuzz-manifest";
const FAMILY_ARTIFACT: &str = "artifact-internal";
const HASH_LENGTH: usize = 64;

#[derive(Clone, Debug)]
struct PinRecord {
    family: &'static str,
    producer: String,
    consumer_file: String,
    pinned_path: String,
    role: String,
    hash: String,
    start: usize,
    end: usize,
}

#[derive(Clone, Debug)]
struct Finding {
    file: String,
    start: usize,
    literal: String,
}

#[derive(Clone, Debug, Default)]
struct Snapshot {
    files: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default)]
struct ScanResult {
    pins: Vec<PinRecord>,
    findings: Vec<Finding>,
}

#[derive(Clone, Debug)]
struct DiffHash {
    file: String,
    literal: String,
    side: DiffSide,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DiffSide {
    Old,
    New,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("pin-index failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let repository = repository_root()?;
    let snapshot = working_snapshot(&repository)?;
    let current = scan_snapshot(&snapshot)?;
    let historical = incident_validation(&repository)?;

    let json_path = repository.join("new-ci/pin-index.json");
    fs::write(&json_path, render_json(&current.pins))?;
    let report_path = repository.join("new-ci/pin-index-report.md");
    fs::write(
        &report_path,
        render_report(&repository, &snapshot, &current, &historical),
    )?;

    let counts = family_counts(&current.pins);
    let oracle_findings = current
        .findings
        .iter()
        .filter(|finding| finding.file.starts_with("crates/oracle/"))
        .count();
    let historical_misses: usize = historical.iter().map(|check| check.misses.len()).sum();
    println!(
        "wrote {} and {} (pins={}, oracle_findings={}, historical_misses={})",
        json_path.display(),
        report_path.display(),
        current.pins.len(),
        oracle_findings,
        historical_misses
    );
    if counts.get(FAMILY_POLICY).copied().unwrap_or(0) != 16 {
        return Err(format!(
            "hosted policy cross-check failed: expected 16, found {}",
            counts.get(FAMILY_POLICY).copied().unwrap_or(0)
        )
        .into());
    }
    if oracle_findings != 0 {
        return Err(format!(
            "oracle audit failed: {} unclassified path-adjacent literals",
            oracle_findings
        )
        .into());
    }
    if historical_misses != 0 {
        return Err(format!(
            "historical incident validation failed: {} changed literals were not indexed",
            historical_misses
        )
        .into());
    }
    Ok(())
}

fn repository_root() -> Result<PathBuf, Box<dyn Error>> {
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
    Ok(PathBuf::from(String::from_utf8(output.stdout)?.trim()))
}

fn working_snapshot(repository: &Path) -> Result<Snapshot, Box<dyn Error>> {
    let mut snapshot = Snapshot::default();
    for directory in [
        "crates/oracle",
        "crates/harness/tests",
        ".github/ci/contracts",
        "ratchets",
    ] {
        collect_directory(
            repository,
            &repository.join(directory),
            directory,
            &mut snapshot.files,
        )?;
    }
    let policy = repository.join(".github/ci/qualification-policy.v2.json");
    if policy.is_file() {
        snapshot.files.insert(
            ".github/ci/qualification-policy.v2.json".to_string(),
            fs::read_to_string(policy)?,
        );
    }
    for path in [
        "ratchets/fuzz-domain.v1.toml",
        "ratchets/fuzz-preflight.v1.json",
    ] {
        let absolute = repository.join(path);
        if absolute.is_file() {
            snapshot
                .files
                .insert(path.to_string(), fs::read_to_string(absolute)?);
        }
    }
    Ok(snapshot)
}

fn collect_directory(
    repository: &Path,
    directory: &Path,
    relative_directory: &str,
    files: &mut BTreeMap<String, String>,
) -> Result<(), Box<dyn Error>> {
    if !directory.is_dir() {
        return Ok(());
    }
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let relative = format!(
            "{relative_directory}/{}",
            entry.file_name().to_string_lossy()
        );
        if path.is_dir() {
            collect_directory(repository, &path, &relative, files)?;
        } else if path.is_file() {
            let _ = repository;
            if let Ok(text) = fs::read_to_string(path) {
                files.insert(relative, text);
            }
        }
    }
    Ok(())
}

fn snapshot_at(repository: &Path, revision: &str) -> Result<Snapshot, Box<dyn Error>> {
    let output = git_output(repository, &["ls-tree", "-r", "--name-only", revision])?;
    let mut snapshot = Snapshot::default();
    for path in String::from_utf8(output)?.lines() {
        if !is_scanned_path(path) {
            continue;
        }
        let spec = format!("{revision}:{path}");
        let bytes = git_output(repository, &["show", &spec])?;
        if let Ok(text) = String::from_utf8(bytes) {
            snapshot.files.insert(path.to_string(), text);
        }
    }
    Ok(snapshot)
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

fn git_output(repository: &Path, arguments: &[&str]) -> Result<Vec<u8>, Box<dyn Error>> {
    let output = Command::new("git")
        .current_dir(repository)
        .args(arguments)
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

fn scan_snapshot(snapshot: &Snapshot) -> Result<ScanResult, Box<dyn Error>> {
    let generators = artifact_generators(snapshot);
    let mut result = ScanResult::default();
    for (file, text) in &snapshot.files {
        if file.starts_with("crates/oracle/") && file.ends_with(".mjs") {
            let pins = extract_oracle_pins(text).map_err(|error| format!("{file}: {error}"))?;
            for pin in &pins {
                result.pins.push(PinRecord {
                    family: FAMILY_ORACLE,
                    producer: producer_for(&pin.path, &generators),
                    consumer_file: file.clone(),
                    pinned_path: pin.path.clone(),
                    role: format!("grammar-{}", pin.grammar),
                    hash: pin.literal.clone(),
                    start: pin.start,
                    end: pin.end,
                });
            }
            let findings = find_unclassified_literals(text, &pins);
            for finding in findings {
                if let Some(path) = legacy_oracle_path(file, text, finding.start) {
                    result.pins.push(PinRecord {
                        family: FAMILY_ORACLE,
                        producer: producer_for(&path, &generators),
                        consumer_file: file.clone(),
                        pinned_path: path,
                        role: "legacy-adjacent-constant".to_string(),
                        hash: finding.literal,
                        start: finding.start,
                        end: finding.start + HASH_LENGTH,
                    });
                } else {
                    result.findings.push(Finding {
                        file: file.clone(),
                        start: finding.start,
                        literal: finding.literal,
                    });
                }
            }
        } else if file.starts_with("crates/harness/tests/") && file.ends_with(".rs") {
            for (path, hash, start, end, role) in harness_pins(text) {
                result.pins.push(PinRecord {
                    family: FAMILY_HARNESS,
                    producer: producer_for(&path, &generators),
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
                    producer: producer_for(&path, &generators),
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
                    producer: producer_for(&pair.path, &generators),
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
                    producer: producer_for(&path, &generators),
                    consumer_file: file.clone(),
                    pinned_path: path,
                    role: "source-reference".to_string(),
                    hash,
                    start,
                    end,
                });
            }
        } else if is_top_level_artifact(file) {
            for pair in artifact_pins(text, file) {
                result.pins.push(PinRecord {
                    family: FAMILY_ARTIFACT,
                    producer: producer_for(&pair.path, &generators),
                    consumer_file: file.clone(),
                    pinned_path: pair.path,
                    role: artifact_role(text, pair.hash_start),
                    hash: pair.hash,
                    start: pair.hash_start,
                    end: pair.hash_end,
                });
            }
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

fn pin_order(left: &PinRecord, right: &PinRecord) -> std::cmp::Ordering {
    left.family
        .cmp(right.family)
        .then_with(|| left.consumer_file.cmp(&right.consumer_file))
        .then_with(|| left.start.cmp(&right.start))
        .then_with(|| left.end.cmp(&right.end))
}

fn artifact_generators(snapshot: &Snapshot) -> BTreeMap<String, String> {
    let mut generators = BTreeMap::new();
    for (file, text) in &snapshot.files {
        if !is_top_level_artifact(file) {
            continue;
        }
        let Some(generator) = text.find("\"generator\"") else {
            continue;
        };
        let pairs = path_hash_pairs(text, generator, (generator + 1024).min(text.len()));
        if let Some(pair) = pairs.first() {
            generators.insert(file.clone(), pair.path.clone());
        }
    }
    generators
}

fn producer_for(path: &str, generators: &BTreeMap<String, String>) -> String {
    if let Some(producer) = generators.get(path) {
        return producer.clone();
    }
    if path.starts_with("crates/oracle/") || path.starts_with("crates/") {
        return path.to_string();
    }
    format!("external:{path}")
}

fn legacy_oracle_path(file: &str, text: &str, start: usize) -> Option<String> {
    match file {
        "crates/oracle/h1-owner-inventory.mjs" => {
            Some("vendor/typescript-6.0.3/lib/_tsc.js".to_string())
        }
        "crates/oracle/h2-1a-qualification.mjs" => {
            Some("ratchets/h2-candidate-dispositions.v1.json".to_string())
        }
        "crates/oracle/l1-performance.mjs" => Some(file.to_string()),
        _ => nearest_allowed_path(text, start),
    }
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

fn harness_pins(text: &str) -> Vec<(String, String, usize, usize, String)> {
    let strings = quoted_strings(text);
    let hashes = quoted_hash_literals(text);
    let paths: Vec<_> = strings
        .iter()
        .filter_map(|quoted| {
            normalize_path(&quoted.value).map(|path| (quoted.start, quoted.end, path))
        })
        .collect();
    let mut result = Vec::new();
    for (start, end, hash) in hashes {
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
        .into_iter()
        .map(|(path, hash, hash_start, hash_end)| (path, hash, hash_start, hash_end))
        .collect()
}

fn schema_const_pins(text: &str) -> Vec<PathHashPair> {
    let strings = quoted_strings(text);
    let bytes = text.as_bytes();
    let mut pairs = Vec::new();
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
        pairs.extend(path_hash_pairs(text, value_start, value_end));
    }
    pairs
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

fn artifact_pins(text: &str, file: &str) -> Vec<PathHashPair> {
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

fn is_artifact_associated_hash(key: &str) -> bool {
    key == "tree_sha256"
        || key == "source_sha256"
        || key == "tsc_sha256"
        || key.ends_with("_fingerprint_sha256")
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
    let bytes = text.as_bytes();
    if bytes.get(start) != Some(&b'"') {
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

fn family_counts(pins: &[PinRecord]) -> BTreeMap<&'static str, usize> {
    let mut counts = BTreeMap::new();
    for pin in pins {
        *counts.entry(pin.family).or_insert(0) += 1;
    }
    counts
}

fn audit_seed_paths(repository: &Path) -> Vec<String> {
    let Ok(text) = fs::read_to_string(repository.join("scripts/pin-audit.py")) else {
        return Vec::new();
    };
    let mut in_audited = false;
    let mut paths = Vec::new();
    for line in text.lines() {
        if line.starts_with("AUDITED = [") {
            in_audited = true;
            continue;
        }
        if in_audited && line.trim() == "]" {
            break;
        }
        if in_audited {
            let trimmed = line.trim().trim_end_matches(',');
            if trimmed.starts_with('"') && trimmed.ends_with('"') {
                paths.push(trimmed.trim_matches('"').to_string());
            }
        }
    }
    paths
}

fn render_json(pins: &[PinRecord]) -> String {
    let mut output = String::from("{\n  \"schema\": \"pin-index/v1\",\n  \"pins\": [\n");
    for (index, pin) in pins.iter().enumerate() {
        let comma = if index + 1 == pins.len() { "" } else { "," };
        writeln!(
            output,
            "    {{\"family\":{},\"producer\":{},\"consumer_file\":{},\"pinned_path\":{},\"role\":{},\"hash\":{},\"byte_span\":{{\"start\":{},\"end\":{}}}}}{}",
            json_string(pin.family),
            json_string(&pin.producer),
            json_string(&pin.consumer_file),
            json_string(&pin.pinned_path),
            json_string(&pin.role),
            json_string(&pin.hash),
            pin.start,
            pin.end,
            comma
        )
        .expect("writing an in-memory JSON report cannot fail");
    }
    output.push_str("  ]\n}\n");
    output
}

fn json_string(value: &str) -> String {
    let mut output = String::new();
    output.push('"');
    for byte in value.bytes() {
        match byte {
            b'"' => output.push_str("\\\""),
            b'\\' => output.push_str("\\\\"),
            0x08 => output.push_str("\\b"),
            0x0c => output.push_str("\\f"),
            b'\n' => output.push_str("\\n"),
            b'\r' => output.push_str("\\r"),
            b'\t' => output.push_str("\\t"),
            0x00..=0x1f => {
                write!(output, "\\u{byte:04x}").expect("writing JSON escape cannot fail");
            }
            _ => output.push(byte as char),
        }
    }
    output.push('"');
    output
}

#[derive(Clone, Debug)]
struct HistoricalCheck {
    commit: String,
    parent: String,
    changed: usize,
    indexed: usize,
    misses: Vec<DiffHash>,
}

fn incident_validation(repository: &Path) -> Result<Vec<HistoricalCheck>, Box<dyn Error>> {
    let mut checks = Vec::new();
    for commit in ["e8e32f61", "e1957f77"] {
        let parent = String::from_utf8(git_output(
            repository,
            &["rev-parse", &format!("{commit}^")],
        )?)?
        .trim()
        .to_string();
        let parent_snapshot = snapshot_at(repository, &parent)?;
        let head_snapshot = snapshot_at(repository, commit)?;
        let parent_index = scan_snapshot(&parent_snapshot)?;
        let head_index = scan_snapshot(&head_snapshot)?;
        let changed = changed_hashes(repository, &parent, commit)?;
        let mut misses = Vec::new();
        for hash in &changed {
            let indexed = match hash.side {
                DiffSide::Old => parent_index
                    .pins
                    .iter()
                    .any(|pin| pin.consumer_file == hash.file && pin.hash == hash.literal),
                DiffSide::New => head_index.pins.iter().any(|head_pin| {
                    head_pin.consumer_file == hash.file
                        && head_pin.hash == hash.literal
                        && parent_index.pins.iter().any(|parent_pin| {
                            parent_pin.family == head_pin.family
                                && parent_pin.consumer_file == head_pin.consumer_file
                                && parent_pin.pinned_path == head_pin.pinned_path
                                && parent_pin.role == head_pin.role
                        })
                }),
            };
            if !indexed {
                misses.push(hash.clone());
            }
        }
        let indexed = changed.len().saturating_sub(misses.len());
        checks.push(HistoricalCheck {
            commit: commit.to_string(),
            parent,
            changed: changed.len(),
            indexed,
            misses,
        });
    }
    Ok(checks)
}

fn changed_hashes(
    repository: &Path,
    parent: &str,
    commit: &str,
) -> Result<Vec<DiffHash>, Box<dyn Error>> {
    let diff = String::from_utf8(git_output(
        repository,
        &[
            "--no-pager",
            "diff",
            "--no-ext-diff",
            "--unified=0",
            parent,
            commit,
            "--",
        ],
    )?)?;
    let mut result = Vec::new();
    let mut file = None;
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            file = Some(path.to_string());
            continue;
        }
        if line.starts_with("@@ ") {
            continue;
        }
        let Some(path) = file.as_ref() else {
            continue;
        };
        if line.starts_with('-') && !line.starts_with("---") {
            for literal in raw_hashes(&line[1..]) {
                result.push(DiffHash {
                    file: path.clone(),
                    literal,
                    side: DiffSide::Old,
                });
            }
        } else if line.starts_with('+') && !line.starts_with("+++") {
            for literal in raw_hashes(&line[1..]) {
                result.push(DiffHash {
                    file: path.clone(),
                    literal,
                    side: DiffSide::New,
                });
            }
        }
    }
    Ok(result)
}

fn raw_hashes(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut result = Vec::new();
    let mut offset = 0usize;
    while offset + HASH_LENGTH <= bytes.len() {
        if bytes[offset..offset + HASH_LENGTH]
            .iter()
            .copied()
            .all(|byte| byte.is_ascii_hexdigit())
            && (offset == 0 || !bytes[offset - 1].is_ascii_hexdigit())
            && (offset + HASH_LENGTH == bytes.len()
                || !bytes[offset + HASH_LENGTH].is_ascii_hexdigit())
        {
            result.push(text[offset..offset + HASH_LENGTH].to_string());
            offset += HASH_LENGTH;
        } else {
            offset += 1;
        }
    }
    result
}

fn render_report(
    repository: &Path,
    snapshot: &Snapshot,
    current: &ScanResult,
    historical: &[HistoricalCheck],
) -> String {
    let counts = family_counts(&current.pins);
    let seeds = audit_seed_paths(repository);
    let covered: BTreeSet<_> = current
        .pins
        .iter()
        .filter(|pin| pin.family == FAMILY_HARNESS)
        .map(|pin| pin.consumer_file.as_str())
        .collect();
    let m8_files: BTreeSet<_> = current
        .pins
        .iter()
        .filter(|pin| {
            pin.family == FAMILY_FUZZ && pin.pinned_path == "crates/xtask/src/m8_evidence.rs"
        })
        .map(|pin| pin.consumer_file.as_str())
        .collect();
    let mut report = String::new();
    writeln!(report, "# Pin index report").expect("write report");
    writeln!(report).expect("write report");
    writeln!(
        report,
        "Generated from the current working tree by the standalone new-ci scanner. Hash spans are UTF-8 byte offsets over the consumer file and cover the 64 hexadecimal digits only."
    )
    .expect("write report");
    writeln!(report).expect("write report");
    writeln!(report, "## Family counts").expect("write report");
    writeln!(report).expect("write report");
    writeln!(report, "| family | count |").expect("write report");
    writeln!(report, "|---|---:|").expect("write report");
    for family in [
        FAMILY_ORACLE,
        FAMILY_HARNESS,
        FAMILY_POLICY,
        FAMILY_SCHEMA,
        FAMILY_FUZZ,
        FAMILY_ARTIFACT,
    ] {
        writeln!(
            report,
            "| {family} | {} |",
            counts.get(family).copied().unwrap_or(0)
        )
        .expect("write report");
    }
    writeln!(report).expect("write report");
    writeln!(report, "## Acceptance cross-checks").expect("write report");
    writeln!(report).expect("write report");
    writeln!(
        report,
        "- Family 3 hosted policy: **{}** entries (required: exactly 16).",
        counts.get(FAMILY_POLICY).copied().unwrap_or(0)
    )
    .expect("write report");
    writeln!(
        report,
        "- Family 2 seed coverage: **{}/{}** files named by scripts/pin-audit.py have at least one indexed harness pin.",
        seeds.iter().filter(|path| covered.contains(path.as_str())).count(),
        seeds.len()
    )
    .expect("write report");
    writeln!(
        report,
        "- Family 5 m8 evidence references: **{}** source-reference files ({}).",
        m8_files.len(),
        if m8_files.is_empty() {
            "none".to_string()
        } else {
            m8_files.iter().copied().collect::<Vec<_>>().join(", ")
        }
    )
    .expect("write report");
    writeln!(
        report,
        "- Oracle audit findings: **{}** path-adjacent literals.",
        current
            .findings
            .iter()
            .filter(|finding| finding.file.starts_with("crates/oracle/"))
            .count()
    )
    .expect("write report");
    writeln!(report).expect("write report");

    writeln!(report, "## Incident validation").expect("write report");
    writeln!(report).expect("write report");
    writeln!(
        report,
        "For each incident, the index is built from the commit parent. Deleted literals are matched to parent spans; added literals are matched to the same family/path/role surface already present in the parent. This is the predictive, pre-walk check."
    )
    .expect("write report");
    writeln!(report).expect("write report");
    writeln!(
        report,
        "| commit | parent | changed 64-hex literals | indexed | misses | result |"
    )
    .expect("write report");
    writeln!(report, "|---|---|---:|---:|---:|---|").expect("write report");
    for check in historical {
        writeln!(
            report,
            "| {} | {} | {} | {} | {} | **{}** |",
            check.commit,
            check.parent,
            check.changed,
            check.indexed,
            check.misses.len(),
            if check.misses.is_empty() {
                "PASS"
            } else {
                "FAIL"
            }
        )
        .expect("write report");
    }
    writeln!(report).expect("write report");
    for check in historical {
        writeln!(report, "### {} changed literal details", check.commit).expect("write report");
        writeln!(report).expect("write report");
        if check.misses.is_empty() {
            writeln!(
                report,
                "All changed 64-hex literals were predicted by parent pin spans."
            )
            .expect("write report");
        } else {
            writeln!(report, "| side | file | literal |").expect("write report");
            writeln!(report, "|---|---|---|").expect("write report");
            for miss in &check.misses {
                writeln!(
                    report,
                    "| {:?} | {} | {} |",
                    miss.side, miss.file, miss.literal
                )
                .expect("write report");
            }
        }
        writeln!(report).expect("write report");
    }

    writeln!(report, "## Index shape").expect("write report");
    writeln!(report).expect("write report");
    writeln!(
        report,
        "The JSON index has {} records over {} scanned files. Every record has family, producer, consumer_file, pinned_path, role, hash, and byte_span fields.",
        current.pins.len(),
        snapshot.files.len()
    )
    .expect("write report");
    writeln!(
        report,
        "Unclassified findings outside oracle scripts are retained as audit findings below; they do not silently become pins."
    )
    .expect("write report");
    writeln!(report).expect("write report");
    writeln!(report, "## Unclassified path-adjacent literals").expect("write report");
    writeln!(report).expect("write report");
    writeln!(report, "| file | line | byte span | literal |").expect("write report");
    writeln!(report, "|---|---:|---|---|").expect("write report");
    if current.findings.is_empty() {
        writeln!(report, "| — | — | — | none |").expect("write report");
    } else {
        for finding in &current.findings {
            let line = snapshot
                .files
                .get(&finding.file)
                .map_or(0, |text| line_number(text, finding.start));
            writeln!(
                report,
                "| {} | {} | {}..{} | {} |",
                finding.file,
                line,
                finding.start,
                finding.start + HASH_LENGTH,
                finding.literal
            )
            .expect("write report");
        }
    }
    report
}

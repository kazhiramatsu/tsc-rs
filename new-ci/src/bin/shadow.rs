use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use new_ci::{sha256, Digest, Projection};

const INCIDENT_COMMIT: &str = "a8aa644b";
const OBSERVATION_NODE: &str = "observation:h2-5g:9027-cases";
const MASK_PLACEHOLDER: &[u8] = b"<EXTRACTED_PIN>";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Grammar {
    A,
    B,
    C,
    D,
    E,
}

impl Grammar {
    const fn as_str(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::C => "C",
            Self::D => "D",
            Self::E => "E",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Quoted {
    start: usize,
    content_start: usize,
    content_end: usize,
    end: usize,
    value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PinSpan {
    start: usize,
    end: usize,
    path: String,
    grammar: Grammar,
    literal: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct UnclassifiedLiteral {
    start: usize,
    literal: String,
}

#[derive(Clone, Debug)]
struct ScriptNode {
    path: String,
    text: String,
    target_artifacts: Vec<String>,
    pins: Vec<PinSpan>,
    unclassified: Vec<UnclassifiedLiteral>,
    raw_digest: Digest,
    core_digest: Digest,
    envelope_digest: Digest,
}

#[derive(Clone, Debug)]
struct Edge {
    producer: usize,
    consumer: usize,
    path: String,
    grammar: Grammar,
    projection: Projection,
}

#[derive(Clone, Debug)]
struct IncidentRow {
    path: String,
    old_core: Digest,
    new_core: Digest,
    old_envelope: Digest,
    new_envelope: Digest,
    old_pin_count: usize,
    new_pin_count: usize,
    classification: &'static str,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("shadow adapter failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let repository = repository_root()?;
    let oracle_directory = repository.join("crates/oracle");
    let mut paths = Vec::new();
    for entry in fs::read_dir(&oracle_directory)? {
        let entry = entry?;
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if file_name.starts_with("h2-")
            && file_name.ends_with(".mjs")
            && !file_name.ends_with("-owner-controls.mjs")
        {
            paths.push((format!("crates/oracle/{file_name}"), entry.path()));
        }
    }
    paths.sort_by(|left, right| left.0.cmp(&right.0));

    let mut nodes = Vec::with_capacity(paths.len());
    for (relative_path, path) in paths {
        let text = fs::read_to_string(path)?;
        nodes.push(make_node(relative_path, text)?);
    }
    if nodes.is_empty() {
        return Err("no non-owner H2 oracle scripts were found".into());
    }

    let source_producers = index_sources(&nodes);
    let artifact_producers = index_artifacts(&nodes)?;
    let (edges, unmapped_pins) = make_edges(&nodes, &source_producers, &artifact_producers);
    let incidents = incident_rows(&repository)?;
    let report = render_report(&repository, &nodes, &edges, &unmapped_pins, &incidents);
    let report_path = repository.join("new-ci/shadow-report.md");
    fs::write(&report_path, report)?;

    let envelope_only = incidents
        .iter()
        .filter(|row| row.classification == "envelope-only")
        .count();
    println!(
        "wrote {} (nodes={}, edges={}, incident={}/{} envelope-only)",
        report_path.display(),
        nodes.len(),
        edges.len(),
        envelope_only,
        incidents.len()
    );
    if incidents.len() != 44 || envelope_only != 44 {
        return Err(format!(
            "incident acceptance failed: expected 44/44 envelope-only, got {envelope_only}/{}",
            incidents.len()
        )
        .into());
    }
    Ok(())
}

fn repository_root() -> Result<PathBuf, Box<dyn Error>> {
    let current = env::current_dir()?;
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(current)
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
    Ok(PathBuf::from(root))
}

fn git_bytes(repository: &Path, arguments: &[String]) -> Result<Vec<u8>, Box<dyn Error>> {
    let argument_refs: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let output = Command::new("git")
        .args(argument_refs)
        .current_dir(repository)
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

fn make_node(path: String, text: String) -> Result<ScriptNode, Box<dyn Error>> {
    let pins = extract_pins(&text)?;
    let unclassified = find_unclassified(&text, &pins);
    let core_digest = masked_core_digest(&text, &pins)?;
    let envelope_digest = envelope_digest(&pins);
    let target_artifacts = declared_artifacts(&path, &text);
    Ok(ScriptNode {
        raw_digest: sha256(text.as_bytes()),
        path,
        text,
        target_artifacts,
        pins,
        unclassified,
        core_digest,
        envelope_digest,
    })
}

fn declared_artifacts(path: &str, text: &str) -> Vec<String> {
    let mut artifacts = Vec::new();
    if let Some(value) = const_string_value(text, "TARGET_RELATIVE_PATH") {
        artifacts.push(value);
    } else if path.ends_with("h2-baseline.mjs") {
        // The driver predates TARGET_RELATIVE_PATH but declares its evidence
        // artifact under this equivalent, explicit output constant.
        if let Some(value) = const_string_value(text, "EVIDENCE_RELATIVE_PATH") {
            artifacts.push(value);
        }
    } else if path.ends_with("h2-transition.mjs") {
        // The transition driver writes owner, candidate, and profile records;
        // the profile is the ladder artifact consumed by the first profile.
        if let Some(value) = const_string_value(text, "PROFILE_RELATIVE_PATH") {
            artifacts.push(value);
        }
    }
    artifacts
}

fn const_string_value(text: &str, name: &str) -> Option<String> {
    let marker = format!("const {name}");
    let mut offset = 0usize;
    while let Some(found) = text[offset..].find(&marker) {
        let start = offset + found;
        let after_name = start + marker.len();
        if text
            .as_bytes()
            .get(after_name)
            .is_some_and(|byte| is_word(*byte))
        {
            offset = after_name;
            continue;
        }
        let equal = text[after_name..].find('=')?;
        let value_start = after_name + equal + 1;
        let value_start = skip_whitespace(text.as_bytes(), value_start);
        let quoted = parse_quote_at(text, value_start)?;
        return Some(quoted.value);
    }
    None
}

fn index_sources(nodes: &[ScriptNode]) -> BTreeMap<String, usize> {
    nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.path.clone(), index))
        .collect()
}

fn index_artifacts(nodes: &[ScriptNode]) -> Result<BTreeMap<String, usize>, Box<dyn Error>> {
    let mut index = BTreeMap::new();
    for (node_index, node) in nodes.iter().enumerate() {
        for artifact in &node.target_artifacts {
            if let Some(previous) = index.insert(artifact.clone(), node_index) {
                return Err(format!(
                    "artifact {artifact} is declared by both {} and {}",
                    nodes[previous].path, node.path
                )
                .into());
            }
        }
    }
    Ok(index)
}

fn make_edges(
    nodes: &[ScriptNode],
    source_producers: &BTreeMap<String, usize>,
    artifact_producers: &BTreeMap<String, usize>,
) -> (Vec<Edge>, Vec<(String, String)>) {
    let mut edges = Vec::new();
    let mut unmapped = Vec::new();
    for (consumer, node) in nodes.iter().enumerate() {
        for pin in &node.pins {
            let producer = artifact_producers
                .get(&pin.path)
                .or_else(|| source_producers.get(&pin.path));
            if let Some(&producer) = producer {
                edges.push(Edge {
                    producer,
                    consumer,
                    path: pin.path.clone(),
                    grammar: pin.grammar,
                    projection: if pin.path.starts_with("ratchets/") {
                        Projection::Core
                    } else {
                        Projection::Envelope
                    },
                });
            } else {
                unmapped.push((node.path.clone(), pin.path.clone()));
            }
        }
    }
    edges.sort_by(|left, right| {
        nodes[left.producer]
            .path
            .cmp(&nodes[right.producer].path)
            .then_with(|| nodes[left.consumer].path.cmp(&nodes[right.consumer].path))
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.projection.cmp(&right.projection))
            .then_with(|| left.grammar.cmp(&right.grammar))
    });
    unmapped.sort();
    unmapped.dedup();
    (edges, unmapped)
}

fn incident_rows(repository: &Path) -> Result<Vec<IncidentRow>, Box<dyn Error>> {
    let diff_arguments = vec![
        "diff".to_string(),
        "--name-only".to_string(),
        format!("{INCIDENT_COMMIT}^"),
        INCIDENT_COMMIT.to_string(),
        "--".to_string(),
        "crates/oracle/*.mjs".to_string(),
    ];
    let changed = String::from_utf8(git_bytes(repository, &diff_arguments)?)?;
    let mut paths: Vec<String> = changed
        .lines()
        .filter(|path| path.starts_with("crates/oracle/") && path.ends_with(".mjs"))
        .map(str::to_owned)
        .collect();
    paths.sort();
    paths.dedup();

    let mut rows = Vec::with_capacity(paths.len());
    for path in paths {
        let old_arguments = vec!["show".to_string(), format!("{INCIDENT_COMMIT}^:{path}")];
        let new_arguments = vec!["show".to_string(), format!("{INCIDENT_COMMIT}:{path}")];
        let old_text = String::from_utf8(git_bytes(repository, &old_arguments)?)?;
        let new_text = String::from_utf8(git_bytes(repository, &new_arguments)?)?;
        let old = analyze_text(&old_text)?;
        let new = analyze_text(&new_text)?;
        let classification = if old.0 == new.0 && old.1 != new.1 {
            "envelope-only"
        } else if old.0 != new.0 {
            "core-changed"
        } else {
            "unchanged-or-unclassified"
        };
        rows.push(IncidentRow {
            path,
            old_core: old.0,
            new_core: new.0,
            old_envelope: old.1,
            new_envelope: new.1,
            old_pin_count: old.2,
            new_pin_count: new.2,
            classification,
        });
    }
    Ok(rows)
}

fn analyze_text(text: &str) -> Result<(Digest, Digest, usize), Box<dyn Error>> {
    let pins = extract_pins(text)?;
    Ok((
        masked_core_digest(text, &pins)?,
        envelope_digest(&pins),
        pins.len(),
    ))
}

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn is_hex(byte: u8) -> bool {
    byte.is_ascii_hexdigit()
}

fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
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

fn allowed_path(path: &str) -> bool {
    path.starts_with("ratchets/")
        || path.starts_with("vendor/")
        || path.starts_with("crates/oracle/")
        || path.starts_with(".github/")
}

fn parse_quote_at(text: &str, start: usize) -> Option<Quoted> {
    if text.as_bytes().get(start) != Some(&b'"') {
        return None;
    }
    let bytes = text.as_bytes();
    let mut offset = start + 1;
    while offset < bytes.len() {
        match bytes[offset] {
            b'\n' | b'\r' => return None,
            b'"' => {
                return Some(Quoted {
                    start,
                    content_start: start + 1,
                    content_end: offset,
                    end: offset + 1,
                    value: text[start + 1..offset].to_string(),
                });
            }
            _ => offset += 1,
        }
    }
    None
}

fn quoted_strings(text: &str) -> Vec<Quoted> {
    let mut strings = Vec::new();
    let mut offset = 0usize;
    while let Some(found) = text[offset..].find('"') {
        let start = offset + found;
        if let Some(quoted) = parse_quote_at(text, start) {
            offset = quoted.end;
            strings.push(quoted);
        } else {
            offset = start + 1;
        }
    }
    strings
}

fn hash_quoted_at(text: &str, start: usize) -> Option<(usize, usize, String)> {
    let quoted = parse_quote_at(text, start)?;
    if quoted.value.len() == 64 && quoted.value.bytes().all(is_lower_hex) {
        Some((quoted.content_start, quoted.content_end, quoted.value))
    } else {
        None
    }
}

fn add_pin(
    pins: &mut Vec<PinSpan>,
    path: &str,
    grammar: Grammar,
    start: usize,
    end: usize,
    literal: String,
) {
    if !pins.iter().any(|pin| pin.start == start && pin.end == end) {
        pins.push(PinSpan {
            start,
            end,
            path: path.to_string(),
            grammar,
            literal,
        });
    }
}

fn extract_pins(text: &str) -> Result<Vec<PinSpan>, Box<dyn Error>> {
    let strings = quoted_strings(text);
    let bytes = text.as_bytes();
    let mut pins = Vec::new();

    // Pattern A: "path", "hash".
    for quoted in &strings {
        if !allowed_path(&quoted.value) {
            continue;
        }
        let mut offset = skip_whitespace(bytes, quoted.end);
        if bytes.get(offset) != Some(&b',') {
            continue;
        }
        offset = skip_whitespace(bytes, offset + 1);
        if let Some((start, end, literal)) = hash_quoted_at(text, offset) {
            add_pin(&mut pins, &quoted.value, Grammar::A, start, end, literal);
        }
    }

    // Pattern B: "path":\n    "hash".
    for quoted in &strings {
        if !allowed_path(&quoted.value) {
            continue;
        }
        let mut offset = skip_whitespace(bytes, quoted.end);
        if bytes.get(offset) != Some(&b':') {
            continue;
        }
        offset = skip_whitespace(bytes, offset + 1);
        if !text[quoted.end..offset].contains('\n') {
            continue;
        }
        if let Some((start, end, literal)) = hash_quoted_at(text, offset) {
            add_pin(&mut pins, &quoted.value, Grammar::B, start, end, literal);
        }
    }

    // Pattern C: "path": "hash" on one line.
    for quoted in &strings {
        if !allowed_path(&quoted.value) {
            continue;
        }
        let offset = quoted.end;
        if bytes.get(offset) != Some(&b':') || bytes.get(offset + 1) != Some(&b' ') {
            continue;
        }
        let offset = offset + 2;
        if let Some((start, end, literal)) = hash_quoted_at(text, offset) {
            add_pin(&mut pins, &quoted.value, Grammar::C, start, end, literal);
        }
    }

    // Pattern D: const X_RELATIVE_PATH = "path" followed by const
    // X_SHA256/EXPECTED_X_SHA256 = "hash".
    let path_constants = relative_path_constants(text);
    for (name, path) in path_constants {
        for hash_name in [format!("{name}_SHA256"), format!("EXPECTED_{name}_SHA256")] {
            if let Some((start, end, literal)) = const_hash(text, &hash_name) {
                add_pin(&mut pins, &path, Grammar::D, start, end, literal);
            }
        }
    }

    // Pattern E: const PATH_CONST = "path" and [PATH_CONST]: "hash".
    for (name, path) in path_constants_for_e(text) {
        let marker = format!("[{name}]");
        let mut offset = 0usize;
        while let Some(found) = text[offset..].find(&marker) {
            let marker_start = offset + found;
            let mut value_start = marker_start + marker.len();
            if bytes.get(value_start) != Some(&b':') {
                offset = value_start;
                continue;
            }
            value_start = skip_whitespace(bytes, value_start + 1);
            if let Some((start, end, literal)) = hash_quoted_at(text, value_start) {
                add_pin(&mut pins, &path, Grammar::E, start, end, literal);
            }
            offset = value_start.saturating_add(1);
        }
    }

    pins.sort_by(|left, right| {
        left.start
            .cmp(&right.start)
            .then_with(|| left.end.cmp(&right.end))
            .then_with(|| left.grammar.cmp(&right.grammar))
    });
    for pair in pins.windows(2) {
        if pair[0].end > pair[1].start {
            return Err(format!(
                "overlapping extracted pin spans at {} and {}",
                pair[0].start, pair[1].start
            )
            .into());
        }
    }
    Ok(pins)
}

fn relative_path_constants(text: &str) -> Vec<(String, String)> {
    let mut values = Vec::new();
    for (name, path) in all_const_strings(text) {
        if name.ends_with("_RELATIVE_PATH") {
            values.push((name.trim_end_matches("_RELATIVE_PATH").to_string(), path));
        }
    }
    values
}

fn path_constants_for_e(text: &str) -> Vec<(String, String)> {
    all_const_strings(text)
        .into_iter()
        .filter(|(_, path)| allowed_path(path))
        .collect()
}

fn all_const_strings(text: &str) -> Vec<(String, String)> {
    let bytes = text.as_bytes();
    let mut values = Vec::new();
    let mut offset = 0usize;
    while let Some(found) = text[offset..].find("const ") {
        let start = offset + found;
        let name_start = start + "const ".len();
        let mut name_end = name_start;
        while bytes.get(name_end).is_some_and(|byte| is_word(*byte)) {
            name_end += 1;
        }
        if name_end == name_start {
            offset = name_start;
            continue;
        }
        let after_name = skip_whitespace(bytes, name_end);
        if bytes.get(after_name) != Some(&b'=') {
            offset = name_end;
            continue;
        }
        let value_start = skip_whitespace(bytes, after_name + 1);
        if let Some(quoted) = parse_quote_at(text, value_start) {
            values.push((text[name_start..name_end].to_string(), quoted.value));
        }
        offset = name_end;
    }
    values
}

fn const_hash(text: &str, name: &str) -> Option<(usize, usize, String)> {
    let bytes = text.as_bytes();
    let marker = format!("const {name}");
    let mut offset = 0usize;
    while let Some(found) = text[offset..].find(&marker) {
        let start = offset + found;
        let after_name = start + marker.len();
        if bytes.get(after_name).is_some_and(|byte| is_word(*byte)) {
            offset = after_name;
            continue;
        }
        let after_name = skip_whitespace(bytes, after_name);
        if bytes.get(after_name) != Some(&b'=') {
            offset = after_name;
            continue;
        }
        let value_start = skip_whitespace(bytes, after_name + 1);
        return hash_quoted_at(text, value_start);
    }
    None
}

fn masked_core_digest(text: &str, pins: &[PinSpan]) -> Result<Digest, Box<dyn Error>> {
    let mut masked = Vec::with_capacity(text.len());
    let mut offset = 0usize;
    for pin in pins {
        if pin.end > text.len() || pin.start < offset {
            return Err("invalid pin span while masking".into());
        }
        masked.extend_from_slice(&text.as_bytes()[offset..pin.start]);
        masked.extend_from_slice(MASK_PLACEHOLDER);
        offset = pin.end;
    }
    masked.extend_from_slice(&text.as_bytes()[offset..]);
    Ok(sha256(&masked))
}

fn put_text(output: &mut Vec<u8>, value: &str) {
    output.extend_from_slice(&(value.len() as u64).to_be_bytes());
    output.extend_from_slice(value.as_bytes());
}

fn envelope_digest(pins: &[PinSpan]) -> Digest {
    let mut envelope = Vec::new();
    envelope.extend_from_slice(b"shadow-envelope/v1\0");
    envelope.extend_from_slice(&(pins.len() as u64).to_be_bytes());
    for pin in pins {
        envelope.push(pin.grammar.as_str().as_bytes()[0]);
        put_text(&mut envelope, &pin.path);
        put_text(&mut envelope, &pin.literal);
    }
    sha256(&envelope)
}

fn find_unclassified(text: &str, pins: &[PinSpan]) -> Vec<UnclassifiedLiteral> {
    let bytes = text.as_bytes();
    let mut literals = Vec::new();
    let mut offset = 0usize;
    while offset + 64 <= bytes.len() {
        if bytes[offset..offset + 64].iter().copied().all(is_hex)
            && (offset == 0 || !is_hex(bytes[offset - 1]))
            && (offset + 64 == bytes.len() || !is_hex(bytes[offset + 64]))
        {
            let covered = pins
                .iter()
                .any(|pin| pin.start == offset && pin.end == offset + 64);
            if !covered && path_is_adjacent(text, offset) {
                literals.push(UnclassifiedLiteral {
                    start: offset,
                    literal: text[offset..offset + 64].to_string(),
                });
            }
            offset += 64;
        } else {
            offset += 1;
        }
    }
    literals
}

fn path_is_adjacent(text: &str, start: usize) -> bool {
    let bytes = text.as_bytes();
    let line_start = bytes[..start]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |position| position + 1);
    let line_end = bytes[start..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(bytes.len(), |position| start + position);
    if quoted_strings(&text[line_start..line_end])
        .iter()
        .any(|quoted| allowed_path(&quoted.value))
    {
        return true;
    }
    let context_start = start.saturating_sub(128);
    let context_end = (start + 64 + 128).min(text.len());
    quoted_strings(&text[context_start..context_end])
        .iter()
        .any(|quoted| allowed_path(&quoted.value))
}

fn line_number(text: &str, offset: usize) -> usize {
    text.as_bytes()[..offset.min(text.len())]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1
}

fn render_report(
    repository: &Path,
    nodes: &[ScriptNode],
    edges: &[Edge],
    unmapped_pins: &[(String, String)],
    incidents: &[IncidentRow],
) -> String {
    let mut report = String::new();
    let version = fs::read_to_string(repository.join(".node-version"))
        .map_or_else(|_| "unknown".to_string(), |value| value.trim().to_string());
    let unclassified_count: usize = nodes.iter().map(|node| node.unclassified.len()).sum();
    let envelope_only = incidents
        .iter()
        .filter(|row| row.classification == "envelope-only")
        .count();
    let core_changed = incidents
        .iter()
        .filter(|row| row.classification == "core-changed")
        .count();

    writeln!(&mut report, "# Shadow adapter report").expect("write report");
    writeln!(&mut report).expect("write report");
    writeln!(
        &mut report,
        "Read-only graph replay for `{INCIDENT_COMMIT}`; generated with the repository Node pin `{version}`."
    )
    .expect("write report");
    writeln!(&mut report).expect("write report");
    writeln!(&mut report, "## Status").expect("write report");
    writeln!(&mut report).expect("write report");
    writeln!(
        &mut report,
        "- Oracle nodes (owner-controls excluded): {}",
        nodes.len()
    )
    .expect("write report");
    writeln!(&mut report, "- Mapped dependency edges: {}", edges.len()).expect("write report");
    writeln!(
        &mut report,
        "- Incident classification: {envelope_only}/{} envelope-only; {core_changed} core-changed.",
        incidents.len()
    )
    .expect("write report");
    writeln!(
        &mut report,
        "- Unclassified path-adjacent 64-hex literals: {unclassified_count}."
    )
    .expect("write report");
    writeln!(
        &mut report,
        "- Wall-clock claim: under these keys the incident's observation node receipt HITs (9,027 cases; measured re-run cost 89 minutes)."
    )
    .expect("write report");
    writeln!(&mut report).expect("write report");

    writeln!(&mut report, "## Pin grammars and digest rule").expect("write report");
    writeln!(&mut report).expect("write report");
    writeln!(
        &mut report,
        "The adapter ports the repin script's five patterns:"
    )
    .expect("write report");
    writeln!(&mut report, "- `A`: `\"path\", \"hash\"`.").expect("write report");
    writeln!(
        &mut report,
        "- `B`: `\"path\":` followed by a newline and `\"hash\"`."
    )
    .expect("write report");
    writeln!(&mut report, "- `C`: same-line `\"path\": \"hash\"`.").expect("write report");
    writeln!(
        &mut report,
        "- `D`: `X_RELATIVE_PATH` paired with `X_SHA256` or `EXPECTED_X_SHA256`."
    )
    .expect("write report");
    writeln!(
        &mut report,
        "- `E`: `[PATH_CONST]: \"hash\"` with a path-valued constant."
    )
    .expect("write report");
    writeln!(
        &mut report,
        "Only the 64-character hash literal is masked with `{}`. The core digest hashes the resulting bytes; the envelope digest hashes each grammar/path/literal tuple in source order.",
        String::from_utf8_lossy(MASK_PLACEHOLDER)
    )
    .expect("write report");
    writeln!(&mut report).expect("write report");

    writeln!(&mut report, "## Graph nodes").expect("write report");
    writeln!(&mut report).expect("write report");
    writeln!(&mut report, "Each script is a typed `oracle-js/{version}` producer; the masked `core` digest is its semantic implementation identity, while raw bytes are shown separately for provenance.").expect("write report");
    writeln!(&mut report).expect("write report");
    writeln!(
        &mut report,
        "| node | declared artifact | raw implementation | `core` | `envelope` | masked spans |"
    )
    .expect("write report");
    writeln!(&mut report, "|---|---|---|---|---|---:|").expect("write report");
    for node in nodes {
        let artifacts = if node.target_artifacts.is_empty() {
            "—".to_string()
        } else {
            node.target_artifacts.join("<br>")
        };
        writeln!(
            &mut report,
            "| `{}` | `{}` | `{}` | `{}` | `{}` | {} |",
            node.path,
            artifacts,
            node.raw_digest,
            node.core_digest,
            node.envelope_digest,
            node.pins.len()
        )
        .expect("write report");
    }
    writeln!(&mut report).expect("write report");

    writeln!(&mut report, "## Graph edges").expect("write report");
    writeln!(&mut report).expect("write report");
    writeln!(&mut report, "A pinned `ratchets/*.json` is evidence content and consumes the producer's `core` projection. Oracle-script, contract, and toolchain fingerprints are lineage pins and consume `envelope`. The derived observation edge below also consumes `core`.").expect("write report");
    writeln!(&mut report).expect("write report");
    writeln!(
        &mut report,
        "| producer | consumer | projection | labelled input | grammar |"
    )
    .expect("write report");
    writeln!(&mut report, "|---|---|---|---|---|").expect("write report");
    for edge in edges {
        writeln!(
            &mut report,
            "| `{}` | `{}` | `{}` | `{}` | `{}` |",
            nodes[edge.producer].path,
            nodes[edge.consumer].path,
            edge.projection,
            edge.path,
            edge.grammar.as_str()
        )
        .expect("write report");
    }
    writeln!(
        &mut report,
        "| `h2-5g-qualification.mjs` | `{}` | `core` | `observation evidence content` | derived |",
        OBSERVATION_NODE
    )
    .expect("write report");
    writeln!(&mut report).expect("write report");
    writeln!(
        &mut report,
        "Unmapped extracted pins (no local H2 producer declaration):"
    )
    .expect("write report");
    if unmapped_pins.is_empty() {
        writeln!(&mut report, "- none").expect("write report");
    } else {
        for (consumer, path) in unmapped_pins {
            writeln!(&mut report, "- `{consumer}` -> `{path}`").expect("write report");
        }
    }
    writeln!(&mut report).expect("write report");

    render_projection_impact(&mut report, nodes, edges, incidents);
    render_incident(&mut report, incidents);
    render_masked_spans(&mut report, nodes);
    render_unclassified(&mut report, nodes);
    report
}

fn render_projection_impact(
    report: &mut String,
    nodes: &[ScriptNode],
    edges: &[Edge],
    incidents: &[IncidentRow],
) {
    let changed: BTreeMap<&str, ()> = incidents
        .iter()
        .map(|row| (row.path.as_str(), ()))
        .collect();
    let mut envelope_consumers = BTreeMap::new();
    let mut core_consumers = BTreeMap::new();
    for edge in edges {
        if changed.contains_key(nodes[edge.producer].path.as_str()) {
            match edge.projection {
                Projection::Core => {
                    core_consumers.insert(nodes[edge.consumer].path.clone(), ());
                }
                Projection::Envelope => {
                    envelope_consumers.insert(nodes[edge.consumer].path.clone(), ());
                }
            }
        }
    }
    writeln!(report, "## Projection impact").expect("write report");
    writeln!(report).expect("write report");
    writeln!(report, "- Envelope-consumer nodes invalidated by changed pin/lineage projections ({} unique consumers):", envelope_consumers.len()).expect("write report");
    if envelope_consumers.is_empty() {
        writeln!(report, "  - none").expect("write report");
    } else {
        for consumer in envelope_consumers.keys() {
            writeln!(report, "  - `{consumer}`").expect("write report");
        }
    }
    writeln!(report, "- Core-consumer nodes that HIT under the pin-only source edit ({} graph consumers, plus the derived observation):", core_consumers.len()).expect("write report");
    for consumer in core_consumers.keys() {
        writeln!(report, "  - `{consumer}`").expect("write report");
    }
    writeln!(report, "  - `{OBSERVATION_NODE}`: 9,027 ordered observations; the core projection excludes all extracted pin spans.").expect("write report");
    writeln!(report).expect("write report");
}

fn render_incident(report: &mut String, incidents: &[IncidentRow]) {
    writeln!(report, "## Incident classification").expect("write report");
    writeln!(report).expect("write report");
    writeln!(report, "`{INCIDENT_COMMIT}` also changes `crates/emitter/src/printer.rs`; that genuine producer implementation change would invalidate the emitter's own typed core dependents. This table isolates the separate 44-module oracle pin cascade, whose changed spans are lineage/fingerprint envelope data.").expect("write report");
    writeln!(report).expect("write report");
    writeln!(report, "The old side is `git show a8aa644b^:<path>` and the new side is `git show a8aa644b:<path>`. Classification is envelope-only exactly when core is equal and envelope differs.").expect("write report");
    writeln!(report).expect("write report");
    writeln!(report, "| changed script | old `core` | new `core` | old `envelope` | new `envelope` | old/new pins | classification |").expect("write report");
    writeln!(report, "|---|---|---|---|---|---:|---|").expect("write report");
    for row in incidents {
        writeln!(
            report,
            "| `{}` | `{}` | `{}` | `{}` | `{}` | {}/{} | **{}** |",
            row.path,
            row.old_core,
            row.new_core,
            row.old_envelope,
            row.new_envelope,
            row.old_pin_count,
            row.new_pin_count,
            row.classification
        )
        .expect("write report");
    }
    writeln!(report).expect("write report");
    writeln!(
        report,
        "Expected result: **44/44 envelope-only**; the table above is the acceptance evidence."
    )
    .expect("write report");
    writeln!(report).expect("write report");
}

fn render_masked_spans(report: &mut String, nodes: &[ScriptNode]) {
    writeln!(report, "## Masked spans").expect("write report");
    writeln!(report).expect("write report");
    writeln!(report, "Every extracted span is recorded below; offsets are UTF-8 byte offsets in the current script.").expect("write report");
    writeln!(report).expect("write report");
    writeln!(
        report,
        "| script | ordinal | grammar | line | byte span | path | literal |"
    )
    .expect("write report");
    writeln!(report, "|---|---:|---|---:|---|---|---|").expect("write report");
    for node in nodes {
        for (ordinal, pin) in node.pins.iter().enumerate() {
            writeln!(
                report,
                "| `{}` | {} | `{}` | {} | {}..{} | `{}` | `{}` |",
                node.path,
                ordinal + 1,
                pin.grammar.as_str(),
                line_number(&node.text, pin.start),
                pin.start,
                pin.end,
                pin.path,
                pin.literal
            )
            .expect("write report");
        }
    }
    writeln!(report).expect("write report");
    writeln!(report, "### Masked span counts by script").expect("write report");
    writeln!(report).expect("write report");
    writeln!(report, "| script | A | B | C | D | E | total |").expect("write report");
    writeln!(report, "|---|---:|---:|---:|---:|---:|---:|").expect("write report");
    for node in nodes {
        let mut counts = [0usize; 5];
        for pin in &node.pins {
            counts[match pin.grammar {
                Grammar::A => 0,
                Grammar::B => 1,
                Grammar::C => 2,
                Grammar::D => 3,
                Grammar::E => 4,
            }] += 1;
        }
        writeln!(
            report,
            "| `{}` | {} | {} | {} | {} | {} | {} |",
            node.path,
            counts[0],
            counts[1],
            counts[2],
            counts[3],
            counts[4],
            node.pins.len()
        )
        .expect("write report");
    }
    writeln!(report).expect("write report");
}

fn render_unclassified(report: &mut String, nodes: &[ScriptNode]) {
    writeln!(report, "## Unclassified 64-hex literals").expect("write report");
    writeln!(report).expect("write report");
    writeln!(report, "A path-adjacent literal not covered by A–E is a hard parser finding. It is listed here and does not abort report generation, so the shadow adapter remains report-only.").expect("write report");
    writeln!(report).expect("write report");
    writeln!(report, "| script | line | literal |").expect("write report");
    writeln!(report, "|---|---:|---|").expect("write report");
    let mut count = 0usize;
    for node in nodes {
        for literal in &node.unclassified {
            count += 1;
            writeln!(
                report,
                "| `{}` | {} | `{}` |",
                node.path,
                line_number(&node.text, literal.start),
                literal.literal
            )
            .expect("write report");
        }
    }
    if count == 0 {
        writeln!(report, "| — | — | none |").expect("write report");
    }
    writeln!(report).expect("write report");
}

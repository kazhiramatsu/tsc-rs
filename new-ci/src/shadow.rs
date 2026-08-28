//! Phase 0, report-only adapter for the current JavaScript oracle ladder.
//!
//! This module intentionally has no dependency on the root workspace.  It
//! reads the repository snapshot, projects the discovered checks into the M1
//! receipt-key types, and writes an immutable-by-name observation report.  It
//! never invokes `--write`, never invokes the 5g observation path, and never
//! treats a diagnostic observation as a cache hit.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::env;
use std::error::Error;
use std::fmt::{self, Write as FmtWrite};
use std::fs::{self, DirEntry, FileType};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::pins::{
    extract_oracle_pins, find_unclassified_literals, normalize_path, quoted_strings, ExtractedPin,
    Grammar, UnclassifiedLiteral,
};
use crate::{
    sha256, Action, DependencyOutput, Digest, ManifestEntry, Projection, ReceiptKey,
    SemanticInputManifest,
};

pub const ADAPTER_SCHEMA: &str = "new-ci-shadow-run/v1";
pub const ACTION_TOOL: &str = "node-current-oracle";
pub const RECEIPT_SCHEMA: &str = "shadow-receipt/v1";
pub const SAMPLE_MAX_CHECKS: usize = 8;
pub const SAMPLE_MAX_WALL_SECONDS: u64 = 10 * 60;
const PIN_MASK: &[u8] = b"<EXTRACTED_PIN>";
const CHAIN_WALK_CERTIFICATE_ROOT: &str = "target/chain-walk";
const FIVE_G_RUNG: &str = "h2-5g-qualification";

/// The classification assigned to every discovered H2 script.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ItemClass {
    Producer,
    CheckedSidecar,
    ImportedHelper,
    RestrictedProducer,
    Unknown,
}

impl ItemClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Producer => "producer",
            Self::CheckedSidecar => "checked-sidecar",
            Self::ImportedHelper => "imported-helper",
            Self::RestrictedProducer => "restricted-producer",
            Self::Unknown => "unknown",
        }
    }

    pub const fn is_checkable(self) -> bool {
        matches!(
            self,
            Self::Producer | Self::CheckedSidecar | Self::RestrictedProducer
        )
    }
}

impl fmt::Display for ItemClass {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The active first-slice report taxonomy.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ComparisonClass {
    ByteEqual,
    ByteDrift,
    SchemaDrift,
    MissingRung,
    BudgetSkip,
}

impl ComparisonClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ByteEqual => "BYTE_EQUAL",
            Self::ByteDrift => "BYTE_DRIFT",
            Self::SchemaDrift => "SCHEMA_DRIFT",
            Self::MissingRung => "MISSING_RUNG",
            Self::BudgetSkip => "BUDGET_SKIP",
        }
    }

    pub const fn named_severity(self) -> &'static str {
        match self {
            Self::ByteEqual => "CLEAN",
            Self::ByteDrift | Self::SchemaDrift => "BLOCKER",
            Self::MissingRung => "INCIDENT",
            Self::BudgetSkip => "INCOMPLETE",
        }
    }
}

impl fmt::Display for ComparisonClass {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A discovery finding is data, not an error that aborts a report-only run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveryFinding {
    pub class: ComparisonClass,
    pub path: String,
    pub reason: String,
}

#[derive(Debug)]
pub struct DiscoveryError {
    message: String,
}

impl DiscoveryError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for DiscoveryError {}

impl From<io::Error> for DiscoveryError {
    fn from(error: io::Error) -> Self {
        Self::new(error.to_string())
    }
}

/// One typed pin extracted by the existing M1 machinery.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PinRecord {
    pub ordinal: usize,
    pub start: usize,
    pub end: usize,
    pub path: String,
    pub grammar: Grammar,
    pub literal: String,
    pub envelope_term: Digest,
}

impl PinRecord {
    fn from_extracted(ordinal: usize, pin: &ExtractedPin) -> Self {
        let envelope_term = pin_term_digest(ordinal, pin);
        Self {
            ordinal,
            start: pin.start,
            end: pin.end,
            path: pin.path.clone(),
            grammar: pin.grammar,
            literal: pin.literal.clone(),
            envelope_term,
        }
    }
}

/// An H2 script in the complete on-disk inventory.
#[derive(Clone, Debug)]
pub struct DiscoveredScript {
    pub path: String,
    pub class: ItemClass,
    pub raw_digest: Digest,
    pub core_digest: Option<Digest>,
    pub envelope_digest: Option<Digest>,
    pub declared_artifacts: Vec<String>,
    pub check_argv: Option<Vec<String>>,
    pub imported_helpers: Vec<String>,
    pub pins: Vec<PinRecord>,
    pub unclassified: Vec<UnclassifiedLiteral>,
    pub schema_error: Option<String>,
    text: String,
}

impl DiscoveredScript {
    pub fn rung(&self) -> String {
        self.path
            .strip_prefix("crates/oracle/")
            .unwrap_or(&self.path)
            .strip_suffix(".mjs")
            .unwrap_or(&self.path)
            .to_string()
    }

    pub fn is_comparable(&self) -> bool {
        self.schema_error.is_none() && self.unclassified.is_empty() && self.core_digest.is_some()
    }
}

/// A checked-in H2 ratchet/evidence artifact.
#[derive(Clone, Debug)]
pub struct DiscoveredArtifact {
    pub path: String,
    pub raw_digest: Digest,
    pub size: u64,
    pub core_digest: Digest,
    pub envelope_digest: Digest,
    pub semantic_digest: Option<Digest>,
    pub schema_verdict: String,
    pub producers: Vec<String>,
}

/// A local module loaded by a producer.  Its bytes are implementation input,
/// never an ambient process dependency.
#[derive(Clone, Debug)]
pub struct LoadedHelper {
    pub path: String,
    pub raw_digest: Digest,
    pub size: u64,
}

/// One typed producer edge extracted from a pin.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProducerEdge {
    pub producer: String,
    pub consumer: String,
    pub path: String,
    pub projection: Projection,
    pub pin_ordinal: usize,
    pub pin_digest: Digest,
}

/// Complete dynamic inventory and graph.
#[derive(Clone, Debug)]
pub struct Discovery {
    pub repository_root: PathBuf,
    pub scripts: Vec<DiscoveredScript>,
    pub artifacts: Vec<DiscoveredArtifact>,
    pub helpers: Vec<LoadedHelper>,
    pub edges: Vec<ProducerEdge>,
    pub topological_order: Vec<String>,
    pub walk_order: Vec<String>,
    pub findings: Vec<DiscoveryFinding>,
    pub inventory_digest: Digest,
    files: BTreeMap<String, Vec<u8>>,
}

impl Discovery {
    pub fn script(&self, path: &str) -> Option<&DiscoveredScript> {
        self.scripts.iter().find(|script| script.path == path)
    }

    pub fn artifact(&self, path: &str) -> Option<&DiscoveredArtifact> {
        self.artifacts.iter().find(|artifact| artifact.path == path)
    }

    pub fn helper(&self, path: &str) -> Option<&LoadedHelper> {
        self.helpers.iter().find(|helper| helper.path == path)
    }

    pub fn file_bytes(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }

    pub fn checkable_scripts(&self) -> impl Iterator<Item = &DiscoveredScript> {
        self.scripts
            .iter()
            .filter(|script| script.check_argv.is_some() && script.class.is_checkable())
    }
}

/// Discover the H2 scripts, ratchets, helpers, graph, and walk-order drift.
/// All declaration and topology problems are retained as findings so a
/// report can be written even when the inventory is not understood.
pub fn discover(repository_root: impl AsRef<Path>) -> Result<Discovery, DiscoveryError> {
    let repository_root = repository_root.as_ref().to_path_buf();
    let mut files = BTreeMap::new();
    let script_paths =
        direct_matching_files(&repository_root.join("crates/oracle"), "h2-", ".mjs")?;
    let artifact_paths = direct_matching_files(&repository_root.join("ratchets"), "h2-", ".json")?;

    let mut raw_scripts = Vec::with_capacity(script_paths.len());
    let mut imported_paths = BTreeSet::new();
    let mut findings = Vec::new();
    for path in &script_paths {
        let relative = relative_string(&repository_root, path)?;
        let bytes = fs::read(path)?;
        files.insert(relative.clone(), bytes.clone());
        let text = match String::from_utf8(bytes.clone()) {
            Ok(text) => text,
            Err(_) => {
                findings.push(DiscoveryFinding {
                    class: ComparisonClass::SchemaDrift,
                    path: relative.clone(),
                    reason: "oracle script is not valid UTF-8".to_string(),
                });
                String::from_utf8_lossy(&bytes).into_owned()
            }
        };
        let imports = local_imports(&repository_root, path, &text);
        imported_paths.extend(imports);
        raw_scripts.push((relative, text, bytes));
    }

    let mut scripts = Vec::with_capacity(raw_scripts.len());
    for (path, text, bytes) in raw_scripts {
        let pins_result = extract_oracle_pins(&text);
        let (pins, schema_error) = match pins_result {
            Ok(extracted) => (
                extracted
                    .iter()
                    .enumerate()
                    .map(|(ordinal, pin)| PinRecord::from_extracted(ordinal, pin))
                    .collect::<Vec<_>>(),
                None,
            ),
            Err(error) => (Vec::new(), Some(error)),
        };
        let extracted = extract_oracle_pins(&text).unwrap_or_default();
        let unclassified = find_unclassified_literals(&text, &extracted);
        let declared_artifacts = declared_artifacts(&path, &text);
        let check_argv = if has_check_entrypoint(&text) {
            Some(vec![
                "node".to_string(),
                path.clone(),
                "--check".to_string(),
            ])
        } else {
            None
        };
        let class = classify_script(
            &path,
            &declared_artifacts,
            check_argv.is_some(),
            &imported_paths,
        );
        let core_digest = masked_core_digest(text.as_bytes(), &pins).ok();
        let envelope_digest = if schema_error.is_none() {
            Some(pin_envelope_digest(&pins))
        } else {
            None
        };
        if let Some(error) = &schema_error {
            findings.push(DiscoveryFinding {
                class: ComparisonClass::SchemaDrift,
                path: path.clone(),
                reason: format!("pin grammar is not understood: {error}"),
            });
        }
        if !unclassified.is_empty() {
            findings.push(DiscoveryFinding {
                class: ComparisonClass::SchemaDrift,
                path: path.clone(),
                reason: format!(
                    "{} path-adjacent 64-hex literal(s) are not covered by typed M1 pins",
                    unclassified.len()
                ),
            });
        }
        if class == ItemClass::Unknown {
            findings.push(DiscoveryFinding {
                class: ComparisonClass::SchemaDrift,
                path: path.clone(),
                reason: "H2 executable has no understood output declaration or check entrypoint"
                    .to_string(),
            });
        }
        if check_argv.is_some()
            && declared_artifacts.is_empty()
            && class != ItemClass::ImportedHelper
        {
            findings.push(DiscoveryFinding {
                class: ComparisonClass::SchemaDrift,
                path: path.clone(),
                reason: "check entrypoint has no declared artifact".to_string(),
            });
        }
        scripts.push(DiscoveredScript {
            path,
            class,
            raw_digest: sha256(&bytes),
            core_digest,
            envelope_digest,
            declared_artifacts,
            check_argv,
            imported_helpers: Vec::new(),
            pins,
            unclassified,
            schema_error,
            text,
        });
    }
    scripts.sort_by(|left, right| byte_cmp(&left.path, &right.path));

    let mut helpers =
        discover_helpers(&repository_root, &mut files, &imported_paths, &mut findings)?;
    helpers.sort_by(|left, right| byte_cmp(&left.path, &right.path));
    let helper_paths: BTreeSet<String> = helpers.iter().map(|helper| helper.path.clone()).collect();
    for script in &mut scripts {
        let script_path = repository_root.join(&script.path);
        let imports = local_imports(&repository_root, &script_path, &script.text);
        let mut helper_queue = VecDeque::from_iter(imports);
        let mut imported_helpers = BTreeSet::new();
        while let Some(path) = helper_queue.pop_front() {
            if !helper_paths.contains(&path) || !imported_helpers.insert(path.clone()) {
                continue;
            }
            if !path.starts_with("vendor/") {
                if let Some(bytes) = files.get(&path) {
                    let helper_text = String::from_utf8_lossy(bytes);
                    let helper_path = repository_root.join(&path);
                    helper_queue.extend(local_imports(
                        &repository_root,
                        &helper_path,
                        &helper_text,
                    ));
                }
            }
        }
        script.imported_helpers = imported_helpers.into_iter().collect();
        script
            .imported_helpers
            .sort_by(|left, right| byte_cmp(left, right));
        script.imported_helpers.dedup();
    }
    // An imported h2-*.mjs helper remains in the complete script inventory, but
    // is explicitly classified as a helper rather than silently dropped.
    for script in &mut scripts {
        if imported_paths.contains(&script.path)
            && script.declared_artifacts.is_empty()
            && script.check_argv.is_none()
        {
            script.class = ItemClass::ImportedHelper;
        }
    }

    let mut artifacts = Vec::with_capacity(artifact_paths.len());
    for path in artifact_paths {
        let relative = relative_string(&repository_root, &path)?;
        let bytes = fs::read(&path)?;
        files.insert(relative.clone(), bytes.clone());
        let raw_digest = sha256(&bytes);
        let schema_verdict = json_shape_verdict(&bytes);
        artifacts.push(DiscoveredArtifact {
            path: relative.clone(),
            raw_digest,
            size: bytes.len() as u64,
            core_digest: raw_digest,
            envelope_digest: artifact_envelope_digest(&relative, raw_digest, bytes.len() as u64),
            semantic_digest: None,
            schema_verdict,
            producers: Vec::new(),
        });
    }
    artifacts.sort_by(|left, right| byte_cmp(&left.path, &right.path));

    let mut artifact_producers: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for script in &scripts {
        for artifact in &script.declared_artifacts {
            artifact_producers
                .entry(artifact.clone())
                .or_default()
                .push(script.path.clone());
        }
    }
    for producers in artifact_producers.values_mut() {
        producers.sort_by(|left, right| byte_cmp(left, right));
        producers.dedup();
    }
    for artifact in &mut artifacts {
        artifact.producers = artifact_producers
            .get(&artifact.path)
            .cloned()
            .unwrap_or_default();
        if artifact.producers.is_empty() {
            findings.push(DiscoveryFinding {
                class: ComparisonClass::MissingRung,
                path: artifact.path.clone(),
                reason: "checked-in h2 ratchet has no declared producer".to_string(),
            });
        } else if artifact.producers.len() > 1 {
            findings.push(DiscoveryFinding {
                class: ComparisonClass::SchemaDrift,
                path: artifact.path.clone(),
                reason: format!(
                    "artifact has duplicate producers: {}",
                    artifact.producers.join(", ")
                ),
            });
        }
    }
    for (artifact, producers) in &artifact_producers {
        if !artifacts
            .iter()
            .any(|candidate| candidate.path == *artifact)
        {
            findings.push(DiscoveryFinding {
                class: ComparisonClass::MissingRung,
                path: artifact.clone(),
                reason: format!(
                    "declared output is absent on disk (producer {})",
                    producers.join(", ")
                ),
            });
        }
    }

    let artifact_index: BTreeMap<String, String> = artifact_producers
        .iter()
        .filter(|(_, producers)| producers.len() == 1)
        .map(|(path, producers)| (path.clone(), producers[0].clone()))
        .collect();
    let script_index: BTreeSet<String> = scripts.iter().map(|script| script.path.clone()).collect();
    let mut edges = Vec::new();
    for script in &scripts {
        for pin in &script.pins {
            let producer = if let Some(producer) = artifact_index.get(&pin.path) {
                Some((producer.clone(), Projection::Core))
            } else if script_index.contains(&pin.path) {
                Some((pin.path.clone(), Projection::Envelope))
            } else {
                None
            };
            if let Some((producer, projection)) = producer {
                edges.push(ProducerEdge {
                    producer,
                    consumer: script.path.clone(),
                    path: pin.path.clone(),
                    projection,
                    pin_ordinal: pin.ordinal,
                    pin_digest: pin.envelope_term,
                });
            } else if !files.contains_key(&pin.path) && !repository_root.join(&pin.path).is_file() {
                findings.push(DiscoveryFinding {
                    class: ComparisonClass::MissingRung,
                    path: script.path.clone(),
                    reason: format!("typed pin points at missing input {path}", path = pin.path),
                });
            }
        }
    }
    edges.sort_by(|left, right| {
        byte_cmp(&left.producer, &right.producer)
            .then_with(|| byte_cmp(&left.consumer, &right.consumer))
            .then_with(|| byte_cmp(&left.path, &right.path))
            .then_with(|| left.pin_ordinal.cmp(&right.pin_ordinal))
    });

    let walk_order = read_walk_order(&repository_root, &mut findings)?;
    cross_check_walk_order(&scripts, &walk_order, &mut findings);
    let topological_order = topological_sort(&scripts, &edges, &mut findings);

    findings.sort_by(|left, right| {
        left.class
            .cmp(&right.class)
            .then_with(|| byte_cmp(&left.path, &right.path))
            .then_with(|| byte_cmp(&left.reason, &right.reason))
    });
    findings.dedup();
    let inventory_digest = inventory_digest(
        &scripts,
        &artifacts,
        &helpers,
        &edges,
        &walk_order,
        &findings,
    );
    Ok(Discovery {
        repository_root,
        scripts,
        artifacts,
        helpers,
        edges,
        topological_order,
        walk_order,
        findings,
        inventory_digest,
        files,
    })
}

fn direct_matching_files(
    directory: &Path,
    prefix: &str,
    suffix: &str,
) -> Result<Vec<PathBuf>, DiscoveryError> {
    let mut paths = Vec::new();
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(paths),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(prefix) && name.ends_with(suffix) {
            paths.push(entry.path());
        }
    }
    paths.sort_by(|left, right| byte_cmp(&left.to_string_lossy(), &right.to_string_lossy()));
    Ok(paths)
}

fn relative_string(root: &Path, path: &Path) -> Result<String, DiscoveryError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| DiscoveryError::new(format!("path {} escapes repository", path.display())))?;
    let mut normalized = PathBuf::new();
    for component in relative.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => normalized.push(value),
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(DiscoveryError::new(format!(
                        "path {} escapes repository",
                        path.display()
                    )));
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(DiscoveryError::new(format!(
                    "absolute path {} is not a repository-relative import",
                    path.display()
                )));
            }
        }
    }
    Ok(normalized.to_string_lossy().replace('\\', "/"))
}

fn byte_cmp(left: &str, right: &str) -> std::cmp::Ordering {
    left.as_bytes().cmp(right.as_bytes())
}

fn classify_script(
    path: &str,
    declared_artifacts: &[String],
    has_check: bool,
    imported_paths: &BTreeSet<String>,
) -> ItemClass {
    if path == "crates/oracle/h2-baseline.mjs" {
        return ItemClass::RestrictedProducer;
    }
    if imported_paths.contains(path) && declared_artifacts.is_empty() && !has_check {
        return ItemClass::ImportedHelper;
    }
    if path.ends_with("-owner-controls.mjs") {
        return ItemClass::CheckedSidecar;
    }
    if !declared_artifacts.is_empty() && has_check {
        ItemClass::Producer
    } else {
        ItemClass::Unknown
    }
}

fn has_check_entrypoint(text: &str) -> bool {
    text.contains("--check") && (text.contains("process.argv") || text.contains("arguments_"))
}

fn declared_artifacts(path: &str, text: &str) -> Vec<String> {
    let mut names = vec![
        "TARGET_RELATIVE_PATH",
        "EVIDENCE_RELATIVE_PATH",
        "OUTPUT_RELATIVE_PATH",
    ];
    if path.ends_with("h2-transition.mjs") {
        names.extend([
            "OWNER_RELATIVE_PATH",
            "CANDIDATE_RELATIVE_PATH",
            "PROFILE_RELATIVE_PATH",
        ]);
    } else {
        names.push("PROFILE_RELATIVE_PATH");
    }
    let mut result = Vec::new();
    for name in names {
        if let Some(value) = const_string_value(text, name) {
            if value.starts_with("ratchets/") && !result.contains(&value) {
                result.push(value);
            }
        }
    }
    result.sort_by(|left, right| byte_cmp(left, right));
    result
}

fn const_string_value(text: &str, name: &str) -> Option<String> {
    let marker = format!("const {name}");
    let bytes = text.as_bytes();
    let mut offset = 0;
    while let Some(found) = text[offset..].find(&marker) {
        let start = offset + found;
        let after_name = start + marker.len();
        if bytes
            .get(after_name)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            offset = after_name;
            continue;
        }
        let equal_offset = text[after_name..].find('=')?;
        let value_start = after_name + equal_offset + 1;
        let quoted = quoted_strings(&text[value_start..]).into_iter().next()?;
        return Some(quoted.value);
    }
    None
}

fn masked_core_digest(bytes: &[u8], pins: &[PinRecord]) -> Result<Digest, DiscoveryError> {
    let mut masked = Vec::with_capacity(bytes.len());
    let mut offset = 0;
    for pin in pins {
        if pin.start < offset || pin.end > bytes.len() || pin.start > pin.end {
            return Err(DiscoveryError::new("invalid typed pin span"));
        }
        masked.extend_from_slice(&bytes[offset..pin.start]);
        masked.extend_from_slice(PIN_MASK);
        offset = pin.end;
    }
    masked.extend_from_slice(&bytes[offset..]);
    Ok(sha256(&masked))
}

fn pin_term_digest(ordinal: usize, pin: &ExtractedPin) -> Digest {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"shadow-pin-term/v1\0");
    put_string(&mut bytes, &ordinal.to_string());
    put_string(&mut bytes, pin.grammar.as_str());
    put_string(&mut bytes, &pin.path);
    put_string(&mut bytes, &pin.literal);
    put_string(&mut bytes, &pin.start.to_string());
    put_string(&mut bytes, &pin.end.to_string());
    sha256(&bytes)
}

fn pin_envelope_digest(pins: &[PinRecord]) -> Digest {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"shadow-pin-envelope/v1\0");
    put_u64(&mut bytes, pins.len() as u64);
    for pin in pins {
        put_u64(&mut bytes, pin.ordinal as u64);
        put_u64(&mut bytes, pin.start as u64);
        put_u64(&mut bytes, pin.end as u64);
        put_string(&mut bytes, pin.grammar.as_str());
        put_string(&mut bytes, &pin.path);
        put_string(&mut bytes, &pin.literal);
    }
    sha256(&bytes)
}

fn artifact_envelope_digest(path: &str, raw: Digest, size: u64) -> Digest {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"shadow-artifact-envelope/v1\0");
    put_string(&mut bytes, path);
    put_digest(&mut bytes, raw);
    put_u64(&mut bytes, size);
    sha256(&bytes)
}

fn json_shape_verdict(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let trimmed = text.trim();
    if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        "JSON_SHAPE_ONLY_CANONICAL_COMPARATOR_PENDING".to_string()
    } else {
        "SCHEMA_DRIFT:not-a-JSON-container".to_string()
    }
}

fn local_imports(root: &Path, importer: &Path, text: &str) -> BTreeSet<String> {
    let mut specs = BTreeSet::new();
    for quoted in quoted_strings(text) {
        let spec = quoted.value;
        let line_start = text[..quoted.start]
            .rfind('\n')
            .map_or(0, |position| position + 1);
        let statement = text[line_start..quoted.start].trim_start();
        let prefix = statement.trim_end();
        let is_static_import = statement.starts_with("import ")
            || statement.starts_with("import\t")
            || prefix.ends_with(" from")
            || prefix.ends_with("from\t")
            || prefix.ends_with("require(")
            || prefix.ends_with("import(")
            || prefix.ends_with("import (");
        if is_static_import
            && spec.starts_with('.')
            && (spec.ends_with(".mjs") || spec.ends_with(".js"))
        {
            specs.insert(spec);
        }
    }
    let mut paths = BTreeSet::new();
    for spec in specs {
        let candidate = importer.parent().unwrap_or(root).join(spec);
        if let Ok(relative) = relative_string(root, &candidate) {
            if !relative.starts_with("../") {
                paths.insert(relative);
            }
        }
    }
    paths
}

fn discover_helpers(
    root: &Path,
    files: &mut BTreeMap<String, Vec<u8>>,
    initial: &BTreeSet<String>,
    findings: &mut Vec<DiscoveryFinding>,
) -> Result<Vec<LoadedHelper>, DiscoveryError> {
    let mut queue = VecDeque::from_iter(initial.iter().cloned());
    let mut seen = BTreeSet::new();
    let mut helpers = Vec::new();
    while let Some(relative) = queue.pop_front() {
        if !seen.insert(relative.clone()) {
            continue;
        }
        let path = root.join(&relative);
        if !path.is_file() {
            findings.push(DiscoveryFinding {
                class: ComparisonClass::SchemaDrift,
                path: relative,
                reason: "local import does not resolve to a file".to_string(),
            });
            continue;
        }
        let bytes = fs::read(&path)?;
        // Vendored toolchain bundles are hashed implementation inputs.  They
        // are CommonJS bundles rather than local helper modules; scanning an
        // 8 MiB bundle for nested imports adds no useful edge and is
        // needlessly quadratic with the small M1 quote scanner.
        if !relative.starts_with("vendor/") {
            let text = String::from_utf8_lossy(&bytes);
            for import in local_imports(root, &path, &text) {
                queue.push_back(import);
            }
        }
        files.insert(relative.clone(), bytes.clone());
        helpers.push(LoadedHelper {
            path: relative,
            raw_digest: sha256(&bytes),
            size: bytes.len() as u64,
        });
    }
    Ok(helpers)
}

fn read_walk_order(
    root: &Path,
    findings: &mut Vec<DiscoveryFinding>,
) -> Result<Vec<String>, DiscoveryError> {
    let path = root.join("scripts/chain-walk.sh");
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            findings.push(DiscoveryFinding {
                class: ComparisonClass::SchemaDrift,
                path: "scripts/chain-walk.sh".to_string(),
                reason: "canonical walk driver is absent".to_string(),
            });
            return Ok(Vec::new());
        }
        Err(error) => return Err(error.into()),
    };
    let text = String::from_utf8_lossy(&bytes);
    let Some(start) = text.find("ORDER=(") else {
        findings.push(DiscoveryFinding {
            class: ComparisonClass::SchemaDrift,
            path: "scripts/chain-walk.sh".to_string(),
            reason: "canonical walk has no parseable ORDER block".to_string(),
        });
        return Ok(Vec::new());
    };
    let Some(end_relative) = text[start + "ORDER=(".len()..].find(')') else {
        findings.push(DiscoveryFinding {
            class: ComparisonClass::SchemaDrift,
            path: "scripts/chain-walk.sh".to_string(),
            reason: "canonical walk ORDER block is unterminated".to_string(),
        });
        return Ok(Vec::new());
    };
    let body = &text[start + "ORDER=(".len()..start + "ORDER=(".len() + end_relative];
    let mut order = Vec::new();
    for line in body.lines() {
        let line = line.split('#').next().unwrap_or("");
        order.extend(line.split_whitespace().map(str::to_string));
    }
    Ok(order)
}

fn cross_check_walk_order(
    scripts: &[DiscoveredScript],
    walk_order: &[String],
    findings: &mut Vec<DiscoveryFinding>,
) {
    let discovered: BTreeMap<String, &DiscoveredScript> = scripts
        .iter()
        .map(|script| (script.rung(), script))
        .collect();
    let order: BTreeSet<&str> = walk_order.iter().map(String::as_str).collect();
    for script in scripts.iter().filter(|script| {
        script.class == ItemClass::Producer && script.path.starts_with("crates/oracle/h2-")
    }) {
        if !order.contains(script.rung().as_str()) {
            findings.push(DiscoveryFinding {
                class: ComparisonClass::SchemaDrift,
                path: script.path.clone(),
                reason: "producer is absent from canonical walk ORDER".to_string(),
            });
        }
    }
    for rung in walk_order.iter().filter(|rung| rung.starts_with("h2-")) {
        if !discovered.contains_key(rung) {
            findings.push(DiscoveryFinding {
                class: ComparisonClass::MissingRung,
                path: format!("crates/oracle/{rung}.mjs"),
                reason: "canonical walk ORDER names an absent H2 script".to_string(),
            });
        }
    }
}

fn topological_sort(
    scripts: &[DiscoveredScript],
    edges: &[ProducerEdge],
    findings: &mut Vec<DiscoveryFinding>,
) -> Vec<String> {
    let nodes: BTreeSet<String> = scripts
        .iter()
        .filter(|script| {
            script.class != ItemClass::ImportedHelper && !script.declared_artifacts.is_empty()
        })
        .map(|script| script.path.clone())
        .collect();
    let mut indegree: BTreeMap<String, usize> =
        nodes.iter().map(|node| (node.clone(), 0)).collect();
    let mut adjacency: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for edge in edges {
        if !nodes.contains(&edge.producer)
            || !nodes.contains(&edge.consumer)
            || edge.producer == edge.consumer
        {
            if edge.producer == edge.consumer {
                findings.push(DiscoveryFinding {
                    class: ComparisonClass::SchemaDrift,
                    path: edge.consumer.clone(),
                    reason: "typed producer graph contains a self-cycle".to_string(),
                });
            }
            continue;
        }
        if adjacency
            .entry(edge.producer.clone())
            .or_default()
            .insert(edge.consumer.clone())
        {
            if let Some(value) = indegree.get_mut(&edge.consumer) {
                *value += 1;
            }
        }
    }
    let mut ready: BTreeSet<String> = indegree
        .iter()
        .filter_map(|(node, degree)| (*degree == 0).then_some(node.clone()))
        .collect();
    let mut result = Vec::with_capacity(nodes.len());
    while let Some(node) = ready.pop_first() {
        result.push(node.clone());
        if let Some(children) = adjacency.get(&node) {
            for child in children {
                let degree = indegree.get_mut(child).expect("child has indegree");
                *degree -= 1;
                if *degree == 0 {
                    ready.insert(child.clone());
                }
            }
        }
    }
    if result.len() != nodes.len() {
        let mut remaining: Vec<String> = nodes
            .into_iter()
            .filter(|node| !result.contains(node))
            .collect();
        remaining.sort_by(|left, right| byte_cmp(left, right));
        findings.push(DiscoveryFinding {
            class: ComparisonClass::SchemaDrift,
            path: "producer-graph".to_string(),
            reason: "typed producer graph contains a cycle".to_string(),
        });
        result.extend(remaining);
    }
    result
}

fn inventory_digest(
    scripts: &[DiscoveredScript],
    artifacts: &[DiscoveredArtifact],
    helpers: &[LoadedHelper],
    edges: &[ProducerEdge],
    walk_order: &[String],
    findings: &[DiscoveryFinding],
) -> Digest {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"shadow-inventory/v1\0");
    for script in scripts {
        put_string(&mut bytes, &script.path);
        put_string(&mut bytes, script.class.as_str());
        put_digest(&mut bytes, script.raw_digest);
        put_optional_digest(&mut bytes, script.core_digest);
        put_optional_digest(&mut bytes, script.envelope_digest);
        for artifact in &script.declared_artifacts {
            put_string(&mut bytes, artifact);
        }
    }
    for artifact in artifacts {
        put_string(&mut bytes, &artifact.path);
        put_digest(&mut bytes, artifact.raw_digest);
        put_digest(&mut bytes, artifact.envelope_digest);
    }
    for helper in helpers {
        put_string(&mut bytes, &helper.path);
        put_digest(&mut bytes, helper.raw_digest);
    }
    for edge in edges {
        put_string(&mut bytes, &edge.producer);
        put_string(&mut bytes, &edge.consumer);
        put_string(&mut bytes, &edge.path);
        put_string(&mut bytes, edge.projection.as_str());
        put_u64(&mut bytes, edge.pin_ordinal as u64);
        put_digest(&mut bytes, edge.pin_digest);
    }
    for rung in walk_order {
        put_string(&mut bytes, rung);
    }
    for finding in findings {
        put_string(&mut bytes, finding.class.as_str());
        put_string(&mut bytes, &finding.path);
        put_string(&mut bytes, &finding.reason);
    }
    sha256(&bytes)
}

/// Inputs used to construct a projection.  Keeping this explicit makes the
/// source-root baseline and selected runtime testable without a live git tree.
#[derive(Clone, Debug)]
pub struct ProjectionContext {
    pub repository_root: PathBuf,
    pub source_commit: String,
    pub source_tree_digest: Digest,
    pub node_version: String,
    pub selected_runtime: String,
    pub target_os: String,
    pub target_arch: String,
}

impl ProjectionContext {
    pub fn new(
        repository_root: impl AsRef<Path>,
        source_commit: impl Into<String>,
        source_tree_digest: Digest,
        node_version: impl Into<String>,
        selected_runtime: impl Into<String>,
    ) -> Self {
        Self {
            repository_root: repository_root.as_ref().to_path_buf(),
            source_commit: source_commit.into(),
            source_tree_digest,
            node_version: node_version.into(),
            selected_runtime: selected_runtime.into(),
            target_os: env::consts::OS.to_string(),
            target_arch: env::consts::ARCH.to_string(),
        }
    }

    pub fn for_repository(repository_root: impl AsRef<Path>) -> Self {
        let repository_root = repository_root.as_ref().to_path_buf();
        let source_commit = git_text(&repository_root, &["rev-parse", "HEAD"])
            .unwrap_or_else(|_| "unavailable".to_string());
        let source_tree_digest = source_tree_digest(&repository_root);
        let node_version = fs::read_to_string(repository_root.join(".node-version"))
            .map(|value| value.trim().to_string())
            .unwrap_or_else(|_| "unavailable".to_string());
        let selected_runtime = Command::new("node")
            .arg("--version")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "unavailable".to_string());
        Self::new(
            repository_root,
            source_commit,
            source_tree_digest,
            node_version,
            selected_runtime,
        )
    }
}

/// One projected current-oracle check.
#[derive(Clone, Debug)]
pub struct ProjectedCheck {
    pub rung: String,
    pub script_path: String,
    pub class: ItemClass,
    pub artifact_paths: Vec<String>,
    pub action: Action,
    pub manifest: SemanticInputManifest,
    pub dependencies: Vec<DependencyOutput>,
    pub baseline: Option<Digest>,
    pub receipt_key: ReceiptKey,
    pub implementation_core: Digest,
    pub pin_envelope: Digest,
    pub schema_verdict: String,
    pub comparator: String,
    pub comparable: bool,
}

/// All check projections plus aggregate digests used by the RUN record.
#[derive(Clone, Debug)]
pub struct ProjectionSet {
    pub checks: Vec<ProjectedCheck>,
    pub action_digest: Digest,
    pub manifest_digest: Digest,
    pub edge_digest: Digest,
    pub key_digest: Digest,
    pub comparator_digest: Digest,
}

pub fn project(
    discovery: &Discovery,
    context: &ProjectionContext,
) -> Result<ProjectionSet, DiscoveryError> {
    let mut by_path = BTreeMap::new();
    for script in &discovery.scripts {
        by_path.insert(script.path.clone(), script);
    }
    let mut helper_digests = BTreeMap::new();
    for helper in &discovery.helpers {
        helper_digests.insert(helper.path.clone(), helper.raw_digest);
    }
    let artifact_index: BTreeMap<String, &DiscoveredArtifact> = discovery
        .artifacts
        .iter()
        .map(|artifact| (artifact.path.clone(), artifact))
        .collect();
    let mut file_digests: BTreeMap<String, Digest> = discovery
        .files
        .iter()
        .map(|(path, bytes)| (path.clone(), sha256(bytes)))
        .collect();
    let mut order = discovery.topological_order.clone();
    for script in discovery.checkable_scripts() {
        if !order.contains(&script.path) {
            order.push(script.path.clone());
        }
    }
    order.sort_by(|left, right| {
        let left_index = discovery
            .topological_order
            .iter()
            .position(|path| path == left)
            .unwrap_or(usize::MAX);
        let right_index = discovery
            .topological_order
            .iter()
            .position(|path| path == right)
            .unwrap_or(usize::MAX);
        left_index
            .cmp(&right_index)
            .then_with(|| byte_cmp(left, right))
    });
    order.dedup();

    let mut checks = Vec::new();
    for path in order {
        let Some(script) = by_path.get(&path).copied() else {
            continue;
        };
        if !script.class.is_checkable() || script.check_argv.is_none() {
            continue;
        }
        let implementation_core = implementation_digest(script, &helper_digests)?;
        let definition = definition_bytes(script, context);
        let version = format!(
            "node-pin={};runtime={}",
            context.node_version, context.selected_runtime
        );
        let action = Action::new(
            ACTION_TOOL,
            version,
            sha256(&definition),
            implementation_core,
        );
        let mut manifest_map: BTreeMap<String, Digest> = BTreeMap::new();
        manifest_map.insert("source-tree".to_string(), context.source_tree_digest);
        manifest_map.insert(
            "platform:os".to_string(),
            sha256(context.target_os.as_bytes()),
        );
        manifest_map.insert(
            "platform:arch".to_string(),
            sha256(context.target_arch.as_bytes()),
        );
        manifest_map.insert(
            "runtime:.node-version".to_string(),
            sha256(context.node_version.as_bytes()),
        );
        manifest_map.insert(
            "runtime:selected".to_string(),
            sha256(context.selected_runtime.as_bytes()),
        );
        for artifact_path in &script.declared_artifacts {
            if let Some(artifact) = artifact_index.get(artifact_path) {
                // `--check` consumes the checked-in target even though the
                // same path is also the producer's declared output.
                manifest_map.insert(
                    format!("checked-in-target:{artifact_path}"),
                    artifact.raw_digest,
                );
            }
        }
        let schema_verdict = script
            .declared_artifacts
            .first()
            .and_then(|path| artifact_index.get(path))
            .map(|artifact| artifact.schema_verdict.clone())
            .unwrap_or_else(|| "MISSING_RUNG:declared target".to_string());
        let comparator = if script
            .declared_artifacts
            .iter()
            .any(|path| path.ends_with(".json"))
        {
            "canonical-json/v2 TODO".to_string()
        } else {
            "byte/v1".to_string()
        };
        manifest_map.insert(
            "schema:adapter".to_string(),
            sha256(ADAPTER_SCHEMA.as_bytes()),
        );
        manifest_map.insert(
            format!("schema:comparator:{}", script.path),
            sha256(comparator.as_bytes()),
        );
        for artifact_path in &script.declared_artifacts {
            let schema = artifact_index
                .get(artifact_path)
                .map(|artifact| artifact.schema_verdict.as_str())
                .unwrap_or("MISSING_RUNG:declared target");
            manifest_map.insert(format!("schema:{artifact_path}"), sha256(schema.as_bytes()));
        }
        let mut dependencies = Vec::new();
        for pin in &script.pins {
            if let Some(artifact) = artifact_index.get(&pin.path) {
                dependencies.push(DependencyOutput::core(
                    format!("artifact:{}", pin.path),
                    artifact.core_digest,
                ));
            } else if let Some(producer) = by_path.get(&pin.path) {
                if let Some(envelope) = producer.envelope_digest {
                    dependencies.push(DependencyOutput::envelope(
                        format!("script:{}", pin.path),
                        envelope,
                    ));
                }
            } else if let Some(bytes) = discovery.file_bytes(&pin.path) {
                let digest = file_digests
                    .entry(pin.path.clone())
                    .or_insert_with(|| sha256(bytes));
                manifest_map.insert(format!("input:{}", pin.path), *digest);
            } else {
                let absolute = context.repository_root.join(&pin.path);
                if let Ok(bytes) = fs::read(absolute) {
                    manifest_map.insert(format!("input:{}", pin.path), sha256(&bytes));
                }
            }
            // Every typed mask remains independently visible in the key's
            // envelope, so a pin-only edit cannot masquerade as core equality.
            dependencies.push(DependencyOutput::envelope(
                format!("pin:{}:{}", script.path, pin.ordinal),
                pin.envelope_term,
            ));
        }
        for quoted in quoted_strings(&script.text) {
            if let Some(path) = normalize_path(&quoted.value) {
                if artifact_index.contains_key(&path) {
                    continue;
                }
                let digest = file_digests.get(&path).copied().or_else(|| {
                    fs::read(context.repository_root.join(&path))
                        .ok()
                        .map(|bytes| {
                            let digest = sha256(&bytes);
                            file_digests.insert(path.clone(), digest);
                            digest
                        })
                });
                if let Some(digest) = digest {
                    manifest_map.insert(format!("declared-input:{path}"), digest);
                }
            }
        }
        let manifest = SemanticInputManifest::new(
            manifest_map
                .into_iter()
                .map(|(label, digest)| ManifestEntry::new(label, digest)),
        );
        let baseline = Some(context.source_tree_digest);
        let receipt_key =
            ReceiptKey::try_new(RECEIPT_SCHEMA, &action, &manifest, &dependencies, baseline)
                .map_err(|error| {
                    DiscoveryError::new(format!("{} receipt key: {error}", script.path))
                })?;
        checks.push(ProjectedCheck {
            rung: script.rung(),
            script_path: script.path.clone(),
            class: script.class,
            artifact_paths: script.declared_artifacts.clone(),
            action,
            manifest,
            dependencies,
            baseline,
            receipt_key,
            implementation_core,
            pin_envelope: script.envelope_digest.unwrap_or_default(),
            schema_verdict,
            comparator,
            comparable: script.is_comparable(),
        });
    }
    let action_digest = digest_projected_actions(&checks);
    let manifest_digest = digest_projected_manifests(&checks);
    let edge_digest = digest_projected_edges(&checks);
    let key_digest = digest_projected_keys(&checks);
    let comparator_digest = digest_projected_comparators(&checks);
    Ok(ProjectionSet {
        checks,
        action_digest,
        manifest_digest,
        edge_digest,
        key_digest,
        comparator_digest,
    })
}

fn implementation_digest(
    script: &DiscoveredScript,
    helper_digests: &BTreeMap<String, Digest>,
) -> Result<Digest, DiscoveryError> {
    let pins = script
        .pins
        .iter()
        .map(|pin| (pin.start, pin.end))
        .collect::<Vec<_>>();
    let mut masked = Vec::new();
    let mut offset = 0;
    for (start, end) in pins {
        if start < offset || end > script.text.len() || start > end {
            return Err(DiscoveryError::new(format!(
                "{} has invalid pin span",
                script.path
            )));
        }
        masked.extend_from_slice(&script.text.as_bytes()[offset..start]);
        masked.extend_from_slice(PIN_MASK);
        offset = end;
    }
    masked.extend_from_slice(&script.text.as_bytes()[offset..]);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"shadow-implementation/v1\0");
    put_string(&mut bytes, &script.path);
    put_u64(&mut bytes, masked.len() as u64);
    bytes.extend_from_slice(&masked);
    let mut helpers: Vec<_> = script
        .imported_helpers
        .iter()
        .filter_map(|path| helper_digests.get(path).map(|digest| (path, *digest)))
        .collect();
    helpers.sort_by(|left, right| byte_cmp(left.0, right.0));
    put_u64(&mut bytes, helpers.len() as u64);
    for (path, digest) in helpers {
        put_string(&mut bytes, path);
        put_digest(&mut bytes, digest);
    }
    Ok(sha256(&bytes))
}

fn definition_bytes(script: &DiscoveredScript, context: &ProjectionContext) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"shadow-definition/v1\0");
    put_string(&mut bytes, &script.path);
    put_string(&mut bytes, "argv");
    if let Some(argv) = &script.check_argv {
        put_u64(&mut bytes, argv.len() as u64);
        for argument in argv {
            put_string(&mut bytes, argument);
        }
    } else {
        put_u64(&mut bytes, 0);
    }
    put_string(&mut bytes, ".");
    put_string(&mut bytes, "allowlist:NODE_OPTIONS,TS_RS_SHADOW");
    put_string(&mut bytes, "timeout:600s");
    put_string(&mut bytes, "child-policy:one-child,no-network,no-write");
    put_string(&mut bytes, "artifact-kind:checked-in-ratchet-or-evidence");
    put_string(&mut bytes, "verdict-kind:exit-class+machine-verdict+term");
    put_string(&mut bytes, RECEIPT_SCHEMA);
    put_string(&mut bytes, &context.node_version);
    put_string(&mut bytes, &context.selected_runtime);
    for artifact in &script.declared_artifacts {
        put_string(&mut bytes, artifact);
    }
    bytes
}

fn digest_projected_actions(checks: &[ProjectedCheck]) -> Digest {
    let mut bytes = Vec::new();
    for check in checks {
        put_string(&mut bytes, &check.script_path);
        put_digest(&mut bytes, check.action.producer_digest());
    }
    sha256(&bytes)
}

fn digest_projected_manifests(checks: &[ProjectedCheck]) -> Digest {
    let mut bytes = Vec::new();
    for check in checks {
        put_string(&mut bytes, &check.script_path);
        for entry in &check.manifest.entries {
            put_string(&mut bytes, &entry.label);
            put_digest(&mut bytes, entry.digest);
        }
    }
    sha256(&bytes)
}

fn digest_projected_edges(checks: &[ProjectedCheck]) -> Digest {
    let mut bytes = Vec::new();
    for check in checks {
        put_string(&mut bytes, &check.script_path);
        for dependency in &check.dependencies {
            put_string(&mut bytes, &dependency.label);
            put_string(&mut bytes, dependency.projection.as_str());
            put_digest(&mut bytes, dependency.digest);
        }
    }
    sha256(&bytes)
}

fn digest_projected_keys(checks: &[ProjectedCheck]) -> Digest {
    let mut bytes = Vec::new();
    for check in checks {
        put_string(&mut bytes, &check.script_path);
        put_digest(&mut bytes, check.receipt_key.digest());
    }
    sha256(&bytes)
}

fn digest_projected_comparators(checks: &[ProjectedCheck]) -> Digest {
    let mut bytes = Vec::new();
    for check in checks {
        put_string(&mut bytes, &check.script_path);
        put_string(&mut bytes, &check.comparator);
    }
    sha256(&bytes)
}

/// Typed seam for the next slice.  No canonical-JSON normalization is done
/// by Phase 0; the BYTE comparator remains authoritative for this slice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticComparisonTodo;

pub fn compare_canonical_json_next_slice(
    _left: &[u8],
    _right: &[u8],
) -> Result<bool, SemanticComparisonTodo> {
    Err(SemanticComparisonTodo)
}

/// The typed verdict tuple required by the byte-rule comparison.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckVerdict {
    pub rung: String,
    pub artifact_digest: Option<Digest>,
    pub exit_class: String,
    pub machine_verdict: String,
    pub term: String,
}

/// A comparison row with enough provenance to reproduce it.
#[derive(Clone, Debug)]
pub struct ComparisonRow {
    pub rung: String,
    pub artifact: Option<String>,
    pub class: ComparisonClass,
    pub left_digest: Option<Digest>,
    pub right_digest: Option<Digest>,
    pub first_difference: String,
    pub reason_path: String,
    pub comparator: String,
    pub reproduction: String,
}

pub struct ByteRuleInput<'a> {
    pub rung: String,
    pub artifact: Option<String>,
    pub left_digest: Option<Digest>,
    pub right_digest: Option<Digest>,
    pub left_verdict: Option<&'a CheckVerdict>,
    pub right_verdict: Option<&'a CheckVerdict>,
    pub schema_valid: bool,
    pub reason_path: String,
    pub reproduction: String,
}

pub fn compare_byte_rule(input: ByteRuleInput<'_>) -> ComparisonRow {
    let ByteRuleInput {
        rung,
        artifact,
        left_digest,
        right_digest,
        left_verdict,
        right_verdict,
        schema_valid,
        reason_path,
        reproduction,
    } = input;
    let (class, first_difference) = if !schema_valid {
        (
            ComparisonClass::SchemaDrift,
            "declared comparator/schema is unknown or invalid".to_string(),
        )
    } else if left_digest.is_none() || right_digest.is_none() {
        (
            ComparisonClass::MissingRung,
            "artifact or check-verdict tuple is absent".to_string(),
        )
    } else if left_digest != right_digest {
        (
            ComparisonClass::ByteDrift,
            "raw artifact digest differs".to_string(),
        )
    } else if left_verdict != right_verdict {
        (
            ComparisonClass::ByteDrift,
            "typed check-verdict tuple differs".to_string(),
        )
    } else {
        (ComparisonClass::ByteEqual, "none".to_string())
    };
    ComparisonRow {
        rung,
        artifact,
        class,
        left_digest,
        right_digest,
        first_difference,
        reason_path,
        comparator: "byte/v1".to_string(),
        reproduction,
    }
}

pub fn budget_skip_row(
    rung: impl Into<String>,
    artifact: Option<String>,
    reason: impl Into<String>,
) -> ComparisonRow {
    let reason = reason.into();
    ComparisonRow {
        rung: rung.into(),
        artifact,
        class: ComparisonClass::BudgetSkip,
        left_digest: None,
        right_digest: None,
        first_difference: reason.clone(),
        reason_path: "budget.wall-or-invocation-cap".to_string(),
        comparator: "byte/v1".to_string(),
        reproduction: "rerun on the next regular sample trigger".to_string(),
    }
}

/// Single-child hard-budget controller.  It is deliberately independent of
/// scheduling; the bin performs one run and returns.
#[derive(Debug)]
pub struct Budget {
    started: Instant,
    wall_cap: Duration,
    max_invocations: usize,
    invocations: usize,
    max_child_concurrency: usize,
}

impl Budget {
    pub fn new(wall_cap: Duration, max_invocations: usize) -> Self {
        Self {
            started: Instant::now(),
            wall_cap,
            max_invocations,
            invocations: 0,
            max_child_concurrency: 1,
        }
    }

    pub fn try_admit(&mut self) -> bool {
        if self.invocations >= self.max_invocations || self.started.elapsed() >= self.wall_cap {
            return false;
        }
        self.invocations += 1;
        true
    }

    pub fn cap_expired(&self) -> bool {
        self.started.elapsed() >= self.wall_cap
    }

    pub fn invocations(&self) -> usize {
        self.invocations
    }

    pub const fn max_child_concurrency(&self) -> usize {
        self.max_child_concurrency
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    pub fn remaining_wall(&self) -> Duration {
        self.wall_cap.saturating_sub(self.started.elapsed())
    }
}

#[derive(Clone, Debug)]
pub struct CertificateFile {
    pub path: String,
    pub size: u64,
    pub digest: Digest,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct CertificateSnapshot {
    pub found: bool,
    pub certificate_id: Option<String>,
    pub relative_root: Option<String>,
    pub resolution: String,
    pub root_files: Vec<CertificateFile>,
    pub files: Vec<CertificateFile>,
    pub crate_tree_digest: Option<Digest>,
    pub crate_tree_matches: Option<bool>,
    pub final_green: bool,
    pub rounds: Vec<String>,
    pub overrides: Vec<String>,
    pub minted: Vec<String>,
    pub qualification_verdicts: Vec<String>,
}

/// Resolves a certificate by real run directories only.  The `latest`
/// symlink is intentionally ignored, even if it is newer by filesystem time.
pub fn load_certificate(
    repository_root: impl AsRef<Path>,
) -> Result<CertificateSnapshot, DiscoveryError> {
    let repository_root = repository_root.as_ref();
    let runs = repository_root
        .join(CHAIN_WALK_CERTIFICATE_ROOT)
        .join("runs");
    let chain_walk_root = repository_root.join(CHAIN_WALK_CERTIFICATE_ROOT);
    let root_files = ["converged-run-id", "converged-crates.sha256"]
        .into_iter()
        .filter_map(|name| {
            let path = chain_walk_root.join(name);
            let bytes = fs::read(path).ok()?;
            Some(CertificateFile {
                path: name.to_string(),
                size: bytes.len() as u64,
                digest: sha256(&bytes),
                bytes,
            })
        })
        .collect::<Vec<_>>();
    let mut real_runs = Vec::new();
    if let Ok(entries) = fs::read_dir(&runs) {
        for entry in entries {
            let entry = entry?;
            let file_type = fs::symlink_metadata(entry.path())?.file_type();
            if file_type.is_dir() {
                real_runs.push((
                    entry.file_name().to_string_lossy().into_owned(),
                    entry.path(),
                ));
            }
        }
    }
    real_runs.sort_by(|left, right| byte_cmp(&left.0, &right.0));
    let marker_id = root_files
        .iter()
        .find(|file| file.path == "converged-run-id")
        .map(|file| String::from_utf8_lossy(&file.bytes).trim().to_string());
    // The certificate is the newest real run directory.  The marker is
    // provenance only: trusting it to select an older directory would make
    // a stale marker equivalent to the forbidden `latest` symlink.
    let chosen = real_runs.last().cloned();
    let Some((certificate_id, certificate_root)) = chosen else {
        return Ok(CertificateSnapshot {
            root_files,
            resolution: "no-real-run-directory".to_string(),
            ..CertificateSnapshot::default()
        });
    };
    let resolution = if marker_id.as_deref() == Some(certificate_id.as_str()) {
        "converged-run-id-marker".to_string()
    } else {
        "newest-real-run-directory".to_string()
    };
    let mut files = Vec::new();
    collect_certificate_files(&certificate_root, &certificate_root, &mut files)?;
    files.sort_by(|left, right| byte_cmp(&left.path, &right.path));
    let summary = files
        .iter()
        .find(|file| file.path == "summary.log")
        .map(|file| String::from_utf8_lossy(&file.bytes).into_owned())
        .unwrap_or_default();
    let final_green = summary.contains("chain walk: converged and green");
    let rounds = summary
        .lines()
        .filter(|line| line.contains("walk round"))
        .map(str::to_string)
        .collect();
    let overrides = summary
        .lines()
        .filter(|line| line.contains("OVERRIDE"))
        .map(str::to_string)
        .collect();
    let minted = summary
        .lines()
        .filter_map(|line| {
            line.split_once("re-minted:")
                .map(|(_, value)| value.trim().to_string())
        })
        .flat_map(|line| {
            line.split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect();
    let qualification_verdicts = files
        .iter()
        .filter(|file| {
            file.path.contains("qual")
                || file.path.contains("qualification")
                || file.path.contains("verdict")
        })
        .map(|file| file.path.clone())
        .collect();
    let crate_tree_file = root_files
        .iter()
        .find(|file| file.path == "converged-crates.sha256");
    let expected_crate_tree_digest = crate_tree_file
        .and_then(|file| Digest::from_hex(String::from_utf8_lossy(&file.bytes).trim()).ok());
    let observed_crate_tree = crate_tree_digest(repository_root);
    let crate_tree_matches =
        expected_crate_tree_digest.map(|expected| expected == observed_crate_tree);
    Ok(CertificateSnapshot {
        found: true,
        certificate_id: Some(certificate_id.clone()),
        relative_root: Some(format!(
            "{CHAIN_WALK_CERTIFICATE_ROOT}/runs/{certificate_id}"
        )),
        resolution,
        root_files,
        files,
        crate_tree_digest: expected_crate_tree_digest,
        crate_tree_matches,
        final_green,
        rounds,
        overrides,
        minted,
        qualification_verdicts,
    })
}

fn collect_certificate_files(
    root: &Path,
    directory: &Path,
    output: &mut Vec<CertificateFile>,
) -> Result<(), DiscoveryError> {
    let mut entries: Vec<DirEntry> = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by(|left, right| {
        byte_cmp(
            &left.file_name().to_string_lossy(),
            &right.file_name().to_string_lossy(),
        )
    });
    for entry in entries {
        let path = entry.path();
        let file_type: FileType = fs::symlink_metadata(&path)?.file_type();
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            collect_certificate_files(root, &path, output)?;
        } else if file_type.is_file() {
            let bytes = fs::read(&path)?;
            let relative = path
                .strip_prefix(root)
                .map_err(|_| DiscoveryError::new("certificate path escaped run directory"))?
                .to_string_lossy()
                .replace('\\', "/");
            output.push(CertificateFile {
                path: relative,
                size: bytes.len() as u64,
                digest: sha256(&bytes),
                bytes,
            });
        }
    }
    Ok(())
}

/// One observed child check.  Its stdout/stderr are represented by digests,
/// while the exact exit and typed verdict tuple remain in the report.
#[derive(Clone, Debug)]
pub struct CheckRecord {
    pub rung: String,
    pub script_path: String,
    pub status: String,
    pub argv: Vec<String>,
    pub exit_class: String,
    pub machine_verdict: String,
    pub term: String,
    pub artifact_digest: Option<Digest>,
    pub artifact_size: Option<u64>,
    pub stdout_digest: Option<Digest>,
    pub stderr_digest: Option<Digest>,
    pub stdout_size: u64,
    pub stderr_size: u64,
    pub elapsed_ms: u64,
    pub verified: bool,
    pub diagnostic: bool,
    pub certificate_only: bool,
}

impl CheckRecord {
    fn verdict(&self) -> CheckVerdict {
        CheckVerdict {
            rung: self.rung.clone(),
            artifact_digest: self.artifact_digest,
            exit_class: self.exit_class.clone(),
            machine_verdict: self.machine_verdict.clone(),
            term: self.term.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct RunReport {
    pub run_id: String,
    pub mode: String,
    pub context: ProjectionContext,
    pub discovery: Discovery,
    pub projections: ProjectionSet,
    pub certificate: CertificateSnapshot,
    pub checks: Vec<CheckRecord>,
    pub comparisons: Vec<ComparisonRow>,
    pub selected: Vec<String>,
    pub completed: Vec<String>,
    pub skipped: Vec<String>,
    pub certificate_only: Vec<String>,
    pub budget: BudgetSummary,
    pub read_bytes: u64,
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
    pub wall_time_ms: u64,
    pub max_child_concurrency: usize,
}

#[derive(Clone, Debug)]
pub struct BudgetSummary {
    pub wall_cap_seconds: u64,
    pub max_invocations: usize,
    pub invocations: usize,
    pub expired: bool,
}

#[derive(Clone, Debug)]
pub struct WrittenReport {
    pub run_id: String,
    pub directory: PathBuf,
    pub json_path: PathBuf,
    pub markdown_path: PathBuf,
    pub json_bytes: u64,
    pub summary: String,
}

/// Run the report-only sample once.  The output root is caller-owned so the
/// binary can keep generated bytes inside this standalone Cargo project.
pub fn run_sample(
    repository_root: impl AsRef<Path>,
    output_root: impl AsRef<Path>,
) -> Result<WrittenReport, Box<dyn Error>> {
    let started = Instant::now();
    let mut budget = Budget::new(
        Duration::from_secs(SAMPLE_MAX_WALL_SECONDS),
        SAMPLE_MAX_CHECKS,
    );
    let repository_root = repository_root.as_ref().to_path_buf();
    let discovery = discover(&repository_root)?;
    let context = ProjectionContext::for_repository(&repository_root);
    let projections = project(&discovery, &context)?;
    let certificate = load_certificate(&repository_root)?;
    let certificate_id = certificate
        .certificate_id
        .as_deref()
        .unwrap_or("no-certificate");
    let run_id = deterministic_run_id(certificate_id, &context, &discovery, &projections);
    let mut checks = Vec::new();
    let mut comparisons = Vec::new();
    let mut selected = Vec::new();
    let mut completed = Vec::new();
    let mut skipped = Vec::new();
    let mut certificate_only = Vec::new();
    let certificate_usable = certificate.found
        && certificate.final_green
        && certificate.crate_tree_matches == Some(true);

    // Keep structural discovery failures in the comparison stream as well as
    // the inventory.  This makes the taxonomy count describe every reported
    // issue, including a pin/schema/order failure that prevents invocation.
    for finding in &discovery.findings {
        comparisons.push(ComparisonRow {
            rung: finding.path.clone(),
            artifact: None,
            class: finding.class,
            left_digest: None,
            right_digest: None,
            first_difference: finding.reason.clone(),
            reason_path: finding.path.clone(),
            comparator: "byte/v1".to_string(),
            reproduction: "review the discovery finding before trusting a sample".to_string(),
        });
    }

    if !certificate.found {
        comparisons.push(ComparisonRow {
            rung: "chain-walk-certificate".to_string(),
            artifact: None,
            class: ComparisonClass::MissingRung,
            left_digest: None,
            right_digest: None,
            first_difference: "no canonical chain-walk certificate run directory exists"
                .to_string(),
            reason_path: "target/chain-walk/runs/<certificate-id>".to_string(),
            comparator: "byte/v1".to_string(),
            reproduction: "run scripts/chain-walk.sh on the canonical clean checkout".to_string(),
        });
    } else if certificate.crate_tree_matches != Some(true) || !certificate.final_green {
        comparisons.push(ComparisonRow {
            rung: "chain-walk-certificate".to_string(),
            artifact: None,
            class: ComparisonClass::SchemaDrift,
            left_digest: certificate.crate_tree_digest,
            right_digest: Some(crate_tree_digest(&repository_root)),
            first_difference: if certificate.crate_tree_matches == Some(false) {
                "certificate crate tree does not match the checked-out crate tree".to_string()
            } else {
                "certificate is not final-green".to_string()
            },
            reason_path: "target/chain-walk/converged-crates.sha256".to_string(),
            comparator: "byte/v1".to_string(),
            reproduction: "inspect the exact certificate run recorded in the report".to_string(),
        });
    }

    for check in &projections.checks {
        let artifact = check
            .artifact_paths
            .first()
            .and_then(|path| discovery.artifact(path));
        let artifact_digest = artifact.map(|artifact| artifact.raw_digest);
        let artifact_size = artifact.map(|artifact| artifact.size);
        let artifact_path = check.artifact_paths.first().cloned();
        let is_five_g = check.rung == FIVE_G_RUNG;
        let restricted = check.class == ItemClass::RestrictedProducer;
        if !certificate_usable {
            checks.push(CheckRecord {
                rung: check.rung.clone(),
                script_path: check.script_path.clone(),
                status: "certificate-missing-or-stale".to_string(),
                argv: check.action_argv(),
                exit_class: "not-invoked".to_string(),
                machine_verdict: "certificate-only-unavailable".to_string(),
                term: "no-canonical-certificate".to_string(),
                artifact_digest,
                artifact_size,
                stdout_digest: None,
                stderr_digest: None,
                stdout_size: 0,
                stderr_size: 0,
                elapsed_ms: 0,
                verified: false,
                diagnostic: true,
                certificate_only: false,
            });
            comparisons.push(ComparisonRow {
                rung: check.rung.clone(),
                artifact: artifact_path,
                class: ComparisonClass::MissingRung,
                left_digest: artifact_digest,
                right_digest: None,
                first_difference: "canonical certificate does not authorize a check observation"
                    .to_string(),
                reason_path: "target/chain-walk/runs/<certificate-id>".to_string(),
                comparator: check.comparator.clone(),
                reproduction: format!("node {} --check", check.script_path),
            });
            continue;
        }
        if restricted || is_five_g {
            certificate_only.push(check.rung.clone());
            checks.push(CheckRecord {
                rung: check.rung.clone(),
                script_path: check.script_path.clone(),
                status: if is_five_g {
                    "5g-prohibited-certificate-only"
                } else {
                    "restricted-certificate-only"
                }
                .to_string(),
                argv: check.action_argv(),
                exit_class: "certificate-only".to_string(),
                machine_verdict: "certificate-only".to_string(),
                term: if is_five_g {
                    "5g-observation-prohibited"
                } else {
                    "restricted-producer"
                }
                .to_string(),
                artifact_digest,
                artifact_size,
                stdout_digest: None,
                stderr_digest: None,
                stdout_size: 0,
                stderr_size: 0,
                elapsed_ms: 0,
                verified: false,
                diagnostic: true,
                certificate_only: true,
            });
            comparisons.push(ComparisonRow {
                rung: check.rung.clone(),
                artifact: artifact_path,
                class: ComparisonClass::MissingRung,
                left_digest: artifact_digest,
                right_digest: None,
                first_difference: if is_five_g {
                    "5g observation is prohibited in the shadow command".to_string()
                } else {
                    "restricted producer is observed only through certificate bytes".to_string()
                },
                reason_path: check.script_path.clone(),
                comparator: check.comparator.clone(),
                reproduction: format!(
                    "inspect certificate files; do not invoke {}",
                    check.script_path
                ),
            });
            continue;
        }
        if !check.comparable {
            checks.push(CheckRecord {
                rung: check.rung.clone(),
                script_path: check.script_path.clone(),
                status: "schema-drift-not-invoked".to_string(),
                argv: check.action_argv(),
                exit_class: "not-invoked".to_string(),
                machine_verdict: "schema-drift".to_string(),
                term: "unclassified-pin".to_string(),
                artifact_digest,
                artifact_size,
                stdout_digest: None,
                stderr_digest: None,
                stdout_size: 0,
                stderr_size: 0,
                elapsed_ms: 0,
                verified: false,
                diagnostic: true,
                certificate_only: false,
            });
            comparisons.push(ComparisonRow {
                rung: check.rung.clone(),
                artifact: artifact_path,
                class: ComparisonClass::SchemaDrift,
                left_digest: artifact_digest,
                right_digest: None,
                first_difference: "unclassified path-adjacent literal prevents guessed masking"
                    .to_string(),
                reason_path: check.script_path.clone(),
                comparator: check.comparator.clone(),
                reproduction: format!("review typed pin spans in {}", check.script_path),
            });
            continue;
        }
        if !budget.try_admit() {
            skipped.push(check.rung.clone());
            checks.push(CheckRecord {
                rung: check.rung.clone(),
                script_path: check.script_path.clone(),
                status: "budget-skipped".to_string(),
                argv: check.action_argv(),
                exit_class: "not-invoked".to_string(),
                machine_verdict: "budget-skip".to_string(),
                term: "sample-cap".to_string(),
                artifact_digest,
                artifact_size,
                stdout_digest: None,
                stderr_digest: None,
                stdout_size: 0,
                stderr_size: 0,
                elapsed_ms: 0,
                verified: false,
                diagnostic: true,
                certificate_only: false,
            });
            comparisons.push(budget_skip_row(
                check.rung.clone(),
                artifact_path,
                "10-minute sample or eight-check cap reached",
            ));
            continue;
        }
        selected.push(check.rung.clone());
        let observation = invoke_check(&repository_root, check, budget.remaining_wall())?;
        let expected_verdict = CheckVerdict {
            rung: check.rung.clone(),
            artifact_digest,
            exit_class: "exit:0".to_string(),
            machine_verdict: "green".to_string(),
            term: "exit:0".to_string(),
        };
        let observed_verdict = observation.verdict();
        let row = compare_byte_rule(ByteRuleInput {
            rung: check.rung.clone(),
            artifact: artifact_path,
            left_digest: artifact_digest.and(observation.artifact_digest),
            right_digest: artifact_digest,
            left_verdict: Some(&observed_verdict),
            right_verdict: Some(&expected_verdict),
            schema_valid: (check.schema_verdict.starts_with("JSON_SHAPE")
                && check.schema_verdict.contains("PENDING"))
                || check.comparator == "byte/v1",
            reason_path: check.script_path.clone(),
            reproduction: format!("node {} --check", check.script_path),
        });
        if row.class == ComparisonClass::ByteEqual {
            completed.push(check.rung.clone());
        }
        checks.push(observation);
        comparisons.push(row);
    }
    let elapsed = started.elapsed();
    let read_bytes = discovery
        .files
        .values()
        .map(|bytes| bytes.len() as u64)
        .sum();
    let stdout_bytes = checks.iter().map(|check| check.stdout_size).sum();
    let stderr_bytes = checks.iter().map(|check| check.stderr_size).sum();
    let report = RunReport {
        run_id: run_id.clone(),
        mode: "sample".to_string(),
        context,
        discovery,
        projections,
        certificate,
        checks,
        comparisons,
        selected,
        completed,
        skipped,
        certificate_only,
        budget: BudgetSummary {
            wall_cap_seconds: SAMPLE_MAX_WALL_SECONDS,
            max_invocations: SAMPLE_MAX_CHECKS,
            invocations: budget.invocations(),
            expired: budget.cap_expired(),
        },
        read_bytes,
        stdout_bytes,
        stderr_bytes,
        wall_time_ms: elapsed.as_millis().min(u64::MAX as u128) as u64,
        max_child_concurrency: budget.max_child_concurrency(),
    };
    write_report(output_root.as_ref(), &report)
}

fn deterministic_run_id(
    certificate_id: &str,
    context: &ProjectionContext,
    discovery: &Discovery,
    projections: &ProjectionSet,
) -> String {
    let mut bytes = Vec::new();
    put_string(&mut bytes, certificate_id);
    put_string(&mut bytes, &context.source_commit);
    put_digest(&mut bytes, context.source_tree_digest);
    put_digest(&mut bytes, discovery.inventory_digest);
    put_digest(&mut bytes, projections.key_digest);
    let digest = sha256(&bytes).to_hex();
    format!("{}-{}", sanitize_id(certificate_id), &digest[..16])
}

fn sanitize_id(value: &str) -> String {
    let mut result = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' {
            result.push(byte as char);
        } else {
            result.push('_');
        }
    }
    if result.is_empty() {
        "no-certificate".to_string()
    } else {
        result
    }
}

fn invoke_check(
    repository_root: &Path,
    check: &ProjectedCheck,
    cap: Duration,
) -> Result<CheckRecord, Box<dyn Error>> {
    let started = Instant::now();
    let mut command = Command::new("node");
    command
        .arg(&check.script_path)
        .arg("--check")
        .current_dir(repository_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let mut timed_out = false;
    loop {
        if let Some(status) = child.try_wait()? {
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            if let Some(mut pipe) = child.stdout.take() {
                pipe.read_to_end(&mut stdout)?;
            }
            if let Some(mut pipe) = child.stderr.take() {
                pipe.read_to_end(&mut stderr)?;
            }
            let elapsed_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
            let exit_class = if status.success() {
                "exit:0".to_string()
            } else if let Some(code) = status.code() {
                format!("exit:{code}")
            } else {
                "signal".to_string()
            };
            let machine_verdict = if status.success() { "green" } else { "red" };
            return Ok(CheckRecord {
                rung: check.rung.clone(),
                script_path: check.script_path.clone(),
                status: if status.success() {
                    "completed"
                } else {
                    "diagnostic-failure"
                }
                .to_string(),
                argv: check.action_argv(),
                exit_class: if timed_out {
                    "timeout".to_string()
                } else {
                    exit_class.clone()
                },
                machine_verdict: if timed_out {
                    "timeout".to_string()
                } else {
                    machine_verdict.to_string()
                },
                term: if timed_out {
                    "deadline".to_string()
                } else {
                    exit_class.clone()
                },
                artifact_digest: check_artifact_digest(check, repository_root),
                artifact_size: check_artifact_size(check, repository_root),
                stdout_digest: Some(sha256(&stdout)),
                stderr_digest: Some(sha256(&stderr)),
                stdout_size: stdout.len() as u64,
                stderr_size: stderr.len() as u64,
                elapsed_ms,
                verified: status.success() && !timed_out,
                diagnostic: !status.success() || timed_out,
                certificate_only: false,
            });
        }
        if started.elapsed() >= cap {
            timed_out = true;
            let _ = child.kill();
            let _ = child.wait();
        } else {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn check_artifact_digest(check: &ProjectedCheck, root: &Path) -> Option<Digest> {
    check
        .artifact_paths
        .first()
        .and_then(|path| fs::read(root.join(path)).ok().map(|bytes| sha256(&bytes)))
}

fn check_artifact_size(check: &ProjectedCheck, root: &Path) -> Option<u64> {
    check.artifact_paths.first().and_then(|path| {
        fs::metadata(root.join(path))
            .ok()
            .map(|metadata| metadata.len())
    })
}

impl ProjectedCheck {
    fn action_argv(&self) -> Vec<String> {
        vec![
            "node".to_string(),
            self.script_path.clone(),
            "--check".to_string(),
        ]
    }
}

pub fn write_report(
    output_root: &Path,
    report: &RunReport,
) -> Result<WrittenReport, Box<dyn Error>> {
    let directory = output_root.join("runs").join(&report.run_id);
    fs::create_dir_all(&directory)?;
    let json_path = directory.join("report.json");
    let markdown_path = directory.join("report.md");
    let mut report_bytes = 0u64;
    let json = loop {
        let rendered = render_report_json(report, report_bytes);
        let next = rendered.len() as u64;
        if next == report_bytes {
            break rendered;
        }
        report_bytes = next;
    };
    fs::write(&json_path, json.as_bytes())?;
    let markdown = render_report_markdown(report, report_bytes);
    fs::write(&markdown_path, markdown.as_bytes())?;
    let counts = comparison_counts(&report.comparisons);
    let summary = format!(
        "wrote {} and {} (run_id={}, scripts={}, artifacts={}, checks={}, selected={}, completed={}, certificate_only={}, BYTE_EQUAL={}, SCHEMA_DRIFT={}, MISSING_RUNG={}, BUDGET_SKIP={})",
        json_path.display(),
        markdown_path.display(),
        report.run_id,
        report.discovery.scripts.len(),
        report.discovery.artifacts.len(),
        report.projections.checks.len(),
        report.selected.len(),
        report.completed.len(),
        report.certificate_only.len(),
        counts.get(&ComparisonClass::ByteEqual).copied().unwrap_or(0),
        counts.get(&ComparisonClass::SchemaDrift).copied().unwrap_or(0),
        counts.get(&ComparisonClass::MissingRung).copied().unwrap_or(0),
        counts.get(&ComparisonClass::BudgetSkip).copied().unwrap_or(0),
    );
    Ok(WrittenReport {
        run_id: report.run_id.clone(),
        directory,
        json_path,
        markdown_path,
        json_bytes: json.len() as u64,
        summary,
    })
}

fn comparison_counts(rows: &[ComparisonRow]) -> BTreeMap<ComparisonClass, usize> {
    let mut counts = BTreeMap::new();
    for row in rows {
        *counts.entry(row.class).or_insert(0) += 1;
    }
    counts
}

#[derive(Clone, Debug)]
enum JsonValue {
    Null,
    Bool(bool),
    Number(u64),
    String(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    fn render(&self, output: &mut String) {
        match self {
            Self::Null => output.push_str("null"),
            Self::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
            Self::Number(value) => output.push_str(&value.to_string()),
            Self::String(value) => output.push_str(&json_string(value)),
            Self::Array(values) => {
                output.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        output.push(',');
                    }
                    value.render(output);
                }
                output.push(']');
            }
            Self::Object(values) => {
                output.push('{');
                for (index, (key, value)) in values.iter().enumerate() {
                    if index != 0 {
                        output.push(',');
                    }
                    output.push_str(&json_string(key));
                    output.push(':');
                    value.render(output);
                }
                output.push('}');
            }
        }
    }
}

fn json_string(value: &str) -> String {
    let mut result = String::with_capacity(value.len() + 2);
    result.push('"');
    for character in value.chars() {
        match character {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            character if character.is_control() => {
                write!(&mut result, "\\u{:04x}", character as u32).expect("string write");
            }
            character => result.push(character),
        }
    }
    result.push('"');
    result
}

fn j_string(value: impl Into<String>) -> JsonValue {
    JsonValue::String(value.into())
}

fn j_digest(value: Option<Digest>) -> JsonValue {
    value.map_or(JsonValue::Null, |digest| j_string(digest.to_hex()))
}

fn render_report_json(report: &RunReport, report_bytes: u64) -> String {
    let counts = comparison_counts(&report.comparisons);
    let object = JsonValue::Object(vec![
        ("schema".to_string(), j_string(ADAPTER_SCHEMA)),
        ("kind".to_string(), j_string("RUN")),
        ("run_id".to_string(), j_string(&report.run_id)),
        ("mode".to_string(), j_string(&report.mode)),
        ("report_only".to_string(), JsonValue::Bool(true)),
        ("gate_coupled".to_string(), JsonValue::Bool(false)),
        ("oracle_write".to_string(), JsonValue::Bool(false)),
        ("five_g_observation".to_string(), JsonValue::Bool(false)),
        (
            "repository_access".to_string(),
            j_string("read-only crates/ and ratchets/"),
        ),
        (
            "source".to_string(),
            JsonValue::Object(vec![
                (
                    "commit".to_string(),
                    j_string(&report.context.source_commit),
                ),
                (
                    "tree_digest".to_string(),
                    j_string(report.context.source_tree_digest.to_hex()),
                ),
                (
                    "node_version_file".to_string(),
                    j_string(&report.context.node_version),
                ),
                (
                    "selected_runtime".to_string(),
                    j_string(&report.context.selected_runtime),
                ),
                ("target_os".to_string(), j_string(&report.context.target_os)),
                (
                    "target_arch".to_string(),
                    j_string(&report.context.target_arch),
                ),
            ]),
        ),
        (
            "inventory".to_string(),
            JsonValue::Object(vec![
                (
                    "inventory_digest".to_string(),
                    j_string(report.discovery.inventory_digest.to_hex()),
                ),
                (
                    "scripts".to_string(),
                    JsonValue::Array(report.discovery.scripts.iter().map(script_json).collect()),
                ),
                (
                    "ratchets".to_string(),
                    JsonValue::Array(
                        report
                            .discovery
                            .artifacts
                            .iter()
                            .map(artifact_json)
                            .collect(),
                    ),
                ),
                (
                    "helpers".to_string(),
                    JsonValue::Array(report.discovery.helpers.iter().map(helper_json).collect()),
                ),
                (
                    "walk_order".to_string(),
                    JsonValue::Array(report.discovery.walk_order.iter().map(j_string).collect()),
                ),
                (
                    "topological_order".to_string(),
                    JsonValue::Array(
                        report
                            .discovery
                            .topological_order
                            .iter()
                            .map(j_string)
                            .collect(),
                    ),
                ),
                (
                    "findings".to_string(),
                    JsonValue::Array(report.discovery.findings.iter().map(finding_json).collect()),
                ),
            ]),
        ),
        (
            "projection".to_string(),
            JsonValue::Object(vec![
                ("schema".to_string(), j_string(RECEIPT_SCHEMA)),
                (
                    "actions_digest".to_string(),
                    j_string(report.projections.action_digest.to_hex()),
                ),
                (
                    "manifests_digest".to_string(),
                    j_string(report.projections.manifest_digest.to_hex()),
                ),
                (
                    "edges_digest".to_string(),
                    j_string(report.projections.edge_digest.to_hex()),
                ),
                (
                    "keys_digest".to_string(),
                    j_string(report.projections.key_digest.to_hex()),
                ),
                (
                    "comparators_digest".to_string(),
                    j_string(report.projections.comparator_digest.to_hex()),
                ),
                (
                    "checks".to_string(),
                    JsonValue::Array(
                        report
                            .projections
                            .checks
                            .iter()
                            .map(projected_json)
                            .collect(),
                    ),
                ),
                (
                    "typed_edges".to_string(),
                    JsonValue::Array(report.discovery.edges.iter().map(edge_json).collect()),
                ),
            ]),
        ),
        (
            "artifacts".to_string(),
            JsonValue::Array(
                report
                    .discovery
                    .artifacts
                    .iter()
                    .map(artifact_json)
                    .collect(),
            ),
        ),
        (
            "certificate".to_string(),
            certificate_json(&report.certificate),
        ),
        (
            "checks".to_string(),
            JsonValue::Array(report.checks.iter().map(check_json).collect()),
        ),
        (
            "comparisons".to_string(),
            JsonValue::Array(report.comparisons.iter().map(comparison_json).collect()),
        ),
        (
            "taxonomy".to_string(),
            JsonValue::Object(vec![
                (
                    "BYTE_EQUAL".to_string(),
                    JsonValue::Number(
                        counts
                            .get(&ComparisonClass::ByteEqual)
                            .copied()
                            .unwrap_or(0) as u64,
                    ),
                ),
                (
                    "BYTE_DRIFT".to_string(),
                    JsonValue::Number(
                        counts
                            .get(&ComparisonClass::ByteDrift)
                            .copied()
                            .unwrap_or(0) as u64,
                    ),
                ),
                (
                    "SCHEMA_DRIFT".to_string(),
                    JsonValue::Number(
                        counts
                            .get(&ComparisonClass::SchemaDrift)
                            .copied()
                            .unwrap_or(0) as u64,
                    ),
                ),
                (
                    "MISSING_RUNG".to_string(),
                    JsonValue::Number(
                        counts
                            .get(&ComparisonClass::MissingRung)
                            .copied()
                            .unwrap_or(0) as u64,
                    ),
                ),
                (
                    "BUDGET_SKIP".to_string(),
                    JsonValue::Number(
                        counts
                            .get(&ComparisonClass::BudgetSkip)
                            .copied()
                            .unwrap_or(0) as u64,
                    ),
                ),
            ]),
        ),
        (
            "selection".to_string(),
            JsonValue::Object(vec![
                ("selected".to_string(), string_array(&report.selected)),
                ("completed".to_string(), string_array(&report.completed)),
                ("skipped".to_string(), string_array(&report.skipped)),
                (
                    "certificate_only".to_string(),
                    string_array(&report.certificate_only),
                ),
            ]),
        ),
        (
            "budget".to_string(),
            JsonValue::Object(vec![
                (
                    "hard_wall_seconds".to_string(),
                    JsonValue::Number(report.budget.wall_cap_seconds),
                ),
                (
                    "max_invoked_checks".to_string(),
                    JsonValue::Number(report.budget.max_invocations as u64),
                ),
                (
                    "invoked_checks".to_string(),
                    JsonValue::Number(report.budget.invocations as u64),
                ),
                ("skip_and_record".to_string(), JsonValue::Bool(true)),
                (
                    "expired".to_string(),
                    JsonValue::Bool(report.budget.expired),
                ),
                (
                    "max_child_concurrency".to_string(),
                    JsonValue::Number(report.max_child_concurrency as u64),
                ),
            ]),
        ),
        (
            "counters".to_string(),
            JsonValue::Object(vec![
                (
                    "read_bytes".to_string(),
                    JsonValue::Number(report.read_bytes),
                ),
                (
                    "stdout_bytes".to_string(),
                    JsonValue::Number(report.stdout_bytes),
                ),
                (
                    "stderr_bytes".to_string(),
                    JsonValue::Number(report.stderr_bytes),
                ),
                ("report_bytes".to_string(), JsonValue::Number(report_bytes)),
                (
                    "wall_time_ms".to_string(),
                    JsonValue::Number(report.wall_time_ms),
                ),
            ]),
        ),
        (
            "comparator_seams".to_string(),
            JsonValue::Array(vec![JsonValue::Object(vec![
                ("name".to_string(), j_string("canonical-json/v2")),
                ("status".to_string(), j_string("TODO-next-slice")),
                (
                    "raw_byte_rule_remains_authoritative".to_string(),
                    JsonValue::Bool(true),
                ),
            ])]),
        ),
    ]);
    let mut rendered = String::new();
    object.render(&mut rendered);
    rendered.push('\n');
    rendered
}

fn string_array(values: &[String]) -> JsonValue {
    JsonValue::Array(values.iter().map(j_string).collect())
}

fn script_json(script: &DiscoveredScript) -> JsonValue {
    JsonValue::Object(vec![
        ("path".to_string(), j_string(&script.path)),
        ("class".to_string(), j_string(script.class.as_str())),
        (
            "raw_digest".to_string(),
            j_string(script.raw_digest.to_hex()),
        ),
        ("core_digest".to_string(), j_digest(script.core_digest)),
        (
            "envelope_digest".to_string(),
            j_digest(script.envelope_digest),
        ),
        (
            "declared_artifacts".to_string(),
            string_array(&script.declared_artifacts),
        ),
        (
            "check_argv".to_string(),
            script
                .check_argv
                .as_ref()
                .map_or(JsonValue::Null, |argv| string_array(argv)),
        ),
        (
            "imported_helpers".to_string(),
            string_array(&script.imported_helpers),
        ),
        (
            "pins".to_string(),
            JsonValue::Array(script.pins.iter().map(pin_json).collect()),
        ),
        (
            "unclassified_literals".to_string(),
            JsonValue::Array(script.unclassified.iter().map(unclassified_json).collect()),
        ),
        (
            "schema_error".to_string(),
            script
                .schema_error
                .as_ref()
                .map_or(JsonValue::Null, j_string),
        ),
    ])
}

fn pin_json(pin: &PinRecord) -> JsonValue {
    JsonValue::Object(vec![
        ("ordinal".to_string(), JsonValue::Number(pin.ordinal as u64)),
        ("start".to_string(), JsonValue::Number(pin.start as u64)),
        ("end".to_string(), JsonValue::Number(pin.end as u64)),
        ("path".to_string(), j_string(&pin.path)),
        ("grammar".to_string(), j_string(pin.grammar.as_str())),
        ("literal".to_string(), j_string(&pin.literal)),
        (
            "envelope_term".to_string(),
            j_string(pin.envelope_term.to_hex()),
        ),
    ])
}

fn unclassified_json(literal: &UnclassifiedLiteral) -> JsonValue {
    JsonValue::Object(vec![
        ("start".to_string(), JsonValue::Number(literal.start as u64)),
        ("literal".to_string(), j_string(&literal.literal)),
    ])
}

fn artifact_json(artifact: &DiscoveredArtifact) -> JsonValue {
    JsonValue::Object(vec![
        ("path".to_string(), j_string(&artifact.path)),
        ("digest".to_string(), j_string(artifact.raw_digest.to_hex())),
        ("size".to_string(), JsonValue::Number(artifact.size)),
        (
            "semantic_digest".to_string(),
            j_digest(artifact.semantic_digest),
        ),
        (
            "core_digest".to_string(),
            j_string(artifact.core_digest.to_hex()),
        ),
        (
            "envelope_digest".to_string(),
            j_string(artifact.envelope_digest.to_hex()),
        ),
        (
            "schema_verdict".to_string(),
            j_string(&artifact.schema_verdict),
        ),
        ("producers".to_string(), string_array(&artifact.producers)),
    ])
}

fn helper_json(helper: &LoadedHelper) -> JsonValue {
    JsonValue::Object(vec![
        ("path".to_string(), j_string(&helper.path)),
        ("digest".to_string(), j_string(helper.raw_digest.to_hex())),
        ("size".to_string(), JsonValue::Number(helper.size)),
    ])
}

fn finding_json(finding: &DiscoveryFinding) -> JsonValue {
    JsonValue::Object(vec![
        ("class".to_string(), j_string(finding.class.as_str())),
        (
            "severity".to_string(),
            j_string(finding.class.named_severity()),
        ),
        ("path".to_string(), j_string(&finding.path)),
        ("reason".to_string(), j_string(&finding.reason)),
    ])
}

fn edge_json(edge: &ProducerEdge) -> JsonValue {
    JsonValue::Object(vec![
        ("producer".to_string(), j_string(&edge.producer)),
        ("consumer".to_string(), j_string(&edge.consumer)),
        ("path".to_string(), j_string(&edge.path)),
        ("projection".to_string(), j_string(edge.projection.as_str())),
        (
            "pin_ordinal".to_string(),
            JsonValue::Number(edge.pin_ordinal as u64),
        ),
        ("pin_digest".to_string(), j_string(edge.pin_digest.to_hex())),
    ])
}

fn projected_json(check: &ProjectedCheck) -> JsonValue {
    JsonValue::Object(vec![
        ("rung".to_string(), j_string(&check.rung)),
        ("script".to_string(), j_string(&check.script_path)),
        ("class".to_string(), j_string(check.class.as_str())),
        ("artifacts".to_string(), string_array(&check.artifact_paths)),
        (
            "action".to_string(),
            JsonValue::Object(vec![
                ("tool".to_string(), j_string(&check.action.tool)),
                ("version".to_string(), j_string(&check.action.version)),
                (
                    "definition_digest".to_string(),
                    j_string(check.action.definition_digest.to_hex()),
                ),
                (
                    "implementation_digest".to_string(),
                    j_string(check.action.implementation_digest.to_hex()),
                ),
                (
                    "producer_digest".to_string(),
                    j_string(check.action.producer_digest().to_hex()),
                ),
            ]),
        ),
        (
            "manifest".to_string(),
            JsonValue::Array(
                check
                    .manifest
                    .entries
                    .iter()
                    .map(|entry| {
                        JsonValue::Object(vec![
                            ("label".to_string(), j_string(&entry.label)),
                            ("digest".to_string(), j_string(entry.digest.to_hex())),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "dependencies".to_string(),
            JsonValue::Array(
                check
                    .dependencies
                    .iter()
                    .map(|dependency| {
                        JsonValue::Object(vec![
                            ("label".to_string(), j_string(&dependency.label)),
                            (
                                "projection".to_string(),
                                j_string(dependency.projection.as_str()),
                            ),
                            ("digest".to_string(), j_string(dependency.digest.to_hex())),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("baseline".to_string(), j_digest(check.baseline)),
        (
            "receipt_key".to_string(),
            j_string(check.receipt_key.to_hex()),
        ),
        (
            "implementation_core".to_string(),
            j_string(check.implementation_core.to_hex()),
        ),
        (
            "pin_envelope".to_string(),
            j_string(check.pin_envelope.to_hex()),
        ),
        (
            "schema_verdict".to_string(),
            j_string(&check.schema_verdict),
        ),
        ("comparator".to_string(), j_string(&check.comparator)),
        ("comparable".to_string(), JsonValue::Bool(check.comparable)),
    ])
}

fn certificate_json(certificate: &CertificateSnapshot) -> JsonValue {
    JsonValue::Object(vec![
        ("found".to_string(), JsonValue::Bool(certificate.found)),
        (
            "certificate_id".to_string(),
            certificate
                .certificate_id
                .as_ref()
                .map_or(JsonValue::Null, j_string),
        ),
        (
            "relative_root".to_string(),
            certificate
                .relative_root
                .as_ref()
                .map_or(JsonValue::Null, j_string),
        ),
        ("resolution".to_string(), j_string(&certificate.resolution)),
        (
            "crate_tree_digest".to_string(),
            j_digest(certificate.crate_tree_digest),
        ),
        (
            "crate_tree_matches".to_string(),
            certificate
                .crate_tree_matches
                .map_or(JsonValue::Null, JsonValue::Bool),
        ),
        (
            "final_green".to_string(),
            JsonValue::Bool(certificate.final_green),
        ),
        ("rounds".to_string(), string_array(&certificate.rounds)),
        (
            "overrides".to_string(),
            string_array(&certificate.overrides),
        ),
        ("minted".to_string(), string_array(&certificate.minted)),
        (
            "qualification_verdicts".to_string(),
            string_array(&certificate.qualification_verdicts),
        ),
        (
            "files".to_string(),
            JsonValue::Array(
                certificate
                    .files
                    .iter()
                    .map(|file| {
                        JsonValue::Object(vec![
                            ("path".to_string(), j_string(&file.path)),
                            ("digest".to_string(), j_string(file.digest.to_hex())),
                            ("size".to_string(), JsonValue::Number(file.size)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "root_files".to_string(),
            JsonValue::Array(
                certificate
                    .root_files
                    .iter()
                    .map(|file| {
                        JsonValue::Object(vec![
                            ("path".to_string(), j_string(&file.path)),
                            ("digest".to_string(), j_string(file.digest.to_hex())),
                            ("size".to_string(), JsonValue::Number(file.size)),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

fn check_json(check: &CheckRecord) -> JsonValue {
    JsonValue::Object(vec![
        ("rung".to_string(), j_string(&check.rung)),
        ("script".to_string(), j_string(&check.script_path)),
        ("status".to_string(), j_string(&check.status)),
        ("argv".to_string(), string_array(&check.argv)),
        ("exit_class".to_string(), j_string(&check.exit_class)),
        (
            "machine_verdict".to_string(),
            j_string(&check.machine_verdict),
        ),
        ("term".to_string(), j_string(&check.term)),
        (
            "artifact_digest".to_string(),
            j_digest(check.artifact_digest),
        ),
        (
            "artifact_size".to_string(),
            check
                .artifact_size
                .map_or(JsonValue::Null, JsonValue::Number),
        ),
        ("stdout_digest".to_string(), j_digest(check.stdout_digest)),
        ("stderr_digest".to_string(), j_digest(check.stderr_digest)),
        (
            "stdout_size".to_string(),
            JsonValue::Number(check.stdout_size),
        ),
        (
            "stderr_size".to_string(),
            JsonValue::Number(check.stderr_size),
        ),
        (
            "elapsed_ms".to_string(),
            JsonValue::Number(check.elapsed_ms),
        ),
        ("verified".to_string(), JsonValue::Bool(check.verified)),
        ("diagnostic".to_string(), JsonValue::Bool(check.diagnostic)),
        (
            "certificate_only".to_string(),
            JsonValue::Bool(check.certificate_only),
        ),
    ])
}

fn comparison_json(row: &ComparisonRow) -> JsonValue {
    JsonValue::Object(vec![
        ("rung".to_string(), j_string(&row.rung)),
        (
            "artifact".to_string(),
            row.artifact.as_ref().map_or(JsonValue::Null, j_string),
        ),
        ("class".to_string(), j_string(row.class.as_str())),
        ("severity".to_string(), j_string(row.class.named_severity())),
        ("left_digest".to_string(), j_digest(row.left_digest)),
        ("right_digest".to_string(), j_digest(row.right_digest)),
        (
            "first_difference".to_string(),
            j_string(&row.first_difference),
        ),
        ("reason_path".to_string(), j_string(&row.reason_path)),
        ("comparator".to_string(), j_string(&row.comparator)),
        ("reproduction".to_string(), j_string(&row.reproduction)),
    ])
}

fn render_report_markdown(report: &RunReport, json_bytes: u64) -> String {
    let counts = comparison_counts(&report.comparisons);
    let mut output = String::new();
    writeln!(&mut output, "# Phase 0 shadow RUN report").expect("report write");
    writeln!(&mut output).expect("report write");
    writeln!(&mut output, "- Run ID: `{}`", report.run_id).expect("report write");
    writeln!(
        &mut output,
        "- Mode: `{}`; report-only: `true`; gate-coupled: `false`",
        report.mode
    )
    .expect("report write");
    writeln!(
        &mut output,
        "- Source commit: `{}`; source tree digest: `{}`",
        report.context.source_commit, report.context.source_tree_digest
    )
    .expect("report write");
    writeln!(
        &mut output,
        "- Canonical certificate: `{}` (resolution: `{}`)",
        report
            .certificate
            .certificate_id
            .as_deref()
            .unwrap_or("missing"),
        report.certificate.resolution
    )
    .expect("report write");
    writeln!(&mut output).expect("report write");
    writeln!(&mut output, "## Summary").expect("report write");
    writeln!(&mut output).expect("report write");
    writeln!(
        &mut output,
        "Scripts: {}; ratchets: {}; loaded helpers: {}; graph edges: {}.",
        report.discovery.scripts.len(),
        report.discovery.artifacts.len(),
        report.discovery.helpers.len(),
        report.discovery.edges.len()
    )
    .expect("report write");
    writeln!(
        &mut output,
        "Checks: {}; selected: {}; completed: {}; skipped: {}; certificate-only: {}.",
        report.projections.checks.len(),
        report.selected.len(),
        report.completed.len(),
        report.skipped.len(),
        report.certificate_only.len()
    )
    .expect("report write");
    writeln!(
        &mut output,
        "BYTE_EQUAL: {}; SCHEMA_DRIFT: {}; MISSING_RUNG: {}; BUDGET_SKIP: {}; report bytes: {}.",
        counts
            .get(&ComparisonClass::ByteEqual)
            .copied()
            .unwrap_or(0),
        counts
            .get(&ComparisonClass::SchemaDrift)
            .copied()
            .unwrap_or(0),
        counts
            .get(&ComparisonClass::MissingRung)
            .copied()
            .unwrap_or(0),
        counts
            .get(&ComparisonClass::BudgetSkip)
            .copied()
            .unwrap_or(0),
        json_bytes
    )
    .expect("report write");
    writeln!(&mut output).expect("report write");
    writeln!(&mut output, "## Dynamic inventory").expect("report write");
    writeln!(&mut output).expect("report write");
    writeln!(
        &mut output,
        "| path | class | raw digest | core | envelope | declared artifacts |"
    )
    .expect("report write");
    writeln!(&mut output, "|---|---|---|---|---|---|").expect("report write");
    for script in &report.discovery.scripts {
        writeln!(
            &mut output,
            "| `{}` | `{}` | `{}` | `{}` | `{}` | `{}` |",
            script.path,
            script.class,
            script.raw_digest,
            script
                .core_digest
                .map_or("—".to_string(), |value| value.to_hex()),
            script
                .envelope_digest
                .map_or("—".to_string(), |value| value.to_hex()),
            if script.declared_artifacts.is_empty() {
                "—".to_string()
            } else {
                script.declared_artifacts.join("<br>")
            }
        )
        .expect("report write");
    }
    writeln!(&mut output).expect("report write");
    writeln!(&mut output, "## Comparisons (BYTE rule, first slice)").expect("report write");
    writeln!(&mut output).expect("report write");
    writeln!(
        &mut output,
        "| rung | class | severity | artifact | first difference | reason path |"
    )
    .expect("report write");
    writeln!(&mut output, "|---|---|---|---|---|---|").expect("report write");
    for row in &report.comparisons {
        writeln!(
            &mut output,
            "| `{}` | `{}` | `{}` | `{}` | {} | `{}` |",
            row.rung,
            row.class,
            row.class.named_severity(),
            row.artifact.as_deref().unwrap_or("—"),
            row.first_difference,
            row.reason_path
        )
        .expect("report write");
    }
    writeln!(&mut output).expect("report write");
    writeln!(&mut output, "## Certificate files").expect("report write");
    writeln!(&mut output).expect("report write");
    if report.certificate.files.is_empty() && report.certificate.root_files.is_empty() {
        writeln!(
            &mut output,
            "No real certificate run directory was found; no `latest` symlink was followed."
        )
        .expect("report write");
    } else {
        writeln!(&mut output, "| file | size | digest |").expect("report write");
        writeln!(&mut output, "|---|---:|---|").expect("report write");
        for file in &report.certificate.root_files {
            writeln!(
                &mut output,
                "| `target/chain-walk/{}` | {} | `{}` |",
                file.path, file.size, file.digest
            )
            .expect("report write");
        }
        for file in &report.certificate.files {
            writeln!(
                &mut output,
                "| `{}` | {} | `{}` |",
                file.path, file.size, file.digest
            )
            .expect("report write");
        }
    }
    writeln!(&mut output).expect("report write");
    writeln!(&mut output, "## Projection digests").expect("report write");
    writeln!(&mut output).expect("report write");
    writeln!(
        &mut output,
        "- Actions: `{}`",
        report.projections.action_digest
    )
    .expect("report write");
    writeln!(
        &mut output,
        "- Manifests: `{}`",
        report.projections.manifest_digest
    )
    .expect("report write");
    writeln!(&mut output, "- Edges: `{}`", report.projections.edge_digest).expect("report write");
    writeln!(
        &mut output,
        "- Receipt keys: `{}`",
        report.projections.key_digest
    )
    .expect("report write");
    writeln!(
        &mut output,
        "- Comparators: `{}` (`canonical-json/v2` is a typed TODO seam)",
        report.projections.comparator_digest
    )
    .expect("report write");
    output
}

fn git_text(root: &Path, args: &[&str]) -> Result<String, DiscoveryError> {
    let output = Command::new("git").current_dir(root).args(args).output()?;
    if !output.status.success() {
        return Err(DiscoveryError::new(
            String::from_utf8_lossy(&output.stderr).trim(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn source_tree_digest(root: &Path) -> Digest {
    if let Ok(tree) = git_text(root, &["ls-tree", "-r", "--full-tree", "HEAD"]) {
        return sha256(tree.as_bytes());
    }
    digest_directory(root, |path| {
        path.starts_with(".git") || path.starts_with("target") || path.starts_with("new-ci/")
    })
}

fn crate_tree_digest(root: &Path) -> Digest {
    let mut files = Vec::new();
    collect_files_with_suffix(root, &root.join("crates"), ".rs", &mut files);
    files.sort_by(|left, right| byte_cmp(left.0.as_str(), right.0.as_str()));
    let mut inner = String::new();
    for (path, bytes) in files {
        let digest = sha256(&bytes);
        writeln!(&mut inner, "{}  {}", digest, path).expect("tree digest write");
    }
    sha256(inner.as_bytes())
}

fn collect_files_with_suffix(
    root: &Path,
    directory: &Path,
    suffix: &str,
    output: &mut Vec<(String, Vec<u8>)>,
) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by(|left, right| {
        byte_cmp(
            &left.file_name().to_string_lossy(),
            &right.file_name().to_string_lossy(),
        )
    });
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_files_with_suffix(root, &path, suffix, output);
        } else if path.is_file() && path.to_string_lossy().ends_with(suffix) {
            if let Ok(bytes) = fs::read(&path) {
                let relative = path
                    .strip_prefix(root)
                    .map_or_else(
                        |_| path.to_string_lossy().into_owned(),
                        |value| value.to_string_lossy().into_owned(),
                    )
                    .replace('\\', "/");
                output.push((relative, bytes));
            }
        }
    }
}

fn digest_directory<F>(root: &Path, exclude: F) -> Digest
where
    F: Fn(&str) -> bool + Copy,
{
    let mut files = Vec::new();
    collect_all_files(root, root, &exclude, &mut files);
    files.sort_by(|left, right| byte_cmp(left.0.as_str(), right.0.as_str()));
    let mut bytes = Vec::new();
    for (path, file_bytes) in files {
        put_string(&mut bytes, &path);
        put_digest(&mut bytes, sha256(&file_bytes));
    }
    sha256(&bytes)
}

fn collect_all_files<F>(
    root: &Path,
    directory: &Path,
    exclude: &F,
    output: &mut Vec<(String, Vec<u8>)>,
) where
    F: Fn(&str) -> bool + Copy,
{
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        if exclude(&relative) {
            continue;
        }
        if path.is_dir() {
            collect_all_files(root, &path, exclude, output);
        } else if path.is_file() {
            if let Ok(bytes) = fs::read(path) {
                output.push((relative, bytes));
            }
        }
    }
}

fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_string(output: &mut Vec<u8>, value: &str) {
    put_u64(output, value.len() as u64);
    output.extend_from_slice(value.as_bytes());
}

fn put_digest(output: &mut Vec<u8>, value: Digest) {
    output.extend_from_slice(value.as_bytes());
}

fn put_optional_digest(output: &mut Vec<u8>, value: Option<Digest>) {
    match value {
        Some(value) => {
            output.push(1);
            put_digest(output, value);
        }
        None => output.push(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_root(label: &str) -> PathBuf {
        let root = env::temp_dir().join(format!("new-ci-shadow-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/oracle")).expect("oracle directory");
        fs::create_dir_all(root.join("ratchets")).expect("ratchets directory");
        fs::create_dir_all(root.join("scripts")).expect("scripts directory");
        fs::write(root.join(".node-version"), "25.2.1\n").expect("node version");
        root
    }

    fn producer_text(target: &str, pin: &str) -> String {
        format!(
            "const TARGET_RELATIVE_PATH = \"{target}\";\nconst CONTRACT_RELATIVE_PATH = \".github/ci/contracts/example.schema.json\";\nconst PIN = [\"ratchets/h2-upstream.v1.json\", \"{pin}\"];\nconst mode = process.argv[2];\nif (mode === \"--check\") {{ process.stdout.write(\"ok\\n\"); }}\nimport \"./h2-helper.mjs\";\n"
        )
    }

    #[test]
    fn discovery_classifies_producers_sidecars_helpers_and_restricted() {
        let root = test_root("classification");
        let hash = "a".repeat(64);
        fs::write(
            root.join("crates/oracle/h2-producer.mjs"),
            producer_text("ratchets/h2-producer.v1.json", &hash),
        )
        .expect("producer");
        fs::write(
            root.join("crates/oracle/h2-owner-controls.mjs"),
            producer_text("ratchets/h2-owner-controls.v1.json", &hash),
        )
        .expect("sidecar");
        fs::write(
            root.join("crates/oracle/h2-baseline.mjs"),
            "const EVIDENCE_RELATIVE_PATH = \"ratchets/h2-baseline.v1.json\"; const mode = process.argv[2]; if (mode === \"--check\") {}",
        )
        .expect("restricted");
        fs::write(
            root.join("crates/oracle/h2-helper.mjs"),
            "export const helper = 1;\n",
        )
        .expect("helper");
        fs::write(root.join("ratchets/h2-producer.v1.json"), "{}\n").expect("producer artifact");
        fs::write(root.join("ratchets/h2-owner-controls.v1.json"), "{}\n")
            .expect("sidecar artifact");
        fs::write(root.join("ratchets/h2-baseline.v1.json"), "{}\n").expect("baseline artifact");
        fs::write(root.join("ratchets/h2-upstream.v1.json"), "{}\n").expect("upstream artifact");
        fs::write(
            root.join("scripts/chain-walk.sh"),
            "ORDER=(\n h2-producer\n)\n",
        )
        .expect("order");
        let discovery = discover(&root).expect("discovery");
        assert_eq!(
            discovery
                .script("crates/oracle/h2-producer.mjs")
                .unwrap()
                .class,
            ItemClass::Producer
        );
        assert_eq!(
            discovery
                .script("crates/oracle/h2-owner-controls.mjs")
                .unwrap()
                .class,
            ItemClass::CheckedSidecar
        );
        assert_eq!(
            discovery
                .script("crates/oracle/h2-baseline.mjs")
                .unwrap()
                .class,
            ItemClass::RestrictedProducer
        );
        assert_eq!(
            discovery
                .script("crates/oracle/h2-helper.mjs")
                .unwrap()
                .class,
            ItemClass::ImportedHelper
        );
        assert!(discovery
            .helpers
            .iter()
            .any(|helper| helper.path == "crates/oracle/h2-helper.mjs"));
        assert!(discovery
            .topological_order
            .iter()
            .any(|path| path.ends_with("h2-producer.mjs")));
    }

    #[test]
    fn projection_key_stability_separates_masked_core_from_pin_envelope() {
        let root = test_root("projection");
        let first = "a".repeat(64);
        let second = "b".repeat(64);
        let path = root.join("crates/oracle/h2-producer.mjs");
        fs::write(&path, producer_text("ratchets/h2-producer.v1.json", &first))
            .expect("first producer");
        fs::write(root.join("ratchets/h2-producer.v1.json"), "{}\n").expect("target");
        fs::write(root.join("ratchets/h2-upstream.v1.json"), "{}\n").expect("upstream");
        fs::write(
            root.join("scripts/chain-walk.sh"),
            "ORDER=( h2-producer )\n",
        )
        .expect("order");
        let context =
            ProjectionContext::new(&root, "commit", sha256(b"tree"), "25.2.1", "node-test");
        let first_projection = project(&discover(&root).expect("first discovery"), &context)
            .expect("first projection");
        fs::write(
            &path,
            producer_text("ratchets/h2-producer.v1.json", &second),
        )
        .expect("second producer");
        let second_projection = project(&discover(&root).expect("second discovery"), &context)
            .expect("second projection");
        let first = &first_projection.checks[0];
        let second = &second_projection.checks[0];
        assert_eq!(
            first.action.implementation_digest,
            second.action.implementation_digest
        );
        assert_ne!(first.pin_envelope, second.pin_envelope);
        assert_ne!(first.receipt_key, second.receipt_key);
        assert_eq!(first.action.tool, ACTION_TOOL);
        assert!(first
            .manifest
            .entries
            .iter()
            .any(|entry| entry.label.starts_with("checked-in-target:")));
    }

    #[test]
    fn report_writer_emits_complete_shape() {
        let root = test_root("report");
        fs::write(root.join("scripts/chain-walk.sh"), "ORDER=()\n").expect("order");
        let discovery = discover(&root).expect("discovery");
        let context =
            ProjectionContext::new(&root, "commit", sha256(b"tree"), "25.2.1", "node-test");
        let projections = project(&discovery, &context).expect("projection");
        let report = RunReport {
            run_id: "certificate-digest".to_string(),
            mode: "sample".to_string(),
            context,
            discovery,
            projections,
            certificate: CertificateSnapshot::default(),
            checks: Vec::new(),
            comparisons: vec![ComparisonRow {
                rung: "certificate".to_string(),
                artifact: None,
                class: ComparisonClass::MissingRung,
                left_digest: None,
                right_digest: None,
                first_difference: "missing".to_string(),
                reason_path: "certificate".to_string(),
                comparator: "byte/v1".to_string(),
                reproduction: "rerun".to_string(),
            }],
            selected: Vec::new(),
            completed: Vec::new(),
            skipped: Vec::new(),
            certificate_only: Vec::new(),
            budget: BudgetSummary {
                wall_cap_seconds: 600,
                max_invocations: 8,
                invocations: 0,
                expired: false,
            },
            read_bytes: 0,
            stdout_bytes: 0,
            stderr_bytes: 0,
            wall_time_ms: 1,
            max_child_concurrency: 1,
        };
        let output = root.join("target/new-ci-shadow");
        let written = write_report(&output, &report).expect("write report");
        let json = fs::read_to_string(written.json_path).expect("json");
        assert!(json.contains("\"schema\":\"new-ci-shadow-run/v1\""));
        assert!(json.contains("\"inventory\""));
        assert!(json.contains("\"projection\""));
        assert!(json.contains("\"certificate\""));
        assert!(json.contains("\"comparisons\""));
        assert!(json.contains("\"budget\""));
        assert!(json.contains("\"report_bytes\":"));
    }

    #[test]
    fn certificate_loader_chooses_newest_real_run_and_records_root_files() {
        let root = test_root("certificate");
        let runs = root.join("target/chain-walk/runs");
        fs::create_dir_all(runs.join("001")).expect("old run");
        fs::create_dir_all(runs.join("002")).expect("new run");
        fs::write(
            runs.join("001/summary.log"),
            "chain walk: converged and green\n",
        )
        .expect("old summary");
        fs::write(
            runs.join("002/summary.log"),
            "chain walk: converged and green\n",
        )
        .expect("new summary");
        fs::create_dir_all(root.join("target/chain-walk")).expect("chain-walk root");
        fs::write(root.join("target/chain-walk/converged-run-id"), "001\n").expect("marker");
        fs::write(
            root.join("target/chain-walk/converged-crates.sha256"),
            crate_tree_digest(&root).to_hex(),
        )
        .expect("crate tree record");
        let certificate = load_certificate(&root).expect("certificate");
        assert_eq!(certificate.certificate_id.as_deref(), Some("002"));
        assert_eq!(certificate.resolution, "newest-real-run-directory");
        assert_eq!(certificate.crate_tree_matches, Some(true));
        assert_eq!(certificate.root_files.len(), 2);
        assert!(certificate
            .files
            .iter()
            .any(|file| file.path == "summary.log"));
    }

    #[test]
    fn budget_skip_path_is_hard_and_recorded() {
        let mut budget = Budget::new(Duration::ZERO, 8);
        assert!(!budget.try_admit());
        let row = budget_skip_row(
            "h2-sample",
            Some("ratchets/h2-sample.v1.json".to_string()),
            "cap",
        );
        assert_eq!(row.class, ComparisonClass::BudgetSkip);
        assert_eq!(row.class.named_severity(), "INCOMPLETE");
        assert_eq!(budget.invocations(), 0);
    }
}

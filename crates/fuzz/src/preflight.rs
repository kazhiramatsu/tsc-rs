//! Report-only M9 preflight inventory.
//!
//! This module deliberately does not run either compiler, create fuzz
//! history, or change the M8 B3 producer. It loads the three reviewed,
//! workspace-relative M9.0 manifests, validates their source pins, and
//! derives the blockers that later slices must close.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Component, Path};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use toml_edit::{Array, DocumentMut, Item, Table};

pub const DOMAIN_MANIFEST_REL: &str = "ratchets/fuzz-domain.v1.toml";
pub const ORACLE_DEVIATIONS_REL: &str = "ratchets/fuzz-oracle-deviations.v1.json";
pub const PREFLIGHT_REPORT_REL: &str = "ratchets/fuzz-preflight.v1.json";

const SCHEMA: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManifestStatus {
    Draft,
}

impl ManifestStatus {
    fn parse(value: &str, context: &str) -> Result<Self, PreflightError> {
        match value {
            "draft" => Ok(Self::Draft),
            other => Err(PreflightError::new(format!(
                "{context} has unsupported manifest status {other:?}; report-only schema 1 accepts only draft"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreflightError {
    message: String,
}

impl PreflightError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for PreflightError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for PreflightError {}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CheckStatus {
    Ready,
    Pending,
    Unknown,
}

impl CheckStatus {
    fn parse(value: &str, context: &str) -> Result<Self, PreflightError> {
        match value {
            "ready" => Ok(Self::Ready),
            "pending" => Ok(Self::Pending),
            "unknown" => Ok(Self::Unknown),
            other => Err(PreflightError::new(format!(
                "{context} has unknown status {other:?}; expected ready, pending, or unknown"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Pending => "pending",
            Self::Unknown => "unknown",
        }
    }

    fn is_blocker(self) -> bool {
        self != Self::Ready
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InventoryCheck {
    pub id: String,
    pub status: CheckStatus,
    pub detail: String,
    pub blocks: Vec<String>,
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainBranch {
    pub check: InventoryCheck,
    pub role: DomainBranchRole,
    pub witness_seed: Option<u64>,
    pub witness_case: Option<u64>,
    pub strata: Vec<String>,
    pub script_kinds: Vec<String>,
    pub topologies: Vec<String>,
    pub options: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomainBranchRole {
    LegacySmokeOnly,
    Production,
}

impl DomainBranchRole {
    fn parse(value: &str, context: &str) -> Result<Self, PreflightError> {
        match value {
            "legacy-smoke-only" => Ok(Self::LegacySmokeOnly),
            "production" => Ok(Self::Production),
            other => Err(PreflightError::new(format!(
                "{context} has unknown domain branch role {other:?}; expected legacy-smoke-only or production"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::LegacySmokeOnly => "legacy-smoke-only",
            Self::Production => "production",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainManifest {
    pub status: ManifestStatus,
    pub source_references: Vec<SourceReference>,
    pub branches: Vec<DomainBranch>,
    pub requirements: Vec<InventoryCheck>,
    pub checks: Vec<InventoryCheck>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleDeviation {
    pub check: InventoryCheck,
    pub source_contract: String,
    pub input_sha256: Option<String>,
    pub oracle_outcome_sha256: Option<String>,
    pub rust_outcome_sha256: Option<String>,
    pub positive_canary_sha256: Option<String>,
    pub adjacent_negative_canary_sha256: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleDeviationManifest {
    pub status: ManifestStatus,
    pub source_references: Vec<SourceReference>,
    pub deviations: Vec<OracleDeviation>,
    pub checks: Vec<InventoryCheck>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreflightReportManifest {
    pub status: ManifestStatus,
    pub source_references: Vec<SourceReference>,
    pub checks: Vec<InventoryCheck>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceReference {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreflightBlocker {
    pub id: String,
    pub status: CheckStatus,
    pub detail: String,
    pub blocks: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreflightSummary {
    pub ready: bool,
    pub total_checks: usize,
    pub ready_checks: usize,
    pub pending_checks: usize,
    pub unknown_checks: usize,
    pub blockers: Vec<PreflightBlocker>,
}

impl PreflightSummary {
    pub fn render_text(&self) -> String {
        let mut output = format!(
            "M9 preflight: ready={} checks={} ready={} pending={} unknown={}",
            self.ready,
            self.total_checks,
            self.ready_checks,
            self.pending_checks,
            self.unknown_checks
        );
        for blocker in &self.blockers {
            output.push('\n');
            output.push('[');
            output.push_str(blocker.status.as_str());
            output.push_str("] ");
            output.push_str(&blocker.id);
            output.push_str(": ");
            output.push_str(&blocker.detail);
            output.push_str(" (blocks: ");
            output.push_str(&blocker.blocks.join(", "));
            output.push(')');
        }
        output
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreflightInventory {
    domain: DomainManifest,
    oracle_deviations: OracleDeviationManifest,
    preflight_report: PreflightReportManifest,
    summary: PreflightSummary,
}

impl PreflightInventory {
    pub fn domain(&self) -> &DomainManifest {
        &self.domain
    }

    pub fn oracle_deviations(&self) -> &OracleDeviationManifest {
        &self.oracle_deviations
    }

    pub fn preflight_report(&self) -> &PreflightReportManifest {
        &self.preflight_report
    }

    pub fn summary(&self) -> &PreflightSummary {
        &self.summary
    }

    pub fn is_ready(&self) -> bool {
        self.summary.ready
    }

    pub fn blocker_ids(&self) -> impl Iterator<Item = &str> {
        self.summary
            .blockers
            .iter()
            .map(|blocker| blocker.id.as_str())
    }

    pub fn require_ready(&self) -> Result<(), PreflightNotReady> {
        if self.is_ready() {
            return Ok(());
        }
        Err(PreflightNotReady {
            blocker_ids: self.blocker_ids().map(str::to_owned).collect(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreflightNotReady {
    blocker_ids: Vec<String>,
}

impl PreflightNotReady {
    pub fn blocker_ids(&self) -> impl Iterator<Item = &str> {
        self.blocker_ids.iter().map(String::as_str)
    }
}

impl fmt::Display for PreflightNotReady {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "M9 preflight is not ready; blockers: {}",
            self.blocker_ids.join(", ")
        )
    }
}

impl Error for PreflightNotReady {}

/// Load and verify the three fixed M9.0a manifests below `workspace`.
///
/// Missing/malformed manifests, unsupported schema/status values, duplicate
/// identities, unsafe reference paths, and source hash drift are operational
/// errors. `pending` and `unknown` checks are valid report data; they become
/// blockers in [`PreflightSummary`] and fail only [`PreflightInventory::require_ready`].
pub fn load_preflight_inventory(
    workspace: impl AsRef<Path>,
) -> Result<PreflightInventory, PreflightError> {
    let workspace = workspace.as_ref();
    let domain_path = workspace.join(DOMAIN_MANIFEST_REL);
    let oracle_path = workspace.join(ORACLE_DEVIATIONS_REL);
    let report_path = workspace.join(PREFLIGHT_REPORT_REL);

    let domain = load_domain_manifest(&domain_path)?;
    let oracle_deviations = load_oracle_manifest(&oracle_path)?;
    let preflight_report = load_report_manifest(&report_path)?;

    for (name, references) in [
        (DOMAIN_MANIFEST_REL, domain.source_references.as_slice()),
        (
            ORACLE_DEVIATIONS_REL,
            oracle_deviations.source_references.as_slice(),
        ),
        (
            PREFLIGHT_REPORT_REL,
            preflight_report.source_references.as_slice(),
        ),
    ] {
        verify_source_references(workspace, name, references)?;
    }

    let manifest_status_checks = [
        derived_manifest_status_check("manifest.domain.status", domain.status),
        derived_manifest_status_check(
            "manifest.oracle-deviations.status",
            oracle_deviations.status,
        ),
        derived_manifest_status_check("manifest.preflight.status", preflight_report.status),
    ];
    let all_checks = manifest_status_checks
        .iter()
        .chain(domain.checks.iter())
        .chain(domain.branches.iter().map(|branch| &branch.check))
        .chain(domain.requirements.iter())
        .chain(
            oracle_deviations
                .deviations
                .iter()
                .map(|deviation| &deviation.check),
        )
        .chain(oracle_deviations.checks.iter())
        .chain(preflight_report.checks.iter())
        .collect::<Vec<_>>();
    if all_checks.is_empty() {
        return Err(PreflightError::new(
            "M9 preflight manifests contain no inventory checks",
        ));
    }
    let mut ids = BTreeSet::new();
    for check in &all_checks {
        validate_check(check)?;
        if !ids.insert(check.id.as_str()) {
            return Err(PreflightError::new(format!(
                "M9 preflight check id {:?} is duplicated across manifests",
                check.id
            )));
        }
    }

    for branch in &domain.branches {
        validate_domain_branch(branch)?;
    }
    for deviation in &oracle_deviations.deviations {
        validate_oracle_deviation(deviation)?;
    }

    let ready_checks = all_checks
        .iter()
        .filter(|check| check.status == CheckStatus::Ready)
        .count();
    let pending_checks = all_checks
        .iter()
        .filter(|check| check.status == CheckStatus::Pending)
        .count();
    let unknown_checks = all_checks
        .iter()
        .filter(|check| check.status == CheckStatus::Unknown)
        .count();
    let blockers = all_checks
        .into_iter()
        .filter(|check| check.status.is_blocker())
        .map(|check| PreflightBlocker {
            id: check.id.clone(),
            status: check.status,
            detail: check.detail.clone(),
            blocks: check.blocks.clone(),
        })
        .collect::<Vec<_>>();
    let summary = PreflightSummary {
        ready: blockers.is_empty(),
        total_checks: ready_checks + pending_checks + unknown_checks,
        ready_checks,
        pending_checks,
        unknown_checks,
        blockers,
    };

    Ok(PreflightInventory {
        domain,
        oracle_deviations,
        preflight_report,
        summary,
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct JsonSourceReference {
    path: String,
    sha256: String,
}

impl From<JsonSourceReference> for SourceReference {
    fn from(reference: JsonSourceReference) -> Self {
        Self {
            path: reference.path,
            sha256: reference.sha256,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct JsonCheck {
    id: String,
    status: String,
    detail: String,
    #[serde(default)]
    blocks: Vec<String>,
    #[serde(default)]
    evidence: Vec<String>,
}

impl JsonCheck {
    fn into_check(self, context: &str) -> Result<InventoryCheck, PreflightError> {
        Ok(InventoryCheck {
            id: self.id,
            status: CheckStatus::parse(&self.status, context)?,
            detail: self.detail,
            blocks: self.blocks,
            evidence: self.evidence,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct JsonOracleManifest {
    schema: u32,
    status: String,
    #[serde(default)]
    source_references: Vec<JsonSourceReference>,
    deviations: Vec<JsonOracleDeviation>,
    #[serde(default)]
    checks: Vec<JsonCheck>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct JsonOracleDeviation {
    id: String,
    status: String,
    detail: String,
    #[serde(default)]
    blocks: Vec<String>,
    #[serde(default)]
    evidence: Vec<String>,
    source_contract: String,
    #[serde(default)]
    input_sha256: Option<String>,
    #[serde(default)]
    oracle_outcome_sha256: Option<String>,
    #[serde(default)]
    rust_outcome_sha256: Option<String>,
    #[serde(default)]
    positive_canary_sha256: Option<String>,
    #[serde(default)]
    adjacent_negative_canary_sha256: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct JsonPreflightReport {
    schema: u32,
    status: String,
    #[serde(default)]
    source_references: Vec<JsonSourceReference>,
    checks: Vec<JsonCheck>,
}

fn load_oracle_manifest(path: &Path) -> Result<OracleDeviationManifest, PreflightError> {
    let raw: JsonOracleManifest = read_json(path)?;
    let status = validate_header(path, raw.schema, &raw.status)?;
    let deviations = raw
        .deviations
        .into_iter()
        .map(|deviation| {
            let context = format!("oracle deviation {:?}", deviation.id);
            let check = JsonCheck {
                id: deviation.id,
                status: deviation.status,
                detail: deviation.detail,
                blocks: deviation.blocks,
                evidence: deviation.evidence,
            }
            .into_check(&context)?;
            Ok(OracleDeviation {
                check,
                source_contract: deviation.source_contract,
                input_sha256: deviation.input_sha256,
                oracle_outcome_sha256: deviation.oracle_outcome_sha256,
                rust_outcome_sha256: deviation.rust_outcome_sha256,
                positive_canary_sha256: deviation.positive_canary_sha256,
                adjacent_negative_canary_sha256: deviation.adjacent_negative_canary_sha256,
            })
        })
        .collect::<Result<Vec<_>, PreflightError>>()?;
    let checks = raw
        .checks
        .into_iter()
        .map(|check| {
            let context = format!("oracle manifest check {:?}", check.id);
            check.into_check(&context)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(OracleDeviationManifest {
        status,
        source_references: raw.source_references.into_iter().map(Into::into).collect(),
        deviations,
        checks,
    })
}

fn load_report_manifest(path: &Path) -> Result<PreflightReportManifest, PreflightError> {
    let raw: JsonPreflightReport = read_json(path)?;
    let status = validate_header(path, raw.schema, &raw.status)?;
    let checks = raw
        .checks
        .into_iter()
        .map(|check| {
            let context = format!("preflight check {:?}", check.id);
            check.into_check(&context)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PreflightReportManifest {
        status,
        source_references: raw.source_references.into_iter().map(Into::into).collect(),
        checks,
    })
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, PreflightError> {
    let bytes = fs::read(path).map_err(|error| {
        PreflightError::new(format!(
            "cannot read M9 preflight manifest {}: {error}",
            path.display()
        ))
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        PreflightError::new(format!(
            "malformed M9 preflight manifest {}: {error}",
            path.display()
        ))
    })
}

fn load_domain_manifest(path: &Path) -> Result<DomainManifest, PreflightError> {
    let text = fs::read_to_string(path).map_err(|error| {
        PreflightError::new(format!(
            "cannot read M9 preflight manifest {}: {error}",
            path.display()
        ))
    })?;
    let document = text.parse::<DocumentMut>().map_err(|error| {
        PreflightError::new(format!(
            "malformed M9 preflight manifest {}: {error}",
            path.display()
        ))
    })?;
    reject_unknown_keys(
        document.as_table(),
        &[
            "schema",
            "status",
            "source_references",
            "branches",
            "requirements",
            "checks",
        ],
        &path.display().to_string(),
    )?;
    let schema =
        u32::try_from(required_integer(document.as_table(), "schema", path)?).map_err(|_| {
            PreflightError::new(format!(
                "{}.schema must be an unsigned 32-bit integer",
                path.display()
            ))
        })?;
    let status = required_string(document.as_table(), "status", path)?;
    let status = validate_header(path, schema, status)?;

    let source_references =
        optional_tables(document.get("source_references"), path, "source_references")?
            .iter()
            .enumerate()
            .map(|(index, table)| {
                let context = format!("{} source_references[{index}]", path.display());
                reject_unknown_keys(table, &["path", "sha256"], &context)?;
                Ok(SourceReference {
                    path: required_table_string(table, "path", &context)?.to_owned(),
                    sha256: required_table_string(table, "sha256", &context)?.to_owned(),
                })
            })
            .collect::<Result<Vec<_>, PreflightError>>()?;

    let branches = required_tables(document.get("branches"), "branches", path)?
        .iter()
        .enumerate()
        .map(|(index, table)| parse_domain_branch(table, path, index))
        .collect::<Result<Vec<_>, _>>()?;
    let requirements = required_tables(document.get("requirements"), "requirements", path)?
        .iter()
        .enumerate()
        .map(|(index, table)| {
            parse_toml_check(
                table,
                &format!("{} requirements[{index}]", path.display()),
                &["id", "status", "detail", "blocks", "evidence"],
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let checks = optional_tables(document.get("checks"), path, "checks")?
        .iter()
        .enumerate()
        .map(|(index, table)| {
            parse_toml_check(
                table,
                &format!("{} checks[{index}]", path.display()),
                &["id", "status", "detail", "blocks", "evidence"],
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(DomainManifest {
        status,
        source_references,
        branches,
        requirements,
        checks,
    })
}

fn parse_domain_branch(
    table: &Table,
    path: &Path,
    index: usize,
) -> Result<DomainBranch, PreflightError> {
    let context = format!("{} branches[{index}]", path.display());
    reject_unknown_keys(
        table,
        &[
            "id",
            "status",
            "detail",
            "blocks",
            "evidence",
            "role",
            "witness_seed",
            "witness_case",
            "strata",
            "script_kinds",
            "topologies",
            "options",
        ],
        &context,
    )?;
    let check = parse_toml_check(
        table,
        &context,
        &[
            "id",
            "status",
            "detail",
            "blocks",
            "evidence",
            "role",
            "witness_seed",
            "witness_case",
            "strata",
            "script_kinds",
            "topologies",
            "options",
        ],
    )?;
    let role = DomainBranchRole::parse(required_table_string(table, "role", &context)?, &context)?;
    let witness_seed = table
        .get("witness_seed")
        .map(|item| {
            item.as_integer()
                .and_then(|value| u64::try_from(value).ok())
                .ok_or_else(|| {
                    PreflightError::new(format!(
                        "{context}.witness_seed must be a non-negative integer"
                    ))
                })
        })
        .transpose()?;
    let witness_case = table
        .get("witness_case")
        .map(|item| {
            item.as_integer()
                .and_then(|value| u64::try_from(value).ok())
                .ok_or_else(|| {
                    PreflightError::new(format!(
                        "{context}.witness_case must be a non-negative integer"
                    ))
                })
        })
        .transpose()?;
    Ok(DomainBranch {
        check,
        role,
        witness_seed,
        witness_case,
        strata: optional_string_array(table.get("strata"), &context, "strata")?,
        script_kinds: optional_string_array(table.get("script_kinds"), &context, "script_kinds")?,
        topologies: optional_string_array(table.get("topologies"), &context, "topologies")?,
        options: optional_string_array(table.get("options"), &context, "options")?,
    })
}

fn parse_toml_check(
    table: &Table,
    context: &str,
    allowed: &[&str],
) -> Result<InventoryCheck, PreflightError> {
    reject_unknown_keys(table, allowed, context)?;
    let id = required_table_string(table, "id", context)?.to_owned();
    let status = CheckStatus::parse(required_table_string(table, "status", context)?, context)?;
    let detail = required_table_string(table, "detail", context)?.to_owned();
    Ok(InventoryCheck {
        id,
        status,
        detail,
        blocks: optional_string_array(table.get("blocks"), context, "blocks")?,
        evidence: optional_string_array(table.get("evidence"), context, "evidence")?,
    })
}

fn required_tables<'a>(
    item: Option<&'a Item>,
    key: &str,
    path: &Path,
) -> Result<&'a toml_edit::ArrayOfTables, PreflightError> {
    item.and_then(Item::as_array_of_tables)
        .ok_or_else(|| {
            PreflightError::new(format!(
                "{} requires a non-empty [[{key}]] array",
                path.display()
            ))
        })
        .and_then(|tables| {
            if tables.is_empty() {
                Err(PreflightError::new(format!(
                    "{} requires a non-empty [[{key}]] array",
                    path.display()
                )))
            } else {
                Ok(tables)
            }
        })
}

fn optional_tables<'a>(
    item: Option<&'a Item>,
    path: &Path,
    key: &str,
) -> Result<Vec<&'a Table>, PreflightError> {
    match item {
        Some(item) => item
            .as_array_of_tables()
            .map(|tables| tables.iter().collect())
            .ok_or_else(|| {
                PreflightError::new(format!(
                    "{}.{key} must be an array of tables",
                    path.display()
                ))
            }),
        None => Ok(Vec::new()),
    }
}

fn required_integer(table: &Table, key: &str, path: &Path) -> Result<i64, PreflightError> {
    table.get(key).and_then(Item::as_integer).ok_or_else(|| {
        PreflightError::new(format!("{}.{} must be an integer", path.display(), key))
    })
}

fn required_string<'a>(
    table: &'a Table,
    key: &str,
    path: &Path,
) -> Result<&'a str, PreflightError> {
    table
        .get(key)
        .and_then(Item::as_str)
        .ok_or_else(|| PreflightError::new(format!("{}.{} must be a string", path.display(), key)))
}

fn required_table_string<'a>(
    table: &'a Table,
    key: &str,
    context: &str,
) -> Result<&'a str, PreflightError> {
    table
        .get(key)
        .and_then(Item::as_str)
        .ok_or_else(|| PreflightError::new(format!("{context}.{key} must be a string")))
}

fn optional_string_array(
    item: Option<&Item>,
    context: &str,
    key: &str,
) -> Result<Vec<String>, PreflightError> {
    match item {
        None => Ok(Vec::new()),
        Some(item) => {
            let values = item.as_array().ok_or_else(|| {
                PreflightError::new(format!("{context}.{key} must be an array of strings"))
            })?;
            string_array(values, context, key)
        }
    }
}

fn string_array(values: &Array, context: &str, key: &str) -> Result<Vec<String>, PreflightError> {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            value.as_str().map(str::to_owned).ok_or_else(|| {
                PreflightError::new(format!("{context}.{key}[{index}] must be a string"))
            })
        })
        .collect()
}

fn reject_unknown_keys(
    table: &Table,
    allowed: &[&str],
    context: &str,
) -> Result<(), PreflightError> {
    for (key, _) in table.iter() {
        if !allowed.contains(&key) {
            return Err(PreflightError::new(format!(
                "{context} contains unknown schema-1 key {key:?}"
            )));
        }
    }
    Ok(())
}

fn validate_header(
    path: &Path,
    schema: u32,
    status: &str,
) -> Result<ManifestStatus, PreflightError> {
    if schema != SCHEMA {
        return Err(PreflightError::new(format!(
            "{} has unsupported schema {schema}; expected {SCHEMA}",
            path.display()
        )));
    }
    ManifestStatus::parse(status, &path.display().to_string())
}

fn derived_manifest_status_check(id: &str, status: ManifestStatus) -> InventoryCheck {
    InventoryCheck {
        id: id.to_owned(),
        status: CheckStatus::Pending,
        detail: "manifest remains draft".to_owned(),
        blocks: vec!["M9.5-fingerprint-freeze".to_owned()],
        evidence: vec![format!("status={}", status.as_str())],
    }
}

fn validate_check(check: &InventoryCheck) -> Result<(), PreflightError> {
    if check.id.trim().is_empty() {
        return Err(PreflightError::new(
            "M9 preflight check id must not be empty",
        ));
    }
    if check.detail.trim().is_empty() {
        return Err(PreflightError::new(format!(
            "M9 preflight check {:?} has an empty detail",
            check.id
        )));
    }
    validate_nonempty_unique_strings(&check.blocks, &format!("{} blocks", check.id))?;
    validate_nonempty_unique_strings(&check.evidence, &format!("{} evidence", check.id))?;
    if check.status == CheckStatus::Ready && !check.blocks.is_empty() {
        return Err(PreflightError::new(format!(
            "ready M9 preflight check {:?} must not block a later slice",
            check.id
        )));
    }
    if check.status.is_blocker() && check.blocks.is_empty() {
        return Err(PreflightError::new(format!(
            "{} M9 preflight check {:?} must name the slice it blocks",
            check.status.as_str(),
            check.id
        )));
    }
    Ok(())
}

fn validate_domain_branch(branch: &DomainBranch) -> Result<(), PreflightError> {
    for (name, values) in [
        ("strata", branch.strata.as_slice()),
        ("script_kinds", branch.script_kinds.as_slice()),
        ("topologies", branch.topologies.as_slice()),
        ("options", branch.options.as_slice()),
    ] {
        validate_nonempty_unique_strings(values, &format!("{} {name}", branch.check.id))?;
    }
    if branch.check.status == CheckStatus::Ready {
        if branch.witness_seed.is_none() || branch.witness_case.is_none() {
            return Err(PreflightError::new(format!(
                "ready domain branch {:?} lacks witness_seed or witness_case",
                branch.check.id
            )));
        }
        for (name, values) in [
            ("strata", branch.strata.as_slice()),
            ("script_kinds", branch.script_kinds.as_slice()),
            ("topologies", branch.topologies.as_slice()),
            ("options", branch.options.as_slice()),
        ] {
            if values.is_empty() {
                return Err(PreflightError::new(format!(
                    "ready domain branch {:?} has no {name}; unknown coverage must not be encoded as an empty ready value",
                    branch.check.id
                )));
            }
        }
    }
    Ok(())
}

fn validate_oracle_deviation(deviation: &OracleDeviation) -> Result<(), PreflightError> {
    if deviation.source_contract.trim().is_empty() {
        return Err(PreflightError::new(format!(
            "oracle deviation {:?} has an empty source_contract",
            deviation.check.id
        )));
    }
    for (name, hash) in [
        ("input_sha256", deviation.input_sha256.as_deref()),
        (
            "oracle_outcome_sha256",
            deviation.oracle_outcome_sha256.as_deref(),
        ),
        (
            "rust_outcome_sha256",
            deviation.rust_outcome_sha256.as_deref(),
        ),
        (
            "positive_canary_sha256",
            deviation.positive_canary_sha256.as_deref(),
        ),
        (
            "adjacent_negative_canary_sha256",
            deviation.adjacent_negative_canary_sha256.as_deref(),
        ),
    ] {
        if let Some(hash) = hash {
            validate_sha256(hash, &format!("{} {name}", deviation.check.id))?;
        }
    }
    if deviation.check.status == CheckStatus::Ready
        && [
            deviation.input_sha256.as_ref(),
            deviation.oracle_outcome_sha256.as_ref(),
            deviation.rust_outcome_sha256.as_ref(),
            deviation.positive_canary_sha256.as_ref(),
            deviation.adjacent_negative_canary_sha256.as_ref(),
        ]
        .iter()
        .any(|hash| hash.is_none())
    {
        return Err(PreflightError::new(format!(
            "ready oracle deviation {:?} lacks an exact input/outcome/canary hash; unknown evidence must not be encoded as ready",
            deviation.check.id
        )));
    }
    Ok(())
}

fn validate_nonempty_unique_strings(
    values: &[String],
    context: &str,
) -> Result<(), PreflightError> {
    let mut seen = BTreeSet::new();
    for value in values {
        if value.trim().is_empty() {
            return Err(PreflightError::new(format!(
                "{context} contains an empty value"
            )));
        }
        if !seen.insert(value) {
            return Err(PreflightError::new(format!(
                "{context} contains duplicate value {value:?}"
            )));
        }
    }
    Ok(())
}

fn verify_source_references(
    workspace: &Path,
    manifest: &str,
    references: &[SourceReference],
) -> Result<(), PreflightError> {
    if references.is_empty() {
        return Err(PreflightError::new(format!(
            "{manifest} requires at least one source reference"
        )));
    }
    let canonical_workspace = workspace.canonicalize().map_err(|error| {
        PreflightError::new(format!(
            "cannot canonicalize M9 preflight workspace {}: {error}",
            workspace.display()
        ))
    })?;
    let mut paths = BTreeSet::new();
    for reference in references {
        validate_reference_path(&reference.path, manifest)?;
        validate_sha256(
            &reference.sha256,
            &format!("{manifest} reference {}", reference.path),
        )?;
        let path = workspace.join(&reference.path);
        let canonical_path = path.canonicalize().map_err(|error| {
            PreflightError::new(format!(
                "{manifest} source reference {} is missing/unreadable: {error}",
                reference.path
            ))
        })?;
        if !canonical_path.starts_with(&canonical_workspace) || !canonical_path.is_file() {
            return Err(PreflightError::new(format!(
                "{manifest} source reference {} escapes the workspace or is not a file",
                reference.path
            )));
        }
        if !paths.insert(canonical_path.clone()) {
            return Err(PreflightError::new(format!(
                "{manifest} repeats source reference {:?}",
                reference.path
            )));
        }
        let actual = sha256_file(&canonical_path)?;
        if actual != reference.sha256 {
            return Err(PreflightError::new(format!(
                "{manifest} source reference {} hash mismatch: expected {}, actual {}",
                reference.path, reference.sha256, actual
            )));
        }
    }
    Ok(())
}

fn validate_reference_path(path: &str, manifest: &str) -> Result<(), PreflightError> {
    let candidate = Path::new(path);
    if path.is_empty()
        || candidate.is_absolute()
        || candidate
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(PreflightError::new(format!(
            "{manifest} has unsafe source reference path {path:?}"
        )));
    }
    Ok(())
}

fn validate_sha256(value: &str, context: &str) -> Result<(), PreflightError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(PreflightError::new(format!(
            "{context} must be a lowercase 64-character SHA-256"
        )));
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, PreflightError> {
    let bytes = fs::read(path).map_err(|error| {
        PreflightError::new(format!(
            "cannot read M9 preflight source reference {}: {error}",
            path.display()
        ))
    })?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestWorkspace {
        root: PathBuf,
    }

    impl TestWorkspace {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(1);
            let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "tsc-rs-fuzz-preflight-{}-{timestamp}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(root.join("ratchets")).unwrap();
            fs::create_dir_all(root.join("src")).unwrap();
            fs::write(root.join("src/input.txt"), "pinned\n").unwrap();
            Self { root }
        }

        fn pinned_hash(&self) -> String {
            sha256_file(&self.root.join("src/input.txt")).unwrap()
        }

        fn write_valid_manifests(
            &self,
            manifest_status: &str,
            branch_status: &str,
            deviation_status: &str,
        ) {
            let hash = self.pinned_hash();
            let branch_blocks = if branch_status == "ready" {
                "[]"
            } else {
                "[\"M9.1\"]"
            };
            let deviation_blocks = if deviation_status == "ready" {
                "[]"
            } else {
                "[\"M9.1\"]"
            };
            let exact_hashes = if deviation_status == "ready" {
                format!(
                    r#"
    ,"input_sha256": "{hash}"
    ,"oracle_outcome_sha256": "{hash}"
    ,"rust_outcome_sha256": "{hash}"
    ,"positive_canary_sha256": "{hash}"
    ,"adjacent_negative_canary_sha256": "{hash}""#
                )
            } else {
                String::new()
            };
            let domain = format!(
                r#"schema = 1
status = "{manifest_status}"

[[source_references]]
path = "src/input.txt"
sha256 = "{hash}"

[[branches]]
id = "domain.branch.assignment"
status = "{branch_status}"
detail = "legacy assignment branch"
blocks = {branch_blocks}
evidence = ["generated_source arm 0"]
role = "legacy-smoke-only"
witness_seed = 1
witness_case = 0
strata = ["relations"]
script_kinds = ["ts"]
topologies = ["single-file"]
options = ["default"]

[[requirements]]
id = "domain.requirement.syntax"
status = "ready"
detail = "syntax stratum is inventoried"
blocks = []
evidence = ["manifest"]
"#
            );
            fs::write(self.root.join(DOMAIN_MANIFEST_REL), domain).unwrap();
            let oracle = format!(
                r#"{{
  "schema": 1,
  "status": "{manifest_status}",
  "source_references": [{{"path":"src/input.txt","sha256":"{hash}"}}],
  "deviations": [{{
    "id": "oracle.deviation.async-sync",
    "status": "{deviation_status}",
    "detail": "recorded M8 crash shape",
    "blocks": {deviation_blocks},
    "evidence": ["m8-readiness row 1"],
    "source_contract": "m8-readiness.md#recorded"{exact_hashes}
  }}]
}}"#
            );
            fs::write(self.root.join(ORACLE_DEVIATIONS_REL), oracle).unwrap();
            let report = format!(
                r#"{{
  "schema": 1,
  "status": "{manifest_status}",
  "source_references": [{{"path":"src/input.txt","sha256":"{hash}"}}],
  "checks": [{{
    "id": "preflight.true-replay",
    "status": "ready",
    "detail": "replay surface inventoried",
    "blocks": [],
    "evidence": ["audit"]
  }}]
}}"#
            );
            fs::write(self.root.join(PREFLIGHT_REPORT_REL), report).unwrap();
        }
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn pending_and_unknown_are_reported_without_becoming_ready() {
        let workspace = TestWorkspace::new();
        workspace.write_valid_manifests("draft", "pending", "unknown");

        let inventory = load_preflight_inventory(&workspace.root).unwrap();
        assert!(!inventory.is_ready());
        assert_eq!(inventory.summary().total_checks, 7);
        assert_eq!(inventory.summary().ready_checks, 2);
        assert_eq!(inventory.summary().pending_checks, 4);
        assert_eq!(inventory.summary().unknown_checks, 1);
        assert_eq!(
            inventory.blocker_ids().collect::<Vec<_>>(),
            [
                "manifest.domain.status",
                "manifest.oracle-deviations.status",
                "manifest.preflight.status",
                "domain.branch.assignment",
                "oracle.deviation.async-sync"
            ]
        );
        let rendered = inventory.summary().render_text();
        assert!(rendered.contains("[pending] domain.branch.assignment"));
        assert!(rendered.contains("[unknown] oracle.deviation.async-sync"));
        assert!(inventory.require_ready().is_err());
    }

    #[test]
    fn all_rows_ready_still_block_while_manifests_are_draft() {
        let workspace = TestWorkspace::new();
        workspace.write_valid_manifests("draft", "ready", "ready");

        let inventory = load_preflight_inventory(&workspace.root).unwrap();
        assert!(!inventory.is_ready());
        assert!(inventory.require_ready().is_err());
        assert_eq!(
            inventory.blocker_ids().collect::<Vec<_>>(),
            [
                "manifest.domain.status",
                "manifest.oracle-deviations.status",
                "manifest.preflight.status",
            ]
        );
    }

    #[test]
    fn frozen_is_unsupported_in_report_only_schema_one() {
        let workspace = TestWorkspace::new();
        workspace.write_valid_manifests("frozen", "ready", "ready");

        let error = load_preflight_inventory(&workspace.root)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("report-only schema 1 accepts only draft"),
            "{error}"
        );
    }

    #[test]
    fn unknown_domain_values_cannot_hide_in_an_empty_ready_branch() {
        let workspace = TestWorkspace::new();
        workspace.write_valid_manifests("draft", "ready", "ready");
        let path = workspace.root.join(DOMAIN_MANIFEST_REL);
        let text = fs::read_to_string(&path)
            .unwrap()
            .replace("strata = [\"relations\"]", "strata = []");
        fs::write(path, text).unwrap();

        let error = load_preflight_inventory(&workspace.root)
            .unwrap_err()
            .to_string();
        assert!(error.contains("has no strata"), "{error}");
    }

    #[test]
    fn ready_oracle_deviation_requires_every_exact_hash() {
        let workspace = TestWorkspace::new();
        workspace.write_valid_manifests("draft", "ready", "ready");
        let path = workspace.root.join(ORACLE_DEVIATIONS_REL);
        let text = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .filter(|line| !line.contains("\"positive_canary_sha256\""))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(path, text).unwrap();

        let error = load_preflight_inventory(&workspace.root)
            .unwrap_err()
            .to_string();
        assert!(error.contains("lacks an exact input/outcome/canary hash"));
    }

    #[test]
    fn source_reference_hash_drift_is_an_operational_error() {
        let workspace = TestWorkspace::new();
        workspace.write_valid_manifests("draft", "pending", "unknown");
        fs::write(workspace.root.join("src/input.txt"), "drift\n").unwrap();

        let error = load_preflight_inventory(&workspace.root)
            .unwrap_err()
            .to_string();
        assert!(error.contains("hash mismatch"), "{error}");
    }

    #[test]
    fn malformed_missing_and_unknown_status_are_operational_errors() {
        let workspace = TestWorkspace::new();
        workspace.write_valid_manifests("draft", "pending", "unknown");
        let report_path = workspace.root.join(PREFLIGHT_REPORT_REL);
        fs::write(&report_path, "{").unwrap();
        assert!(load_preflight_inventory(&workspace.root)
            .unwrap_err()
            .to_string()
            .contains("malformed"));

        workspace.write_valid_manifests("draft", "pending", "unknown");
        fs::remove_file(&report_path).unwrap();
        assert!(load_preflight_inventory(&workspace.root)
            .unwrap_err()
            .to_string()
            .contains("cannot read"));

        workspace.write_valid_manifests("draft", "pending", "unknown");
        let text = fs::read_to_string(&report_path)
            .unwrap()
            .replace("\"status\": \"ready\"", "\"status\": \"waived\"");
        fs::write(&report_path, text).unwrap();
        let error = load_preflight_inventory(&workspace.root)
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown status"), "{error}");

        workspace.write_valid_manifests("draft", "pending", "unknown");
        let text = fs::read_to_string(&report_path)
            .unwrap()
            .replace("\"status\": \"draft\"", "\"status\": \"reviewed\"");
        fs::write(&report_path, text).unwrap();
        let error = load_preflight_inventory(&workspace.root)
            .unwrap_err()
            .to_string();
        assert!(error.contains("unsupported manifest status"), "{error}");
    }

    #[test]
    fn duplicate_check_ids_across_manifests_are_rejected() {
        let workspace = TestWorkspace::new();
        workspace.write_valid_manifests("draft", "pending", "unknown");
        let path = workspace.root.join(PREFLIGHT_REPORT_REL);
        let text = fs::read_to_string(&path)
            .unwrap()
            .replace("preflight.true-replay", "domain.branch.assignment");
        fs::write(path, text).unwrap();

        let error = load_preflight_inventory(&workspace.root)
            .unwrap_err()
            .to_string();
        assert!(error.contains("duplicated across manifests"), "{error}");
    }

    #[test]
    fn schema_extensions_and_wrapping_schema_values_are_rejected() {
        let workspace = TestWorkspace::new();
        workspace.write_valid_manifests("draft", "pending", "unknown");
        let report_path = workspace.root.join(PREFLIGHT_REPORT_REL);
        let text = fs::read_to_string(&report_path)
            .unwrap()
            .replace("\"schema\": 1,", "\"schema\": 1, \"surprise\": true,");
        fs::write(&report_path, text).unwrap();
        let error = load_preflight_inventory(&workspace.root)
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown field"), "{error}");

        workspace.write_valid_manifests("draft", "pending", "unknown");
        let domain_path = workspace.root.join(DOMAIN_MANIFEST_REL);
        let text = fs::read_to_string(&domain_path)
            .unwrap()
            .replace("schema = 1", "schema = 4294967297");
        fs::write(&domain_path, text).unwrap();
        let error = load_preflight_inventory(&workspace.root)
            .unwrap_err()
            .to_string();
        assert!(error.contains("unsigned 32-bit integer"), "{error}");
    }

    #[test]
    fn unsafe_invalid_empty_and_duplicate_source_references_are_rejected() {
        let workspace = TestWorkspace::new();
        workspace.write_valid_manifests("draft", "pending", "unknown");
        let report_path = workspace.root.join(PREFLIGHT_REPORT_REL);
        let text = fs::read_to_string(&report_path).unwrap().replace(
            "\"path\":\"src/input.txt\"",
            "\"path\":\"../src/input.txt\"",
        );
        fs::write(&report_path, text).unwrap();
        let error = load_preflight_inventory(&workspace.root)
            .unwrap_err()
            .to_string();
        assert!(error.contains("unsafe source reference path"), "{error}");

        workspace.write_valid_manifests("draft", "pending", "unknown");
        let text = fs::read_to_string(&report_path)
            .unwrap()
            .replace(&workspace.pinned_hash(), &"A".repeat(64));
        fs::write(&report_path, text).unwrap();
        let error = load_preflight_inventory(&workspace.root)
            .unwrap_err()
            .to_string();
        assert!(error.contains("lowercase 64-character SHA-256"), "{error}");

        workspace.write_valid_manifests("draft", "pending", "unknown");
        let text = fs::read_to_string(&report_path).unwrap();
        let reference = format!(
            r#""source_references": [{{"path":"src/input.txt","sha256":"{}"}}]"#,
            workspace.pinned_hash()
        );
        fs::write(
            &report_path,
            text.replace(&reference, r#""source_references": []"#),
        )
        .unwrap();
        let error = load_preflight_inventory(&workspace.root)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("requires at least one source reference"),
            "{error}"
        );

        workspace.write_valid_manifests("draft", "pending", "unknown");
        let text = fs::read_to_string(&report_path).unwrap();
        let hash = workspace.pinned_hash();
        let reference = format!(r#"{{"path":"src/input.txt","sha256":"{hash}"}}"#);
        let aliased_reference = format!(r#"{{"path":"src/./input.txt","sha256":"{hash}"}}"#);
        fs::write(
            &report_path,
            text.replace(
                &format!("[{reference}]"),
                &format!("[{reference},{aliased_reference}]"),
            ),
        )
        .unwrap();
        let error = load_preflight_inventory(&workspace.root)
            .unwrap_err()
            .to_string();
        assert!(error.contains("repeats source reference"), "{error}");
    }

    #[test]
    fn checked_in_manifests_load_as_an_explicitly_blocked_draft() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let inventory = load_preflight_inventory(workspace).unwrap();

        assert_eq!(inventory.domain().status, ManifestStatus::Draft);
        assert_eq!(inventory.oracle_deviations().status, ManifestStatus::Draft);
        assert_eq!(inventory.preflight_report().status, ManifestStatus::Draft);
        assert!(!inventory.is_ready());
        assert!(inventory.summary().pending_checks > 0);
        assert!(inventory.summary().unknown_checks > 0);
        assert_eq!(
            inventory.blocker_ids().take(3).collect::<Vec<_>>(),
            [
                "manifest.domain.status",
                "manifest.oracle-deviations.status",
                "manifest.preflight.status",
            ]
        );
    }
}

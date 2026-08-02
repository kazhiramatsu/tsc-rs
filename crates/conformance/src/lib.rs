#![forbid(unsafe_code)]

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, Item, Table};
use tsc_checker::{
    check_program, check_program_with_libs_at, CompilerOptions, InputFile, PartialCheck,
};
use tsc_diagnostics::{
    compute_line_map, get_line_and_character_of_position, Diagnostic, MessageChain,
};
use tsc_oracle::{OracleDiag, OracleMessageChain, OraclePool};

pub mod families;
pub mod goldens_diff;
mod h0_memory;
mod host_resolution;
mod identity;
pub mod ratchet;
mod rendered;
mod scope;
mod shadow_diff;

pub use families::{
    check as families_check, report as families_report,
    verify_report_freshness as families_verify_report,
};
pub use host_resolution::{
    check_host_resolution_registry, check_host_resolution_registry_with_history_proof,
    draft_host_resolution_registry, HOST_RESOLUTION_REL_PATH,
};
pub use identity::ExactIdentity;
pub use rendered::{
    check_or_extend_rendered_hashes, run_t4_report, RenderHashMode, RenderHashSummary,
    T4CaseReport, T4Report, T4ReportOptions,
};
pub use scope::audit as scope_audit;
use scope::ScopeManifest;
pub use shadow_diff::{
    conformance_diff, ConformanceDiffReport, ShadowTierDiff, ShadowTierIdentity,
    ShadowTierObservation, ShadowTierSetDiff,
};

pub type ConformanceResult<T> = Result<T, Box<dyn Error>>;

/// Resolve one exact schema-2 oracle occurrence against the committed
/// golden that owns it. D2a's `port-plan` consumes this rather than
/// accepting a code-only selector that could conflate duplicate rows.
pub fn resolve_exact_oracle_identity(
    workspace: &Path,
    identity: &ExactIdentity,
) -> ConformanceResult<GoldenDiag> {
    let golden = read_golden(&workspace.join("goldens"), &identity.fixture)?;
    let case = golden
        .cases
        .iter()
        .find(|case| case.matrix_key == identity.matrix_key)
        .ok_or_else(|| {
            format!(
                "exact diagnostic {} has no golden matrix case",
                identity.label()
            )
        })?;
    let identities =
        identity::assign_case_identities(&identity.fixture, &identity.matrix_key, &case.oracle)?;
    let mut matches = identities
        .iter()
        .enumerate()
        .filter(|(_, candidate)| *candidate == identity);
    let Some((index, _)) = matches.next() else {
        return Err(format!(
            "stale exact diagnostic {}: no committed oracle occurrence carries it",
            identity.label()
        )
        .into());
    };
    if matches.next().is_some() {
        return Err(format!(
            "ambiguous exact diagnostic {}: occurrence identity resolved more than once",
            identity.label()
        )
        .into());
    }
    Ok(case.oracle[index].clone())
}

/// Complete, uncapped fixture evidence for a set of diagnostic codes.
/// This is intentionally a human planning query rather than a CI path:
/// it reads the committed schema-2 goldens and returns every fixture
/// carrying at least one requested oracle row.
pub fn oracle_fixtures_for_codes(
    workspace: &Path,
    codes: &BTreeSet<u32>,
) -> ConformanceResult<BTreeMap<u32, Vec<String>>> {
    let mut evidence = codes
        .iter()
        .map(|&code| (code, BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    if codes.is_empty() {
        return Ok(BTreeMap::new());
    }
    let fixtures = select_fixtures(&RefreshOptions {
        workspace: workspace.to_owned(),
        limit: None,
        files: Vec::new(),
    })?;
    let goldens_root = workspace.join("goldens");
    for fixture in fixtures {
        let key = fixture_key(workspace, &fixture)?;
        let golden = read_golden(&goldens_root, &key)?;
        for case in golden.cases {
            for diagnostic in case.oracle {
                if let Some(fixtures) = evidence.get_mut(&diagnostic.code) {
                    fixtures.insert(key.clone());
                }
            }
        }
    }
    Ok(evidence
        .into_iter()
        .map(|(code, fixtures)| (code, fixtures.into_iter().collect()))
        .collect())
}

/// The 2XXX diagnostic code range — the single source for the A1
/// `2xxx` view and the A2 band pin/census code checks.
pub(crate) const TWO_XXX_CODES: std::ops::Range<u32> = 2000..3000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticBand {
    All,
    TwoXxx,
    /// The M1 gate band: oracle side restricted to getSyntacticDiagnostics
    /// (pass provenance in schema-2 goldens), tsrs side to parse diagnostics.
    Syntactic,
}

impl DiagnosticBand {
    pub fn name(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::TwoXxx => "2xxx",
            Self::Syntactic => "syntactic",
        }
    }

    fn contains(self, code: u32) -> bool {
        match self {
            Self::All | Self::Syntactic => true,
            Self::TwoXxx => TWO_XXX_CODES.contains(&code),
        }
    }

    fn matches_oracle(self, diag: &GoldenDiag) -> bool {
        match self {
            Self::Syntactic => diag.pass.as_deref() == Some("syntactic"),
            _ => self.contains(diag.code),
        }
    }

    fn ratchet_key(self) -> &'static str {
        match self {
            Self::All => "t0",
            Self::TwoXxx => "t0-2xxx",
            Self::Syntactic => "t0-syntactic",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GoldenFile {
    pub schema: u32,
    pub fixture: String,
    pub cases: Vec<GoldenCase>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GoldenCase {
    pub matrix_key: String,
    /// Legacy schema-2 goldens always serialized this empty tsrs side.
    /// Schema 3 is oracle-only and omits it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tsrs: Vec<GoldenDiag>,
    pub oracle: Vec<GoldenDiag>,
    /// Schema-3 formatter-only metadata. Each entry is an index into
    /// `oracle` whose genuine tsc diagnostic carried a truthy but empty
    /// `relatedInformation` array. Schema-2's structured diagnostic
    /// records deliberately collapsed that state with `undefined`, so
    /// keep the sparse presence data beside (not inside) `oracle`: the
    /// A3 extension must leave every pre-existing oracle JSON byte and
    /// its `oracle_sha256` unchanged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub oracle_empty_related_information: Vec<usize>,
    /// Schema-2 compatibility only: the old value is an FNV hash of
    /// serialized diagnostic JSON and MUST NOT be interpreted as T4.
    /// Schema 3 omits the field; conformance computes the current tsrs
    /// rendered SHA-256 in memory.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tsrs_cli_hash: String,
    /// Schema 2: legacy JSON/FNV placeholder. Schema >=3: lowercase
    /// SHA-256 of the normalized UTF-8 bytes produced by the vendored
    /// TS 6.0.3 context formatter (ANSI removed, LF fixed).
    #[serde(default)]
    pub oracle_cli_hash: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GoldenDiag {
    pub file: Option<String>,
    pub start: Option<u32>,
    pub length: Option<u32>,
    pub line: Option<u32>,
    pub col: Option<u32>,
    pub code: u32,
    /// Oracle pass provenance ("syntactic" | "semantic" | "suggestion");
    /// None on schema-1 goldens and on tsrs-side diagnostics.
    #[serde(default)]
    pub pass: Option<String>,
    pub category: String,
    pub chain: GoldenMessageChain,
    #[serde(default)]
    pub related: Vec<GoldenRelated>,
    #[serde(default)]
    pub reports_unnecessary: bool,
    #[serde(default)]
    pub reports_deprecated: bool,
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GoldenRelated {
    pub file: Option<String>,
    pub start: Option<u32>,
    pub length: Option<u32>,
    pub code: u32,
    pub category: String,
    pub chain: GoldenMessageChain,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GoldenMessageChain {
    pub text: String,
    pub code: u32,
    pub category: String,
    #[serde(default)]
    pub next: Vec<GoldenMessageChain>,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct T0Key {
    pub file: Option<String>,
    pub code: u32,
    pub line: Option<u32>,
    pub col: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct RefreshOptions {
    pub workspace: PathBuf,
    pub limit: Option<usize>,
    pub files: Vec<PathBuf>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RefreshSummary {
    pub fixtures: usize,
    pub cases: usize,
    pub oracle_diagnostics: usize,
    pub goldens_root: String,
}

#[derive(Clone, Debug)]
pub struct ConformanceOptions {
    pub workspace: PathBuf,
    pub limit: Option<usize>,
    pub files: Vec<PathBuf>,
    pub out_json: PathBuf,
    pub band: DiagnosticBand,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ConformanceSummary {
    pub band: String,
    pub fixtures_total: usize,
    pub cases_total: usize,
    pub oracle_diagnostics: usize,
    pub tsrs_diagnostics: usize,
    pub matched_t0_diagnostics: usize,
    pub t0_rate: f64,
    /// Tier metrics. These began as pre-5.8a report-only shadow
    /// observations; after M8's one-time A1 activation the identical
    /// complete-multiset bucket identities are accepted-set gates.
    /// Of the T0-matched buckets, these counts match category (T1),
    /// exact span + top message text (T2), and the full chain +
    /// relatedInformation (T3). Nested: t3 ≤ t2 ≤ t1 ≤ t0.
    pub shadow_t1_matched: usize,
    pub shadow_t2_matched: usize,
    pub shadow_t3_matched: usize,
    pub shadow_t1_rate: f64,
    pub shadow_t2_rate: f64,
    pub shadow_t3_rate: f64,
    /// Exact observation identities for the all-corpus tiers. The
    /// report remains evidence-only; A1 independently persists the
    /// same per-case bucket sets as the active authority.
    pub shadow_tier_identities: ShadowTierObservation,
    pub exact_match_cases: usize,
    pub mismatch_cases: usize,
    pub false_positive_diagnostics: usize,
    pub false_negative_diagnostics: usize,
    /// Oracle-only rows inside a source range where the checker
    /// actually reached an explicit partial-check boundary.
    /// This is evidence that a blocking semantic condition was reached,
    /// not proof that the diagnostic's code-specific trigger was tested.
    pub fn_with_partial_boundary_evidence: usize,
    /// Oracle-only rows for which no reached partial-check boundary
    /// covered the diagnostic position.
    pub fn_without_partial_boundary_evidence: usize,
    pub top_fn_partial_boundary_reasons: Vec<(String, usize)>,
    pub top_false_positive_codes: Vec<(u32, usize)>,
    pub top_false_negative_codes: Vec<(u32, usize)>,
    /// M8's supported-scope view. The all-corpus fields above remain
    /// the standing visibility metric and NEW_FP=0 gate; these fields
    /// remove only exact, reviewed schema-2 oracle occurrences from
    /// the denominator (measurement-integrity.md §3) — occurrence
    /// counts, not T0 buckets. An exclusion therefore cannot hide a
    /// neighboring diagnostic, another occurrence in the same bucket,
    /// or a false positive in the same fixture.
    pub scope_status: String,
    pub scope_manifest_entries: usize,
    pub scope_excluded_diagnostics: usize,
    pub scope_unresolved_diagnostics: usize,
    /// Excluded occurrences the resolution predicate (§3.2) proves
    /// resolved: a matched singleton bucket or a matched
    /// multiplicity-complete duplicate bucket. Such an entry must be
    /// deleted with its tombstone; it can never satisfy readiness.
    pub scope_resolved_t0_diagnostics: usize,
    pub supported_oracle_diagnostics: usize,
    pub supported_tsrs_diagnostics: usize,
    pub supported_matched_t0_diagnostics: usize,
    pub supported_t0_rate: f64,
    pub supported_t1_matched: usize,
    pub supported_t2_matched: usize,
    pub supported_t3_matched: usize,
    pub supported_t1_rate: f64,
    pub supported_t2_rate: f64,
    pub supported_t3_rate: f64,
    /// Exact report-only identities after applying the A2 scope view.
    pub supported_shadow_tier_identities: ShadowTierObservation,
    /// Exact report-only buckets that still fail one of the supported
    /// T1-T3 comparators. The first failed tier partitions the residual
    /// without losing the complete expected/actual diagnostic shapes
    /// needed to assign an owning implementation slice.
    pub supported_tier_mismatches: Vec<SupportedTierMismatch>,
    pub supported_exact_match_cases: usize,
    pub supported_mismatch_cases: usize,
    pub supported_false_negative_diagnostics: usize,
    /// Exact schema-2 oracle occurrences whose supported T0 bucket is
    /// absent. M8 planning consumes these identities directly instead
    /// of reconstructing occurrences from aggregate codes or T0 keys.
    pub supported_false_negative_identities: Vec<ExactIdentity>,
    pub ratchet_rate: f64,
    pub ratchet_allowed_regression: f64,
    pub mismatches: Vec<MismatchEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MismatchEntry {
    pub fixture: String,
    pub matrix_key: String,
    pub false_positive: Vec<T0Key>,
    pub false_negative: Vec<T0Key>,
    pub fn_partial_boundary_audit: Vec<FnPartialBoundaryAudit>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SupportedTierMismatch {
    pub fixture: String,
    pub matrix_key: String,
    pub diagnostic: T0Key,
    pub first_failed_tier: String,
    pub actual: Vec<GoldenDiag>,
    pub expected: Vec<GoldenDiag>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FnPartialBoundaryAudit {
    pub diagnostic: T0Key,
    pub reached_partial_boundary: bool,
    /// All named partial boundaries containing this oracle diagnostic,
    /// sorted and deduplicated for deterministic reports.
    pub reasons: Vec<String>,
}

pub fn run_empty_engine_smoke() -> usize {
    check_program(&[], &CompilerOptions::default())
        .diagnostics
        .len()
}

#[derive(Clone, Debug)]
pub struct PrefixConformanceOptions {
    pub workspace: PathBuf,
    pub limit: Option<usize>,
    pub files: Vec<PathBuf>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PrefixConformanceSummary {
    pub fixtures: usize,
    pub cases: usize,
    pub mismatched_cases: usize,
    pub mismatches: Vec<PrefixMismatch>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PrefixMismatch {
    pub fixture: String,
    pub matrix_key: String,
    pub file: String,
    pub cut: usize,
    pub false_positive: Vec<T0Key>,
    pub false_negative: Vec<T0Key>,
}

/// greenfield §7.6 prefix-determinism, reformulated as oracle fidelity on
/// truncated inputs: our syntactic diagnostics for `file[..k]` must equal
/// the tsc oracle's getSyntacticDiagnostics on the SAME truncated program.
/// (Internal prefix-stability of diagnostics is unsatisfiable for a
/// tsc-faithful parser; see docs/NOTES-m1.md.)
pub fn run_prefix_conformance(
    options: &PrefixConformanceOptions,
) -> ConformanceResult<PrefixConformanceSummary> {
    let fixtures = select_fixtures(&RefreshOptions {
        workspace: options.workspace.clone(),
        limit: options.limit,
        files: options.files.clone(),
    })?;
    let vendor_lib_dir = options.workspace.join("vendor/typescript-6.0.3/lib");
    let temp_root = temp_root("tsc-rs-prefix-conformance");
    if temp_root.exists() {
        fs::remove_dir_all(&temp_root)?;
    }
    fs::create_dir_all(&temp_root)?;

    let pool = OraclePool::new(OraclePool::default_size())?;
    let mut cases = 0usize;
    let mut mismatches = Vec::new();

    for (fixture_index, fixture) in fixtures.iter().enumerate() {
        let fixture_key = fixture_key(&options.workspace, fixture)?;
        if fixture_index > 0 && fixture_index % 50 == 0 {
            eprintln!(
                "prefix-conformance progress: {}/{} fixtures",
                fixture_index,
                fixtures.len()
            );
        }
        let programs = tsc_harness::expand_fixture_file(fixture, &vendor_lib_dir)?;
        for (program_index, program) in programs.iter().enumerate() {
            for file_index in 0..program.files.len() {
                // package.json validation diags come from tsc's module
                // resolution (unported program machinery), so truncated
                // .json files cannot be compared faithfully yet.
                if program.files[file_index].name.ends_with(".json") {
                    continue;
                }
                let text = base64_decode_to_string(&program.files[file_index].text_b64)?;
                let cut = midpoint_char_boundary(&text);
                let mut truncated = program.clone();
                truncated.files[file_index].text_b64 = base64_encode(&text.as_bytes()[..cut]);

                let out_dir = temp_root
                    .join(fixture_index.to_string())
                    .join(program_index.to_string())
                    .join(file_index.to_string());
                let paths =
                    tsc_harness::write_program_jsons(std::slice::from_ref(&truncated), &out_dir)?;
                let oracle = pool.diagnostics(&paths[0]).map_err(|err| {
                    format!(
                        "oracle failed for {fixture_key} [{}] prefix of {}: {err}",
                        program.matrix_key, program.files[file_index].name
                    )
                })?;

                let file_texts = file_texts_for_program(&truncated, &vendor_lib_dir)?;
                let expected = t0_set(
                    oracle
                        .iter()
                        .filter(|diag| diag.pass.as_deref() == Some("syntactic"))
                        .map(|diag| GoldenDiag::from_oracle(diag, &file_texts))
                        .collect::<Vec<_>>()
                        .iter(),
                );

                let input_files = truncated
                    .files
                    .iter()
                    .map(|file| {
                        Ok(InputFile {
                            name: file.name.clone(),
                            text: base64_decode_to_string(&file.text_b64)?,
                        })
                    })
                    .collect::<ConformanceResult<Vec<_>>>()?;
                let libs = read_lib_inputs(&truncated.libs, &vendor_lib_dir)?;
                let result = check_program_with_libs_at(
                    &libs,
                    &input_files,
                    &tsc_harness::compiler_options_from_program(&truncated),
                    &truncated.cwd,
                );
                let actual = t0_set(
                    result
                        .syntactic_diagnostics
                        .iter()
                        .map(|diag| GoldenDiag::from_tsrs(diag, &file_texts))
                        .collect::<Vec<_>>()
                        .iter(),
                );

                cases += 1;
                let false_positive: Vec<T0Key> = actual.difference(&expected).cloned().collect();
                let false_negative: Vec<T0Key> = expected.difference(&actual).cloned().collect();
                if !false_positive.is_empty() || !false_negative.is_empty() {
                    mismatches.push(PrefixMismatch {
                        fixture: fixture_key.clone(),
                        matrix_key: program.matrix_key.clone(),
                        file: program.files[file_index].name.clone(),
                        cut,
                        false_positive,
                        false_negative,
                    });
                }
            }
        }
    }

    fs::remove_dir_all(&temp_root)?;
    Ok(PrefixConformanceSummary {
        fixtures: fixtures.len(),
        cases,
        mismatched_cases: mismatches.len(),
        mismatches,
    })
}

/// tsc's midpoint cut rule, shared with the xtask invariants runner.
fn midpoint_char_boundary(text: &str) -> usize {
    let midpoint = text.len() / 2;
    text.char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= midpoint)
        .last()
        .unwrap_or(0)
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(triple >> 18) as usize & 0x3f] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 0x3f] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(triple >> 6) as usize & 0x3f] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[triple as usize & 0x3f] as char
        } else {
            '='
        });
    }
    out
}

pub fn refresh_oracle_goldens(options: &RefreshOptions) -> ConformanceResult<RefreshSummary> {
    let fixtures = select_fixtures(options)?;
    let vendor_lib_dir = options.workspace.join("vendor/typescript-6.0.3/lib");
    let goldens_root = options.workspace.join("goldens");
    let temp_root = temp_root("tsc-rs-oracle-refresh");
    if temp_root.exists() {
        fs::remove_dir_all(&temp_root)?;
    }
    fs::create_dir_all(&temp_root)?;

    let pool = OraclePool::new(OraclePool::default_size())?;
    // Goldens are the gating truth: refuse to write any before the
    // LAUNCHED driver's process.version matches the tree's producer
    // Node pin (.node-version alone is a declaration; this is the
    // enforcement half).
    ratchet::verify_launched_node(&options.workspace, &pool)?;
    let mut case_count = 0usize;
    let mut oracle_diag_count = 0usize;

    for (fixture_index, fixture) in fixtures.iter().enumerate() {
        let fixture_key = fixture_key(&options.workspace, fixture)?;
        let existing_path = golden_path(&goldens_root, &fixture_key);
        if existing_path.exists() {
            let existing = read_golden(&goldens_root, &fixture_key)?;
            if existing.schema >= 3 {
                return Err(format!(
                    "golden {fixture_key} is schema {}; ordinary oracle-refresh may not \
                     downgrade or reinterpret A3 rendered hashes — use \
                     `cargo xtask oracle-refresh --render-hashes --check`",
                    existing.schema
                )
                .into());
            }
        }
        if fixture_index > 0 && fixture_index % 250 == 0 {
            eprintln!(
                "oracle refresh progress: {}/{} fixtures",
                fixture_index,
                fixtures.len()
            );
        }
        let programs = tsc_harness::expand_fixture_file(fixture, &vendor_lib_dir)?;
        let out_dir = temp_root.join(fixture_index.to_string());
        let paths = tsc_harness::write_program_jsons(&programs, &out_dir)?;
        let mut cases = Vec::with_capacity(programs.len());

        for (program, path) in programs.iter().zip(paths.iter()) {
            let file_texts = file_texts_for_program(program, &vendor_lib_dir)?;
            let oracle = pool.diagnostics(path).map_err(|err| {
                format!(
                    "oracle failed for {fixture_key} [{}]: {err}",
                    program.matrix_key
                )
            })?;
            let oracle = oracle
                .iter()
                .map(|diag| GoldenDiag::from_oracle(diag, &file_texts))
                .collect::<Vec<_>>();
            oracle_diag_count += oracle.len();
            case_count += 1;

            cases.push(GoldenCase {
                matrix_key: program.matrix_key.clone(),
                tsrs: Vec::new(),
                oracle_empty_related_information: Vec::new(),
                oracle_cli_hash: stable_json_hash(&oracle)?,
                oracle,
                tsrs_cli_hash: stable_json_hash(&Vec::<GoldenDiag>::new())?,
            });
        }

        let golden = GoldenFile {
            schema: 2,
            fixture: fixture_key,
            cases,
        };
        write_golden(&goldens_root, &golden)?;
    }

    fs::remove_dir_all(&temp_root)?;
    Ok(RefreshSummary {
        fixtures: fixtures.len(),
        cases: case_count,
        oracle_diagnostics: oracle_diag_count,
        goldens_root: goldens_root.display().to_string(),
    })
}

/// A gating conformance run: enforces the accepted-set ratchet
/// (measurement-integrity.md §2) on top of the integer/FP gates.
pub fn run_conformance(options: &ConformanceOptions) -> ConformanceResult<ConformanceSummary> {
    run_conformance_inner(options, SetGate::Enforce, false, None, None, false)
        .map(|run| run.summary)
}

/// The A5 rollup path: the identical gating run, additionally
/// collecting the per-bucket families observation and finishing the
/// rollup from it — the corpus is checked ONCE for both artifacts.
/// Full band=all runs only: the observation must never come from a
/// projection or an A1 summary (measurement-integrity.md §5). Map
/// validation, anchor verification, and the before-run input
/// fingerprints happen in `prepare_report` BEFORE the run; the
/// after-run fingerprint equality happens in `finish_report`.
pub fn run_conformance_with_families_report(
    options: &ConformanceOptions,
    report_out: &Path,
) -> ConformanceResult<ConformanceSummary> {
    let preparation = families::prepare_report(&options.workspace)?;
    let run = run_conformance_inner(options, SetGate::Enforce, true, None, None, false)?;
    let observation = run
        .observation
        .expect("observing run collects an observation");
    families::finish_report(
        &options.workspace,
        preparation,
        &run.summary,
        &observation,
        report_out,
    )?;
    Ok(run.summary)
}

/// The `ratchet update` measurement path: identical run, but it
/// RETURNS the per-view identity sets instead of gating against the
/// accepted artifact (which may not exist yet at bootstrap).
pub(crate) fn run_conformance_collect(
    options: &ConformanceOptions,
) -> ConformanceResult<ConformanceRun> {
    run_conformance_inner(options, SetGate::Collect, false, None, None, false)
}

pub(crate) fn run_conformance_collect_with_t4(
    options: &ConformanceOptions,
    planned_t4_pins: Option<&ratchet::T4OraclePins>,
    planned_t4_empty_related_information: Option<&rendered::T4OracleEmptyRelatedInformation>,
) -> ConformanceResult<ConformanceRun> {
    // After the one-time activation, T4 pins live in schema-3 goldens and
    // both planned arguments are intentionally None. Keep the explicit
    // force bit separate from those transition-only arguments so an ordinary
    // ratchet update cannot reinterpret every accepted T4 case as removed.
    run_conformance_inner(
        options,
        SetGate::Collect,
        false,
        planned_t4_pins,
        planned_t4_empty_related_information,
        true,
    )
}

/// The merge-gate shape: expand and execute each case once, then grade
/// all three fixed views from that case's aggregate and syntactic
/// streams before advancing to the next case. View grading stays
/// sequential, so this does not increase CPU concurrency or retain a
/// corpus-sized cache of checker results.
///
/// No program JSON or per-case cache files are written. `out_json`
/// retains the historical CI behavior: each view writes it in order,
/// leaving the syntactic report there when all gates pass.
/// `completed_view` runs immediately after each view's gates pass, so
/// a later-view failure does not hide earlier summaries in CI logs.
pub fn run_ci_conformance(
    workspace: &Path,
    out_json: &Path,
    families_report_out: &Path,
    mut completed_view: impl FnMut(&ConformanceSummary),
) -> ConformanceResult<CiConformanceSummaries> {
    let options = ConformanceOptions {
        workspace: workspace.to_owned(),
        limit: None,
        files: Vec::new(),
        out_json: out_json.to_owned(),
        band: DiagnosticBand::All,
    };

    let preparation = families::prepare_report(workspace)?;
    let measured = measure_conformance(
        &options,
        &ratchet::FIXED_VIEWS,
        SetGate::Enforce,
        true,
        None,
        None,
        false,
    )?;
    let MeasuredConformance {
        views,
        mut observation,
        accepted,
        executed_fixtures,
        full_run,
    } = measured;
    let mut preparation = Some(preparation);
    let completed = complete_ci_views(
        views,
        out_json,
        accepted.as_ref(),
        &executed_fixtures,
        full_run,
        |summary| {
            let preparation = preparation
                .take()
                .ok_or("All families preparation was consumed more than once")?;
            let all_observation = observation
                .take()
                .ok_or("All families observation is missing")?;
            families::finish_report(
                workspace,
                preparation,
                summary,
                &all_observation,
                families_report_out,
            )
        },
        &mut completed_view,
    )?;
    let [all, two_xxx, syntactic]: [ConformanceSummary; 3] = completed
        .try_into()
        .map_err(|_| "CI conformance did not complete exactly three fixed views")?;

    Ok(CiConformanceSummaries {
        all,
        two_xxx,
        syntactic,
    })
}

#[allow(clippy::too_many_arguments)]
fn complete_ci_views(
    views: Vec<MeasuredView>,
    out_json: &Path,
    accepted: Option<&ratchet::AcceptedState>,
    executed_fixtures: &BTreeSet<String>,
    full_run: bool,
    mut finish_all: impl FnMut(&ConformanceSummary) -> ConformanceResult<()>,
    mut completed_view: impl FnMut(&ConformanceSummary),
) -> ConformanceResult<Vec<ConformanceSummary>> {
    process_fixed_views(
        views.into_iter().map(|view| (view.band, view.result)),
        |band, measurement| {
            let view = measurement.into_full()?;
            write_and_enforce_view(&view, out_json, accepted, executed_fixtures, full_run)?;
            if band == DiagnosticBand::All {
                finish_all(&view.summary)?;
            }
            completed_view(&view.summary);
            // Drop this view's potentially-large RunSets at its gate instead
            // of retaining three complete FinishedConformanceViews.
            Ok(view.summary)
        },
    )
}

/// Applies fixed-view gates in the normative order. View-local failures stay
/// dormant until their turn, while a missing, duplicate, reordered, or extra
/// view is a normal release-build error rather than a debug-only assertion.
fn process_fixed_views<T, U>(
    views: impl IntoIterator<Item = (DiagnosticBand, ConformanceResult<T>)>,
    mut complete: impl FnMut(DiagnosticBand, T) -> ConformanceResult<U>,
) -> ConformanceResult<Vec<U>> {
    let views = views.into_iter().collect::<Vec<_>>();
    if views.len() != ratchet::FIXED_VIEWS.len() {
        return Err(format!(
            "CI conformance fixed-view order requires exactly {} views, observed {}",
            ratchet::FIXED_VIEWS.len(),
            views.len()
        )
        .into());
    }
    for ((observed, _), expected) in views.iter().zip(ratchet::FIXED_VIEWS) {
        if *observed != expected {
            return Err(format!(
                "CI conformance fixed-view order mismatch: expected {}, observed {}",
                expected.name(),
                observed.name()
            )
            .into());
        }
    }

    let mut completed = Vec::with_capacity(ratchet::FIXED_VIEWS.len());
    for (expected, (_, result)) in ratchet::FIXED_VIEWS.into_iter().zip(views) {
        completed.push(complete(expected, result?)?);
    }
    Ok(completed)
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CiConformanceSummaries {
    pub all: ConformanceSummary,
    pub two_xxx: ConformanceSummary,
    pub syntactic: ConformanceSummary,
}

pub struct ConformanceRun {
    pub summary: ConformanceSummary,
    /// Per fixed view (all/2xxx/syntactic): matched T0 buckets and
    /// multiplicity-complete buckets, keyed fixture -> matrix.
    pub sets: ratchet::RunSets,
    /// The A5 per-bucket observation, when requested.
    pub observation: Option<families::Observation>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum SetGate {
    Enforce,
    Collect,
}

struct ViewAccumulator {
    band: DiagnosticBand,
    ratchet: Ratchet,
    t1_ratchet: Option<Ratchet>,
    run_sets: ratchet::RunSets,
    case_count: usize,
    exact_match_cases: usize,
    oracle_diagnostics: usize,
    tsrs_diagnostics: usize,
    matched_t0_diagnostics: usize,
    shadow_t1_matched: usize,
    shadow_t2_matched: usize,
    shadow_t3_matched: usize,
    shadow_t1_identities: BTreeSet<ShadowTierIdentity>,
    shadow_t2_identities: BTreeSet<ShadowTierIdentity>,
    shadow_t3_identities: BTreeSet<ShadowTierIdentity>,
    shadow_oracle_records: Vec<Vec<u8>>,
    fp_count: usize,
    fn_count: usize,
    fn_with_partial_boundary_count: usize,
    fn_without_partial_boundary_count: usize,
    fn_trigger_reasons: BTreeMap<String, usize>,
    fp_codes: BTreeMap<u32, usize>,
    fn_codes: BTreeMap<u32, usize>,
    mismatches: Vec<MismatchEntry>,
    scope_excluded_diagnostics: usize,
    scope_unresolved_diagnostics: usize,
    scope_resolved_t0_diagnostics: usize,
    supported_oracle_diagnostics: usize,
    supported_tsrs_diagnostics: usize,
    supported_matched_t0_diagnostics: usize,
    supported_t1_matched: usize,
    supported_t2_matched: usize,
    supported_t3_matched: usize,
    supported_t1_identities: BTreeSet<ShadowTierIdentity>,
    supported_t2_identities: BTreeSet<ShadowTierIdentity>,
    supported_t3_identities: BTreeSet<ShadowTierIdentity>,
    supported_tier_mismatches: Vec<SupportedTierMismatch>,
    supported_shadow_oracle_records: Vec<Vec<u8>>,
    supported_exact_match_cases: usize,
    supported_fn_count: usize,
    supported_false_negative_identities: BTreeSet<ExactIdentity>,
}

/// Ratchet collection needs the identity sets for every fixed view, but the
/// full mismatch/summary vectors only for the selected view. Keeping those
/// projections lightweight preserves the pre-fusion memory contract.
struct RunSetsAccumulator {
    band: DiagnosticBand,
    run_sets: ratchet::RunSets,
}

enum ViewAccumulatorKind {
    Full(Box<ViewAccumulator>),
    RunSetsOnly(RunSetsAccumulator),
}

struct PendingViewAccumulator {
    band: DiagnosticBand,
    state: ConformanceResult<ViewAccumulatorKind>,
}

impl ViewAccumulator {
    fn new(band: DiagnosticBand, ratchet_path: &Path) -> ConformanceResult<Self> {
        Ok(Self {
            band,
            ratchet: read_ratchet(ratchet_path, band)?,
            t1_ratchet: (band == DiagnosticBand::All)
                .then(|| read_ratchet_section(ratchet_path, "t1"))
                .transpose()?,
            run_sets: [(band.name().to_owned(), Default::default())]
                .into_iter()
                .collect(),
            case_count: 0,
            exact_match_cases: 0,
            oracle_diagnostics: 0,
            tsrs_diagnostics: 0,
            matched_t0_diagnostics: 0,
            shadow_t1_matched: 0,
            shadow_t2_matched: 0,
            shadow_t3_matched: 0,
            shadow_t1_identities: BTreeSet::new(),
            shadow_t2_identities: BTreeSet::new(),
            shadow_t3_identities: BTreeSet::new(),
            shadow_oracle_records: Vec::new(),
            fp_count: 0,
            fn_count: 0,
            fn_with_partial_boundary_count: 0,
            fn_without_partial_boundary_count: 0,
            fn_trigger_reasons: BTreeMap::new(),
            fp_codes: BTreeMap::new(),
            fn_codes: BTreeMap::new(),
            mismatches: Vec::new(),
            scope_excluded_diagnostics: 0,
            scope_unresolved_diagnostics: 0,
            scope_resolved_t0_diagnostics: 0,
            supported_oracle_diagnostics: 0,
            supported_tsrs_diagnostics: 0,
            supported_matched_t0_diagnostics: 0,
            supported_t1_matched: 0,
            supported_t2_matched: 0,
            supported_t3_matched: 0,
            supported_t1_identities: BTreeSet::new(),
            supported_t2_identities: BTreeSet::new(),
            supported_t3_identities: BTreeSet::new(),
            supported_tier_mismatches: Vec::new(),
            supported_shadow_oracle_records: Vec::new(),
            supported_exact_match_cases: 0,
            supported_fn_count: 0,
            supported_false_negative_identities: BTreeSet::new(),
        })
    }
}

impl RunSetsAccumulator {
    fn new(band: DiagnosticBand) -> Self {
        Self {
            band,
            run_sets: [(band.name().to_owned(), Default::default())]
                .into_iter()
                .collect(),
        }
    }

    fn observe_case(
        &mut self,
        fixture_key: &str,
        program: &tsc_harness::ProgramJson,
        golden_case: &GoldenCase,
        case_tsrs: &CaseTsrs,
    ) {
        let oracle_side = golden_case
            .oracle
            .iter()
            .filter(|diagnostic| self.band.matches_oracle(diagnostic));
        let case_sets = match self.band {
            DiagnosticBand::Syntactic => {
                ratchet::bucket_sets(oracle_side, case_tsrs.syntactic.iter())
            }
            _ => ratchet::bucket_sets(
                oracle_side,
                case_tsrs
                    .all
                    .iter()
                    .filter(|diagnostic| self.band.contains(diagnostic.code)),
            ),
        };
        if !case_sets.matched.is_empty() {
            self.run_sets
                .entry(self.band.name().to_owned())
                .or_default()
                .entry(fixture_key.to_owned())
                .or_default()
                .insert(program.matrix_key.clone(), case_sets);
        }
    }
}

impl ViewAccumulatorKind {
    fn band(&self) -> DiagnosticBand {
        match self {
            Self::Full(accumulator) => accumulator.band,
            Self::RunSetsOnly(accumulator) => accumulator.band,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn observe_case(
        &mut self,
        fixture_key: &str,
        program: &tsc_harness::ProgramJson,
        golden_schema: u32,
        golden_case: &GoldenCase,
        case_tsrs: &CaseTsrs,
        excluded_indices: &BTreeSet<usize>,
        vendor_lib_dir: &Path,
        measure_t4: bool,
        planned_t4_pins: Option<&ratchet::T4OraclePins>,
        planned_t4_empty_related_information: Option<&rendered::T4OracleEmptyRelatedInformation>,
        observation: Option<&mut families::Observation>,
    ) -> ConformanceResult<()> {
        match self {
            Self::Full(accumulator) => accumulator.observe_case(
                fixture_key,
                program,
                golden_schema,
                golden_case,
                case_tsrs,
                excluded_indices,
                vendor_lib_dir,
                measure_t4,
                planned_t4_pins,
                planned_t4_empty_related_information,
                observation,
            ),
            Self::RunSetsOnly(accumulator) => {
                accumulator.observe_case(fixture_key, program, golden_case, case_tsrs);
                Ok(())
            }
        }
    }
}

impl ViewAccumulator {
    #[allow(clippy::too_many_arguments)]
    fn observe_case(
        &mut self,
        fixture_key: &str,
        program: &tsc_harness::ProgramJson,
        golden_schema: u32,
        golden_case: &GoldenCase,
        case_tsrs: &CaseTsrs,
        excluded_indices: &BTreeSet<usize>,
        vendor_lib_dir: &Path,
        measure_t4: bool,
        planned_t4_pins: Option<&ratchet::T4OraclePins>,
        planned_t4_empty_related_information: Option<&rendered::T4OracleEmptyRelatedInformation>,
        observation: Option<&mut families::Observation>,
    ) -> ConformanceResult<()> {
        let oracle_side = golden_case
            .oracle
            .iter()
            .filter(|diag| self.band.matches_oracle(diag));
        let case_sets = match self.band {
            DiagnosticBand::Syntactic => {
                ratchet::bucket_sets(oracle_side, case_tsrs.syntactic.iter())
            }
            _ => ratchet::bucket_sets(
                oracle_side,
                case_tsrs
                    .all
                    .iter()
                    .filter(|diag| self.band.contains(diag.code)),
            ),
        };
        let tier_matches = ShadowTierMatches {
            t1: case_sets.t1.clone(),
            t2: case_sets.t2.clone(),
            t3: case_sets.t3.clone(),
        };
        if !case_sets.matched.is_empty() {
            self.run_sets
                .entry(self.band.name().to_owned())
                .or_default()
                .entry(fixture_key.to_owned())
                .or_default()
                .insert(program.matrix_key.clone(), case_sets);
        }

        let current = match self.band {
            DiagnosticBand::Syntactic => &case_tsrs.syntactic,
            _ => &case_tsrs.all,
        };
        let actual = t0_set(current.iter().filter(|diag| self.band.contains(diag.code)));
        let expected = t0_set(
            golden_case
                .oracle
                .iter()
                .filter(|diag| self.band.matches_oracle(diag)),
        );
        let excluded_records = excluded_indices
            .iter()
            .copied()
            .filter(|index| self.band.matches_oracle(&golden_case.oracle[*index]))
            .collect::<Vec<_>>();

        // Include the selected case even when this band has zero oracle
        // diagnostics. Otherwise two disjoint empty projections would share a
        // universe hash and appear comparable to `conformance-diff`.
        let case_record = serde_json::to_vec(&("case", fixture_key, &program.matrix_key))?;
        self.shadow_oracle_records.push(case_record.clone());
        self.supported_shadow_oracle_records.push(case_record);
        for (index, diagnostic) in golden_case.oracle.iter().enumerate() {
            if !self.band.matches_oracle(diagnostic) {
                continue;
            }
            let record =
                serde_json::to_vec(&("diagnostic", fixture_key, &program.matrix_key, diagnostic))?;
            self.shadow_oracle_records.push(record.clone());
            if !excluded_indices.contains(&index) {
                self.supported_shadow_oracle_records.push(record);
            }
        }

        let fp = actual.difference(&expected).cloned().collect::<Vec<_>>();
        let fn_ = expected.difference(&actual).cloned().collect::<Vec<_>>();
        let fn_partial_boundary_audit =
            classify_fn_partial_boundaries(&fn_, &golden_case.oracle, &case_tsrs.partial_checks);
        for audit in &fn_partial_boundary_audit {
            if audit.reached_partial_boundary {
                self.fn_with_partial_boundary_count += 1;
                for reason in &audit.reasons {
                    *self.fn_trigger_reasons.entry(reason.clone()).or_default() += 1;
                }
            } else {
                self.fn_without_partial_boundary_count += 1;
            }
        }

        // Exact exclusions are occurrence-level. The tsrs side loses a bucket
        // only when every oracle occurrence at that key is excluded.
        let (supported_expected, fully_excluded) = if excluded_indices.is_empty() {
            (Cow::Borrowed(&expected), BTreeSet::new())
        } else {
            let (supported_expected, fully_excluded) =
                scope::supported_case_view(&golden_case.oracle, self.band, excluded_indices);
            (Cow::Owned(supported_expected), fully_excluded)
        };
        let supported_actual = if fully_excluded.is_empty() {
            Cow::Borrowed(&actual)
        } else {
            Cow::Owned(
                actual
                    .iter()
                    .filter(|key| !fully_excluded.contains(*key))
                    .cloned()
                    .collect::<BTreeSet<_>>(),
            )
        };

        if measure_t4 && self.band == DiagnosticBand::All {
            let oracle_t4_pin = if let Some(fixtures) = planned_t4_pins {
                fixtures
                    .get(fixture_key)
                    .and_then(|cases| cases.get(&program.matrix_key))
                    .map(String::as_str)
            } else {
                (golden_schema >= 3).then_some(golden_case.oracle_cli_hash.as_str())
            }
            .ok_or_else(|| {
                format!(
                    "active T4 measurement lacks a genuine oracle pin for \
                     {fixture_key} [{}]",
                    program.matrix_key
                )
            })?;
            let oracle_empty_related_information =
                if let Some(fixtures) = planned_t4_empty_related_information {
                    fixtures
                        .get(fixture_key)
                        .and_then(|cases| cases.get(&program.matrix_key))
                        .map(Vec::as_slice)
                } else {
                    (golden_schema >= 3)
                        .then_some(golden_case.oracle_empty_related_information.as_slice())
                }
                .ok_or_else(|| {
                    format!(
                        "active T4 measurement lacks empty-related-information metadata for \
                         {fixture_key} [{}]",
                        program.matrix_key
                    )
                })?;
            if rendered::supported_case_t4_matches(
                program,
                vendor_lib_dir,
                (&golden_case.oracle, oracle_empty_related_information),
                (&case_tsrs.all, &case_tsrs.all_empty_related_information),
                excluded_indices,
                &fully_excluded,
                oracle_t4_pin,
            )? {
                self.run_sets
                    .get_mut(DiagnosticBand::All.name())
                    .expect("T4 is measured only with the All fixed view")
                    .entry(fixture_key.to_owned())
                    .or_default()
                    .entry(program.matrix_key.clone())
                    .or_default()
                    .t4 = true;
            }
        }

        let supported_fn = supported_expected
            .difference(&supported_actual)
            .cloned()
            .collect::<BTreeSet<_>>();
        if !supported_fn.is_empty() {
            self.supported_false_negative_identities.extend(
                exact_supported_false_negative_identities(
                    fixture_key,
                    &program.matrix_key,
                    &golden_case.oracle,
                    self.band,
                    excluded_indices,
                    &supported_fn,
                )?,
            );
        }

        let mut resolved_excluded = 0usize;
        let mut unresolved_excluded = 0usize;
        for index in &excluded_records {
            let bucket = t0_key(&golden_case.oracle[*index]);
            let oracle_multiplicity = golden_case
                .oracle
                .iter()
                .filter(|diag| self.band.matches_oracle(diag) && t0_key(diag) == bucket)
                .count();
            let tsrs_multiplicity = current
                .iter()
                .filter(|diag| self.band.contains(diag.code) && t0_key(diag) == bucket)
                .count();
            if scope::occurrence_resolved(
                actual.contains(&bucket),
                oracle_multiplicity,
                tsrs_multiplicity,
            ) {
                resolved_excluded += 1;
            } else {
                unresolved_excluded += 1;
            }
        }

        if let Some(observation) = observation {
            debug_assert_eq!(self.band, DiagnosticBand::All);
            observation.cases.push(families::CaseObservation::collect(
                fixture_key,
                &program.matrix_key,
                &golden_case.oracle,
                &case_tsrs.all,
                excluded_indices,
                &actual,
                fp.len(),
            )?);
        }
        if fp.is_empty() && fn_.is_empty() {
            self.exact_match_cases += 1;
        } else {
            self.mismatches.push(MismatchEntry {
                fixture: fixture_key.to_owned(),
                matrix_key: program.matrix_key.clone(),
                false_positive: fp.clone(),
                false_negative: fn_.clone(),
                fn_partial_boundary_audit,
            });
        }
        if fp.is_empty() && supported_fn.is_empty() {
            self.supported_exact_match_cases += 1;
        }

        for diag in &fp {
            *self.fp_codes.entry(diag.code).or_default() += 1;
        }
        for diag in &fn_ {
            *self.fn_codes.entry(diag.code).or_default() += 1;
        }

        self.matched_t0_diagnostics += expected.intersection(&actual).count();
        self.shadow_t1_matched += tier_matches.t1.len();
        self.shadow_t2_matched += tier_matches.t2.len();
        self.shadow_t3_matched += tier_matches.t3.len();
        extend_shadow_identities(
            &mut self.shadow_t1_identities,
            fixture_key,
            &program.matrix_key,
            tier_matches.t1,
        );
        extend_shadow_identities(
            &mut self.shadow_t2_identities,
            fixture_key,
            &program.matrix_key,
            tier_matches.t2,
        );
        extend_shadow_identities(
            &mut self.shadow_t3_identities,
            fixture_key,
            &program.matrix_key,
            tier_matches.t3,
        );
        self.supported_matched_t0_diagnostics +=
            supported_expected.intersection(&supported_actual).count();

        // Supported tiers remove exact oracle records; the tsrs side drops
        // only fully-excluded buckets because it has no occurrence identity.
        let supported_tier_matches = shadow_tier_matches(
            current.iter().filter(|diagnostic| {
                self.band.contains(diagnostic.code) && !fully_excluded.contains(&t0_key(diagnostic))
            }),
            golden_case
                .oracle
                .iter()
                .enumerate()
                .filter(|(index, diagnostic)| {
                    self.band.matches_oracle(diagnostic) && !excluded_indices.contains(index)
                })
                .map(|(_, diagnostic)| diagnostic),
        );
        self.supported_tier_mismatches
            .extend(collect_supported_tier_mismatches(
                fixture_key,
                &program.matrix_key,
                current,
                &golden_case.oracle,
                self.band,
                excluded_indices,
                &fully_excluded,
                &supported_expected,
                &supported_tier_matches,
            ));
        self.supported_t1_matched += supported_tier_matches.t1.len();
        self.supported_t2_matched += supported_tier_matches.t2.len();
        self.supported_t3_matched += supported_tier_matches.t3.len();
        extend_shadow_identities(
            &mut self.supported_t1_identities,
            fixture_key,
            &program.matrix_key,
            supported_tier_matches.t1,
        );
        extend_shadow_identities(
            &mut self.supported_t2_identities,
            fixture_key,
            &program.matrix_key,
            supported_tier_matches.t2,
        );
        extend_shadow_identities(
            &mut self.supported_t3_identities,
            fixture_key,
            &program.matrix_key,
            supported_tier_matches.t3,
        );
        self.scope_excluded_diagnostics += excluded_records.len();
        self.scope_unresolved_diagnostics += unresolved_excluded;
        self.scope_resolved_t0_diagnostics += resolved_excluded;
        self.supported_oracle_diagnostics += supported_expected.len();
        self.supported_tsrs_diagnostics += supported_actual.len();
        self.supported_fn_count += supported_fn.len();
        self.oracle_diagnostics += expected.len();
        self.tsrs_diagnostics += actual.len();
        self.fp_count += fp.len();
        self.fn_count += fn_.len();
        self.case_count += 1;
        Ok(())
    }

    fn finish(
        self,
        fixtures_total: usize,
        scope_status: &str,
        scope_manifest_entries: usize,
    ) -> FinishedConformanceView {
        let t0_rate = shadow_rate(self.matched_t0_diagnostics, self.oracle_diagnostics);
        let summary = ConformanceSummary {
            band: self.band.name().to_owned(),
            fixtures_total,
            cases_total: self.case_count,
            oracle_diagnostics: self.oracle_diagnostics,
            tsrs_diagnostics: self.tsrs_diagnostics,
            matched_t0_diagnostics: self.matched_t0_diagnostics,
            t0_rate,
            shadow_t1_matched: self.shadow_t1_matched,
            shadow_t2_matched: self.shadow_t2_matched,
            shadow_t3_matched: self.shadow_t3_matched,
            shadow_t1_rate: shadow_rate(self.shadow_t1_matched, self.oracle_diagnostics),
            shadow_t2_rate: shadow_rate(self.shadow_t2_matched, self.oracle_diagnostics),
            shadow_t3_rate: shadow_rate(self.shadow_t3_matched, self.oracle_diagnostics),
            shadow_tier_identities: ShadowTierObservation::new(
                self.shadow_oracle_records,
                self.shadow_t1_identities,
                self.shadow_t2_identities,
                self.shadow_t3_identities,
            ),
            exact_match_cases: self.exact_match_cases,
            mismatch_cases: self.case_count - self.exact_match_cases,
            false_positive_diagnostics: self.fp_count,
            false_negative_diagnostics: self.fn_count,
            fn_with_partial_boundary_evidence: self.fn_with_partial_boundary_count,
            fn_without_partial_boundary_evidence: self.fn_without_partial_boundary_count,
            top_fn_partial_boundary_reasons: top_string_counts(self.fn_trigger_reasons),
            top_false_positive_codes: top_codes(self.fp_codes),
            top_false_negative_codes: top_codes(self.fn_codes),
            scope_status: scope_status.to_owned(),
            scope_manifest_entries,
            scope_excluded_diagnostics: self.scope_excluded_diagnostics,
            scope_unresolved_diagnostics: self.scope_unresolved_diagnostics,
            scope_resolved_t0_diagnostics: self.scope_resolved_t0_diagnostics,
            supported_oracle_diagnostics: self.supported_oracle_diagnostics,
            supported_tsrs_diagnostics: self.supported_tsrs_diagnostics,
            supported_matched_t0_diagnostics: self.supported_matched_t0_diagnostics,
            supported_t0_rate: shadow_rate(
                self.supported_matched_t0_diagnostics,
                self.supported_oracle_diagnostics,
            ),
            supported_t1_matched: self.supported_t1_matched,
            supported_t2_matched: self.supported_t2_matched,
            supported_t3_matched: self.supported_t3_matched,
            supported_t1_rate: shadow_rate(
                self.supported_t1_matched,
                self.supported_oracle_diagnostics,
            ),
            supported_t2_rate: shadow_rate(
                self.supported_t2_matched,
                self.supported_oracle_diagnostics,
            ),
            supported_t3_rate: shadow_rate(
                self.supported_t3_matched,
                self.supported_oracle_diagnostics,
            ),
            supported_shadow_tier_identities: ShadowTierObservation::new(
                self.supported_shadow_oracle_records,
                self.supported_t1_identities,
                self.supported_t2_identities,
                self.supported_t3_identities,
            ),
            supported_tier_mismatches: self.supported_tier_mismatches,
            supported_exact_match_cases: self.supported_exact_match_cases,
            supported_mismatch_cases: self.case_count - self.supported_exact_match_cases,
            supported_false_negative_diagnostics: self.supported_fn_count,
            supported_false_negative_identities: self
                .supported_false_negative_identities
                .into_iter()
                .collect(),
            ratchet_rate: self.ratchet.rate,
            ratchet_allowed_regression: self.ratchet.allowed_regression,
            mismatches: self.mismatches,
        };
        FinishedConformanceView {
            band: self.band,
            summary,
            sets: self.run_sets,
            ratchet: self.ratchet,
            t1_ratchet: self.t1_ratchet,
        }
    }
}

struct FinishedConformanceView {
    band: DiagnosticBand,
    summary: ConformanceSummary,
    sets: ratchet::RunSets,
    ratchet: Ratchet,
    t1_ratchet: Option<Ratchet>,
}

enum FinishedViewMeasurement {
    Full(Box<FinishedConformanceView>),
    RunSetsOnly {
        band: DiagnosticBand,
        sets: ratchet::RunSets,
    },
}

impl ViewAccumulatorKind {
    fn finish(
        self,
        fixtures_total: usize,
        scope_status: &str,
        scope_manifest_entries: usize,
    ) -> ConformanceResult<FinishedViewMeasurement> {
        Ok(match self {
            Self::Full(accumulator) => FinishedViewMeasurement::Full(Box::new(
                (*accumulator).finish(fixtures_total, scope_status, scope_manifest_entries),
            )),
            Self::RunSetsOnly(accumulator) => FinishedViewMeasurement::RunSetsOnly {
                band: accumulator.band,
                sets: accumulator.run_sets,
            },
        })
    }
}

impl FinishedViewMeasurement {
    fn band(&self) -> DiagnosticBand {
        match self {
            Self::Full(view) => view.band,
            Self::RunSetsOnly { band, .. } => *band,
        }
    }

    fn into_full(self) -> ConformanceResult<FinishedConformanceView> {
        match self {
            Self::Full(view) => Ok(*view),
            Self::RunSetsOnly { band, .. } => Err(format!(
                "fixed {} view did not retain the summary required by this gate",
                band.name()
            )
            .into()),
        }
    }
}

struct MeasuredView {
    band: DiagnosticBand,
    result: ConformanceResult<FinishedViewMeasurement>,
}

struct MeasuredConformance {
    views: Vec<MeasuredView>,
    observation: Option<families::Observation>,
    accepted: Option<ratchet::AcceptedState>,
    executed_fixtures: BTreeSet<String>,
    full_run: bool,
}

#[allow(clippy::too_many_arguments)]
fn measure_conformance(
    options: &ConformanceOptions,
    views: &[DiagnosticBand],
    set_gate: SetGate,
    families_observe: bool,
    planned_t4_pins: Option<&ratchet::T4OraclePins>,
    planned_t4_empty_related_information: Option<&rendered::T4OracleEmptyRelatedInformation>,
    force_t4_measurement: bool,
) -> ConformanceResult<MeasuredConformance> {
    measure_conformance_with(
        options,
        views,
        set_gate,
        families_observe,
        planned_t4_pins,
        planned_t4_empty_related_information,
        force_t4_measurement,
        current_case_tsrs,
    )
}

fn initialize_view_accumulators(
    views: &[DiagnosticBand],
    set_gate: SetGate,
    selected_band: DiagnosticBand,
    ratchet_path: &Path,
) -> Vec<PendingViewAccumulator> {
    views
        .iter()
        .copied()
        .map(|view| {
            let state = if set_gate == SetGate::Collect && view != selected_band {
                Ok(ViewAccumulatorKind::RunSetsOnly(RunSetsAccumulator::new(
                    view,
                )))
            } else {
                ViewAccumulator::new(view, ratchet_path)
                    .map(Box::new)
                    .map(ViewAccumulatorKind::Full)
            };
            PendingViewAccumulator { band: view, state }
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn measure_conformance_with(
    options: &ConformanceOptions,
    views: &[DiagnosticBand],
    set_gate: SetGate,
    families_observe: bool,
    planned_t4_pins: Option<&ratchet::T4OraclePins>,
    planned_t4_empty_related_information: Option<&rendered::T4OracleEmptyRelatedInformation>,
    force_t4_measurement: bool,
    mut execute_case: impl FnMut(&str, &tsc_harness::ProgramJson, &Path) -> ConformanceResult<CaseTsrs>,
) -> ConformanceResult<MeasuredConformance> {
    if views.is_empty() {
        return Err("conformance measurement requires at least one fixed view".into());
    }
    if views
        .iter()
        .enumerate()
        .any(|(index, view)| views[..index].contains(view))
    {
        return Err("conformance measurement contains duplicate fixed views".into());
    }
    let fixtures = select_fixtures(&RefreshOptions {
        workspace: options.workspace.clone(),
        limit: options.limit,
        files: options.files.clone(),
    })?;
    let vendor_lib_dir = options.workspace.join("vendor/typescript-6.0.3/lib");
    let goldens_root = options.workspace.join("goldens");
    let ratchet_path = options.workspace.join("ratchet.toml");
    let full_run = options.limit.is_none() && options.files.is_empty();
    if families_observe {
        families::ensure_observation_eligible(options.band, full_run)?;
        if !views.contains(&DiagnosticBand::All) {
            return Err("families observation requires the All fixed view".into());
        }
    }
    if planned_t4_pins.is_some() != planned_t4_empty_related_information.is_some() {
        return Err(
            "planned T4 pins and empty-related-information metadata must be supplied together"
                .into(),
        );
    }
    let accepted = match set_gate {
        SetGate::Enforce => Some(ratchet::load_accepted_for_gating(&options.workspace)?),
        SetGate::Collect => None,
    };
    let measure_t4 = options.band == DiagnosticBand::All
        && views.contains(&DiagnosticBand::All)
        && (force_t4_measurement
            || planned_t4_pins.is_some()
            || accepted.as_ref().is_some_and(|accepted| accepted.t4_active));
    // Each fixed view owns its error state. The shared fixture/checker pass
    // continues after a later-view initialization or grading error so CI can
    // preserve the historical All -> 2xxx -> syntactic gate/callback order.
    // Ratchet collection retains full summary vectors only for its selected
    // view; the other projections need identity sets alone.
    let mut accumulators =
        initialize_view_accumulators(views, set_gate, options.band, &ratchet_path);
    let mut observation = families_observe.then(families::Observation::default);
    let mut scope = ScopeManifest::load(&options.workspace.join("m8-scope.json"))?;
    let mut executed_fixtures = BTreeSet::new();

    for fixture in &fixtures {
        let fixture_key = fixture_key(&options.workspace, fixture)?;
        let golden = read_golden(&goldens_root, &fixture_key)?;
        if golden.schema < 2 {
            if let Some(syntactic) = accumulators
                .iter_mut()
                .find(|accumulator| accumulator.band == DiagnosticBand::Syntactic)
            {
                if syntactic.state.is_ok() {
                    syntactic.state = Err(format!(
                        "golden {fixture_key} has schema {} without pass provenance; \
                         run `cargo xtask oracle-refresh`",
                        golden.schema
                    )
                    .into());
                }
            }
        }
        executed_fixtures.insert(fixture_key.clone());
        let golden_by_matrix = golden
            .cases
            .iter()
            .map(|case| (case.matrix_key.as_str(), case))
            .collect::<BTreeMap<_, _>>();
        let programs = tsc_harness::expand_fixture_file(fixture, &vendor_lib_dir)?;
        let expanded_keys = programs
            .iter()
            .map(|program| program.matrix_key.as_str())
            .collect::<BTreeSet<_>>();
        if let Some(orphan) = orphan_golden_case(&golden.cases, &expanded_keys) {
            return Err(format!(
                "golden case {fixture_key} [{orphan}] has no expanded program; the goldens \
                 and the expansion matrix have drifted — refresh the goldens under a \
                 reviewed transition before gating on them"
            )
            .into());
        }

        for program in programs {
            let golden_case = golden_by_matrix
                .get(program.matrix_key.as_str())
                .ok_or_else(|| {
                    format!("missing golden case {fixture_key} [{}]", program.matrix_key)
                })?;
            let case_tsrs = execute_case(&fixture_key, &program, &vendor_lib_dir)?;
            let excluded_indices = scope.exclusions_for_case(
                &fixture_key,
                &program.matrix_key,
                &golden_case.oracle,
            )?;
            for pending in &mut accumulators {
                let case_observation = if pending.band == DiagnosticBand::All {
                    observation.as_mut()
                } else {
                    None
                };
                let Ok(accumulator) = &mut pending.state else {
                    continue;
                };
                let result = accumulator.observe_case(
                    &fixture_key,
                    &program,
                    golden.schema,
                    golden_case,
                    &case_tsrs,
                    &excluded_indices,
                    &vendor_lib_dir,
                    measure_t4,
                    planned_t4_pins,
                    planned_t4_empty_related_information,
                    case_observation,
                );
                if let Err(error) = result {
                    pending.state = Err(error);
                }
            }
        }
    }

    if full_run && options.band == DiagnosticBand::All && views.contains(&DiagnosticBand::All) {
        scope.finish_full_validation()?;
    }
    if let Some(observation) = observation.as_mut() {
        observation.fixtures_total = fixtures.len();
    }
    let scope_status = scope.status().name().to_owned();
    let scope_manifest_entries = scope.entry_count();
    let views = accumulators
        .into_iter()
        .map(|pending| {
            let band = pending.band;
            let result = pending.state.and_then(|accumulator| {
                if accumulator.band() != band {
                    return Err(format!(
                        "conformance accumulator order mismatch: expected {}, observed {}",
                        band.name(),
                        accumulator.band().name()
                    )
                    .into());
                }
                accumulator.finish(fixtures.len(), &scope_status, scope_manifest_entries)
            });
            MeasuredView { band, result }
        })
        .collect();
    Ok(MeasuredConformance {
        views,
        observation,
        accepted,
        executed_fixtures,
        full_run,
    })
}

fn write_and_enforce_view(
    view: &FinishedConformanceView,
    out_json: &Path,
    accepted: Option<&ratchet::AcceptedState>,
    executed_fixtures: &BTreeSet<String>,
    full_run: bool,
) -> ConformanceResult<()> {
    if let Some(parent) = out_json.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(out_json, serde_json::to_string_pretty(&view.summary)?)?;

    if let Some(accepted) = accepted {
        ratchet::enforce_accepted(
            &accepted.artifact,
            &view.sets,
            view.band,
            executed_fixtures,
            full_run,
        )?;
    }
    let regressed = full_run
        && match (view.ratchet.matched, view.ratchet.total) {
            (Some(matched), Some(total)) if view.summary.ratchet_allowed_regression == 0.0 => {
                (view.summary.matched_t0_diagnostics as u128) * (total as u128)
                    < (matched as u128) * (view.summary.oracle_diagnostics as u128)
            }
            _ => {
                view.summary.t0_rate + view.summary.ratchet_allowed_regression
                    < view.summary.ratchet_rate
            }
        };
    if regressed {
        return Err(format!(
            "T0 ratchet regression: measured {:.6} ({}/{}), required {:.6} (allowed regression {:.6})",
            view.summary.t0_rate,
            view.summary.matched_t0_diagnostics,
            view.summary.oracle_diagnostics,
            view.summary.ratchet_rate,
            view.summary.ratchet_allowed_regression
        )
        .into());
    }
    if let Some(t1_ratchet) = view.t1_ratchet.filter(|_| full_run) {
        let t1_regressed = match (t1_ratchet.matched, t1_ratchet.total) {
            (Some(matched), Some(total)) if t1_ratchet.allowed_regression == 0.0 => {
                (view.summary.shadow_t1_matched as u128) * (total as u128)
                    < (matched as u128) * (view.summary.oracle_diagnostics as u128)
            }
            _ => view.summary.shadow_t1_rate + t1_ratchet.allowed_regression < t1_ratchet.rate,
        };
        if t1_regressed {
            return Err(format!(
                "T1 ratchet regression: measured {:.6} ({}/{}), required {:.6} (allowed regression {:.6})",
                view.summary.shadow_t1_rate,
                view.summary.shadow_t1_matched,
                view.summary.oracle_diagnostics,
                t1_ratchet.rate,
                t1_ratchet.allowed_regression
            )
            .into());
        }
    }
    if view.summary.false_positive_diagnostics > 0 {
        return Err(format!(
            "NEW_FP hard gate failed: {} false positive diagnostics",
            view.summary.false_positive_diagnostics
        )
        .into());
    }
    if view.summary.scope_status == "frozen" && view.summary.scope_resolved_t0_diagnostics > 0 {
        return Err(format!(
            "stale M8 scope gate failed: {} excluded diagnostic(s) now match at T0; delete their dispositions so higher tiers grade them",
            view.summary.scope_resolved_t0_diagnostics
        )
        .into());
    }
    Ok(())
}

fn run_conformance_inner(
    options: &ConformanceOptions,
    set_gate: SetGate,
    families_observe: bool,
    planned_t4_pins: Option<&ratchet::T4OraclePins>,
    planned_t4_empty_related_information: Option<&rendered::T4OracleEmptyRelatedInformation>,
    force_t4_measurement: bool,
) -> ConformanceResult<ConformanceRun> {
    // Ratchet updates collect all fixed identity sets in one traversal;
    // ordinary conformance gates only the explicitly selected view.
    let measured_views = match set_gate {
        SetGate::Collect => ratchet::FIXED_VIEWS.as_slice(),
        SetGate::Enforce => std::slice::from_ref(&options.band),
    };
    let mut measured = measure_conformance(
        options,
        measured_views,
        set_gate,
        families_observe,
        planned_t4_pins,
        planned_t4_empty_related_information,
        force_t4_measurement,
    )?;

    if set_gate == SetGate::Collect {
        let mut selected_summary = None;
        let mut run_sets = ratchet::RunSets::new();
        for measured_view in measured.views {
            let view = measured_view.result?;
            if view.band() != measured_view.band {
                return Err(format!(
                    "conformance measured-view order mismatch: expected {}, observed {}",
                    measured_view.band.name(),
                    view.band().name()
                )
                .into());
            }
            match view {
                FinishedViewMeasurement::Full(view) => {
                    let view = *view;
                    run_sets.extend(view.sets);
                    if view.band == options.band {
                        selected_summary = Some(view.summary);
                    }
                }
                FinishedViewMeasurement::RunSetsOnly { band, sets } => {
                    if band == options.band {
                        return Err(format!(
                            "selected {} conformance view retained only ratchet sets",
                            band.name()
                        )
                        .into());
                    }
                    run_sets.extend(sets);
                }
            }
        }
        let summary = selected_summary.ok_or_else(|| {
            format!(
                "selected {} conformance view was not measured",
                options.band.name()
            )
        })?;
        if let Some(parent) = options.out_json.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&options.out_json, serde_json::to_string_pretty(&summary)?)?;
        return Ok(ConformanceRun {
            summary,
            sets: run_sets,
            observation: measured.observation,
        });
    }

    let measured_view = measured
        .views
        .pop()
        .ok_or("a gating run did not measure its selected view")?;
    if !measured.views.is_empty() || measured_view.band != options.band {
        return Err(format!(
            "gating conformance view mismatch: expected exactly {}, observed {} view(s) ending in {}",
            options.band.name(),
            measured.views.len() + 1,
            measured_view.band.name()
        )
        .into());
    }
    let view = measured_view.result?.into_full()?;
    if view.band != options.band {
        return Err(format!(
            "gating conformance result mismatch: expected {}, observed {}",
            options.band.name(),
            view.band.name()
        )
        .into());
    }
    write_and_enforce_view(
        &view,
        &options.out_json,
        measured.accepted.as_ref(),
        &measured.executed_fixtures,
        measured.full_run,
    )?;
    Ok(ConformanceRun {
        summary: view.summary,
        sets: view.sets,
        observation: measured.observation,
    })
}
fn exact_supported_false_negative_identities(
    fixture: &str,
    matrix_key: &str,
    oracle: &[GoldenDiag],
    band: DiagnosticBand,
    excluded_indices: &BTreeSet<usize>,
    supported_false_negative_buckets: &BTreeSet<T0Key>,
) -> ConformanceResult<Vec<ExactIdentity>> {
    let identities = identity::assign_case_identities(fixture, matrix_key, oracle)?;
    Ok(oracle
        .iter()
        .zip(identities)
        .enumerate()
        .filter(|(index, (diagnostic, _))| {
            band.matches_oracle(diagnostic)
                && !excluded_indices.contains(index)
                && supported_false_negative_buckets.contains(&t0_key(diagnostic))
        })
        .map(|(_, (_, identity))| identity)
        .collect())
}

/// A golden case whose matrix key no expanded program carries. Both
/// sides come from the same fixture in a coherent tree; a mismatch
/// means a harness/expansion change landed without an oracle refresh,
/// and the A5 domain (golden-derived) would silently disagree with
/// the run observation (expansion-derived) if this were let through.
fn orphan_golden_case<'a>(
    cases: &'a [GoldenCase],
    expanded_keys: &BTreeSet<&str>,
) -> Option<&'a str> {
    cases
        .iter()
        .map(|case| case.matrix_key.as_str())
        .find(|key| !expanded_keys.contains(key))
}

fn shadow_rate(matched: usize, total: usize) -> f64 {
    if total == 0 {
        1.0
    } else {
        matched as f64 / total as f64
    }
}

/// Tier grading. Before the one-time M8 activation these fields remain
/// report-only shadow evidence; afterwards the identical bucket sets
/// are persisted in and gated by A1. A key contributes 1 to a tier
/// only when the two buckets are equal AS MULTISETS under that tier's
/// OWN equivalence (review round 3: tiers compare independently — T1
/// must not depend on how T2's finer key would pair elements):
///   T1 = category
///   T2 = T1 + exact start/length + top message text
///   T3 = T2 + full chain tree + relatedInformation
/// The equivalences nest, so equal-T3 multisets imply equal-T2 imply
/// equal-T1, and per-key counting keeps the tiers nested under
/// matched_t0's set semantics. tsrs-side related info flows through
/// from_tsrs since pre-5.8a (it was dropped before).
#[derive(Default)]
struct ShadowTierMatches {
    t1: BTreeSet<T0Key>,
    t2: BTreeSet<T0Key>,
    t3: BTreeSet<T0Key>,
}

fn shadow_tier_matches<'a>(
    actual: impl Iterator<Item = &'a GoldenDiag>,
    expected: impl Iterator<Item = &'a GoldenDiag>,
) -> ShadowTierMatches {
    let sets = ratchet::bucket_sets(expected, actual);
    ShadowTierMatches {
        t1: sets.t1,
        t2: sets.t2,
        t3: sets.t3,
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_supported_tier_mismatches(
    fixture: &str,
    matrix_key: &str,
    actual: &[GoldenDiag],
    expected: &[GoldenDiag],
    band: DiagnosticBand,
    excluded_indices: &BTreeSet<usize>,
    fully_excluded: &BTreeSet<T0Key>,
    supported_expected: &BTreeSet<T0Key>,
    matches: &ShadowTierMatches,
) -> Vec<SupportedTierMismatch> {
    supported_expected
        .iter()
        .filter_map(|diagnostic| {
            let first_failed_tier = if !matches.t1.contains(diagnostic) {
                "t1"
            } else if !matches.t2.contains(diagnostic) {
                "t2"
            } else if !matches.t3.contains(diagnostic) {
                "t3"
            } else {
                return None;
            };
            let actual = actual
                .iter()
                .filter(|candidate| {
                    band.contains(candidate.code)
                        && !fully_excluded.contains(&t0_key(candidate))
                        && t0_key(candidate) == *diagnostic
                })
                .cloned()
                .collect();
            let expected = expected
                .iter()
                .enumerate()
                .filter(|(index, candidate)| {
                    band.matches_oracle(candidate)
                        && !excluded_indices.contains(index)
                        && t0_key(candidate) == *diagnostic
                })
                .map(|(_, diagnostic)| diagnostic.clone())
                .collect();
            Some(SupportedTierMismatch {
                fixture: fixture.to_owned(),
                matrix_key: matrix_key.to_owned(),
                diagnostic: diagnostic.clone(),
                first_failed_tier: first_failed_tier.to_owned(),
                actual,
                expected,
            })
        })
        .collect()
}

fn extend_shadow_identities(
    identities: &mut BTreeSet<ShadowTierIdentity>,
    fixture: &str,
    matrix_key: &str,
    diagnostics: BTreeSet<T0Key>,
) {
    identities.extend(
        diagnostics
            .into_iter()
            .map(|diagnostic| ShadowTierIdentity {
                fixture: fixture.to_owned(),
                matrix_key: matrix_key.to_owned(),
                diagnostic,
            }),
    );
}

impl GoldenDiag {
    fn from_oracle(diag: &OracleDiag, file_texts: &BTreeMap<String, String>) -> Self {
        let (line, col) = line_col_for_oracle(diag, file_texts);
        Self {
            file: diag.file.clone(),
            start: diag.start,
            length: diag.length,
            line,
            col,
            code: diag.code,
            pass: diag.pass.clone(),
            category: diag.category.clone(),
            chain: GoldenMessageChain::from_oracle(&diag.chain),
            related: diag
                .related
                .iter()
                .map(|related| GoldenRelated {
                    file: related.file.clone(),
                    start: related.start,
                    length: related.length,
                    code: related.code,
                    category: related.category.clone(),
                    chain: GoldenMessageChain::from_oracle(&related.chain),
                })
                .collect(),
            reports_unnecessary: diag.reports_unnecessary,
            reports_deprecated: diag.reports_deprecated,
            source: diag.source.clone(),
        }
    }

    fn from_tsrs(diag: &Diagnostic, file_texts: &BTreeMap<String, String>) -> Self {
        let (line, col) = line_col_for_tsrs(diag, file_texts);
        Self {
            file: diag.file_name.clone(),
            start: diag.start,
            length: diag.length,
            line,
            col,
            code: diag.code(),
            pass: None,
            category: diag.category().name().to_owned(),
            chain: GoldenMessageChain::from_tsrs(&diag.message),
            related: diag
                .related
                .iter()
                .map(|related| GoldenRelated {
                    file: related.file_name.clone(),
                    start: related.start,
                    length: related.length,
                    code: related.message.code,
                    category: related.message.category.name().to_owned(),
                    chain: GoldenMessageChain::from_tsrs(&related.message),
                })
                .collect(),
            reports_unnecessary: diag.reports_unnecessary.unwrap_or(false),
            reports_deprecated: diag.reports_deprecated.unwrap_or(false),
            source: diag.source.clone(),
        }
    }
}

impl GoldenMessageChain {
    fn from_oracle(chain: &OracleMessageChain) -> Self {
        Self {
            text: chain.text.clone(),
            code: chain.code,
            category: chain.category.clone(),
            next: chain.next.iter().map(Self::from_oracle).collect(),
        }
    }

    fn from_tsrs(chain: &MessageChain) -> Self {
        Self {
            text: chain.text.clone(),
            code: chain.code,
            category: chain.category.name().to_owned(),
            next: chain.next.iter().map(Self::from_tsrs).collect(),
        }
    }
}

/// The lib texts for a program, read from the vendored lib directory
/// (the same files the oracle host loads for programJson.libs). The
/// corpus reuses a handful of lib sets across thousands of cases and
/// re-reading ~9MB of vendored libs per case dominated the conformer,
/// so the loaded set is cached per (lib dir, lib list).
fn read_lib_inputs(
    libs: &[String],
    vendor_lib_dir: &Path,
) -> ConformanceResult<Arc<Vec<InputFile>>> {
    type LibInputKey = (PathBuf, Vec<String>);
    static CACHE: OnceLock<Mutex<BTreeMap<LibInputKey, Arc<Vec<InputFile>>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    let key = (vendor_lib_dir.to_owned(), libs.to_vec());
    if let Some(inputs) = cache.lock().expect("lib input cache").get(&key) {
        return Ok(inputs.clone());
    }

    let inputs = libs
        .iter()
        .map(|name| {
            let text = fs::read_to_string(vendor_lib_dir.join(name))
                .map_err(|err| format!("failed to read lib {name}: {err}"))?;
            Ok(InputFile {
                name: name.clone(),
                text,
            })
        })
        .collect::<ConformanceResult<Vec<_>>>()?;
    let inputs = Arc::new(inputs);
    cache
        .lock()
        .expect("lib input cache")
        .insert(key, inputs.clone());
    Ok(inputs)
}

/// One case's tsrs diagnostic streams. A single checker execution
/// yields both the aggregate pass (the All/2XXX source) and the
/// syntactic pass, so one run grades every fixed view.
struct CaseTsrs {
    all: Vec<GoldenDiag>,
    /// Indices in the canonical aggregate stream whose Rust
    /// Diagnostic has a present-but-empty related-information property.
    /// GoldenDiag intentionally cannot serialize this formatter-only
    /// distinction because schema 2 fixed the structured oracle bytes.
    all_empty_related_information: BTreeSet<usize>,
    syntactic: Vec<GoldenDiag>,
    partial_checks: Vec<PartialCheck>,
}

fn current_case_tsrs(
    fixture: &str,
    program: &tsc_harness::ProgramJson,
    vendor_lib_dir: &Path,
) -> ConformanceResult<CaseTsrs> {
    let mut files = Vec::new();
    let mut file_texts = BTreeMap::new();

    for file in &program.files {
        let text = base64_decode_to_string(&file.text_b64)?;
        file_texts.insert(file.name.clone(), text.clone());
        files.push(InputFile {
            name: file.name.clone(),
            text,
        });
    }

    let libs = read_lib_inputs(&program.libs, vendor_lib_dir)?;
    if h0_memory::supports_fixture(fixture) {
        let result = h0_memory::run(program, libs.as_slice(), &files)?;
        let all_empty_related_information = result
            .all
            .iter()
            .enumerate()
            .filter_map(|(index, diagnostic)| {
                (diagnostic.related_information_present && diagnostic.related.is_empty())
                    .then_some(index)
            })
            .collect();
        return Ok(CaseTsrs {
            all: result
                .all
                .iter()
                .map(|diag| GoldenDiag::from_tsrs(diag, &file_texts))
                .collect(),
            all_empty_related_information,
            syntactic: result
                .syntactic
                .iter()
                .map(|diag| GoldenDiag::from_tsrs(diag, &file_texts))
                .collect(),
            partial_checks: Vec::new(),
        });
    }
    let result = check_program_with_libs_at(
        &libs,
        &files,
        &tsc_harness::compiler_options_from_program(program),
        &program.cwd,
    );
    let all_empty_related_information = result
        .diagnostics
        .iter()
        .enumerate()
        .filter_map(|(index, diagnostic)| {
            (diagnostic.related_information_present && diagnostic.related.is_empty())
                .then_some(index)
        })
        .collect();
    Ok(CaseTsrs {
        all: result
            .diagnostics
            .iter()
            .map(|diag| GoldenDiag::from_tsrs(diag, &file_texts))
            .collect(),
        all_empty_related_information,
        syntactic: result
            .syntactic_diagnostics
            .iter()
            .map(|diag| GoldenDiag::from_tsrs(diag, &file_texts))
            .collect(),
        partial_checks: result.partial_checks,
    })
}

fn file_texts_for_program(
    program: &tsc_harness::ProgramJson,
    vendor_lib_dir: &Path,
) -> ConformanceResult<BTreeMap<String, String>> {
    let mut file_texts = BTreeMap::new();
    for lib in read_lib_inputs(&program.libs, vendor_lib_dir)?.iter() {
        file_texts.insert(lib.name.clone(), lib.text.clone());
    }
    for file in &program.files {
        file_texts.insert(file.name.clone(), base64_decode_to_string(&file.text_b64)?);
    }
    Ok(file_texts)
}

fn line_col_for_oracle(
    diag: &OracleDiag,
    file_texts: &BTreeMap<String, String>,
) -> (Option<u32>, Option<u32>) {
    let Some(file_name) = &diag.file else {
        return (None, None);
    };
    let Some(start) = diag.start else {
        return (None, None);
    };
    let Some(text) = file_texts.get(file_name) else {
        return (None, None);
    };
    let map = compute_line_map(text);
    let line_col = get_line_and_character_of_position(&map.line_starts, start);
    (Some(line_col.line), Some(line_col.character))
}

fn line_col_for_tsrs(
    diag: &Diagnostic,
    file_texts: &BTreeMap<String, String>,
) -> (Option<u32>, Option<u32>) {
    let Some(file_name) = &diag.file_name else {
        return (None, None);
    };
    // Diagnostic.start is already UTF-16 (the parser converts when pushing);
    // converting again through byte_to_utf16 shifted columns on files with
    // non-ASCII text.
    let Some(start) = diag.start else {
        return (None, None);
    };
    let Some(text) = file_texts.get(file_name) else {
        return (None, None);
    };
    let map = compute_line_map(text);
    let line_col = get_line_and_character_of_position(&map.line_starts, start);
    (Some(line_col.line), Some(line_col.character))
}

pub(crate) fn t0_key(diag: &GoldenDiag) -> T0Key {
    T0Key {
        file: diag.file.clone(),
        code: diag.code,
        line: diag.line,
        col: diag.col,
    }
}

fn classify_fn_partial_boundaries(
    false_negatives: &[T0Key],
    oracle: &[GoldenDiag],
    partial_checks: &[PartialCheck],
) -> Vec<FnPartialBoundaryAudit> {
    false_negatives
        .iter()
        .map(|key| {
            let mut reasons = BTreeSet::new();
            for diagnostic in oracle.iter().filter(|diagnostic| {
                diagnostic.pass.as_deref() != Some("syntactic") && t0_key(diagnostic) == *key
            }) {
                let (Some(file), Some(start)) = (&diagnostic.file, diagnostic.start) else {
                    continue;
                };
                for partial in partial_checks.iter().filter(|partial| {
                    partial.file_name == *file
                        && start >= partial.start
                        && start < partial.start.saturating_add(partial.length.max(1))
                }) {
                    reasons.insert(partial.reason.clone());
                }
            }
            FnPartialBoundaryAudit {
                diagnostic: key.clone(),
                reached_partial_boundary: !reasons.is_empty(),
                reasons: reasons.into_iter().collect(),
            }
        })
        .collect()
}

fn t0_set<'a>(diagnostics: impl Iterator<Item = &'a GoldenDiag>) -> BTreeSet<T0Key> {
    diagnostics.map(t0_key).collect()
}

fn write_golden(root: &Path, golden: &GoldenFile) -> ConformanceResult<()> {
    let path = golden_path(root, &golden.fixture);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, encode_golden(golden)?)?;
    Ok(())
}

pub(crate) fn encode_golden(golden: &GoldenFile) -> ConformanceResult<Vec<u8>> {
    let json = serde_json::to_vec_pretty(golden)?;
    Ok(zstd::stream::encode_all(json.as_slice(), 3)?)
}

fn read_golden(root: &Path, fixture: &str) -> ConformanceResult<GoldenFile> {
    let path = golden_path(root, fixture);
    let bytes = fs::read(path)?;
    let mut decoder = zstd::stream::Decoder::new(bytes.as_slice())?;
    let mut json = String::new();
    decoder.read_to_string(&mut json)?;
    Ok(serde_json::from_str(&json)?)
}

fn golden_path(root: &Path, fixture: &str) -> PathBuf {
    root.join(format!("{fixture}.json.zst"))
}

fn fixture_key(workspace: &Path, fixture: &Path) -> ConformanceResult<String> {
    let corpus_root = workspace.join("ts-tests/tests/cases");
    let rel = fixture.strip_prefix(&corpus_root)?;
    Ok(rel.to_string_lossy().replace('\\', "/"))
}

fn select_fixtures(options: &RefreshOptions) -> ConformanceResult<Vec<PathBuf>> {
    let mut fixtures = if options.files.is_empty() {
        collect_fixture_paths(&options.workspace.join("ts-tests/tests/cases/conformance"))?
    } else {
        options
            .files
            .iter()
            .map(|path| {
                if path.is_absolute() {
                    path.clone()
                } else {
                    options.workspace.join(path)
                }
            })
            .collect()
    };
    fixtures.sort();
    if let Some(limit) = options.limit {
        fixtures.truncate(limit);
    }
    Ok(fixtures)
}

fn collect_fixture_paths(root: &Path) -> ConformanceResult<Vec<PathBuf>> {
    let mut stack = vec![root.to_owned()];
    let mut fixtures = Vec::new();
    while let Some(path) = stack.pop() {
        for entry in fs::read_dir(&path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if is_fixture_path(&path) {
                fixtures.push(path);
            }
        }
    }
    Ok(fixtures)
}

fn is_fixture_path(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("ts" | "tsx" | "js" | "jsx")
    )
}

fn stable_json_hash<T: Serialize>(value: &T) -> ConformanceResult<String> {
    let json = serde_json::to_vec(value)?;
    let mut hash = 0xcbf29ce484222325u64;
    for byte in json {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    Ok(format!("{hash:016x}"))
}

fn top_codes(codes: BTreeMap<u32, usize>) -> Vec<(u32, usize)> {
    let mut codes = codes.into_iter().collect::<Vec<_>>();
    codes.sort_by(|(left_code, left_count), (right_code, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| left_code.cmp(right_code))
    });
    codes.truncate(20);
    codes
}

fn top_string_counts(counts: BTreeMap<String, usize>) -> Vec<(String, usize)> {
    let mut counts = counts.into_iter().collect::<Vec<_>>();
    counts.sort_by(|(left_name, left_count), (right_name, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| left_name.cmp(right_name))
    });
    counts.truncate(20);
    counts
}

#[derive(Clone, Copy, Debug)]
struct Ratchet {
    rate: f64,
    /// Exact matched/total counts; when present the zero-regression
    /// gate compares integers instead of the rounded `rate`.
    matched: Option<u64>,
    total: Option<u64>,
    allowed_regression: f64,
}

fn read_ratchet(path: &Path, band: DiagnosticBand) -> ConformanceResult<Ratchet> {
    read_ratchet_section(path, band.ratchet_key())
}

fn parse_ratchet_document(path: &Path, text: &str) -> ConformanceResult<DocumentMut> {
    text.parse::<DocumentMut>()
        .map_err(|err| format!("invalid ratchet.toml at {}: {err}", path.display()).into())
}

fn ratchet_section<'a>(
    document: &'a DocumentMut,
    path: &Path,
    section: &str,
) -> ConformanceResult<&'a Table> {
    document
        .as_table()
        .get(section)
        .and_then(Item::as_table)
        .ok_or_else(|| {
            format!(
                "missing ratchet.toml section [{section}] in {}",
                path.display()
            )
            .into()
        })
}

fn ratchet_float(
    table: &Table,
    path: &Path,
    section: &str,
    key: &str,
) -> ConformanceResult<Option<f64>> {
    let Some(item) = table.get(key) else {
        return Ok(None);
    };
    let parsed = item
        .as_float()
        .or_else(|| item.as_integer().map(|value| value as f64))
        .ok_or_else(|| format!("[{section}].{key} must be a number in {}", path.display()))?;
    if !parsed.is_finite() {
        return Err(format!("[{section}].{key} must be finite in {}", path.display()).into());
    }
    Ok(Some(parsed))
}

fn ratchet_u64(
    table: &Table,
    path: &Path,
    section: &str,
    key: &str,
) -> ConformanceResult<Option<u64>> {
    let Some(item) = table.get(key) else {
        return Ok(None);
    };
    let value = item.as_integer().ok_or_else(|| {
        format!(
            "[{section}].{key} must be a non-negative integer in {}",
            path.display()
        )
    })?;
    Ok(Some(u64::try_from(value).map_err(|_| {
        format!(
            "[{section}].{key} must be a non-negative integer in {}",
            path.display()
        )
    })?))
}

fn read_ratchet_section(path: &Path, section: &str) -> ConformanceResult<Ratchet> {
    let text = fs::read_to_string(path)?;
    let document = parse_ratchet_document(path, &text)?;
    let table = ratchet_section(&document, path, section)?;
    let rate = ratchet_float(table, path, section, "rate")?;
    let matched = ratchet_u64(table, path, section, "matched")?;
    let total = ratchet_u64(table, path, section, "total")?;
    let allowed_regression = ratchet_float(table, path, section, "allowed_regression")?;

    if matched.is_some() != total.is_some() {
        return Err(format!(
            "[{section}] must set both `matched` and `total` (or neither) in {}",
            path.display()
        )
        .into());
    }
    let rate = match (rate, matched, total) {
        (Some(rate), _, _) => rate,
        (None, Some(matched), Some(total)) if total > 0 => matched as f64 / total as f64,
        _ => return Err(format!("missing [{section}].rate in {}", path.display()).into()),
    };
    Ok(Ratchet {
        rate,
        matched,
        total,
        allowed_regression: allowed_regression.unwrap_or(0.0),
    })
}

fn base64_decode_to_string(input: &str) -> ConformanceResult<String> {
    let mut bytes = Vec::with_capacity(input.len() / 4 * 3);
    let mut chunk = [0u8; 4];
    let mut chunk_len = 0usize;

    for byte in input.bytes().filter(|byte| !byte.is_ascii_whitespace()) {
        chunk[chunk_len] = byte;
        chunk_len += 1;
        if chunk_len == 4 {
            decode_base64_chunk(&chunk, &mut bytes)?;
            chunk_len = 0;
        }
    }

    if chunk_len != 0 {
        return Err("invalid base64 length".into());
    }

    Ok(String::from_utf8(bytes)?)
}

fn decode_base64_chunk(chunk: &[u8; 4], out: &mut Vec<u8>) -> ConformanceResult<()> {
    let a = decode_base64_value(chunk[0])?;
    let b = decode_base64_value(chunk[1])?;
    let c = if chunk[2] == b'=' {
        None
    } else {
        Some(decode_base64_value(chunk[2])?)
    };
    let d = if chunk[3] == b'=' {
        None
    } else {
        Some(decode_base64_value(chunk[3])?)
    };

    out.push((a << 2) | (b >> 4));
    if let Some(c) = c {
        out.push(((b & 0b0000_1111) << 4) | (c >> 2));
        if let Some(d) = d {
            out.push(((c & 0b0000_0011) << 6) | d);
        }
    }
    Ok(())
}

fn decode_base64_value(byte: u8) -> ConformanceResult<u8> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(format!("invalid base64 byte: {byte}").into()),
    }
}

fn temp_root(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{prefix}-{}", std::process::id()))
}

/// Shared git harness for the A1/A2/A5 artifact tests: real
/// repositories in temp directories. One process-wide counter keeps
/// paths unique across the three test modules, and environment fixes
/// (like the commit.gpgsign guard) live in exactly one place.
#[cfg(test)]
pub(crate) mod test_git {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub(crate) fn temp_dir(name: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "tsc-rs-test-{name}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub(crate) fn git_test(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args([
                "-c",
                "user.name=tsrs",
                "-c",
                "user.email=tsrs@test",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    pub(crate) fn init_repo(name: &str) -> PathBuf {
        let dir = temp_dir(name);
        git_test(&dir, &["init", "-q", "-b", "main"]);
        dir
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn finish_empty_fixed_views(ratchet_path: &Path) -> Vec<MeasuredView> {
        initialize_view_accumulators(
            &ratchet::FIXED_VIEWS,
            SetGate::Enforce,
            DiagnosticBand::All,
            ratchet_path,
        )
        .into_iter()
        .map(|pending| {
            let result = pending
                .state
                .and_then(|accumulator| accumulator.finish(0, "draft", 0));
            MeasuredView {
                band: pending.band,
                result,
            }
        })
        .collect()
    }

    fn read_summary_band(path: &Path) -> String {
        serde_json::from_slice::<ConformanceSummary>(&fs::read(path).unwrap())
            .unwrap()
            .band
    }

    #[test]
    fn later_view_initialization_error_preserves_prior_callback_order() {
        let directory = test_git::temp_dir("deferred-fixed-view-error");
        let ratchet_path = directory.join("ratchet.toml");
        fs::write(
            &ratchet_path,
            "[t0]\nrate = 0.0\n\
             [t1]\nrate = 0.0\n\
             [t0-2xxx]\nrate = 0.0\n\
             [t0-syntactic]\nrate = \"invalid\"\n",
        )
        .unwrap();

        let measured = finish_empty_fixed_views(&ratchet_path);
        let out_json = directory.join("out.json");
        let mut callbacks = Vec::new();
        let mut all_finishes = 0usize;
        let result = complete_ci_views(
            measured,
            &out_json,
            None,
            &BTreeSet::new(),
            false,
            |_| {
                all_finishes += 1;
                Ok(())
            },
            |summary| callbacks.push((summary.band.clone(), read_summary_band(&out_json))),
        );
        let error = match result {
            Ok(_) => panic!("invalid syntactic ratchet must fail at the syntactic gate"),
            Err(error) => error,
        };

        assert_eq!(all_finishes, 1);
        assert_eq!(
            callbacks,
            [
                ("all".to_owned(), "all".to_owned()),
                ("2xxx".to_owned(), "2xxx".to_owned()),
            ]
        );
        assert_eq!(read_summary_band(&out_json), "2xxx");
        assert!(
            error
                .to_string()
                .contains("[t0-syntactic].rate must be a number"),
            "{error}"
        );
    }

    #[test]
    fn later_view_gate_error_writes_its_summary_without_callback() {
        let directory = test_git::temp_dir("fixed-view-gate-error");
        let ratchet_path = directory.join("ratchet.toml");
        fs::write(
            &ratchet_path,
            "[t0]\nrate = 0.0\n\
             [t1]\nrate = 0.0\n\
             [t0-2xxx]\nrate = 0.0\n\
             [t0-syntactic]\nrate = 2.0\n",
        )
        .unwrap();

        let out_json = directory.join("out.json");
        let mut callbacks = Vec::new();
        let result = complete_ci_views(
            finish_empty_fixed_views(&ratchet_path),
            &out_json,
            None,
            &BTreeSet::new(),
            true,
            |_| Ok(()),
            |summary| callbacks.push(summary.band.clone()),
        );
        let error = match result {
            Ok(_) => panic!("syntactic ratchet regression must fail at its gate"),
            Err(error) => error,
        };

        assert_eq!(callbacks, ["all", "2xxx"]);
        assert_eq!(read_summary_band(&out_json), "syntactic");
        assert!(
            error.to_string().contains("T0 ratchet regression"),
            "{error}"
        );
    }

    #[test]
    fn fixed_view_processor_rejects_noncanonical_order() {
        let mut callbacks = Vec::new();
        let result = process_fixed_views(
            [
                (DiagnosticBand::TwoXxx, Ok(())),
                (DiagnosticBand::All, Ok(())),
                (DiagnosticBand::Syntactic, Ok(())),
            ],
            |band, ()| {
                callbacks.push(band);
                Ok(())
            },
        );
        let error = match result {
            Ok(_) => panic!("reordered fixed views must fail in release builds"),
            Err(error) => error,
        };

        assert!(callbacks.is_empty());
        assert!(
            error.to_string().contains("expected all, observed 2xxx"),
            "{error}"
        );
    }

    #[test]
    fn ratchet_collection_retains_full_state_only_for_selected_view() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap();
        let accumulators = initialize_view_accumulators(
            &ratchet::FIXED_VIEWS,
            SetGate::Collect,
            DiagnosticBand::All,
            &workspace.join("ratchet.toml"),
        );

        assert!(matches!(
            accumulators[0].state,
            Ok(ViewAccumulatorKind::Full(_))
        ));
        assert!(accumulators[1..]
            .iter()
            .all(|pending| matches!(pending.state, Ok(ViewAccumulatorKind::RunSetsOnly(_)))));
    }

    #[test]
    fn fused_fixed_views_match_single_view_grading_and_execute_each_case_once() {
        fn full_view(measured: &MeasuredView) -> &FinishedConformanceView {
            match measured.result.as_ref() {
                Ok(FinishedViewMeasurement::Full(view)) => view.as_ref(),
                Ok(FinishedViewMeasurement::RunSetsOnly { band, .. }) => {
                    panic!("{} unexpectedly retained only run sets", band.name())
                }
                Err(error) => panic!("view measurement failed: {error}"),
            }
        }

        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .to_owned();
        let options = ConformanceOptions {
            workspace: workspace.clone(),
            limit: Some(1),
            files: Vec::new(),
            out_json: test_git::temp_dir("fused-conformance-parity").join("unused.json"),
            band: DiagnosticBand::All,
        };
        let executions = Cell::new(0usize);
        let fused = measure_conformance_with(
            &options,
            &ratchet::FIXED_VIEWS,
            SetGate::Enforce,
            false,
            None,
            None,
            false,
            |fixture, program, vendor_lib_dir| {
                executions.set(executions.get() + 1);
                current_case_tsrs(fixture, program, vendor_lib_dir)
            },
        )
        .unwrap();
        assert_eq!(fused.views.len(), ratchet::FIXED_VIEWS.len());
        assert_eq!(
            executions.get(),
            full_view(&fused.views[0]).summary.cases_total
        );
        assert!(fused
            .views
            .iter()
            .all(|view| full_view(view).summary.cases_total == executions.get()));

        let force_t4_measurement = fused
            .accepted
            .as_ref()
            .is_some_and(|accepted| accepted.t4_active);
        let collected = run_conformance_inner(
            &options,
            SetGate::Collect,
            false,
            None,
            None,
            force_t4_measurement,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_vec(&collected.summary).unwrap(),
            serde_json::to_vec(&full_view(&fused.views[0]).summary).unwrap(),
            "Collect selected-view summary differs from Full measurement"
        );
        let mut full_sets = ratchet::RunSets::new();
        for measured in &fused.views {
            full_sets.extend(full_view(measured).sets.clone());
        }
        assert_eq!(
            collected.sets, full_sets,
            "RunSets-only projections differ from Full accumulators"
        );

        for (index, band) in ratchet::FIXED_VIEWS.iter().copied().enumerate() {
            let mut single_options = options.clone();
            single_options.band = band;
            let single = measure_conformance(
                &single_options,
                std::slice::from_ref(&band),
                SetGate::Enforce,
                false,
                None,
                None,
                false,
            )
            .unwrap();
            assert_eq!(single.views.len(), 1);
            assert_eq!(fused.views[index].band, band);
            let fused_view = full_view(&fused.views[index]);
            let single_view = full_view(&single.views[0]);
            assert_eq!(
                serde_json::to_vec(&fused_view.summary).unwrap(),
                serde_json::to_vec(&single_view.summary).unwrap(),
                "fused {} summary differs from its single-view grade",
                band.name()
            );
            assert_eq!(fused_view.sets, single_view.sets);
        }
    }

    fn diag(category: &str, start: u32, text: &str) -> GoldenDiag {
        GoldenDiag {
            file: Some("a.ts".to_owned()),
            start: Some(start),
            length: Some(1),
            line: Some(1),
            col: Some(1),
            code: 2322,
            pass: None,
            category: category.to_owned(),
            chain: GoldenMessageChain {
                text: text.to_owned(),
                code: 2322,
                category: category.to_owned(),
                next: Vec::new(),
            },
            related: Vec::new(),
            reports_unnecessary: false,
            reports_deprecated: false,
            source: None,
        }
    }

    /// Review round 3: tiers compare independent multisets — a
    /// category-multiset match must register T1 even when the
    /// category↔text CORRESPONDENCE differs (which is a T2 miss),
    /// and multiplicity differences miss every tier.
    #[test]
    fn shadow_tiers_grade_buckets_as_independent_multisets() {
        // Same T0 key (same file/code/line/col): one error + one
        // warning per side, texts swapped across categories.
        let actual = [diag("error", 5, "A"), diag("warning", 5, "B")];
        let expected = [diag("error", 5, "B"), diag("warning", 5, "A")];
        let matched = shadow_tier_matches(actual.iter(), expected.iter());
        assert_eq!(
            (matched.t1.len(), matched.t2.len(), matched.t3.len()),
            (1, 0, 0)
        );

        // Identical buckets → all tiers.
        let actual = [diag("error", 5, "A"), diag("warning", 5, "B")];
        let expected = [diag("warning", 5, "B"), diag("error", 5, "A")];
        let matched = shadow_tier_matches(actual.iter(), expected.iter());
        assert_eq!(
            (matched.t1.len(), matched.t2.len(), matched.t3.len()),
            (1, 1, 1)
        );

        // Multiplicity difference on a shared key → no tier.
        let actual = [diag("error", 5, "A")];
        let expected = [diag("error", 5, "A"), diag("error", 5, "A")];
        let matched = shadow_tier_matches(actual.iter(), expected.iter());
        assert_eq!(
            (matched.t1.len(), matched.t2.len(), matched.t3.len()),
            (0, 0, 0)
        );

        // Chain-tail divergence: T2 matches, T3 misses.
        let mut deep = diag("error", 5, "A");
        deep.chain.next.push(GoldenMessageChain {
            text: "tail".to_owned(),
            code: 1,
            category: "error".to_owned(),
            next: Vec::new(),
        });
        let actual = [deep];
        let expected = [diag("error", 5, "A")];
        let matched = shadow_tier_matches(actual.iter(), expected.iter());
        assert_eq!(
            (matched.t1.len(), matched.t2.len(), matched.t3.len()),
            (1, 1, 0)
        );
    }

    #[test]
    fn supported_tier_residual_preserves_both_bucket_shapes() {
        let actual = [diag("error", 5, "actual")];
        let expected = [diag("suggestion", 6, "expected")];
        let key = t0_key(&expected[0]);
        let matches = shadow_tier_matches(actual.iter(), expected.iter());
        let residual = collect_supported_tier_mismatches(
            "conformance/a.ts",
            "",
            &actual,
            &expected,
            DiagnosticBand::All,
            &BTreeSet::new(),
            &BTreeSet::new(),
            &BTreeSet::from([key.clone()]),
            &matches,
        );
        assert_eq!(residual.len(), 1);
        assert_eq!(residual[0].diagnostic, key);
        assert_eq!(residual[0].first_failed_tier, "t1");
        assert_eq!(residual[0].actual, actual);
        assert_eq!(residual[0].expected, expected);
    }

    #[test]
    fn fn_partial_boundary_audit_requires_a_reached_semantic_range() {
        let mut semantic = diag("error", 5, "A");
        semantic.pass = Some("semantic".to_owned());
        let key = t0_key(&semantic);
        let partial = PartialCheck {
            file_name: "a.ts".to_owned(),
            start: 4,
            length: 3,
            reason: "recognized ceiling".to_owned(),
        };
        let classified = classify_fn_partial_boundaries(
            std::slice::from_ref(&key),
            std::slice::from_ref(&semantic),
            std::slice::from_ref(&partial),
        );
        assert!(classified[0].reached_partial_boundary);
        assert_eq!(classified[0].reasons, ["recognized ceiling"]);

        semantic.pass = Some("syntactic".to_owned());
        let classified =
            classify_fn_partial_boundaries(&[key], &[semantic], std::slice::from_ref(&partial));
        assert!(!classified[0].reached_partial_boundary);
    }

    #[test]
    fn supported_false_negative_plan_uses_exact_nonexcluded_occurrences() {
        let mut first = diag("error", 5, "missing");
        first.pass = Some("semantic".to_owned());
        let second = first.clone();
        let mut unrelated = diag("error", 9, "other");
        unrelated.pass = Some("semantic".to_owned());
        unrelated.line = Some(2);
        unrelated.col = Some(3);
        let oracle = vec![first, second, unrelated];
        let missing = BTreeSet::from([t0_key(&oracle[0])]);

        let identities = exact_supported_false_negative_identities(
            "conformance/a.ts",
            "strict=true",
            &oracle,
            DiagnosticBand::All,
            &BTreeSet::from([0]),
            &missing,
        )
        .unwrap();

        assert_eq!(identities.len(), 1);
        assert_eq!(identities[0].fixture, "conformance/a.ts");
        assert_eq!(identities[0].matrix_key, "strict=true");
        assert_eq!(identities[0].pass, "semantic");
        assert_eq!(identities[0].occurrence, 1);
        assert_eq!(identities[0].start, Some(5));
    }

    /// The harness serializes @lib as OptionValue::StringList; the
    /// conversion must lowercase and forward it (a String-only match
    /// silently dropped the option, leaving CompilerOptions.lib None).
    #[test]
    fn lib_string_list_reaches_compiler_options() {
        let program = tsc_harness::ProgramJson {
            schema: 1,
            cwd: ".".to_owned(),
            options: [(
                "lib".to_owned(),
                tsc_harness::OptionValue::StringList(vec!["ES2015".to_owned(), " Dom ".to_owned()]),
            )]
            .into_iter()
            .collect(),
            libs: Vec::new(),
            files: Vec::new(),
            matrix_key: String::new(),
        };
        let options = tsc_harness::compiler_options_from_program(&program);
        assert_eq!(
            options.lib,
            Some(vec!["es2015".to_owned(), "dom".to_owned()])
        );
    }

    #[test]
    fn lib_comma_string_still_supported() {
        let program = tsc_harness::ProgramJson {
            schema: 1,
            cwd: ".".to_owned(),
            options: [(
                "lib".to_owned(),
                tsc_harness::OptionValue::String("ES2020, dom".to_owned()),
            )]
            .into_iter()
            .collect(),
            libs: Vec::new(),
            files: Vec::new(),
            matrix_key: String::new(),
        };
        let options = tsc_harness::compiler_options_from_program(&program);
        assert_eq!(
            options.lib,
            Some(vec!["es2020".to_owned(), "dom".to_owned()])
        );
    }

    #[test]
    fn package_resolution_conditions_reach_compiler_options() {
        let program = tsc_harness::ProgramJson {
            schema: 1,
            cwd: ".".to_owned(),
            options: [
                (
                    "resolvePackageJsonExports".to_owned(),
                    tsc_harness::OptionValue::Bool(false),
                ),
                (
                    "resolvePackageJsonImports".to_owned(),
                    tsc_harness::OptionValue::Bool(true),
                ),
                (
                    "customConditions".to_owned(),
                    tsc_harness::OptionValue::StringList(vec![
                        "webpack".to_owned(),
                        "browser".to_owned(),
                    ]),
                ),
                (
                    "noDtsResolution".to_owned(),
                    tsc_harness::OptionValue::Bool(true),
                ),
            ]
            .into_iter()
            .collect(),
            libs: Vec::new(),
            files: Vec::new(),
            matrix_key: String::new(),
        };
        let options = tsc_harness::compiler_options_from_program(&program);
        assert_eq!(options.resolve_package_json_exports, Some(false));
        assert_eq!(options.resolve_package_json_imports, Some(true));
        assert_eq!(
            options.custom_conditions,
            Some(vec!["webpack".to_owned(), "browser".to_owned()])
        );
        assert_eq!(options.no_dts_resolution, Some(true));
    }

    #[test]
    fn module_detection_reaches_compiler_options() {
        let program = tsc_harness::ProgramJson {
            schema: 1,
            cwd: ".".to_owned(),
            options: [(
                "moduleDetection".to_owned(),
                tsc_harness::OptionValue::String("force".to_owned()),
            )]
            .into_iter()
            .collect(),
            libs: Vec::new(),
            files: Vec::new(),
            matrix_key: String::new(),
        };
        let options = tsc_harness::compiler_options_from_program(&program);
        assert_eq!(options.module_detection, Some(3));
        assert_eq!(options.emit_module_detection_kind(), 3);
    }

    #[test]
    fn import_helpers_reaches_compiler_options() {
        let program = tsc_harness::ProgramJson {
            schema: 1,
            cwd: ".".to_owned(),
            options: [(
                "importHelpers".to_owned(),
                tsc_harness::OptionValue::Bool(true),
            )]
            .into_iter()
            .collect(),
            libs: Vec::new(),
            files: Vec::new(),
            matrix_key: String::new(),
        };
        assert_eq!(
            tsc_harness::compiler_options_from_program(&program).import_helpers,
            Some(true)
        );
    }

    #[test]
    fn allow_arbitrary_extensions_reaches_compiler_options() {
        let program = tsc_harness::ProgramJson {
            schema: 1,
            cwd: ".".to_owned(),
            options: [(
                "allowArbitraryExtensions".to_owned(),
                tsc_harness::OptionValue::Bool(false),
            )]
            .into_iter()
            .collect(),
            libs: Vec::new(),
            files: Vec::new(),
            matrix_key: String::new(),
        };
        assert_eq!(
            tsc_harness::compiler_options_from_program(&program).allow_arbitrary_extensions,
            Some(false)
        );
    }

    #[test]
    fn no_error_truncation_reaches_compiler_options() {
        let program = tsc_harness::ProgramJson {
            schema: 1,
            cwd: ".".to_owned(),
            options: [(
                "noErrorTruncation".to_owned(),
                tsc_harness::OptionValue::Bool(true),
            )]
            .into_iter()
            .collect(),
            libs: Vec::new(),
            files: Vec::new(),
            matrix_key: String::new(),
        };
        assert_eq!(
            tsc_harness::compiler_options_from_program(&program).no_error_truncation,
            Some(true)
        );
    }

    /// Integer ratchets gate exactly: one lost diagnostic must fail
    /// even when the rounded rate would still pass.
    #[test]
    fn ratchet_integer_counts_parse() {
        let dir = temp_root("tsc-rs-ratchet-test");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ratchet.toml");
        fs::write(
            &path,
            "[t0]\nrate = 0.0979\nmatched = 4758\ntotal = 48573\nallowed_regression = 0.0\n",
        )
        .unwrap();
        let ratchet = read_ratchet(&path, DiagnosticBand::All).unwrap();
        assert_eq!(ratchet.matched, Some(4758));
        assert_eq!(ratchet.total, Some(48573));
        assert_eq!(ratchet.allowed_regression, 0.0);
        // The exact-compare shape used by the gate: losing one matched
        // diagnostic on the same corpus regresses.
        let (matched, total) = (ratchet.matched.unwrap(), ratchet.total.unwrap());
        assert!((4758u128) * (total as u128) >= (matched as u128) * (48573u128));
        assert!((4757u128) * (total as u128) < (matched as u128) * (48573u128));
        fs::remove_file(&path).ok();
    }

    #[test]
    fn ratchet_parser_rejects_duplicate_sections_and_keys() {
        let dir = temp_root("tsc-rs-ratchet-duplicates-test");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ratchet.toml");

        fs::write(
            &path,
            "[t0]\nrate = 0.1\nmatched = 1\ntotal = 10\n\
             [t0]\nrate = 0.1\nmatched = 1\ntotal = 10\n",
        )
        .unwrap();
        let err = read_ratchet(&path, DiagnosticBand::All)
            .unwrap_err()
            .to_string();
        assert!(err.contains("invalid ratchet.toml"), "{err}");

        fs::write(
            &path,
            "[t0]\nrate = 0.1\nrate = 0.1\nmatched = 1\ntotal = 10\n",
        )
        .unwrap();
        let err = read_ratchet(&path, DiagnosticBand::All)
            .unwrap_err()
            .to_string();
        assert!(err.contains("invalid ratchet.toml"), "{err}");

        // Quoted and bare keys are the same TOML key. A text-level
        // duplicate checker must not let this semantic duplicate
        // bypass validation.
        fs::write(
            &path,
            "[t0]\nrate = 0.1\n\"rate\" = 0.1\nmatched = 1\ntotal = 10\n",
        )
        .unwrap();
        let err = read_ratchet(&path, DiagnosticBand::All)
            .unwrap_err()
            .to_string();
        assert!(err.contains("invalid ratchet.toml"), "{err}");

        // Dotted and table syntax also share one semantic namespace.
        // The TOML parser must reject a repeated dotted path.
        fs::write(
            &path,
            "t0.rate = 0.1\nt0.\"rate\" = 0.1\nt0.matched = 1\nt0.total = 10\n",
        )
        .unwrap();
        let err = read_ratchet(&path, DiagnosticBand::All)
            .unwrap_err()
            .to_string();
        assert!(err.contains("invalid ratchet.toml"), "{err}");

        // Valid quoted names are resolved by their TOML meaning.
        fs::write(&path, "[\"t0\"]\n\"rate\" = 0.1\nmatched = 1\ntotal = 10\n").unwrap();
        let ratchet = read_ratchet(&path, DiagnosticBand::All).unwrap();
        assert_eq!(ratchet.rate, 0.1);
        assert_eq!(ratchet.matched, Some(1));

        // A section expressed entirely with dotted keys is equivalent
        // to the table form and must be accepted too.
        fs::write(&path, "t0.rate = 0.1\nt0.matched = 1\nt0.total = 10\n").unwrap();
        let ratchet = read_ratchet(&path, DiagnosticBand::All).unwrap();
        assert_eq!(ratchet.rate, 0.1);
        assert_eq!(ratchet.total, Some(10));

        fs::write(
            &path,
            "[t0]\nrate = 0.1\nmatched = 1\ntotal = 10\nallowed_regression = nan\n",
        )
        .unwrap();
        let err = read_ratchet(&path, DiagnosticBand::All)
            .unwrap_err()
            .to_string();
        assert!(err.contains("allowed_regression must be finite"), "{err}");

        fs::remove_file(&path).ok();
    }
}

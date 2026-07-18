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
use tsrs2_checker::{
    check_program, check_program_with_libs, CompilerOptions, InputFile, PartialCheck,
};
use tsrs2_diags::{compute_line_map, get_line_and_character_of_position, Diagnostic, MessageChain};
use tsrs2_oracle::{OracleDiag, OracleMessageChain, OraclePool};

pub mod families;
pub mod goldens_diff;
mod identity;
pub mod ratchet;
mod scope;

pub use families::{
    check as families_check, report as families_report,
    verify_report_freshness as families_verify_report,
};
pub use scope::audit as scope_audit;
use scope::ScopeManifest;

pub type ConformanceResult<T> = Result<T, Box<dyn Error>>;

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
    pub tsrs: Vec<GoldenDiag>,
    pub oracle: Vec<GoldenDiag>,
    pub tsrs_cli_hash: String,
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

#[derive(Clone, Debug, Serialize)]
pub struct ConformanceSummary {
    pub band: String,
    pub fixtures_total: usize,
    pub cases_total: usize,
    pub oracle_diagnostics: usize,
    pub tsrs_diagnostics: usize,
    pub matched_t0_diagnostics: usize,
    pub t0_rate: f64,
    /// Shadow tiers (NON-GATING — greenfield §7.4; measured from
    /// pre-5.8a per the external review, ratchets stay T0-only until
    /// M8): of the T0-matched pairs, how many also match category
    /// (T1), exact span + top message text (T2), and the full chain
    /// + relatedInformation (T3). Nested: t3 ≤ t2 ≤ t1 ≤ t0.
    pub shadow_t1_matched: usize,
    pub shadow_t2_matched: usize,
    pub shadow_t3_matched: usize,
    pub shadow_t1_rate: f64,
    pub shadow_t2_rate: f64,
    pub shadow_t3_rate: f64,
    pub exact_match_cases: usize,
    pub mismatch_cases: usize,
    pub false_positive_diagnostics: usize,
    pub false_negative_diagnostics: usize,
    /// Oracle-only rows inside a source range where the checker
    /// actually reached a named Unsupported/partial-check boundary.
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
    pub supported_exact_match_cases: usize,
    pub supported_mismatch_cases: usize,
    pub supported_false_negative_diagnostics: usize,
    pub ratchet_rate: f64,
    pub ratchet_allowed_regression: f64,
    pub mismatches: Vec<MismatchEntry>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MismatchEntry {
    pub fixture: String,
    pub matrix_key: String,
    pub false_positive: Vec<T0Key>,
    pub false_negative: Vec<T0Key>,
    pub fn_partial_boundary_audit: Vec<FnPartialBoundaryAudit>,
}

#[derive(Clone, Debug, Serialize)]
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
    let temp_root = temp_root("tsrs2-prefix-conformance");
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
        let programs = tsrs2_harness::expand_fixture_file(fixture, &vendor_lib_dir)?;
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
                    tsrs2_harness::write_program_jsons(std::slice::from_ref(&truncated), &out_dir)?;
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
                let result = check_program_with_libs(
                    &libs,
                    &input_files,
                    &compiler_options_from_program(&truncated),
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
    let temp_root = temp_root("tsrs2-oracle-refresh");
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
        if fixture_index > 0 && fixture_index % 250 == 0 {
            eprintln!(
                "oracle refresh progress: {}/{} fixtures",
                fixture_index,
                fixtures.len()
            );
        }
        let programs = tsrs2_harness::expand_fixture_file(fixture, &vendor_lib_dir)?;
        let out_dir = temp_root.join(fixture_index.to_string());
        let paths = tsrs2_harness::write_program_jsons(&programs, &out_dir)?;
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
    run_conformance_inner(options, SetGate::Enforce, false).map(|run| run.summary)
}

/// The A5 rollup path: the identical gating run, additionally
/// collecting the per-bucket families observation. Full band=all runs
/// only — the observation must never come from a projection or an A1
/// summary (measurement-integrity.md §5).
pub(crate) fn run_conformance_observed(
    options: &ConformanceOptions,
) -> ConformanceResult<(ConformanceSummary, families::Observation)> {
    let run = run_conformance_inner(options, SetGate::Enforce, true)?;
    let observation = run
        .observation
        .expect("observing run collects an observation");
    Ok((run.summary, observation))
}

/// The `ratchet update` measurement path: identical run, but it
/// RETURNS the per-view identity sets instead of gating against the
/// accepted artifact (which may not exist yet at bootstrap).
pub(crate) fn run_conformance_collect(
    options: &ConformanceOptions,
) -> ConformanceResult<ConformanceRun> {
    run_conformance_inner(options, SetGate::Collect, false)
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

fn run_conformance_inner(
    options: &ConformanceOptions,
    set_gate: SetGate,
    families_observe: bool,
) -> ConformanceResult<ConformanceRun> {
    let fixtures = select_fixtures(&RefreshOptions {
        workspace: options.workspace.clone(),
        limit: options.limit,
        files: options.files.clone(),
    })?;
    let vendor_lib_dir = options.workspace.join("vendor/typescript-6.0.3/lib");
    let goldens_root = options.workspace.join("goldens");
    let ratchet_path = options.workspace.join("ratchet.toml");
    let ratchet = read_ratchet(&ratchet_path, options.band)?;
    // Partial runs (`--limit`, `--files`) are gated by the accepted-set
    // projection below, not by the full-corpus integer counts.
    let full_run = options.limit.is_none() && options.files.is_empty();
    if families_observe {
        families::ensure_observation_eligible(options.band, full_run)?;
    }
    let mut observation = families_observe.then(families::Observation::default);
    let accepted = match set_gate {
        SetGate::Enforce => Some(ratchet::load_accepted_for_gating(&options.workspace)?),
        SetGate::Collect => None,
    };
    let t1_ratchet = if options.band == DiagnosticBand::All {
        Some(read_ratchet_section(&ratchet_path, "t1")?)
    } else {
        None
    };
    let mut scope = ScopeManifest::load(&options.workspace.join("m8-scope.json"))?;

    // Updates collect every fixed view into one accepted-state version.
    // Gating runs measure only the explicitly selected fixed view, as
    // required by measurement-integrity.md §2.
    let measured_views = match set_gate {
        SetGate::Collect => ratchet::FIXED_VIEWS.to_vec(),
        SetGate::Enforce => vec![options.band],
    };
    let mut run_sets = measured_views
        .iter()
        .map(|view| (view.name().to_owned(), Default::default()))
        .collect::<ratchet::RunSets>();
    let mut executed_fixtures = BTreeSet::<String>::new();
    let mut case_count = 0usize;
    let mut exact_match_cases = 0usize;
    let mut oracle_diagnostics = 0usize;
    let mut tsrs_diagnostics = 0usize;
    let mut matched_t0_diagnostics = 0usize;
    let mut shadow_t1_matched = 0usize;
    let mut shadow_t2_matched = 0usize;
    let mut shadow_t3_matched = 0usize;
    let mut fp_count = 0usize;
    let mut fn_count = 0usize;
    let mut fn_with_partial_boundary_count = 0usize;
    let mut fn_without_partial_boundary_count = 0usize;
    let mut fn_trigger_reasons = BTreeMap::<String, usize>::new();
    let mut fp_codes = BTreeMap::<u32, usize>::new();
    let mut fn_codes = BTreeMap::<u32, usize>::new();
    let mut mismatches = Vec::new();
    let mut scope_excluded_diagnostics = 0usize;
    let mut scope_unresolved_diagnostics = 0usize;
    let mut scope_resolved_t0_diagnostics = 0usize;
    let mut supported_oracle_diagnostics = 0usize;
    let mut supported_tsrs_diagnostics = 0usize;
    let mut supported_matched_t0_diagnostics = 0usize;
    let mut supported_t1_matched = 0usize;
    let mut supported_t2_matched = 0usize;
    let mut supported_t3_matched = 0usize;
    let mut supported_exact_match_cases = 0usize;
    let mut supported_fn_count = 0usize;

    for fixture in &fixtures {
        let fixture_key = fixture_key(&options.workspace, fixture)?;
        let golden = read_golden(&goldens_root, &fixture_key)?;
        // Pass provenance is required whenever this run records or
        // enforces the syntactic fixed view.
        if measured_views.contains(&DiagnosticBand::Syntactic) && golden.schema < 2 {
            return Err(format!(
                "golden {fixture_key} has schema {} without pass provenance; \
                 run `cargo xtask oracle-refresh`",
                golden.schema
            )
            .into());
        }
        executed_fixtures.insert(fixture_key.clone());
        let golden_by_matrix = golden
            .cases
            .iter()
            .map(|case| (case.matrix_key.as_str(), case))
            .collect::<BTreeMap<_, _>>();
        let programs = tsrs2_harness::expand_fixture_file(fixture, &vendor_lib_dir)?;

        for program in programs {
            let golden_case = golden_by_matrix
                .get(program.matrix_key.as_str())
                .ok_or_else(|| {
                    format!("missing golden case {fixture_key} [{}]", program.matrix_key)
                })?;
            let case_tsrs = current_case_tsrs(&program, &vendor_lib_dir)?;
            // Collection records every fixed view from one pass; a
            // gating run records only its selected view.
            for view in measured_views.iter().copied() {
                let oracle_side = golden_case
                    .oracle
                    .iter()
                    .filter(|diag| view.matches_oracle(diag));
                let case_sets = match view {
                    DiagnosticBand::Syntactic => {
                        ratchet::bucket_sets(oracle_side, case_tsrs.syntactic.iter())
                    }
                    _ => ratchet::bucket_sets(
                        oracle_side,
                        case_tsrs.all.iter().filter(|diag| view.contains(diag.code)),
                    ),
                };
                if !case_sets.matched.is_empty() {
                    run_sets
                        .entry(view.name().to_owned())
                        .or_default()
                        .entry(fixture_key.clone())
                        .or_default()
                        .insert(program.matrix_key.clone(), case_sets);
                }
            }
            let current = match options.band {
                DiagnosticBand::Syntactic => &case_tsrs.syntactic,
                _ => &case_tsrs.all,
            };
            // Exact schema-2 exclusions: indices of the removed oracle
            // RECORDS (measurement-integrity.md §3) — occurrence-level,
            // never a whole T0 bucket unless every record is excluded.
            let excluded_indices = scope.exclusions_for_case(
                &fixture_key,
                &program.matrix_key,
                &golden_case.oracle,
            )?;
            let actual = t0_set(
                current
                    .iter()
                    .filter(|diag| options.band.contains(diag.code)),
            );
            let expected = t0_set(
                golden_case
                    .oracle
                    .iter()
                    .filter(|diag| options.band.matches_oracle(diag)),
            );
            let excluded_records = excluded_indices
                .iter()
                .copied()
                .filter(|index| options.band.matches_oracle(&golden_case.oracle[*index]))
                .collect::<Vec<_>>();

            let fp = actual.difference(&expected).cloned().collect::<Vec<_>>();
            let fn_ = expected.difference(&actual).cloned().collect::<Vec<_>>();
            let fn_partial_boundary_audit = classify_fn_partial_boundaries(
                &fn_,
                &golden_case.oracle,
                &case_tsrs.partial_checks,
            );
            for audit in &fn_partial_boundary_audit {
                if audit.reached_partial_boundary {
                    fn_with_partial_boundary_count += 1;
                    for reason in &audit.reasons {
                        *fn_trigger_reasons.entry(reason.clone()).or_default() += 1;
                    }
                } else {
                    fn_without_partial_boundary_count += 1;
                }
            }
            // The selector removes exact oracle records before the
            // supported comparison; a T0 bucket leaves the supported
            // denominator only when every one of its records is
            // excluded, so an exclusion can never hide a neighboring
            // occurrence in the same bucket. An empty selection (the
            // common case, per case) leaves the band views untouched —
            // borrow them instead of rebuilding both sets in this hot
            // loop.
            let (supported_expected, fully_excluded) = if excluded_indices.is_empty() {
                (Cow::Borrowed(&expected), BTreeSet::new())
            } else {
                let (supported_expected, fully_excluded) = scope::supported_case_view(
                    &golden_case.oracle,
                    options.band,
                    &excluded_indices,
                );
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
            let supported_fn = supported_expected
                .difference(&supported_actual)
                .cloned()
                .collect::<Vec<_>>();
            // Resolution predicate (measurement-integrity.md §3.2),
            // per excluded occurrence: a resolved-t0 entry's
            // disposition must be deleted with the required
            // tombstone, and it can never satisfy readiness.
            let mut resolved_excluded = 0usize;
            let mut unresolved_excluded = 0usize;
            for index in &excluded_records {
                let bucket = t0_key(&golden_case.oracle[*index]);
                let oracle_multiplicity = golden_case
                    .oracle
                    .iter()
                    .filter(|diag| options.band.matches_oracle(diag) && t0_key(diag) == bucket)
                    .count();
                let tsrs_multiplicity = current
                    .iter()
                    .filter(|diag| options.band.contains(diag.code) && t0_key(diag) == bucket)
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
            if let Some(observation) = observation.as_mut() {
                observation.cases.push(families::CaseObservation::collect(
                    &fixture_key,
                    &program.matrix_key,
                    &golden_case.oracle,
                    &case_tsrs.all,
                    &excluded_indices,
                    &actual,
                    fp.len(),
                )?);
            }
            if fp.is_empty() && fn_.is_empty() {
                exact_match_cases += 1;
            } else {
                mismatches.push(MismatchEntry {
                    fixture: fixture_key.clone(),
                    matrix_key: program.matrix_key.clone(),
                    false_positive: fp.clone(),
                    false_negative: fn_.clone(),
                    fn_partial_boundary_audit,
                });
            }
            if fp.is_empty() && supported_fn.is_empty() {
                supported_exact_match_cases += 1;
            }

            for diag in &fp {
                *fp_codes.entry(diag.code).or_default() += 1;
            }
            for diag in &fn_ {
                *fn_codes.entry(diag.code).or_default() += 1;
            }

            matched_t0_diagnostics += expected.intersection(&actual).count();
            let (t1, t2, t3) = shadow_tier_matches(
                current
                    .iter()
                    .filter(|diag| options.band.contains(diag.code)),
                golden_case
                    .oracle
                    .iter()
                    .filter(|diag| options.band.matches_oracle(diag)),
            );
            shadow_t1_matched += t1;
            shadow_t2_matched += t2;
            shadow_t3_matched += t3;
            supported_matched_t0_diagnostics +=
                supported_expected.intersection(&supported_actual).count();
            // Supported tiers remove exact oracle records; the tsrs
            // side drops only fully-excluded buckets (tsrs records
            // carry no occurrence identity). A partially excluded
            // bucket therefore tier-matches only when tsrs emits
            // exactly the remaining records.
            let (supported_t1, supported_t2, supported_t3) = shadow_tier_matches(
                current.iter().filter(|diagnostic| {
                    options.band.contains(diagnostic.code)
                        && !fully_excluded.contains(&t0_key(diagnostic))
                }),
                golden_case
                    .oracle
                    .iter()
                    .enumerate()
                    .filter(|(index, diagnostic)| {
                        options.band.matches_oracle(diagnostic) && !excluded_indices.contains(index)
                    })
                    .map(|(_, diagnostic)| diagnostic),
            );
            supported_t1_matched += supported_t1;
            supported_t2_matched += supported_t2;
            supported_t3_matched += supported_t3;
            scope_excluded_diagnostics += excluded_records.len();
            scope_unresolved_diagnostics += unresolved_excluded;
            scope_resolved_t0_diagnostics += resolved_excluded;
            supported_oracle_diagnostics += supported_expected.len();
            supported_tsrs_diagnostics += supported_actual.len();
            supported_fn_count += supported_fn.len();
            oracle_diagnostics += expected.len();
            tsrs_diagnostics += actual.len();
            fp_count += fp.len();
            fn_count += fn_.len();
            case_count += 1;
        }
    }

    if full_run && options.band == DiagnosticBand::All {
        scope.finish_full_validation()?;
    }

    let t0_rate = if oracle_diagnostics == 0 {
        1.0
    } else {
        matched_t0_diagnostics as f64 / oracle_diagnostics as f64
    };

    let summary = ConformanceSummary {
        band: options.band.name().to_owned(),
        fixtures_total: fixtures.len(),
        cases_total: case_count,
        oracle_diagnostics,
        tsrs_diagnostics,
        matched_t0_diagnostics,
        t0_rate,
        shadow_t1_matched,
        shadow_t2_matched,
        shadow_t3_matched,
        shadow_t1_rate: shadow_rate(shadow_t1_matched, oracle_diagnostics),
        shadow_t2_rate: shadow_rate(shadow_t2_matched, oracle_diagnostics),
        shadow_t3_rate: shadow_rate(shadow_t3_matched, oracle_diagnostics),
        exact_match_cases,
        mismatch_cases: case_count - exact_match_cases,
        false_positive_diagnostics: fp_count,
        false_negative_diagnostics: fn_count,
        fn_with_partial_boundary_evidence: fn_with_partial_boundary_count,
        fn_without_partial_boundary_evidence: fn_without_partial_boundary_count,
        top_fn_partial_boundary_reasons: top_string_counts(fn_trigger_reasons),
        top_false_positive_codes: top_codes(fp_codes),
        top_false_negative_codes: top_codes(fn_codes),
        scope_status: scope.status().name().to_owned(),
        scope_manifest_entries: scope.entry_count(),
        scope_excluded_diagnostics,
        scope_unresolved_diagnostics,
        scope_resolved_t0_diagnostics,
        supported_oracle_diagnostics,
        supported_tsrs_diagnostics,
        supported_matched_t0_diagnostics,
        supported_t0_rate: shadow_rate(
            supported_matched_t0_diagnostics,
            supported_oracle_diagnostics,
        ),
        supported_t1_matched,
        supported_t2_matched,
        supported_t3_matched,
        supported_t1_rate: shadow_rate(supported_t1_matched, supported_oracle_diagnostics),
        supported_t2_rate: shadow_rate(supported_t2_matched, supported_oracle_diagnostics),
        supported_t3_rate: shadow_rate(supported_t3_matched, supported_oracle_diagnostics),
        supported_exact_match_cases,
        supported_mismatch_cases: case_count - supported_exact_match_cases,
        supported_false_negative_diagnostics: supported_fn_count,
        ratchet_rate: ratchet.rate,
        ratchet_allowed_regression: ratchet.allowed_regression,
        mismatches,
    };

    if let Some(parent) = options.out_json.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&options.out_json, serde_json::to_string_pretty(&summary)?)?;

    if let Some(observation) = observation.as_mut() {
        observation.fixtures_total = fixtures.len();
    }
    if set_gate == SetGate::Collect {
        return Ok(ConformanceRun {
            summary,
            sets: run_sets,
            observation,
        });
    }

    // The set ratchet first: it names the exact regressed identity,
    // and it catches the swap the integer gate cannot (one new match
    // traded for one removal). Partial runs enforce the projection to
    // the executed fixtures.
    if let Some(accepted) = &accepted {
        ratchet::enforce_accepted(
            &accepted.artifact,
            &run_sets,
            options.band,
            &executed_fixtures,
            full_run,
        )?;
    }

    // With matched/total recorded the comparison is exact (cross-
    // multiplied integers): a rounded `rate` float would let up to a
    // few diagnostics regress silently. Full runs only — a partial
    // run's denominator is not the recorded corpus.
    let regressed = full_run
        && match (ratchet.matched, ratchet.total) {
            (Some(matched), Some(total)) if summary.ratchet_allowed_regression == 0.0 => {
                (summary.matched_t0_diagnostics as u128) * (total as u128)
                    < (matched as u128) * (summary.oracle_diagnostics as u128)
            }
            _ => summary.t0_rate + summary.ratchet_allowed_regression < summary.ratchet_rate,
        };
    if regressed {
        return Err(format!(
            "T0 ratchet regression: measured {:.6} ({}/{}), required {:.6} (allowed regression {:.6})",
            summary.t0_rate,
            summary.matched_t0_diagnostics,
            summary.oracle_diagnostics,
            summary.ratchet_rate,
            summary.ratchet_allowed_regression
        )
        .into());
    }
    if let Some(t1_ratchet) = t1_ratchet.filter(|_| full_run) {
        let t1_regressed = match (t1_ratchet.matched, t1_ratchet.total) {
            (Some(matched), Some(total)) if t1_ratchet.allowed_regression == 0.0 => {
                (summary.shadow_t1_matched as u128) * (total as u128)
                    < (matched as u128) * (summary.oracle_diagnostics as u128)
            }
            _ => summary.shadow_t1_rate + t1_ratchet.allowed_regression < t1_ratchet.rate,
        };
        if t1_regressed {
            return Err(format!(
                "T1 ratchet regression: measured {:.6} ({}/{}), required {:.6} (allowed regression {:.6})",
                summary.shadow_t1_rate,
                summary.shadow_t1_matched,
                summary.oracle_diagnostics,
                t1_ratchet.rate,
                t1_ratchet.allowed_regression
            )
            .into());
        }
    }
    if summary.false_positive_diagnostics > 0 {
        return Err(format!(
            "NEW_FP hard gate failed: {} false positive diagnostics",
            summary.false_positive_diagnostics
        )
        .into());
    }
    if summary.scope_status == "frozen" && summary.scope_resolved_t0_diagnostics > 0 {
        return Err(format!(
            "stale M8 scope gate failed: {} excluded diagnostic(s) now match at T0; delete their dispositions so higher tiers grade them",
            summary.scope_resolved_t0_diagnostics
        )
        .into());
    }

    Ok(ConformanceRun {
        summary,
        sets: run_sets,
        observation,
    })
}

fn shadow_rate(matched: usize, total: usize) -> f64 {
    if total == 0 {
        1.0
    } else {
        matched as f64 / total as f64
    }
}

/// Shadow tier grading. T1 becomes ratcheted when configured at M7;
/// T2/T3 remain non-gating until M8. Bucket both sides by T0 key; a
/// key contributes 1 to a tier only when the two buckets are equal
/// AS MULTISETS under that tier's OWN equivalence (review round 3:
/// tiers compare independently — T1 must not depend on how T2's
/// finer key would pair elements):
///   T1 = category
///   T2 = T1 + exact start/length + top message text
///   T3 = T2 + full chain tree + relatedInformation
/// The equivalences nest, so equal-T3 multisets imply equal-T2 imply
/// equal-T1, and per-key counting keeps the tiers nested under
/// matched_t0's set semantics. tsrs-side related info flows through
/// from_tsrs since pre-5.8a (it was dropped before).
fn shadow_tier_matches<'a>(
    actual: impl Iterator<Item = &'a GoldenDiag>,
    expected: impl Iterator<Item = &'a GoldenDiag>,
) -> (usize, usize, usize) {
    fn keyed<'a>(
        diags: impl Iterator<Item = &'a GoldenDiag>,
    ) -> BTreeMap<T0Key, Vec<&'a GoldenDiag>> {
        let mut map: BTreeMap<T0Key, Vec<&'a GoldenDiag>> = BTreeMap::new();
        for diag in diags {
            map.entry(t0_key(diag)).or_default().push(diag);
        }
        map
    }
    /// Greedy multiset equality under `eq` — buckets are tiny (almost
    /// always 1), so O(n²) matching beats deriving Ord for chains.
    fn multiset_eq(
        actual: &[&GoldenDiag],
        expected: &[&GoldenDiag],
        eq: impl Fn(&GoldenDiag, &GoldenDiag) -> bool,
    ) -> bool {
        if actual.len() != expected.len() {
            return false;
        }
        let mut used = vec![false; expected.len()];
        'outer: for left in actual {
            for (index, right) in expected.iter().enumerate() {
                if !used[index] && eq(left, right) {
                    used[index] = true;
                    continue 'outer;
                }
            }
            return false;
        }
        true
    }
    fn t1_eq(a: &GoldenDiag, e: &GoldenDiag) -> bool {
        a.category == e.category
    }
    fn t2_eq(a: &GoldenDiag, e: &GoldenDiag) -> bool {
        t1_eq(a, e) && a.start == e.start && a.length == e.length && a.chain.text == e.chain.text
    }
    fn t3_eq(a: &GoldenDiag, e: &GoldenDiag) -> bool {
        t2_eq(a, e) && a.chain == e.chain && a.related == e.related
    }
    let actual = keyed(actual);
    let expected = keyed(expected);
    let (mut t1, mut t2, mut t3) = (0usize, 0usize, 0usize);
    for (key, expected_bucket) in &expected {
        let Some(actual_bucket) = actual.get(key) else {
            continue;
        };
        if !multiset_eq(actual_bucket, expected_bucket, t1_eq) {
            continue;
        }
        t1 += 1;
        if !multiset_eq(actual_bucket, expected_bucket, t2_eq) {
            continue;
        }
        t2 += 1;
        if multiset_eq(actual_bucket, expected_bucket, t3_eq) {
            t3 += 1;
        }
    }
    (t1, t2, t3)
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
            reports_unnecessary: false,
            reports_deprecated: false,
            source: None,
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
    syntactic: Vec<GoldenDiag>,
    partial_checks: Vec<PartialCheck>,
}

fn current_case_tsrs(
    program: &tsrs2_harness::ProgramJson,
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
    let result = check_program_with_libs(&libs, &files, &compiler_options_from_program(program));
    Ok(CaseTsrs {
        all: result
            .diagnostics
            .iter()
            .map(|diag| GoldenDiag::from_tsrs(diag, &file_texts))
            .collect(),
        syntactic: result
            .syntactic_diagnostics
            .iter()
            .map(|diag| GoldenDiag::from_tsrs(diag, &file_texts))
            .collect(),
        partial_checks: result.partial_checks,
    })
}

/// tsc getAllowJSCompilerOption: allowJs ?? !!checkJs. Fixture directive
/// keys are matched case-insensitively like the harness does.
pub fn compiler_options_from_program(program: &tsrs2_harness::ProgramJson) -> CompilerOptions {
    let bool_option = |name: &str| {
        program.options.iter().find_map(|(key, value)| {
            if key.eq_ignore_ascii_case(name) {
                match value {
                    tsrs2_harness::OptionValue::Bool(value) => Some(*value),
                    _ => None,
                }
            } else {
                None
            }
        })
    };
    let target = program.options.iter().find_map(|(key, value)| {
        if key.eq_ignore_ascii_case("target") {
            match value {
                tsrs2_harness::OptionValue::String(value) => target_option_value(value),
                tsrs2_harness::OptionValue::Number(value) => Some(*value),
                _ => None,
            }
        } else {
            None
        }
    });
    let module = program.options.iter().find_map(|(key, value)| {
        if key.eq_ignore_ascii_case("module") {
            match value {
                tsrs2_harness::OptionValue::String(value) => module_option_value(value),
                tsrs2_harness::OptionValue::Number(value) => Some(*value),
                _ => None,
            }
        } else {
            None
        }
    });
    let module_resolution = program.options.iter().find_map(|(key, value)| {
        if key.eq_ignore_ascii_case("moduleResolution") {
            match value {
                tsrs2_harness::OptionValue::String(value) => module_resolution_option_value(value),
                tsrs2_harness::OptionValue::Number(value) => Some(*value),
                _ => None,
            }
        } else {
            None
        }
    });
    CompilerOptions {
        allow_js: bool_option("allowJs").unwrap_or_else(|| bool_option("checkJs").unwrap_or(false)),
        experimental_decorators: bool_option("experimentalDecorators").unwrap_or(false),
        target,
        module,
        always_strict: bool_option("alwaysStrict"),
        strict: bool_option("strict"),
        strict_null_checks: bool_option("strictNullChecks"),
        strict_function_types: bool_option("strictFunctionTypes"),
        strict_bind_call_apply: bool_option("strictBindCallApply"),
        no_implicit_any: bool_option("noImplicitAny"),
        no_implicit_this: bool_option("noImplicitThis"),
        exact_optional_property_types: bool_option("exactOptionalPropertyTypes"),
        no_fallthrough_cases_in_switch: bool_option("noFallthroughCasesInSwitch"),
        allow_unreachable_code: bool_option("allowUnreachableCode"),
        check_js: bool_option("checkJs"),
        no_unchecked_indexed_access: bool_option("noUncheckedIndexedAccess"),
        no_property_access_from_index_signature: bool_option("noPropertyAccessFromIndexSignature"),
        strict_property_initialization: bool_option("strictPropertyInitialization"),
        use_define_for_class_fields: bool_option("useDefineForClassFields"),
        use_unknown_in_catch_variables: bool_option("useUnknownInCatchVariables"),
        no_emit: bool_option("noEmit"),
        downlevel_iteration: bool_option("downlevelIteration"),
        strict_builtin_iterator_return: bool_option("strictBuiltinIteratorReturn"),
        module_resolution,
        es_module_interop: bool_option("esModuleInterop"),
        allow_synthetic_default_imports: bool_option("allowSyntheticDefaultImports"),
        preserve_const_enums: bool_option("preserveConstEnums"),
        base_url: string_option(program, "baseUrl"),
        allow_importing_ts_extensions: bool_option("allowImportingTsExtensions"),
        resolve_json_module: bool_option("resolveJsonModule"),
        skip_lib_check: bool_option("skipLibCheck"),
        jsx: program.options.iter().find_map(|(key, value)| {
            if key.eq_ignore_ascii_case("jsx") {
                match value {
                    tsrs2_harness::OptionValue::String(value) => jsx_option_value(value),
                    tsrs2_harness::OptionValue::Number(value) => Some(*value),
                    _ => None,
                }
            } else {
                None
            }
        }),
        jsx_factory: string_option(program, "jsxFactory"),
        jsx_fragment_factory: string_option(program, "jsxFragmentFactory"),
        jsx_import_source: string_option(program, "jsxImportSource"),
        react_namespace: string_option(program, "reactNamespace"),
        lib: program.options.iter().find_map(|(key, value)| {
            if key.eq_ignore_ascii_case("lib") {
                // The harness serializes @lib as a StringList (it
                // rejects anything else at expansion time); the String
                // arm keeps hand-built programs working.
                match value {
                    tsrs2_harness::OptionValue::StringList(values) => Some(
                        values
                            .iter()
                            .map(|entry| entry.trim().to_ascii_lowercase())
                            .filter(|entry| !entry.is_empty())
                            .collect(),
                    ),
                    tsrs2_harness::OptionValue::String(value) => Some(
                        value
                            .split(',')
                            .map(|entry| entry.trim().to_ascii_lowercase())
                            .filter(|entry| !entry.is_empty())
                            .collect(),
                    ),
                    _ => None,
                }
            } else {
                None
            }
        }),
    }
}

fn string_option(program: &tsrs2_harness::ProgramJson, name: &str) -> Option<String> {
    program.options.iter().find_map(|(key, value)| {
        if key.eq_ignore_ascii_case(name) {
            match value {
                tsrs2_harness::OptionValue::String(value) => Some(value.clone()),
                _ => None,
            }
        } else {
            None
        }
    })
}

/// tsc moduleOptionDeclaration.type (36853-36868) — the module
/// option's string→ModuleKind map.
fn module_option_value(value: &str) -> Option<i32> {
    Some(match value.to_ascii_lowercase().as_str() {
        "none" => 0,
        "commonjs" => 1,
        "amd" => 2,
        "umd" => 3,
        "system" => 4,
        "es6" | "es2015" => 5,
        "es2020" => 6,
        "es2022" => 7,
        "esnext" => 99,
        "node16" => 100,
        "node18" => 101,
        "node20" => 102,
        "nodenext" => 199,
        "preserve" => 200,
        _ => return None,
    })
}

/// tsc moduleResolutionOptionDeclaration.type (37337-37346) — the
/// moduleResolution option's string→ModuleResolutionKind map (node/
/// node10/classic survive as deprecated keys at the 6.0.3 pin).
fn module_resolution_option_value(value: &str) -> Option<i32> {
    Some(match value.to_ascii_lowercase().as_str() {
        "node10" | "node" => 2,
        "classic" => 1,
        "node16" => 3,
        "nodenext" => 99,
        "bundler" => 100,
        _ => return None,
    })
}

/// tsc jsxOptionMap — the jsx option's string→JsxEmit map.
fn jsx_option_value(value: &str) -> Option<i32> {
    Some(match value.to_ascii_lowercase().as_str() {
        "preserve" => 1,
        "react" => 2,
        "react-native" => 3,
        "react-jsx" => 4,
        "react-jsxdev" => 5,
        _ => return None,
    })
}

/// tsc targetOptionDeclaration.type — the target option's string→value
/// map, mirrored from the vendored source.
fn target_option_value(value: &str) -> Option<i32> {
    Some(match value.to_ascii_lowercase().as_str() {
        "es3" => 0,
        "es5" => 1,
        "es6" | "es2015" => 2,
        "es2016" => 3,
        "es2017" => 4,
        "es2018" => 5,
        "es2019" => 6,
        "es2020" => 7,
        "es2021" => 8,
        "es2022" => 9,
        "es2023" => 10,
        "es2024" => 11,
        "es2025" => 12,
        "esnext" => 99,
        _ => return None,
    })
}

fn file_texts_for_program(
    program: &tsrs2_harness::ProgramJson,
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
    let json = serde_json::to_vec_pretty(golden)?;
    let compressed = zstd::stream::encode_all(json.as_slice(), 3)?;
    fs::write(path, compressed)?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let (t1, t2, t3) = shadow_tier_matches(actual.iter(), expected.iter());
        assert_eq!((t1, t2, t3), (1, 0, 0));

        // Identical buckets → all tiers.
        let actual = [diag("error", 5, "A"), diag("warning", 5, "B")];
        let expected = [diag("warning", 5, "B"), diag("error", 5, "A")];
        let (t1, t2, t3) = shadow_tier_matches(actual.iter(), expected.iter());
        assert_eq!((t1, t2, t3), (1, 1, 1));

        // Multiplicity difference on a shared key → no tier.
        let actual = [diag("error", 5, "A")];
        let expected = [diag("error", 5, "A"), diag("error", 5, "A")];
        let (t1, t2, t3) = shadow_tier_matches(actual.iter(), expected.iter());
        assert_eq!((t1, t2, t3), (0, 0, 0));

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
        let (t1, t2, t3) = shadow_tier_matches(actual.iter(), expected.iter());
        assert_eq!((t1, t2, t3), (1, 1, 0));
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

    /// The harness serializes @lib as OptionValue::StringList; the
    /// conversion must lowercase and forward it (a String-only match
    /// silently dropped the option, leaving CompilerOptions.lib None).
    #[test]
    fn lib_string_list_reaches_compiler_options() {
        let program = tsrs2_harness::ProgramJson {
            schema: 1,
            cwd: ".".to_owned(),
            options: [(
                "lib".to_owned(),
                tsrs2_harness::OptionValue::StringList(vec![
                    "ES2015".to_owned(),
                    " Dom ".to_owned(),
                ]),
            )]
            .into_iter()
            .collect(),
            libs: Vec::new(),
            files: Vec::new(),
            matrix_key: String::new(),
        };
        let options = compiler_options_from_program(&program);
        assert_eq!(
            options.lib,
            Some(vec!["es2015".to_owned(), "dom".to_owned()])
        );
    }

    #[test]
    fn lib_comma_string_still_supported() {
        let program = tsrs2_harness::ProgramJson {
            schema: 1,
            cwd: ".".to_owned(),
            options: [(
                "lib".to_owned(),
                tsrs2_harness::OptionValue::String("ES2020, dom".to_owned()),
            )]
            .into_iter()
            .collect(),
            libs: Vec::new(),
            files: Vec::new(),
            matrix_key: String::new(),
        };
        let options = compiler_options_from_program(&program);
        assert_eq!(
            options.lib,
            Some(vec!["es2020".to_owned(), "dom".to_owned()])
        );
    }

    /// Integer ratchets gate exactly: one lost diagnostic must fail
    /// even when the rounded rate would still pass.
    #[test]
    fn ratchet_integer_counts_parse() {
        let dir = temp_root("tsrs2-ratchet-test");
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
        let dir = temp_root("tsrs2-ratchet-duplicates-test");
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

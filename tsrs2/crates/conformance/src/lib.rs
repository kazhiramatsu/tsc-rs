#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use tsrs2_checker::{check_program, check_program_with_libs, CompilerOptions, InputFile};
use tsrs2_diags::{compute_line_map, get_line_and_character_of_position, Diagnostic, MessageChain};
use tsrs2_oracle::{OracleDiag, OracleMessageChain, OraclePool};

pub type ConformanceResult<T> = Result<T, Box<dyn Error>>;

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
            Self::TwoXxx => (2000..3000).contains(&code),
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
    pub top_false_positive_codes: Vec<(u32, usize)>,
    pub top_false_negative_codes: Vec<(u32, usize)>,
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

pub fn run_conformance(options: &ConformanceOptions) -> ConformanceResult<ConformanceSummary> {
    let fixtures = select_fixtures(&RefreshOptions {
        workspace: options.workspace.clone(),
        limit: options.limit,
        files: options.files.clone(),
    })?;
    let vendor_lib_dir = options.workspace.join("vendor/typescript-6.0.3/lib");
    let goldens_root = options.workspace.join("goldens");
    let ratchet = read_ratchet(&options.workspace.join("ratchet.toml"), options.band)?;

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
    let mut fp_codes = BTreeMap::<u32, usize>::new();
    let mut fn_codes = BTreeMap::<u32, usize>::new();
    let mut mismatches = Vec::new();

    for fixture in &fixtures {
        let fixture_key = fixture_key(&options.workspace, fixture)?;
        let golden = read_golden(&goldens_root, &fixture_key)?;
        if options.band == DiagnosticBand::Syntactic && golden.schema < 2 {
            return Err(format!(
                "golden {fixture_key} has schema {} without pass provenance; \
                 run `cargo xtask oracle-refresh` before --syntactic-only",
                golden.schema
            )
            .into());
        }
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
            let current = current_tsrs_diagnostics(&program, &vendor_lib_dir, options.band)?;
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

            let fp = actual.difference(&expected).cloned().collect::<Vec<_>>();
            let fn_ = expected.difference(&actual).cloned().collect::<Vec<_>>();
            if fp.is_empty() && fn_.is_empty() {
                exact_match_cases += 1;
            } else {
                mismatches.push(MismatchEntry {
                    fixture: fixture_key.clone(),
                    matrix_key: program.matrix_key.clone(),
                    false_positive: fp.clone(),
                    false_negative: fn_.clone(),
                });
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
            oracle_diagnostics += expected.len();
            tsrs_diagnostics += actual.len();
            fp_count += fp.len();
            fn_count += fn_.len();
            case_count += 1;
        }
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
        top_false_positive_codes: top_codes(fp_codes),
        top_false_negative_codes: top_codes(fn_codes),
        ratchet_rate: ratchet.rate,
        ratchet_allowed_regression: ratchet.allowed_regression,
        mismatches,
    };

    if let Some(parent) = options.out_json.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&options.out_json, serde_json::to_string_pretty(&summary)?)?;

    // With matched/total recorded the comparison is exact (cross-
    // multiplied integers): a rounded `rate` float would let up to a
    // few diagnostics regress silently.
    let regressed = match (ratchet.matched, ratchet.total) {
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
    if summary.false_positive_diagnostics > 0 {
        return Err(format!(
            "NEW_FP hard gate failed: {} false positive diagnostics",
            summary.false_positive_diagnostics
        )
        .into());
    }

    Ok(summary)
}

fn shadow_rate(matched: usize, total: usize) -> f64 {
    if total == 0 {
        1.0
    } else {
        matched as f64 / total as f64
    }
}

/// Shadow tier grading (NON-GATING). Bucket both sides by T0 key; a
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

fn current_tsrs_diagnostics(
    program: &tsrs2_harness::ProgramJson,
    vendor_lib_dir: &Path,
    band: DiagnosticBand,
) -> ConformanceResult<Vec<GoldenDiag>> {
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
    let diagnostics = match band {
        DiagnosticBand::Syntactic => &result.syntactic_diagnostics,
        _ => &result.diagnostics,
    };
    Ok(diagnostics
        .iter()
        .map(|diag| GoldenDiag::from_tsrs(diag, &file_texts))
        .collect())
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

fn t0_key(diag: &GoldenDiag) -> T0Key {
    T0Key {
        file: diag.file.clone(),
        code: diag.code,
        line: diag.line,
        col: diag.col,
    }
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
    let text = fs::read_to_string(path)?;
    let section = band.ratchet_key();
    let mut in_section = false;
    let mut rate = None;
    let mut matched = None;
    let mut total = None;
    let mut allowed_regression = None;

    for raw_line in text.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_section = &line[1..line.len() - 1] == section;
            continue;
        }
        if !in_section {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "rate" => rate = Some(value.parse::<f64>()?),
            "matched" => matched = Some(value.parse::<u64>()?),
            "total" => total = Some(value.parse::<u64>()?),
            "allowed_regression" => allowed_regression = Some(value.parse::<f64>()?),
            _ => {}
        }
    }

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
}

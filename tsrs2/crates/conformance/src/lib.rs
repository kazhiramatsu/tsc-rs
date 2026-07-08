#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tsrs2_checker::{check_program, CompilerOptions, InputFile};
use tsrs2_diags::{compute_line_map, get_line_and_character_of_position, Diagnostic, MessageChain};
use tsrs2_oracle::{OracleDiag, OracleMessageChain, OraclePool};

pub type ConformanceResult<T> = Result<T, Box<dyn Error>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticBand {
    All,
    TwoXxx,
}

impl DiagnosticBand {
    pub fn name(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::TwoXxx => "2xxx",
        }
    }

    fn contains(self, code: u32) -> bool {
        match self {
            Self::All => true,
            Self::TwoXxx => (2000..3000).contains(&code),
        }
    }

    fn ratchet_key(self) -> &'static str {
        match self {
            Self::All => "t0",
            Self::TwoXxx => "t0-2xxx",
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
            schema: 1,
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
    let mut fp_count = 0usize;
    let mut fn_count = 0usize;
    let mut fp_codes = BTreeMap::<u32, usize>::new();
    let mut fn_codes = BTreeMap::<u32, usize>::new();
    let mut mismatches = Vec::new();

    for fixture in &fixtures {
        let fixture_key = fixture_key(&options.workspace, fixture)?;
        let golden = read_golden(&goldens_root, &fixture_key)?;
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
            let current = current_tsrs_diagnostics(&program, &vendor_lib_dir)?;
            let actual = t0_set(
                current
                    .iter()
                    .filter(|diag| options.band.contains(diag.code)),
            );
            let expected = t0_set(
                golden_case
                    .oracle
                    .iter()
                    .filter(|diag| options.band.contains(diag.code)),
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

    if summary.t0_rate + summary.ratchet_allowed_regression < summary.ratchet_rate {
        return Err(format!(
            "T0 ratchet regression: measured {:.6}, required {:.6} (allowed regression {:.6})",
            summary.t0_rate, summary.ratchet_rate, summary.ratchet_allowed_regression
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
            category: diag.category().name().to_owned(),
            chain: GoldenMessageChain::from_tsrs(&diag.message),
            related: Vec::new(),
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

fn current_tsrs_diagnostics(
    program: &tsrs2_harness::ProgramJson,
    _vendor_lib_dir: &Path,
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

    let result = check_program(&files, &CompilerOptions::default());
    Ok(result
        .diagnostics
        .iter()
        .map(|diag| GoldenDiag::from_tsrs(diag, &file_texts))
        .collect())
}

fn file_texts_for_program(
    program: &tsrs2_harness::ProgramJson,
    vendor_lib_dir: &Path,
) -> ConformanceResult<BTreeMap<String, String>> {
    let mut file_texts = BTreeMap::new();
    for lib in &program.libs {
        file_texts.insert(lib.clone(), fs::read_to_string(vendor_lib_dir.join(lib))?);
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
    let Some(start) = diag.start else {
        return (None, None);
    };
    let Some(text) = file_texts.get(file_name) else {
        return (None, None);
    };
    let map = compute_line_map(text);
    let position = map
        .byte_to_utf16
        .get(start as usize)
        .copied()
        .unwrap_or(start);
    let line_col = get_line_and_character_of_position(&map.line_starts, position);
    (Some(line_col.line), Some(line_col.character))
}

fn t0_set<'a>(diagnostics: impl Iterator<Item = &'a GoldenDiag>) -> BTreeSet<T0Key> {
    diagnostics
        .map(|diag| T0Key {
            file: diag.file.clone(),
            code: diag.code,
            line: diag.line,
            col: diag.col,
        })
        .collect()
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
    allowed_regression: f64,
}

fn read_ratchet(path: &Path, band: DiagnosticBand) -> ConformanceResult<Ratchet> {
    let text = fs::read_to_string(path)?;
    let section = band.ratchet_key();
    let mut in_section = false;
    let mut rate = None;
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
        let value = value.trim().parse::<f64>()?;
        match key.trim() {
            "rate" => rate = Some(value),
            "allowed_regression" => allowed_regression = Some(value),
            _ => {}
        }
    }

    Ok(Ratchet {
        rate: rate.ok_or_else(|| format!("missing [{section}].rate in {}", path.display()))?,
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

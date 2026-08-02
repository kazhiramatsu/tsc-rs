//! A3 rendered-output (T4) evidence.
//!
//! Golden schema 2's `*_cli_hash` values are legacy FNV hashes of
//! structured JSON. They are deliberately ignored here. Schema 3 gives
//! `oracle_cli_hash` one unambiguous meaning: lowercase SHA-256 of the
//! exact normalized UTF-8 bytes returned by the vendored TypeScript
//! 6.0.3 `formatDiagnosticsWithColorAndContext` path (ANSI removed,
//! formatter newlines fixed to LF). `tsrs_cli_hash` is never persisted.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};
use tsc_diagnostics::{
    format_sorted_diagnostics_with_context, Diagnostic, DiagnosticCategory, FormatDiagnosticsHost,
    MessageChain, RelatedInfo,
};
use tsc_oracle::OraclePool;

use super::scope::{supported_case_view, ScopeManifest};
use super::{
    current_case_tsrs, encode_golden, file_texts_for_program, fixture_key, golden_path,
    read_golden, select_fixtures, t0_key, ConformanceResult, DiagnosticBand, GoldenDiag,
    GoldenFile, GoldenMessageChain, RefreshOptions,
};

const GOLDEN_RENDER_SCHEMA: u32 = 3;
const REPORT_SCHEMA: u32 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderHashMode {
    /// The one reviewed A3 input-schema extension. It requires the
    /// complete fixed universe and upgrades schema 2 to schema 3 only
    /// after every structured oracle record proves byte-equivalent.
    ExtendSchema3,
    /// Post-A3 check-only mode. No golden byte is written.
    Check,
}

#[derive(Clone, Debug, Serialize)]
pub struct RenderHashSummary {
    pub mode: String,
    pub fixtures: usize,
    pub cases: usize,
    pub oracle_diagnostics: usize,
    pub schema_2_upgraded: usize,
    pub schema_3_checked: usize,
}

pub(crate) struct PlannedGoldenUpdate {
    pub(crate) path: PathBuf,
    pub(crate) original: Vec<u8>,
    pub(crate) replacement: Vec<u8>,
}

pub(crate) struct RenderHashExtensionPlan {
    pub(crate) summary: RenderHashSummary,
    pub(crate) pins: super::ratchet::T4OraclePins,
    pub(crate) empty_related_information: T4OracleEmptyRelatedInformation,
    pub(crate) updates: Vec<PlannedGoldenUpdate>,
}

/// Sparse formatter-only metadata for an A3 plan that has not written
/// its schema-3 goldens yet: fixture -> matrix -> canonical oracle
/// diagnostic indices with a present-but-empty relatedInformation
/// array.
pub(crate) type T4OracleEmptyRelatedInformation = BTreeMap<String, BTreeMap<String, Vec<usize>>>;

#[derive(Clone, Debug)]
pub struct T4ReportOptions {
    pub workspace: PathBuf,
    pub limit: Option<usize>,
    pub files: Vec<PathBuf>,
}

#[derive(Clone, Debug, Serialize)]
pub struct T4Report {
    pub schema: u32,
    pub status: String,
    pub formatter: String,
    pub hash: String,
    pub fixtures: usize,
    pub cases: usize,
    pub schema_3_pinned_cases: usize,
    pub matched_cases: usize,
    pub mismatched_cases: usize,
    pub oracle_pin_failures: usize,
    pub rust_formatter_failures: usize,
    pub cases_detail: Vec<T4CaseReport>,
}

#[derive(Clone, Debug, Serialize)]
pub struct T4CaseReport {
    pub fixture: String,
    pub matrix_key: String,
    pub golden_schema: u32,
    pub excluded_oracle_records: usize,
    pub oracle_cli_hash: String,
    pub tsrs_cli_hash: String,
    pub oracle_full_cli_hash: String,
    pub rust_oracle_full_cli_hash: String,
    pub rust_formatter_matches_oracle: bool,
    pub golden_oracle_cli_hash: Option<String>,
    pub oracle_pin_matches: Option<bool>,
    pub matched: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_difference: Option<RenderedDifference>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RenderedDifference {
    pub line: usize,
    pub oracle: Option<String>,
    pub tsrs: Option<String>,
}

struct TemporaryTree {
    path: PathBuf,
}

impl TemporaryTree {
    fn create(path: PathBuf) -> ConformanceResult<Self> {
        if path.exists() {
            fs::remove_dir_all(&path)?;
        }
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn collect_genuine_empty_related_information(diagnostics: &[tsc_oracle::OracleDiag]) -> Vec<usize> {
    diagnostics
        .iter()
        .enumerate()
        .filter_map(|(index, diagnostic)| {
            (diagnostic.related_information_present && diagnostic.related.is_empty())
                .then_some(index)
        })
        .collect()
}

fn validate_empty_related_information(
    diagnostics: &[GoldenDiag],
    indices: &[usize],
    context: &str,
) -> ConformanceResult<BTreeSet<usize>> {
    let mut validated = BTreeSet::new();
    let mut previous = None;
    for &index in indices {
        if previous.is_some_and(|previous| index <= previous) {
            return Err(format!(
                "{context} empty-related-information indices must be strictly increasing \
                 and unique"
            )
            .into());
        }
        let diagnostic = diagnostics.get(index).ok_or_else(|| {
            format!(
                "{context} empty-related-information index {index} is out of range for {} \
                 diagnostics",
                diagnostics.len()
            )
        })?;
        if !diagnostic.related.is_empty() {
            return Err(format!(
                "{context} empty-related-information index {index} points to a diagnostic \
                 with {} serialized related rows",
                diagnostic.related.len()
            )
            .into());
        }
        validated.insert(index);
        previous = Some(index);
    }
    Ok(validated)
}

fn effective_oracle_empty_related_information(
    golden_schema: u32,
    diagnostics: &[GoldenDiag],
    stored: &[usize],
    genuine: &[usize],
    context: &str,
) -> ConformanceResult<BTreeSet<usize>> {
    let genuine = validate_empty_related_information(diagnostics, genuine, context)?;
    if golden_schema < GOLDEN_RENDER_SCHEMA {
        if !stored.is_empty() {
            return Err(format!(
                "{context} schema-{golden_schema} case carries schema-3 \
                 empty-related-information metadata"
            )
            .into());
        }
        return Ok(genuine);
    }

    let stored = validate_empty_related_information(diagnostics, stored, context)?;
    if stored != genuine {
        return Err(format!(
            "{context} stored empty-related-information metadata drifted from the genuine \
             TypeScript producer: stored={:?}, genuine={:?}",
            stored, genuine
        )
        .into());
    }
    Ok(stored)
}

/// Produce focused, report-only T4 evidence. This never changes A1,
/// goldens, scope, or ratchets and is therefore safe before A3
/// activation. `--files`/`--limit` callers use this path for formatter
/// work without launching the full oracle corpus.
pub fn run_t4_report(options: &T4ReportOptions) -> ConformanceResult<T4Report> {
    let selection = RefreshOptions {
        workspace: options.workspace.clone(),
        limit: options.limit,
        files: options.files.clone(),
    };
    let fixtures = select_fixtures(&selection)?;
    let vendor_lib_dir = options.workspace.join("vendor/typescript-6.0.3/lib");
    let goldens_root = options.workspace.join("goldens");
    let mut scope = ScopeManifest::load(&options.workspace.join("m8-scope.json"))?;
    let temp_tree = TemporaryTree::create(super::temp_root("tsc-rs-rendered-output-report"))?;
    let pool = OraclePool::new_render_only();
    super::ratchet::verify_launched_render_node(&options.workspace, &pool)?;
    let mut details = Vec::new();

    for (fixture_index, fixture) in fixtures.iter().enumerate() {
        let fixture_name = fixture_key(&options.workspace, fixture)?;
        let golden = read_golden(&goldens_root, &fixture_name)?;
        let golden_by_matrix = golden
            .cases
            .iter()
            .map(|case| (case.matrix_key.as_str(), case))
            .collect::<BTreeMap<_, _>>();
        let programs = tsc_harness::expand_fixture_file(fixture, &vendor_lib_dir)?;
        let program_paths = tsc_harness::write_program_jsons(
            &programs,
            &temp_tree.path().join(fixture_index.to_string()),
        )?;
        for (program, program_path) in programs.iter().zip(program_paths.iter()) {
            let golden_case = golden_by_matrix
                .get(program.matrix_key.as_str())
                .ok_or_else(|| {
                    format!(
                        "missing golden case {fixture_name} [{}]",
                        program.matrix_key
                    )
                })?;
            let file_texts = file_texts_for_program(program, &vendor_lib_dir)?;
            let host = FormatDiagnosticsHost::new(&program.cwd, &file_texts);
            let excluded = scope.exclusions_for_case(
                &fixture_name,
                &program.matrix_key,
                &golden_case.oracle,
            )?;
            let (_, fully_excluded) =
                supported_case_view(&golden_case.oracle, DiagnosticBand::All, &excluded);

            // The report is an explicit A3 producer run: independently
            // collect the genuine structured records and bytes, then
            // require the committed structured golden to remain exact.
            let genuine = pool.diagnostics_with_rendering(program_path)?;
            let genuine_golden = genuine
                .diagnostics
                .iter()
                .map(|diagnostic| GoldenDiag::from_oracle(diagnostic, &file_texts))
                .collect::<Vec<_>>();
            if genuine_golden != golden_case.oracle {
                return Err(format!(
                    "T4 report found structured oracle drift for {fixture_name} [{}]; \
                     an oracle-correction transition is required before rendered evidence",
                    program.matrix_key
                )
                .into());
            }
            let context = format!("T4 report {fixture_name} [{}]", program.matrix_key);
            let genuine_empty_related_information =
                collect_genuine_empty_related_information(&genuine.diagnostics);
            let oracle_empty_related_information = effective_oracle_empty_related_information(
                golden.schema,
                &golden_case.oracle,
                &golden_case.oracle_empty_related_information,
                &genuine_empty_related_information,
                &context,
            )?;

            let oracle_full_hash = rendered_sha256(&genuine.rendered);
            let rust_oracle_full_rendered = format_sorted_diagnostics_with_context(
                &diagnostics_from_golden_with_empty_related_information(
                    &golden_case.oracle,
                    &oracle_empty_related_information,
                )?,
                &host,
            )?;
            let rust_oracle_full_hash = rendered_sha256(&rust_oracle_full_rendered);
            let rust_formatter_matches_oracle = rust_oracle_full_rendered == genuine.rendered;
            let (golden_oracle_hash, pin_matches) = evaluate_golden_oracle_pin(
                golden.schema,
                &golden_case.oracle_cli_hash,
                &oracle_full_hash,
            );

            // Filtering preserves the already canonical tsc order.
            // Never sort serialized golden records again: canonicalHead
            // is intentionally absent from their wire representation.
            let oracle_supported = genuine
                .diagnostics
                .iter()
                .enumerate()
                .filter(|(index, _)| !excluded.contains(index))
                .map(|(_, diagnostic)| diagnostic.clone())
                .collect::<Vec<_>>();
            let oracle_rendered = pool.render_sorted_records(program_path, &oracle_supported)?;

            let current = current_case_tsrs(&fixture_name, program, &vendor_lib_dir)?;
            let tsrs_supported = current
                .all
                .iter()
                .enumerate()
                .filter(|(_, diagnostic)| !fully_excluded.contains(&t0_key(diagnostic)))
                .collect::<Vec<_>>();
            let tsrs_supported = diagnostics_from_indexed_golden_refs(
                tsrs_supported,
                &current.all_empty_related_information,
            )?;
            let tsrs_rendered = format_sorted_diagnostics_with_context(&tsrs_supported, &host)?;

            let oracle_hash = rendered_sha256(&oracle_rendered);
            let tsrs_hash = rendered_sha256(&tsrs_rendered);
            let matched = oracle_rendered == tsrs_rendered
                && rust_formatter_matches_oracle
                && pin_matches.unwrap_or(true);
            let first_difference = if oracle_rendered != tsrs_rendered {
                Some(first_rendered_difference(&oracle_rendered, &tsrs_rendered))
            } else if !rust_formatter_matches_oracle {
                Some(first_rendered_difference(
                    &genuine.rendered,
                    &rust_oracle_full_rendered,
                ))
            } else {
                None
            };
            details.push(T4CaseReport {
                fixture: fixture_name.clone(),
                matrix_key: program.matrix_key.clone(),
                golden_schema: golden.schema,
                excluded_oracle_records: excluded.len(),
                oracle_cli_hash: oracle_hash,
                tsrs_cli_hash: tsrs_hash,
                oracle_full_cli_hash: oracle_full_hash,
                rust_oracle_full_cli_hash: rust_oracle_full_hash,
                rust_formatter_matches_oracle,
                golden_oracle_cli_hash: golden_oracle_hash,
                oracle_pin_matches: pin_matches,
                matched,
                first_difference,
            });
        }
    }

    if options.limit.is_none() && options.files.is_empty() {
        scope.finish_full_validation()?;
    }
    let schema_3_pinned_cases = details
        .iter()
        .filter(|case| case.golden_schema >= GOLDEN_RENDER_SCHEMA)
        .count();
    let matched_cases = details.iter().filter(|case| case.matched).count();
    let oracle_pin_failures = details
        .iter()
        .filter(|case| case.oracle_pin_matches == Some(false))
        .count();
    let rust_formatter_failures = details
        .iter()
        .filter(|case| !case.rust_formatter_matches_oracle)
        .count();
    Ok(T4Report {
        schema: REPORT_SCHEMA,
        status: "report-only".to_owned(),
        formatter:
            "typescript-6.0.3/formatDiagnosticsWithColorAndContext-minus-ansi;cwd=program;newline=lf"
                .to_owned(),
        hash: "sha256(normalized-rendered-utf8)".to_owned(),
        fixtures: fixtures.len(),
        cases: details.len(),
        schema_3_pinned_cases,
        matched_cases,
        mismatched_cases: details.len() - matched_cases,
        oracle_pin_failures,
        rust_formatter_failures,
        cases_detail: details,
    })
}

/// One-time schema extension or post-A3 check. This is deliberately
/// separate from ordinary `oracle-refresh`: schema 2 placeholders are
/// never silently reinterpreted as rendered hashes.
pub fn check_or_extend_rendered_hashes(
    options: &RefreshOptions,
    mode: RenderHashMode,
) -> ConformanceResult<RenderHashSummary> {
    if mode == RenderHashMode::ExtendSchema3 {
        return Err(
            "schema-3 rendered hashes may be written only by the atomic \
             `ratchet update --transition t4-input-schema-extension` transaction"
                .into(),
        );
    }
    Ok(plan_rendered_hashes(options, mode)?.summary)
}

pub(crate) fn plan_rendered_hash_extension(
    options: &RefreshOptions,
) -> ConformanceResult<RenderHashExtensionPlan> {
    plan_rendered_hashes(options, RenderHashMode::ExtendSchema3)
}

fn plan_rendered_hashes(
    options: &RefreshOptions,
    mode: RenderHashMode,
) -> ConformanceResult<RenderHashExtensionPlan> {
    if mode == RenderHashMode::ExtendSchema3
        && (options.limit.is_some() || !options.files.is_empty())
    {
        return Err(
            "the A3 schema-3 extension requires the complete fixed oracle universe; \
             focused selections are report-only"
                .into(),
        );
    }

    let fixtures = select_fixtures(options)?;
    let vendor_lib_dir = options.workspace.join("vendor/typescript-6.0.3/lib");
    let goldens_root = options.workspace.join("goldens");
    let temp_root = super::temp_root("tsc-rs-render-hash-refresh");
    if temp_root.exists() {
        fs::remove_dir_all(&temp_root)?;
    }
    fs::create_dir_all(&temp_root)?;
    let pool = OraclePool::new_render_only();
    super::ratchet::verify_launched_render_node(&options.workspace, &pool)?;

    let mut staged = Vec::<GoldenFile>::new();
    let mut cases = 0usize;
    let mut diagnostics = 0usize;
    let mut upgraded = 0usize;
    let mut checked = 0usize;
    let mut pins = super::ratchet::T4OraclePins::new();
    let mut empty_related_information = T4OracleEmptyRelatedInformation::new();

    for (fixture_index, fixture) in fixtures.iter().enumerate() {
        let fixture_name = fixture_key(&options.workspace, fixture)?;
        let mut golden = read_golden(&goldens_root, &fixture_name)?;
        match (mode, golden.schema) {
            (RenderHashMode::ExtendSchema3, 2) => upgraded += 1,
            (RenderHashMode::ExtendSchema3, GOLDEN_RENDER_SCHEMA)
            | (RenderHashMode::Check, GOLDEN_RENDER_SCHEMA) => checked += 1,
            (RenderHashMode::Check, schema) => {
                return Err(format!(
                    "golden {fixture_name} is schema {schema}; schema-2 legacy hashes are not T4 \
                     evidence — run the reviewed A3 schema extension first"
                )
                .into())
            }
            (_, schema) => {
                return Err(format!(
                    "golden {fixture_name} uses unsupported render schema {schema}"
                )
                .into())
            }
        }

        let programs = tsc_harness::expand_fixture_file(fixture, &vendor_lib_dir)?;
        let program_dir = temp_root.join("programs").join(fixture_index.to_string());
        let program_paths = tsc_harness::write_program_jsons(&programs, &program_dir)?;
        let input_schema = golden.schema;
        let mut fixture_empty_related_information = BTreeMap::new();
        let mut golden_by_matrix = golden
            .cases
            .iter_mut()
            .map(|case| (case.matrix_key.clone(), case))
            .collect::<BTreeMap<_, _>>();
        for (program, program_path) in programs.iter().zip(program_paths.iter()) {
            let golden_case = golden_by_matrix
                .get_mut(&program.matrix_key)
                .ok_or_else(|| {
                    format!(
                        "missing golden case {fixture_name} [{}]",
                        program.matrix_key
                    )
                })?;
            let response = pool.diagnostics_with_rendering(program_path)?;
            let file_texts = file_texts_for_program(program, &vendor_lib_dir)?;
            let oracle = response
                .diagnostics
                .iter()
                .map(|diagnostic| GoldenDiag::from_oracle(diagnostic, &file_texts))
                .collect::<Vec<_>>();
            if oracle != golden_case.oracle {
                return Err(format!(
                    "A3 schema extension would change structured oracle records for \
                     {fixture_name} [{}]; rendered hashes may only be added after an \
                     oracle-correction transition",
                    program.matrix_key
                )
                .into());
            }
            let context = format!(
                "A3 rendered-hash plan {fixture_name} [{}]",
                program.matrix_key
            );
            let genuine_empty_related_information =
                collect_genuine_empty_related_information(&response.diagnostics);
            let oracle_empty_related_information = effective_oracle_empty_related_information(
                input_schema,
                &golden_case.oracle,
                &golden_case.oracle_empty_related_information,
                &genuine_empty_related_information,
                &context,
            )?;
            let host = FormatDiagnosticsHost::new(&program.cwd, &file_texts);
            let rust_rendered = format_sorted_diagnostics_with_context(
                &diagnostics_from_golden_with_empty_related_information(
                    &golden_case.oracle,
                    &oracle_empty_related_information,
                )?,
                &host,
            )?;
            if rust_rendered != response.rendered {
                let difference = first_rendered_difference(&response.rendered, &rust_rendered);
                return Err(format!(
                    "A3 schema extension requires Rust/oracle full-render byte parity for \
                     {fixture_name} [{}]; first difference at line {}: oracle={:?}, rust={:?}",
                    program.matrix_key, difference.line, difference.oracle, difference.tsrs
                )
                .into());
            }
            let hash = rendered_sha256(&response.rendered);
            if input_schema >= GOLDEN_RENDER_SCHEMA {
                if !valid_sha256(&golden_case.oracle_cli_hash)
                    || golden_case.oracle_cli_hash != hash
                {
                    return Err(format!(
                        "stale oracle rendered hash for {fixture_name} [{}]: stored={} current={hash}",
                        program.matrix_key, golden_case.oracle_cli_hash
                    )
                    .into());
                }
                if !golden_case.tsrs.is_empty() || !golden_case.tsrs_cli_hash.is_empty() {
                    return Err(format!(
                        "schema-3 golden {fixture_name} [{}] persists tsrs output; schema 3 is oracle-only",
                        program.matrix_key
                    )
                    .into());
                }
            } else {
                golden_case.oracle_cli_hash = hash;
                golden_case.tsrs.clear();
                golden_case.tsrs_cli_hash.clear();
                golden_case.oracle_empty_related_information =
                    genuine_empty_related_information.clone();
            }
            fixture_empty_related_information.insert(
                program.matrix_key.clone(),
                genuine_empty_related_information,
            );
            cases += 1;
            diagnostics += oracle.len();
        }
        empty_related_information.insert(fixture_name.clone(), fixture_empty_related_information);
        pins.insert(
            fixture_name,
            golden
                .cases
                .iter()
                .map(|case| (case.matrix_key.clone(), case.oracle_cli_hash.clone()))
                .collect(),
        );
        if mode == RenderHashMode::ExtendSchema3 && golden.schema == 2 {
            golden.schema = GOLDEN_RENDER_SCHEMA;
            staged.push(golden);
        }
    }

    let mut updates = Vec::new();
    if mode == RenderHashMode::ExtendSchema3 {
        for golden in &staged {
            let path = golden_path(&goldens_root, &golden.fixture);
            updates.push(PlannedGoldenUpdate {
                original: fs::read(&path)?,
                replacement: encode_golden(golden)?,
                path,
            });
        }
    }
    fs::remove_dir_all(&temp_root)?;
    Ok(RenderHashExtensionPlan {
        summary: RenderHashSummary {
            mode: match mode {
                RenderHashMode::ExtendSchema3 => "schema-3-extension",
                RenderHashMode::Check => "check-only",
            }
            .to_owned(),
            fixtures: fixtures.len(),
            cases,
            oracle_diagnostics: diagnostics,
            schema_2_upgraded: upgraded,
            schema_3_checked: checked,
        },
        pins,
        empty_related_information,
        updates,
    })
}

#[cfg(test)]
fn diagnostics_from_golden(records: &[GoldenDiag]) -> ConformanceResult<Vec<Diagnostic>> {
    diagnostics_from_golden_with_empty_related_information(records, &BTreeSet::new())
}

fn diagnostics_from_golden_with_empty_related_information(
    records: &[GoldenDiag],
    empty_related_information: &BTreeSet<usize>,
) -> ConformanceResult<Vec<Diagnostic>> {
    diagnostics_from_indexed_golden_refs(records.iter().enumerate(), empty_related_information)
}

fn diagnostics_from_indexed_golden_refs<'a>(
    records: impl IntoIterator<Item = (usize, &'a GoldenDiag)>,
    empty_related_information: &BTreeSet<usize>,
) -> ConformanceResult<Vec<Diagnostic>> {
    records
        .into_iter()
        .map(|(index, record)| {
            diagnostic_from_golden(record, empty_related_information.contains(&index))
        })
        .collect()
}

/// Node-free A1 gate for one case after T4 activation. The oracle side
/// removes exact A2 occurrences; the tsrs side removes only buckets that
/// are fully excluded, matching the supported-scope tier contract.
pub(crate) fn supported_case_t4_matches(
    program: &tsc_harness::ProgramJson,
    vendor_lib_dir: &std::path::Path,
    oracle: (&[GoldenDiag], &[usize]),
    tsrs: (&[GoldenDiag], &BTreeSet<usize>),
    excluded_indices: &BTreeSet<usize>,
    fully_excluded: &BTreeSet<super::T0Key>,
    oracle_full_sha256: &str,
) -> ConformanceResult<bool> {
    let (oracle, oracle_empty_related_information) = oracle;
    let (tsrs, tsrs_empty_related_information) = tsrs;
    let file_texts = file_texts_for_program(program, vendor_lib_dir)?;
    let host = FormatDiagnosticsHost::new(&program.cwd, &file_texts);
    if !valid_sha256(oracle_full_sha256) {
        return Err("active T4 case carries an invalid oracle rendered SHA-256".into());
    }
    let oracle_empty_related_information = validate_empty_related_information(
        oracle,
        oracle_empty_related_information,
        "active T4 oracle",
    )?;
    let tsrs_empty_related_information = validate_empty_related_information(
        tsrs,
        &tsrs_empty_related_information
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        "current tsrs T4 stream",
    )?;
    let full_oracle = format_sorted_diagnostics_with_context(
        &diagnostics_from_golden_with_empty_related_information(
            oracle,
            &oracle_empty_related_information,
        )?,
        &host,
    )?;
    let actual_full_sha256 = rendered_sha256(&full_oracle);
    if actual_full_sha256 != oracle_full_sha256 {
        return Err(format!(
            "Rust formatter drifted from the genuine oracle T4 pin: expected \
             {oracle_full_sha256}, measured {actual_full_sha256}"
        )
        .into());
    }
    let oracle = oracle
        .iter()
        .enumerate()
        .filter(|(index, _)| !excluded_indices.contains(index));
    let tsrs = tsrs
        .iter()
        .enumerate()
        .filter(|(_, diagnostic)| !fully_excluded.contains(&t0_key(diagnostic)));
    let oracle = format_sorted_diagnostics_with_context(
        &diagnostics_from_indexed_golden_refs(oracle, &oracle_empty_related_information)?,
        &host,
    )?;
    let tsrs = format_sorted_diagnostics_with_context(
        &diagnostics_from_indexed_golden_refs(tsrs, &tsrs_empty_related_information)?,
        &host,
    )?;
    Ok(oracle == tsrs)
}

fn diagnostic_from_golden(
    record: &GoldenDiag,
    empty_related_information: bool,
) -> ConformanceResult<Diagnostic> {
    let mut message = message_from_golden(&record.chain)?;
    // A Diagnostic's outer code/category are independent from the root
    // DiagnosticMessageChain fields. The renderer and sorter consume
    // the outer pair; children retain their own chain metadata.
    message.code = record.code;
    message.category = category_from_name(&record.category)?;
    let mut diagnostic = Diagnostic::new(record.file.clone(), record.start, record.length, message);
    diagnostic.related = record
        .related
        .iter()
        .map(|related| {
            let mut message = message_from_golden(&related.chain)?;
            message.code = related.code;
            message.category = category_from_name(&related.category)?;
            Ok(RelatedInfo {
                file_name: related.file.clone(),
                start: related.start,
                length: related.length,
                message,
            })
        })
        .collect::<ConformanceResult<Vec<_>>>()?;
    diagnostic.related_information_present =
        empty_related_information || !diagnostic.related.is_empty();
    diagnostic.reports_unnecessary = record.reports_unnecessary.then_some(true);
    diagnostic.reports_deprecated = record.reports_deprecated.then_some(true);
    diagnostic.source = record.source.clone();
    Ok(diagnostic)
}

fn message_from_golden(chain: &GoldenMessageChain) -> ConformanceResult<MessageChain> {
    Ok(MessageChain {
        code: chain.code,
        category: category_from_name(&chain.category)?,
        text: chain.text.clone(),
        next_present: !chain.next.is_empty(),
        next: chain
            .next
            .iter()
            .map(message_from_golden)
            .collect::<ConformanceResult<Vec<_>>>()?,
    })
}

fn category_from_name(name: &str) -> ConformanceResult<DiagnosticCategory> {
    match name {
        "warning" => Ok(DiagnosticCategory::Warning),
        "error" => Ok(DiagnosticCategory::Error),
        "suggestion" => Ok(DiagnosticCategory::Suggestion),
        "message" => Ok(DiagnosticCategory::Message),
        other => Err(format!("unknown diagnostic category {other:?} in golden").into()),
    }
}

fn rendered_sha256(rendered: &str) -> String {
    let digest = Sha256::digest(rendered.as_bytes());
    let mut hash = String::with_capacity(64);
    for byte in digest {
        let _ = write!(hash, "{byte:02x}");
    }
    hash
}

fn valid_sha256(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn evaluate_golden_oracle_pin(
    golden_schema: u32,
    stored: &str,
    observed: &str,
) -> (Option<String>, Option<bool>) {
    let stored = (golden_schema >= GOLDEN_RENDER_SCHEMA).then(|| stored.to_owned());
    let matches = stored
        .as_ref()
        .map(|expected| valid_sha256(expected) && expected == observed);
    (stored, matches)
}

fn first_rendered_difference(oracle: &str, tsrs: &str) -> RenderedDifference {
    let mut oracle_lines = oracle.split('\n');
    let mut tsrs_lines = tsrs.split('\n');
    for line in 1.. {
        let oracle_line = oracle_lines.next();
        let tsrs_line = tsrs_lines.next();
        if oracle_line != tsrs_line {
            return RenderedDifference {
                line,
                oracle: oracle_line.map(str::to_owned),
                tsrs: tsrs_line.map(str::to_owned),
            };
        }
        if oracle_line.is_none() {
            unreachable!("equal exhausted rendered outputs have equal SHA-256");
        }
    }
    unreachable!("unbounded line iterator returns on the first difference")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tsc_oracle::{OracleDiag, OracleMessageChain, OracleRelated};

    fn oracle_chain(code: u32, category: &str, text: &str) -> OracleMessageChain {
        OracleMessageChain {
            text: text.to_owned(),
            code,
            category: category.to_owned(),
            next: Vec::new(),
        }
    }

    fn oracle_diag(
        file: Option<&str>,
        start: Option<u32>,
        length: Option<u32>,
        code: u32,
        category: &str,
        text: &str,
    ) -> OracleDiag {
        OracleDiag {
            file: file.map(str::to_owned),
            start,
            length,
            code,
            pass: None,
            category: category.to_owned(),
            chain: oracle_chain(code, category, text),
            related: Vec::new(),
            related_information_present: false,
            reports_unnecessary: false,
            reports_deprecated: false,
            source: None,
        }
    }

    #[test]
    fn schema3_hash_contract_is_sha256_of_exact_rendered_utf8() {
        assert_eq!(
            rendered_sha256("error TS1: x\n"),
            "70897b64d4f29f0963accdf7d4b618f72f1313eb86d9f68fa6208815ebd8eb1d"
        );
        assert!(valid_sha256(
            "70897b64d4f29f0963accdf7d4b618f72f1313eb86d9f68fa6208815ebd8eb1d"
        ));
        assert!(!valid_sha256("CBF29CE484222325"));
    }

    #[test]
    fn schema2_legacy_hashes_never_become_t4_pins() {
        let observed = "a".repeat(64);
        assert_eq!(
            evaluate_golden_oracle_pin(2, "cbf29ce484222325", &observed),
            (None, None)
        );
        assert_eq!(
            evaluate_golden_oracle_pin(2, &observed, &observed),
            (None, None),
            "even a SHA-256-shaped schema-2 value is legacy evidence"
        );
        assert_eq!(
            evaluate_golden_oracle_pin(3, &observed, &observed),
            (Some(observed.clone()), Some(true))
        );
        assert_eq!(
            evaluate_golden_oracle_pin(3, &"b".repeat(64), &observed),
            (Some("b".repeat(64)), Some(false))
        );
        assert_eq!(
            evaluate_golden_oracle_pin(3, "not-a-sha256", &observed),
            (Some("not-a-sha256".to_owned()), Some(false))
        );
    }

    #[test]
    fn schema3_empty_related_metadata_does_not_change_structured_oracle_bytes() {
        let file_texts = BTreeMap::new();
        let absent = oracle_diag(
            None,
            None,
            None,
            2769,
            "error",
            "No overload matches this call.",
        );
        let mut present = absent.clone();
        present.related_information_present = true;
        let absent_structured = GoldenDiag::from_oracle(&absent, &file_texts);
        let oracle = vec![GoldenDiag::from_oracle(&present, &file_texts)];
        assert_eq!(oracle[0], absent_structured);
        let mut case = super::super::GoldenCase {
            matrix_key: String::new(),
            tsrs: Vec::new(),
            oracle,
            oracle_empty_related_information: Vec::new(),
            tsrs_cli_hash: String::new(),
            oracle_cli_hash: "a".repeat(64),
        };
        let structured_before = serde_json::to_vec(&case.oracle).unwrap();
        assert!(!serde_json::to_string(&case)
            .unwrap()
            .contains("oracle_empty_related_information"));

        case.oracle_empty_related_information = vec![0];
        assert_eq!(serde_json::to_vec(&case.oracle).unwrap(), structured_before);
        assert!(serde_json::to_string(&case)
            .unwrap()
            .contains(r#""oracle_empty_related_information":[0]"#));
    }

    #[test]
    fn empty_related_metadata_rehydrates_the_formatter_presence_bit() {
        let file_texts = BTreeMap::new();
        let records = vec![GoldenDiag::from_oracle(
            &oracle_diag(
                None,
                None,
                None,
                2769,
                "error",
                "No overload matches this call.",
            ),
            &file_texts,
        )];
        let absent = diagnostics_from_golden(&records).unwrap();
        let present =
            diagnostics_from_golden_with_empty_related_information(&records, &BTreeSet::from([0]))
                .unwrap();
        assert!(!absent[0].related_information_present);
        assert!(present[0].related_information_present);

        let host = FormatDiagnosticsHost::new("/workspace", &file_texts);
        assert_eq!(
            format_sorted_diagnostics_with_context(&absent, &host).unwrap(),
            "error TS2769: No overload matches this call.\n"
        );
        assert_eq!(
            format_sorted_diagnostics_with_context(&present, &host).unwrap(),
            "error TS2769: No overload matches this call.\n\n"
        );
    }

    #[test]
    fn empty_related_metadata_is_validated_and_projection_keeps_original_indices() {
        let file_texts = BTreeMap::new();
        let first = GoldenDiag::from_oracle(
            &oracle_diag(None, None, None, 1, "error", "first"),
            &file_texts,
        );
        let second = GoldenDiag::from_oracle(
            &oracle_diag(None, None, None, 2, "error", "second"),
            &file_texts,
        );
        let records = vec![first, second];
        let indices = validate_empty_related_information(&records, &[1], "test").unwrap();
        let projected =
            diagnostics_from_indexed_golden_refs(records.iter().enumerate().skip(1), &indices)
                .unwrap();
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].code(), 2);
        assert!(projected[0].related_information_present);

        let duplicate = validate_empty_related_information(&records, &[1, 1], "test")
            .unwrap_err()
            .to_string();
        assert!(duplicate.contains("strictly increasing"), "{duplicate}");
        let out_of_range = validate_empty_related_information(&records, &[2], "test")
            .unwrap_err()
            .to_string();
        assert!(out_of_range.contains("out of range"), "{out_of_range}");
        let mut non_empty = records.clone();
        non_empty[0].related.push(super::super::GoldenRelated {
            file: None,
            start: None,
            length: None,
            code: 1,
            category: "message".to_owned(),
            chain: GoldenMessageChain {
                text: "related".to_owned(),
                code: 1,
                category: "message".to_owned(),
                next: Vec::new(),
            },
        });
        let points_to_rows = validate_empty_related_information(&non_empty, &[0], "test")
            .unwrap_err()
            .to_string();
        assert!(
            points_to_rows.contains("serialized related rows"),
            "{points_to_rows}"
        );

        assert_eq!(
            effective_oracle_empty_related_information(2, &records, &[], &[1], "test").unwrap(),
            BTreeSet::from([1])
        );
        let schema2_metadata =
            effective_oracle_empty_related_information(2, &records, &[1], &[1], "test")
                .unwrap_err()
                .to_string();
        assert!(schema2_metadata.contains("schema-2"), "{schema2_metadata}");
        let stale = effective_oracle_empty_related_information(3, &records, &[], &[1], "test")
            .unwrap_err()
            .to_string();
        assert!(stale.contains("drifted"), "{stale}");
    }

    #[test]
    fn first_difference_pins_newline_sensitive_report_bytes() {
        assert_eq!(
            serde_json::to_string(&first_rendered_difference(
                "a.ts:1:1 - error TS1: x\n",
                "a.ts:1:1 - error TS1: x\r\n"
            ))
            .unwrap(),
            r#"{"line":1,"oracle":"a.ts:1:1 - error TS1: x","tsrs":"a.ts:1:1 - error TS1: x\r"}"#
        );
    }

    #[test]
    fn rust_and_vendored_node_pin_every_formatter_structure() {
        let temp = super::super::temp_root("tsc-rs-render-vector");
        fs::create_dir_all(&temp).unwrap();
        let program_json = temp.join("program.json");
        fs::write(
            &program_json,
            r#"{
  "schema": 1,
  "cwd": "/workspace/src",
  "options": {"noLib": true},
  "libs": [],
  "files": [
    {"name": "main.ts", "textB64": "Y29uc3QJZmFjZSA9ICLwn5iAIjsNCmINCmMNCmQNCmUNCmYNCg=="},
    {"name": "origin.ts", "textB64": "ZXhwb3J0IGNvbnN0IG9yaWdpbiA9IDE7Cg=="},
    {"name": "../z.ts", "textB64": "ego="}
  ],
  "matrixKey": ""
}
"#,
        )
        .unwrap();

        let mut error = oracle_diag(Some("main.ts"), Some(14), Some(2), 2322, "error", "Head");
        // The outer Diagnostic header owns these bytes; a chain root
        // may carry different metadata and must not replace it.
        error.chain.code = 9999;
        error.chain.category = "message".to_owned();
        error.chain.next = vec![oracle_chain(2322, "error", "Child")];
        error.related = vec![OracleRelated {
            file: Some("origin.ts".to_owned()),
            start: Some(13),
            length: Some(6),
            code: 2728,
            category: "message".to_owned(),
            chain: oracle_chain(9998, "warning", "Origin"),
        }];
        let mut suggestion = oracle_diag(
            Some("main.ts"),
            Some(20),
            Some(13),
            80001,
            "suggestion",
            "Hint",
        );
        suggestion.pass = Some("suggestion".to_owned());
        suggestion.reports_unnecessary = true;
        let z = oracle_diag(Some("../z.ts"), Some(0), Some(1), 1, "error", "Z");
        let fileless = oracle_diag(None, None, None, 999, "message", "Global");
        let records = vec![z, suggestion.clone(), error, fileless, suggestion];

        let pool = OraclePool::new_render_only();
        let node = pool.render_records(&program_json, &records).unwrap();
        let file_texts = BTreeMap::from([
            (
                "main.ts".to_owned(),
                "const\tface = \"😀\";\r\nb\r\nc\r\nd\r\ne\r\nf\r\n".to_owned(),
            ),
            (
                "origin.ts".to_owned(),
                "export const origin = 1;\n".to_owned(),
            ),
            ("../z.ts".to_owned(), "z\n".to_owned()),
        ]);
        let golden = records
            .iter()
            .map(|record| GoldenDiag::from_oracle(record, &file_texts))
            .collect::<Vec<_>>();
        let rust = tsc_diagnostics::format_diagnostics_with_context(
            &diagnostics_from_golden(&golden).unwrap(),
            &FormatDiagnosticsHost::new("/workspace/src", &file_texts),
        )
        .unwrap();

        assert_eq!(rust, node);
        let sorted_node = pool.render_sorted_records(&program_json, &records).unwrap();
        let sorted_rust = format_sorted_diagnostics_with_context(
            &diagnostics_from_golden(&golden).unwrap(),
            &FormatDiagnosticsHost::new("/workspace/src", &file_texts),
        )
        .unwrap();
        assert_eq!(sorted_rust, sorted_node);
        assert_ne!(
            sorted_node, node,
            "already-sorted entry point must preserve input order and duplicates"
        );
        assert_eq!(
            rendered_sha256(&node),
            "849163464e947f86eaf4b616e1280c4c918983968d355eda4162b2b137c37713"
        );
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn focused_schema3_t4_report_stays_report_only_and_checks_active_pins() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let report_temp = super::super::temp_root("tsc-rs-rendered-output-report");
        let report = run_t4_report(&T4ReportOptions {
            workspace,
            limit: None,
            files: vec![PathBuf::from(
                "ts-tests/tests/cases/conformance/decorators/missingDecoratorType.ts",
            )],
        })
        .unwrap();

        assert!(
            !report_temp.exists(),
            "focused report must remove its temporary program JSON tree"
        );
        assert_eq!(report.schema, 2);
        assert_eq!(report.status, "report-only");
        assert_eq!(report.fixtures, 1);
        assert_eq!(report.cases, 2);
        assert_eq!(report.schema_3_pinned_cases, 2);
        assert_eq!(report.matched_cases, 2);
        assert_eq!(report.mismatched_cases, 0);
        assert_eq!(report.oracle_pin_failures, 0);
        assert_eq!(report.rust_formatter_failures, 0);
        assert!(report.cases_detail.iter().all(|case| {
            case.golden_schema == 3
                && case.golden_oracle_cli_hash.as_deref()
                    == Some(case.oracle_full_cli_hash.as_str())
                && case.oracle_pin_matches == Some(true)
                && case.rust_formatter_matches_oracle
                && case.oracle_full_cli_hash == case.rust_oracle_full_cli_hash
                && valid_sha256(&case.oracle_cli_hash)
                && valid_sha256(&case.tsrs_cli_hash)
        }));
    }
}

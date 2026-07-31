//! Deterministic, typed comparison of the two compiler observations.
//!
//! T0 is deliberately a set comparison. T1-T3 are multiset comparisons:
//! duplicate diagnostics are observable even when their projected keys are
//! otherwise equal. The comparator always walks tier first and pass second.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::model::{
    AssembledDiagnostic, CaseExecution, CompletedOutcome, DiagnosticCategory, DiagnosticFile,
    DiagnosticPass, DiagnosticRecord, EngineResult, MessageChain, OptionalU32, RelatedDiagnostic,
    RendererObservation, TerminalOutcome,
};
use crate::normalize::NormalizationSpec;
use crate::schema::{CaseSpec, ValidatedCaseContext};
use crate::{FoundationError, FoundationResult};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComparisonTier {
    T0,
    T1,
    T2,
    T3,
    T4,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DifferenceSide {
    Oracle,
    Tsrs,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RendererDifference {
    Order,
    Dedupe,
    Path,
    Newline,
    Text,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OneSidedDiagnostic {
    pub side: DifferenceSide,
    pub diagnostic: DiagnosticRecord,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticDivergence {
    pub tier: ComparisonTier,
    pub pass: DiagnosticPass,
    pub one_sided: Vec<OneSidedDiagnostic>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RendererDivergence {
    pub class: RendererDifference,
    pub affected: DiagnosticRecord,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    content = "divergence",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
pub enum Divergence {
    Diagnostic(DiagnosticDivergence),
    Renderer(RendererDivergence),
    TsrsTerminal(TerminalOutcome),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "status",
    content = "divergence",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
pub enum Comparison {
    Exact,
    Divergence(Divergence),
}

impl Comparison {
    pub fn validate(&self) -> FoundationResult<()> {
        match self {
            Self::Exact => Ok(()),
            Self::Divergence(Divergence::Diagnostic(divergence)) => {
                if divergence.tier == ComparisonTier::T4 {
                    return Err(FoundationError::new(
                        "diagnostic comparison divergence cannot use renderer tier t4",
                    ));
                }
                if divergence.one_sided.is_empty() {
                    return Err(FoundationError::new(
                        "diagnostic comparison divergence must retain at least one one-sided row",
                    ));
                }
                for (index, row) in divergence.one_sided.iter().enumerate() {
                    if row.diagnostic.pass != divergence.pass {
                        return Err(FoundationError::new(format!(
                            "comparison one_sided[{index}] pass does not match its diagnostic divergence"
                        )));
                    }
                    row.diagnostic
                        .validate(&format!("comparison.one_sided[{index}].diagnostic"))?;
                }
                Ok(())
            }
            Self::Divergence(Divergence::Renderer(divergence)) => {
                divergence.affected.validate("comparison.renderer.affected")
            }
            Self::Divergence(Divergence::TsrsTerminal(outcome)) => EngineResult::Terminal {
                outcome: outcome.clone(),
            }
            .validate("comparison.tsrs_terminal"),
        }
    }

    pub fn canonical_bytes(&self) -> FoundationResult<Vec<u8>> {
        self.validate()?;
        serde_json::to_vec(self)
            .map_err(|error| FoundationError::new(format!("cannot serialize comparison: {error}")))
    }

    pub fn from_json_slice(bytes: &[u8]) -> FoundationResult<Self> {
        let comparison: Self = serde_json::from_slice(bytes)
            .map_err(|error| FoundationError::new(format!("invalid comparison JSON: {error}")))?;
        comparison.validate()?;
        Ok(comparison)
    }

    pub fn from_canonical_slice(bytes: &[u8]) -> FoundationResult<Self> {
        let comparison = Self::from_json_slice(bytes)?;
        if comparison.canonical_bytes()? != bytes {
            return Err(FoundationError::new(
                "comparison input is valid JSON but not canonical compact schema-1 bytes",
            ));
        }
        Ok(comparison)
    }
}

/// Standalone comparison helper for tests and inspection.
///
/// Authoritative producer/verifier code must use
/// [`crate::evaluate::evaluate_case`] so raw evidence, comparison, and class
/// cannot be mixed across executions. Producer failures and all oracle
/// terminal outcomes are errors in this foundation slice: the checked-in
/// registry is still draft, so no caller-constructible whitelist is accepted.
pub fn compare_case(case: &CaseSpec, execution: &CaseExecution) -> FoundationResult<Comparison> {
    let validated = case.validated_context()?;
    execution.validate_with_context(&validated)?;
    let normalization = NormalizationSpec::for_validated_case(validated.case())?;
    compare_validated(execution, &validated, &normalization)
}

pub(crate) fn compare_validated(
    execution: &CaseExecution,
    validated: &ValidatedCaseContext<'_>,
    normalization: &NormalizationSpec,
) -> FoundationResult<Comparison> {
    let CaseExecution::Compared { oracle, tsrs } = execution else {
        return Err(FoundationError::new(
            "producer failure invalidates the case and has no comparison class",
        ));
    };

    match (oracle, tsrs) {
        (
            EngineResult::Completed {
                outcome: oracle_outcome,
            },
            EngineResult::Completed {
                outcome: tsrs_outcome,
            },
        ) => {
            compare_completed(
                oracle_outcome,
                tsrs_outcome,
                validated,
                normalization,
            )
        }
        (EngineResult::Completed { .. }, EngineResult::Terminal { outcome }) => {
            Ok(Comparison::Divergence(Divergence::TsrsTerminal(
                outcome.clone(),
            )))
        }
        (EngineResult::Terminal { .. }, _) => Err(FoundationError::new(
            "oracle terminal outcome is invalid until the exact frozen registry verifier is integrated",
        )),
    }
}

fn compare_completed(
    oracle: &CompletedOutcome,
    tsrs: &CompletedOutcome,
    case: &ValidatedCaseContext<'_>,
    normalization: &NormalizationSpec,
) -> FoundationResult<Comparison> {
    for tier in [
        ComparisonTier::T0,
        ComparisonTier::T1,
        ComparisonTier::T2,
        ComparisonTier::T3,
    ] {
        for pass in DiagnosticPass::ORDERED {
            let one_sided = diagnostic_difference(
                &oracle.diagnostics,
                &tsrs.diagnostics,
                tier,
                pass,
                normalization,
            )?;
            if !one_sided.is_empty() {
                return Ok(Comparison::Divergence(Divergence::Diagnostic(
                    DiagnosticDivergence {
                        tier,
                        pass,
                        one_sided,
                    },
                )));
            }
        }
    }

    if let Some(divergence) =
        renderer_difference(&oracle.renderer, &tsrs.renderer, case, normalization)?
    {
        return Ok(Comparison::Divergence(Divergence::Renderer(divergence)));
    }
    Ok(Comparison::Exact)
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum TierKey {
    T0 {
        file: DiagnosticFile,
        code: u32,
        line: OptionalU32,
        column: OptionalU32,
    },
    T1 {
        file: DiagnosticFile,
        code: u32,
        line: OptionalU32,
        column: OptionalU32,
        category: DiagnosticCategory,
    },
    T2 {
        file: DiagnosticFile,
        code: u32,
        line: OptionalU32,
        column: OptionalU32,
        category: DiagnosticCategory,
        start: OptionalU32,
        length: OptionalU32,
        top_text: String,
    },
    T3 {
        file: DiagnosticFile,
        code: u32,
        line: OptionalU32,
        column: OptionalU32,
        category: DiagnosticCategory,
        start: OptionalU32,
        length: OptionalU32,
        chain: ChainKey,
        related: Vec<RelatedKey>,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ChainKey {
    text: String,
    code: u32,
    category: DiagnosticCategory,
    next: Vec<ChainKey>,
}

impl From<&MessageChain> for ChainKey {
    fn from(chain: &MessageChain) -> Self {
        Self {
            text: chain.text.clone(),
            code: chain.code,
            category: chain.category,
            next: chain.next.iter().map(Self::from).collect(),
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RelatedKey {
    file: Option<String>,
    start: Option<u32>,
    length: Option<u32>,
    code: u32,
    category: DiagnosticCategory,
    chain: ChainKey,
}

impl From<&RelatedDiagnostic> for RelatedKey {
    fn from(related: &RelatedDiagnostic) -> Self {
        Self {
            file: related.file.clone(),
            start: related.start,
            length: related.length,
            code: related.code,
            category: related.category,
            chain: ChainKey::from(&related.chain),
        }
    }
}

fn tier_key(diagnostic: &DiagnosticRecord, tier: ComparisonTier) -> TierKey {
    match tier {
        ComparisonTier::T0 => TierKey::T0 {
            file: diagnostic.file.clone(),
            code: diagnostic.code,
            line: diagnostic.line.clone(),
            column: diagnostic.column.clone(),
        },
        ComparisonTier::T1 => TierKey::T1 {
            file: diagnostic.file.clone(),
            code: diagnostic.code,
            line: diagnostic.line.clone(),
            column: diagnostic.column.clone(),
            category: diagnostic.category,
        },
        ComparisonTier::T2 => TierKey::T2 {
            file: diagnostic.file.clone(),
            code: diagnostic.code,
            line: diagnostic.line.clone(),
            column: diagnostic.column.clone(),
            category: diagnostic.category,
            start: diagnostic.start.clone(),
            length: diagnostic.length.clone(),
            top_text: diagnostic.top_text().to_owned(),
        },
        ComparisonTier::T3 => TierKey::T3 {
            file: diagnostic.file.clone(),
            code: diagnostic.code,
            line: diagnostic.line.clone(),
            column: diagnostic.column.clone(),
            category: diagnostic.category,
            start: diagnostic.start.clone(),
            length: diagnostic.length.clone(),
            chain: ChainKey::from(&diagnostic.chain),
            related: diagnostic.related.iter().map(RelatedKey::from).collect(),
        },
        ComparisonTier::T4 => unreachable!("T4 is compared from renderer observations"),
    }
}

fn diagnostic_difference(
    oracle: &[DiagnosticRecord],
    tsrs: &[DiagnosticRecord],
    tier: ComparisonTier,
    pass: DiagnosticPass,
    normalization: &NormalizationSpec,
) -> FoundationResult<Vec<OneSidedDiagnostic>> {
    let oracle = grouped_diagnostics(oracle, tier, pass);
    let tsrs = grouped_diagnostics(tsrs, tier, pass);
    let mut keys = oracle
        .keys()
        .chain(tsrs.keys())
        .cloned()
        .collect::<Vec<_>>();
    keys.sort();
    keys.dedup();

    let mut difference = Vec::new();
    for key in keys {
        let oracle_rows = oracle.get(&key).map(Vec::as_slice).unwrap_or(&[]);
        let tsrs_rows = tsrs.get(&key).map(Vec::as_slice).unwrap_or(&[]);
        let oracle_count = if tier == ComparisonTier::T0 {
            usize::from(!oracle_rows.is_empty())
        } else {
            oracle_rows.len()
        };
        let tsrs_count = if tier == ComparisonTier::T0 {
            usize::from(!tsrs_rows.is_empty())
        } else {
            tsrs_rows.len()
        };
        if oracle_count > tsrs_count {
            let rows = if tier == ComparisonTier::T0 {
                oracle_rows.to_vec()
            } else {
                residual_rows(oracle_rows, tsrs_rows, normalization)?.0
            };
            let retained = if tier == ComparisonTier::T0 {
                rows.len()
            } else {
                oracle_count - tsrs_count
            };
            for row in rows.into_iter().take(retained) {
                difference.push(OneSidedDiagnostic {
                    side: DifferenceSide::Oracle,
                    diagnostic: row.clone(),
                });
            }
        } else if tsrs_count > oracle_count {
            let rows = if tier == ComparisonTier::T0 {
                tsrs_rows.to_vec()
            } else {
                residual_rows(oracle_rows, tsrs_rows, normalization)?.1
            };
            let retained = if tier == ComparisonTier::T0 {
                rows.len()
            } else {
                tsrs_count - oracle_count
            };
            for row in rows.into_iter().take(retained) {
                difference.push(OneSidedDiagnostic {
                    side: DifferenceSide::Tsrs,
                    diagnostic: row.clone(),
                });
            }
        }
    }
    Ok(difference)
}

fn grouped_diagnostics(
    diagnostics: &[DiagnosticRecord],
    tier: ComparisonTier,
    pass: DiagnosticPass,
) -> BTreeMap<TierKey, Vec<&DiagnosticRecord>> {
    let mut grouped: BTreeMap<TierKey, Vec<&DiagnosticRecord>> = BTreeMap::new();
    for diagnostic in diagnostics.iter().filter(|row| row.pass == pass) {
        grouped
            .entry(tier_key(diagnostic, tier))
            .or_default()
            .push(diagnostic);
    }
    grouped
}

fn residual_rows<'a>(
    oracle: &[&'a DiagnosticRecord],
    tsrs: &[&'a DiagnosticRecord],
    normalization: &NormalizationSpec,
) -> FoundationResult<(Vec<&'a DiagnosticRecord>, Vec<&'a DiagnosticRecord>)> {
    type PositionFreeKey = (u32, String);
    let mut oracle_counts = BTreeMap::<PositionFreeKey, Vec<&DiagnosticRecord>>::new();
    let mut tsrs_counts = BTreeMap::<PositionFreeKey, Vec<&DiagnosticRecord>>::new();
    for row in oracle {
        oracle_counts
            .entry((
                row.code,
                normalization.normalize_after_validation(row.top_text())?,
            ))
            .or_default()
            .push(*row);
    }
    for row in tsrs {
        tsrs_counts
            .entry((
                row.code,
                normalization.normalize_after_validation(row.top_text())?,
            ))
            .or_default()
            .push(*row);
    }

    let mut oracle_residual = Vec::new();
    let mut tsrs_residual = Vec::new();
    let keys = oracle_counts
        .keys()
        .chain(tsrs_counts.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for row in keys {
        let oracle_rows = oracle_counts.get(&row).map(Vec::as_slice).unwrap_or(&[]);
        let tsrs_rows = tsrs_counts.get(&row).map(Vec::as_slice).unwrap_or(&[]);
        let oracle_count = oracle_rows.len();
        let tsrs_count = tsrs_rows.len();
        if oracle_count > tsrs_count {
            oracle_residual.extend(oracle_rows.iter().copied().take(oracle_count - tsrs_count));
        } else if tsrs_count > oracle_count {
            tsrs_residual.extend(tsrs_rows.iter().copied().take(tsrs_count - oracle_count));
        }
    }

    let paired = oracle_residual.len().min(tsrs_residual.len());
    oracle_residual.drain(..paired);
    tsrs_residual.drain(..paired);
    Ok((oracle_residual, tsrs_residual))
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RendererEffectiveKey {
    resolved_file: Option<String>,
    start: Option<u32>,
    length: Option<u32>,
    code: u32,
    message_head: String,
}

fn renderer_effective_key(
    diagnostic: &AssembledDiagnostic,
    case: &ValidatedCaseContext<'_>,
) -> FoundationResult<RendererEffectiveKey> {
    let resolved_file = match &diagnostic.diagnostic.file {
        DiagnosticFile::Global => None,
        DiagnosticFile::File { path } => Some(case.case().resolved_file_name(path)?),
    };
    Ok(RendererEffectiveKey {
        resolved_file,
        start: optional_u32(&diagnostic.diagnostic.start),
        length: optional_u32(&diagnostic.diagnostic.length),
        code: diagnostic
            .canonical_head
            .effective_code(diagnostic.diagnostic.code),
        message_head: diagnostic
            .canonical_head
            .effective_message(diagnostic.diagnostic.top_text())
            .to_owned(),
    })
}

const fn optional_u32(value: &OptionalU32) -> Option<u32> {
    match value {
        OptionalU32::Absent => None,
        OptionalU32::Present { value } => Some(*value),
    }
}

fn renderer_effective_keys(
    diagnostics: &[AssembledDiagnostic],
    case: &ValidatedCaseContext<'_>,
) -> FoundationResult<Vec<RendererEffectiveKey>> {
    diagnostics
        .iter()
        .map(|diagnostic| renderer_effective_key(diagnostic, case))
        .collect()
}

fn renderer_difference(
    oracle: &RendererObservation,
    tsrs: &RendererObservation,
    case: &ValidatedCaseContext<'_>,
    normalization: &NormalizationSpec,
) -> FoundationResult<Option<RendererDivergence>> {
    // Structured final-sequence comparison precedes byte comparison. This
    // permits a dropped/inflated diagnostic to remain observable even when
    // its rendered segment is empty.
    let oracle_keys = renderer_effective_keys(&oracle.deduped, case)?;
    let tsrs_keys = renderer_effective_keys(&tsrs.deduped, case)?;
    if oracle_keys != tsrs_keys {
        let class = if effective_multiset(&oracle_keys) == effective_multiset(&tsrs_keys) {
            RendererDifference::Order
        } else {
            RendererDifference::Dedupe
        };
        return affected_renderer_sequence(
            &oracle.deduped,
            &tsrs.deduped,
            &oracle_keys,
            &tsrs_keys,
            class,
            normalization,
        )
        .map(Some);
    }

    if oracle.aggregate_text == tsrs.aggregate_text {
        return Ok(None);
    }

    let oracle_paths =
        normalization.normalize_renderer_paths_after_validation(&oracle.aggregate_text)?;
    let tsrs_paths =
        normalization.normalize_renderer_paths_after_validation(&tsrs.aggregate_text)?;
    let class = if oracle_paths == tsrs_paths {
        RendererDifference::Path
    } else if normalization.normalize_renderer_newlines(&oracle.aggregate_text)
        == normalization.normalize_renderer_newlines(&tsrs.aggregate_text)
    {
        RendererDifference::Newline
    } else {
        RendererDifference::Text
    };

    let affected = first_affected_segment(oracle, tsrs)?;
    Ok(Some(RendererDivergence { class, affected }))
}

fn affected_renderer_sequence(
    oracle: &[AssembledDiagnostic],
    tsrs: &[AssembledDiagnostic],
    oracle_keys: &[RendererEffectiveKey],
    tsrs_keys: &[RendererEffectiveKey],
    class: RendererDifference,
    normalization: &NormalizationSpec,
) -> FoundationResult<RendererDivergence> {
    let affected = if class == RendererDifference::Dedupe {
        let oracle_counts = effective_multiset(oracle_keys);
        let tsrs_counts = effective_multiset(tsrs_keys);
        let mut keyed = oracle
            .iter()
            .zip(oracle_keys)
            .chain(tsrs.iter().zip(tsrs_keys))
            .filter(|(_, key)| {
                oracle_counts.get(*key).copied().unwrap_or(0)
                    != tsrs_counts.get(*key).copied().unwrap_or(0)
            })
            .map(|(diagnostic, _)| {
                Ok((
                    (
                        diagnostic.diagnostic.code,
                        normalization
                            .normalize_after_validation(diagnostic.diagnostic.top_text())?,
                        diagnostic,
                    ),
                    diagnostic,
                ))
            })
            .collect::<FoundationResult<Vec<_>>>()?;
        keyed.sort_by(|left, right| left.0.cmp(&right.0));
        keyed.into_iter().next().map(|(_, diagnostic)| diagnostic)
    } else {
        let index = first_differing_index(oracle_keys, tsrs_keys).ok_or_else(|| {
            FoundationError::new("renderer sequence differs without an affected diagnostic")
        })?;
        oracle.get(index).or_else(|| tsrs.get(index))
    }
    .ok_or_else(|| FoundationError::new("renderer sequence difference is empty"))?;
    Ok(RendererDivergence {
        class,
        affected: affected.diagnostic.clone(),
    })
}

fn first_differing_index<T: PartialEq>(left: &[T], right: &[T]) -> Option<usize> {
    let common = left.len().min(right.len());
    (0..common)
        .find(|&index| left[index] != right[index])
        .or_else(|| (left.len() != right.len()).then_some(common))
}

fn first_affected_segment(
    oracle: &RendererObservation,
    tsrs: &RendererObservation,
) -> FoundationResult<DiagnosticRecord> {
    for (left, right) in oracle.segments.iter().zip(&tsrs.segments) {
        if left.raw_text != right.raw_text {
            return Ok(left.diagnostic.diagnostic.clone());
        }
    }
    oracle
        .segments
        .get(tsrs.segments.len())
        .or_else(|| tsrs.segments.get(oracle.segments.len()))
        .map(|segment| segment.diagnostic.diagnostic.clone())
        .ok_or_else(|| {
            FoundationError::new(
                "aggregate renderer bytes differ without an affected diagnostic segment",
            )
        })
}

fn effective_multiset(
    diagnostics: &[RendererEffectiveKey],
) -> BTreeMap<&RendererEffectiveKey, usize> {
    let mut counts = BTreeMap::new();
    for diagnostic in diagnostics {
        *counts.entry(diagnostic).or_default() += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{OptionalBool, OptionalString};

    fn diagnostic(start: u32, text: &str) -> DiagnosticRecord {
        DiagnosticRecord {
            pass: DiagnosticPass::Semantic,
            file: DiagnosticFile::File {
                path: "main.ts".to_owned(),
            },
            code: 2322,
            line: OptionalU32::Present { value: 0 },
            column: OptionalU32::Present { value: 0 },
            category: DiagnosticCategory::Error,
            start: OptionalU32::Present { value: start },
            length: OptionalU32::Present { value: 1 },
            chain: MessageChain {
                text: text.to_owned(),
                code: 2322,
                category: DiagnosticCategory::Error,
                next_present: false,
                next: Vec::new(),
            },
            related_information_present: false,
            related: Vec::new(),
            reports_unnecessary: OptionalBool::absent(),
            reports_deprecated: OptionalBool::absent(),
            source: OptionalString::absent(),
        }
    }

    #[test]
    fn t0_uses_line_and_column_not_start() {
        let left = diagnostic(0, "head");
        let right = diagnostic(1, "head");
        assert_eq!(
            tier_key(&left, ComparisonTier::T0),
            tier_key(&right, ComparisonTier::T0)
        );
        assert_ne!(
            tier_key(&left, ComparisonTier::T2),
            tier_key(&right, ComparisonTier::T2)
        );
    }

    #[test]
    fn t3_excludes_formatter_sidecars() {
        let left = diagnostic(0, "head");
        let mut right = left.clone();
        right.chain.next_present = true;
        right.related_information_present = true;
        right.reports_unnecessary = OptionalBool::present(true);
        right.reports_deprecated = OptionalBool::present(false);
        right.source = OptionalString::present("ts");
        assert_eq!(
            tier_key(&left, ComparisonTier::T3),
            tier_key(&right, ComparisonTier::T3)
        );
    }
}

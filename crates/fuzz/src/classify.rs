//! Versioned canonical divergence classes.
//!
//! Classification is intentionally downstream of structured comparison. In
//! particular, rows are selected at the failing tier before they are mapped
//! to the position-free T2 message head. Equal mapped rows on opposite sides
//! are never cancelled.

use serde::{Deserialize, Serialize};

use crate::compare::{Comparison, ComparisonTier, DifferenceSide, Divergence, RendererDifference};
use crate::model::{
    terminal_boundary_is_valid, ClassDiagnosticKey, DiagnosticPass, TerminalBoundaryId,
    TerminalKind, TerminalPhase,
};
use crate::normalize::{validate_class_normalized_text, NormalizationSpec};
use crate::schema::{sha256_hex, CaseSpec};
use crate::{FoundationError, FoundationResult};

pub const CANONICAL_CLASS_SCHEMA: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ClassFailure {
    Tier { tier: ComparisonTier },
    Terminal { phase: TerminalPhase },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClassPass {
    Syntactic,
    Semantic,
    Suggestion,
    AggregateRender,
    Terminal,
}

impl From<DiagnosticPass> for ClassPass {
    fn from(pass: DiagnosticPass) -> Self {
        match pass {
            DiagnosticPass::Syntactic => Self::Syntactic,
            DiagnosticPass::Semantic => Self::Semantic,
            DiagnosticPass::Suggestion => Self::Suggestion,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutcomeSide {
    Oracle,
    Tsrs,
    Both,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClassOutcome {
    pub side: OutcomeSide,
    /// `diagnostic`, `renderer`, or a closed terminal kind/boundary pair.
    pub kind: String,
}

/// One signed multiset occurrence. `side` is the sign; duplicate identical
/// rows are retained rather than folded or cancelled.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClassRow {
    pub side: DifferenceSide,
    pub code: u32,
    pub normalized_message_head: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RendererClass {
    pub class: RendererDifference,
    pub affected_key: ClassDiagnosticKey,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalClass {
    pub schema: u32,
    pub failure: ClassFailure,
    pub pass: ClassPass,
    pub outcome: ClassOutcome,
    pub rows: Vec<ClassRow>,
    pub renderer: Option<RendererClass>,
}

impl CanonicalClass {
    pub fn validate(&self) -> FoundationResult<()> {
        if self.schema != CANONICAL_CLASS_SCHEMA {
            return Err(FoundationError::new(format!(
                "unsupported canonical class schema {}; expected {CANONICAL_CLASS_SCHEMA}",
                self.schema
            )));
        }
        if self.outcome.kind.is_empty() {
            return Err(FoundationError::new(
                "canonical class outcome kind must not be empty",
            ));
        }
        for (index, row) in self.rows.iter().enumerate() {
            if row.normalized_message_head.is_empty() {
                return Err(FoundationError::new(format!(
                    "canonical class rows[{index}].normalized_message_head must not be empty"
                )));
            }
            validate_class_normalized_text(
                &row.normalized_message_head,
                &format!("canonical class rows[{index}].normalized_message_head"),
            )?;
        }
        if self.rows.windows(2).any(|rows| rows[0] > rows[1]) {
            return Err(FoundationError::new(
                "canonical class rows must be sorted by side/code/UTF-8 message bytes",
            ));
        }
        match (&self.failure, self.pass, &self.renderer) {
            (
                ClassFailure::Tier {
                    tier: ComparisonTier::T4,
                },
                ClassPass::AggregateRender,
                Some(_),
            ) if self.rows.is_empty() => {}
            (ClassFailure::Tier { tier }, pass, None)
                if *tier != ComparisonTier::T4
                    && matches!(
                        pass,
                        ClassPass::Syntactic | ClassPass::Semantic | ClassPass::Suggestion
                    )
                    && !self.rows.is_empty()
                    && self.outcome.kind == "diagnostic"
                    && self.outcome.side == outcome_side(self.rows.iter().map(|row| row.side)) => {}
            (ClassFailure::Terminal { .. }, ClassPass::Terminal, None) if self.rows.is_empty() => {}
            _ => {
                return Err(FoundationError::new(
                    "canonical class failure/pass/rows/renderer shape is inconsistent",
                ));
            }
        }
        match (&self.failure, &self.renderer) {
            (
                ClassFailure::Tier {
                    tier: ComparisonTier::T4,
                },
                Some(renderer),
            ) => {
                if self.outcome.kind != "renderer" || self.outcome.side != OutcomeSide::Both {
                    return Err(FoundationError::new(
                        "renderer class outcome must be kind=renderer and side=both",
                    ));
                }
                renderer
                    .affected_key
                    .validate("canonical class renderer.affected_key")?;
                validate_class_normalized_text(
                    &renderer.affected_key.normalized_message_head,
                    "canonical class renderer.affected_key.normalized_message_head",
                )?;
            }
            (ClassFailure::Terminal { phase }, None) => {
                if self.outcome.side != OutcomeSide::Tsrs
                    || !valid_terminal_kind(*phase, &self.outcome.kind)
                {
                    return Err(FoundationError::new(
                        "terminal class must be tsrs-sided with a closed non-empty terminal key",
                    ));
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> FoundationResult<Vec<u8>> {
        self.validate()?;
        serde_json::to_vec(self).map_err(|error| {
            FoundationError::new(format!("cannot serialize canonical class: {error}"))
        })
    }

    pub fn canonical_sha256(&self) -> FoundationResult<String> {
        Ok(sha256_hex(&self.canonical_bytes()?))
    }

    pub fn from_json_slice(bytes: &[u8]) -> FoundationResult<Self> {
        let class: Self = serde_json::from_slice(bytes).map_err(|error| {
            FoundationError::new(format!("invalid canonical class JSON: {error}"))
        })?;
        class.validate()?;
        Ok(class)
    }

    pub fn from_canonical_slice(bytes: &[u8]) -> FoundationResult<Self> {
        let class = Self::from_json_slice(bytes)?;
        if class.canonical_bytes()? != bytes {
            return Err(FoundationError::new(
                "class input is valid JSON but not canonical compact schema-1 bytes",
            ));
        }
        Ok(class)
    }
}

/// Standalone classifier for a trusted [`Comparison`].
///
/// Authoritative producer/verifier code must use
/// [`crate::evaluate::evaluate_case`] so callers cannot forge or mix the
/// comparison and raw execution. Exact cases have no divergence class. A
/// reviewed oracle deviation is an accepted non-parity outcome, not a
/// divergence class; while its fixed registry remains draft, the comparator
/// rejects every oracle terminal.
pub fn classify_case(
    case: &CaseSpec,
    comparison: &Comparison,
) -> FoundationResult<Option<CanonicalClass>> {
    let validated = case.validated_context()?;
    let normalization = NormalizationSpec::for_validated_case(validated.case())?;
    classify_validated(comparison, &normalization)
}

pub(crate) fn classify_validated(
    comparison: &Comparison,
    normalization: &NormalizationSpec,
) -> FoundationResult<Option<CanonicalClass>> {
    if matches!(comparison, Comparison::Exact) {
        return Ok(None);
    }
    let class = match comparison {
        Comparison::Exact => unreachable!("exact comparison returned before classification"),
        Comparison::Divergence(Divergence::Diagnostic(divergence)) => {
            let mut rows = divergence
                .one_sided
                .iter()
                .map(|occurrence| {
                    Ok(ClassRow {
                        side: occurrence.side,
                        code: occurrence.diagnostic.code,
                        normalized_message_head: normalization
                            .normalize_after_validation(occurrence.diagnostic.top_text())?,
                    })
                })
                .collect::<FoundationResult<Vec<_>>>()?;
            rows.sort();
            let side = outcome_side(rows.iter().map(|row| row.side));
            CanonicalClass {
                schema: CANONICAL_CLASS_SCHEMA,
                failure: ClassFailure::Tier {
                    tier: divergence.tier,
                },
                pass: divergence.pass.into(),
                outcome: ClassOutcome {
                    side,
                    kind: "diagnostic".to_owned(),
                },
                rows,
                renderer: None,
            }
        }
        Comparison::Divergence(Divergence::Renderer(divergence)) => CanonicalClass {
            schema: CANONICAL_CLASS_SCHEMA,
            failure: ClassFailure::Tier {
                tier: ComparisonTier::T4,
            },
            pass: ClassPass::AggregateRender,
            outcome: ClassOutcome {
                side: OutcomeSide::Both,
                kind: "renderer".to_owned(),
            },
            rows: Vec::new(),
            renderer: Some(RendererClass {
                class: divergence.class,
                affected_key: ClassDiagnosticKey {
                    code: divergence.affected.code,
                    normalized_message_head: normalization
                        .normalize_after_validation(divergence.affected.top_text())?,
                },
            }),
        },
        Comparison::Divergence(Divergence::TsrsTerminal(outcome)) => CanonicalClass {
            schema: CANONICAL_CLASS_SCHEMA,
            failure: ClassFailure::Terminal {
                phase: outcome.phase,
            },
            pass: ClassPass::Terminal,
            outcome: ClassOutcome {
                side: OutcomeSide::Tsrs,
                kind: terminal_key(outcome.phase, outcome.kind, outcome.boundary_id)?,
            },
            rows: Vec::new(),
            renderer: None,
        },
    };
    class.validate()?;
    Ok(Some(class))
}

fn outcome_side(sides: impl Iterator<Item = DifferenceSide>) -> OutcomeSide {
    let mut oracle = false;
    let mut tsrs = false;
    for side in sides {
        match side {
            DifferenceSide::Oracle => oracle = true,
            DifferenceSide::Tsrs => tsrs = true,
        }
    }
    match (oracle, tsrs) {
        (true, true) => OutcomeSide::Both,
        (true, false) => OutcomeSide::Oracle,
        (false, true) => OutcomeSide::Tsrs,
        (false, false) => OutcomeSide::Both,
    }
}

const fn terminal_kind_name(kind: TerminalKind) -> &'static str {
    match kind {
        TerminalKind::Panic => "panic",
        TerminalKind::Crash => "crash",
        TerminalKind::Timeout => "timeout",
        TerminalKind::Oom => "oom",
        TerminalKind::Unsupported => "unsupported",
    }
}

fn valid_terminal_kind(phase: TerminalPhase, kind: &str) -> bool {
    matches!(
        (phase, kind),
        (_, "panic:phase-invariant")
            | (TerminalPhase::Parse, "panic:parser-invariant")
            | (
                TerminalPhase::Format,
                "panic:renderer-invariant" | "panic:renderer-state"
            )
            | (_, "crash:process-signal")
            | (_, "timeout:deadline")
            | (_, "oom:allocation-limit")
            | (_, "unsupported:feature-gate")
    )
}

fn terminal_key(
    phase: TerminalPhase,
    kind: TerminalKind,
    boundary_id: TerminalBoundaryId,
) -> FoundationResult<String> {
    if !terminal_boundary_is_valid(phase, kind, boundary_id) {
        return Err(FoundationError::new(
            "terminal boundary_id is not allowed for this phase/kind",
        ));
    }
    Ok(format!(
        "{}:{}",
        terminal_kind_name(kind),
        boundary_id.as_str()
    ))
}

#[cfg(test)]
#[path = "../tests/unit/classify/tests.rs"]
mod tests;

//! Typed diagnostic, renderer, engine, and producer outcomes.

use serde::{Deserialize, Serialize};

use crate::schema::{sha256_hex, validate_public_file_name, CaseSpec, ValidatedCaseContext};
use crate::{FoundationError, FoundationResult};

pub const ENGINE_OUTCOME_SCHEMA: u32 = 1;
pub const CASE_EXECUTION_SCHEMA: u32 = 1;
pub const MAX_MESSAGE_CHAIN_DEPTH: usize = 32;
pub const MAX_MESSAGE_CHAIN_NODES: usize = 4_096;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticPass {
    Syntactic,
    Semantic,
    Suggestion,
}

impl DiagnosticPass {
    pub const ORDERED: [Self; 3] = [Self::Syntactic, Self::Semantic, Self::Suggestion];
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticCategory {
    Warning,
    Error,
    Suggestion,
    Message,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum DiagnosticFile {
    Global,
    File { path: String },
}

impl DiagnosticFile {
    fn validate(&self, context: &str) -> FoundationResult<()> {
        if let Self::File { path } = self {
            validate_public_file_name(path, context)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum OptionalU32 {
    Absent,
    Present { value: u32 },
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OptionalBool {
    pub present: bool,
    pub value: Option<bool>,
}

impl OptionalBool {
    pub const fn absent() -> Self {
        Self {
            present: false,
            value: None,
        }
    }

    pub const fn present(value: bool) -> Self {
        Self {
            present: true,
            value: Some(value),
        }
    }

    fn validate(&self, context: &str) -> FoundationResult<()> {
        validate_presence(self.present, self.value.is_some(), context)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OptionalString {
    pub present: bool,
    pub value: Option<String>,
}

impl OptionalString {
    pub const fn absent() -> Self {
        Self {
            present: false,
            value: None,
        }
    }

    pub fn present(value: impl Into<String>) -> Self {
        Self {
            present: true,
            value: Some(value.into()),
        }
    }

    fn validate(&self, context: &str) -> FoundationResult<()> {
        validate_presence(self.present, self.value.is_some(), context)
    }
}

fn validate_presence(present: bool, has_value: bool, context: &str) -> FoundationResult<()> {
    if present != has_value {
        return Err(FoundationError::new(format!(
            "{context} presence/value mismatch"
        )));
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MessageChain {
    pub text: String,
    pub code: u32,
    pub category: DiagnosticCategory,
    pub next_present: bool,
    pub next: Vec<MessageChain>,
}

impl MessageChain {
    pub fn validate(&self, context: &str) -> FoundationResult<()> {
        let mut pending = vec![(self, 1_usize)];
        let mut nodes = 0_usize;
        while let Some((node, depth)) = pending.pop() {
            if depth > MAX_MESSAGE_CHAIN_DEPTH {
                return Err(FoundationError::new(format!(
                    "{context} exceeds maximum message-chain depth {MAX_MESSAGE_CHAIN_DEPTH}"
                )));
            }
            nodes = nodes
                .checked_add(1)
                .ok_or_else(|| FoundationError::new("message-chain node count overflows usize"))?;
            if nodes > MAX_MESSAGE_CHAIN_NODES {
                return Err(FoundationError::new(format!(
                    "{context} exceeds maximum message-chain node count {MAX_MESSAGE_CHAIN_NODES}"
                )));
            }
            if node.text.is_empty() {
                return Err(FoundationError::new(format!(
                    "{context} node {nodes}.text must not be empty"
                )));
            }
            if !node.next_present && !node.next.is_empty() {
                return Err(FoundationError::new(format!(
                    "{context} node {nodes}.next contains records while next_present is false"
                )));
            }
            let discovered_nodes = nodes
                .checked_add(pending.len())
                .and_then(|count| count.checked_add(node.next.len()))
                .ok_or_else(|| FoundationError::new("message-chain node count overflows usize"))?;
            if discovered_nodes > MAX_MESSAGE_CHAIN_NODES {
                return Err(FoundationError::new(format!(
                    "{context} exceeds maximum message-chain node count {MAX_MESSAGE_CHAIN_NODES}"
                )));
            }
            pending.extend(node.next.iter().rev().map(|child| (child, depth + 1)));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RelatedDiagnostic {
    pub file_present: bool,
    pub file: Option<String>,
    pub start_present: bool,
    pub start: Option<u32>,
    pub length_present: bool,
    pub length: Option<u32>,
    pub code: u32,
    pub category: DiagnosticCategory,
    pub chain: MessageChain,
}

impl RelatedDiagnostic {
    fn validate(&self, context: &str) -> FoundationResult<()> {
        validate_presence(
            self.file_present,
            self.file.is_some(),
            &format!("{context}.file"),
        )?;
        validate_presence(
            self.start_present,
            self.start.is_some(),
            &format!("{context}.start"),
        )?;
        validate_presence(
            self.length_present,
            self.length.is_some(),
            &format!("{context}.length"),
        )?;
        if self.file.is_none() && (self.start.is_some() || self.length.is_some()) {
            return Err(FoundationError::new(format!(
                "{context} related diagnostic without a file must not carry a span"
            )));
        }
        if self.start.is_some() != self.length.is_some() {
            return Err(FoundationError::new(format!(
                "{context} start and length presence must match"
            )));
        }
        if let Some(file) = &self.file {
            validate_public_file_name(file, &format!("{context}.file"))?;
        }
        self.chain.validate(&format!("{context}.chain"))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticRecord {
    pub pass: DiagnosticPass,
    pub file: DiagnosticFile,
    pub code: u32,
    pub line: OptionalU32,
    pub column: OptionalU32,
    pub category: DiagnosticCategory,
    pub start: OptionalU32,
    pub length: OptionalU32,
    pub chain: MessageChain,
    pub related_information_present: bool,
    pub related: Vec<RelatedDiagnostic>,
    pub reports_unnecessary: OptionalBool,
    pub reports_deprecated: OptionalBool,
    pub source: OptionalString,
}

impl DiagnosticRecord {
    pub fn validate(&self, context: &str) -> FoundationResult<()> {
        self.file.validate(&format!("{context}.file"))?;
        if matches!(self.file, DiagnosticFile::Global)
            && (!matches!(self.line, OptionalU32::Absent)
                || !matches!(self.column, OptionalU32::Absent)
                || !matches!(self.start, OptionalU32::Absent)
                || !matches!(self.length, OptionalU32::Absent))
        {
            return Err(FoundationError::new(format!(
                "{context} global diagnostic must have absent line/column/start/length"
            )));
        }
        if matches!(self.line, OptionalU32::Present { .. })
            != matches!(self.column, OptionalU32::Present { .. })
        {
            return Err(FoundationError::new(format!(
                "{context} line and column presence must match"
            )));
        }
        if matches!(self.start, OptionalU32::Present { .. })
            != matches!(self.length, OptionalU32::Present { .. })
        {
            return Err(FoundationError::new(format!(
                "{context} start and length presence must match"
            )));
        }
        if matches!(self.start, OptionalU32::Present { .. })
            != matches!(self.line, OptionalU32::Present { .. })
        {
            return Err(FoundationError::new(format!(
                "{context} start/length and line/column presence must match"
            )));
        }
        self.chain.validate(&format!("{context}.chain"))?;
        if !self.related_information_present && !self.related.is_empty() {
            return Err(FoundationError::new(format!(
                "{context}.related has records while related_information_present is false"
            )));
        }
        for (index, related) in self.related.iter().enumerate() {
            related.validate(&format!("{context}.related[{index}]"))?;
        }
        self.reports_unnecessary
            .validate(&format!("{context}.reports_unnecessary"))?;
        self.reports_deprecated
            .validate(&format!("{context}.reports_deprecated"))?;
        self.source.validate(&format!("{context}.source"))
    }

    pub fn top_text(&self) -> &str {
        &self.chain.text
    }

    fn validate_with_context(
        &self,
        case: &ValidatedCaseContext<'_>,
        context: &str,
    ) -> FoundationResult<()> {
        if let DiagnosticFile::File { path } = &self.file {
            let source = case.source(path)?;
            match (&self.start, &self.length, &self.line, &self.column) {
                (
                    OptionalU32::Absent,
                    OptionalU32::Absent,
                    OptionalU32::Absent,
                    OptionalU32::Absent,
                ) => {}
                (
                    OptionalU32::Present { value: start },
                    OptionalU32::Present { value: length },
                    OptionalU32::Present { value: line },
                    OptionalU32::Present { value: column },
                ) => {
                    let end = start.checked_add(*length).ok_or_else(|| {
                        FoundationError::new(format!("{context} start+length overflows u32"))
                    })?;
                    let (actual_line, actual_column) =
                        source.line_column_at_utf16(*start, "diagnostic start")?;
                    if *line != actual_line || *column != actual_column {
                        return Err(FoundationError::new(format!(
                            "{context} line/column ({line},{column}) does not match UTF-16 start {start} ({actual_line},{actual_column})"
                        )));
                    }
                    if end > source.total_utf16() {
                        return Err(FoundationError::new(format!(
                            "{context} span ends at {end}, beyond UTF-16 source length {}",
                            source.total_utf16()
                        )));
                    }
                    source.ensure_utf16_boundary(end, "diagnostic span end")?;
                }
                _ => {
                    return Err(FoundationError::new(format!(
                        "{context} has an unsupported partial source location"
                    )));
                }
            }
        }
        for (related_index, related) in self.related.iter().enumerate() {
            related.validate_with_context(case, &format!("{context}.related[{related_index}]"))?;
        }
        Ok(())
    }
}

impl RelatedDiagnostic {
    fn validate_with_context(
        &self,
        case: &ValidatedCaseContext<'_>,
        context: &str,
    ) -> FoundationResult<()> {
        let Some(file) = &self.file else {
            return Ok(());
        };
        let source = case.source(file)?;
        match (self.start, self.length) {
            (Some(start), Some(length)) => {
                source.ensure_utf16_boundary(start, &format!("{context}.start"))?;
                let end = start.checked_add(length).ok_or_else(|| {
                    FoundationError::new(format!("{context} start+length overflows u32"))
                })?;
                source.ensure_utf16_boundary(end, &format!("{context}.end"))
            }
            (None, None) => Ok(()),
            _ => Err(FoundationError::new(format!(
                "{context} start and length presence must match"
            ))),
        }
    }
}

/// Position-free identity used only after structured comparison has selected
/// an affected diagnostic.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClassDiagnosticKey {
    pub code: u32,
    pub normalized_message_head: String,
}

impl ClassDiagnosticKey {
    pub fn validate(&self, context: &str) -> FoundationResult<()> {
        if self.normalized_message_head.is_empty() {
            return Err(FoundationError::new(format!(
                "{context}.normalized_message_head must not be empty"
            )));
        }
        Ok(())
    }
}

/// The private tsc `canonicalHead` used by
/// `sortAndDeduplicateDiagnostics`. It changes the effective diagnostic code
/// and message for sort/dedupe without replacing the raw diagnostic that is
/// eventually rendered.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CanonicalHead {
    Absent,
    Present { code: u32, message_text: String },
}

impl CanonicalHead {
    pub const fn absent() -> Self {
        Self::Absent
    }

    pub fn present(code: u32, message_text: impl Into<String>) -> Self {
        Self::Present {
            code,
            message_text: message_text.into(),
        }
    }

    fn validate(&self, context: &str) -> FoundationResult<()> {
        if let Self::Present { code, message_text } = self {
            if *code == 0 || message_text.is_empty() {
                return Err(FoundationError::new(format!(
                    "{context} must have a non-zero code and non-empty message_text"
                )));
            }
        }
        Ok(())
    }

    pub(crate) const fn effective_code(&self, fallback: u32) -> u32 {
        match self {
            Self::Absent => fallback,
            Self::Present { code, .. } => *code,
        }
    }

    pub(crate) fn effective_message<'a>(&'a self, fallback: &'a str) -> &'a str {
        match self {
            Self::Absent => fallback,
            Self::Present { message_text, .. } => message_text,
        }
    }
}

/// One actual diagnostic in a captured renderer stage. The complete raw
/// record is retained for replay while `canonical_head` preserves tsc's
/// separate effective sort/dedupe identity.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssembledDiagnostic {
    pub diagnostic: DiagnosticRecord,
    pub canonical_head: CanonicalHead,
}

impl AssembledDiagnostic {
    fn validate(&self, context: &str) -> FoundationResult<()> {
        self.diagnostic.validate(&format!("{context}.diagnostic"))?;
        self.canonical_head
            .validate(&format!("{context}.canonical_head"))
    }

    fn validate_with_context(
        &self,
        case: &ValidatedCaseContext<'_>,
        context: &str,
    ) -> FoundationResult<()> {
        self.diagnostic
            .validate_with_context(case, &format!("{context}.diagnostic"))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RenderSegment {
    pub diagnostic: AssembledDiagnostic,
    /// The exact formatter bytes for this diagnostic, including delimiters
    /// and newlines. Stage differences are derived from this raw text.
    pub raw_text: String,
}

impl RenderSegment {
    fn validate(&self, context: &str) -> FoundationResult<()> {
        self.diagnostic.validate(&format!("{context}.diagnostic"))
    }

    fn validate_with_context(
        &self,
        case: &ValidatedCaseContext<'_>,
        context: &str,
    ) -> FoundationResult<()> {
        self.diagnostic
            .validate_with_context(case, &format!("{context}.diagnostic"))
    }
}

/// Independently captured renderer stages. Structured diagnostics are
/// compared separately; these are the actual assembled pre-render sequence
/// and deterministic per-diagnostic stage boundaries.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RendererObservation {
    pub assembled: Vec<AssembledDiagnostic>,
    pub deduped: Vec<AssembledDiagnostic>,
    pub segments: Vec<RenderSegment>,
    pub aggregate_text: String,
}

impl RendererObservation {
    pub fn validate(&self, context: &str) -> FoundationResult<()> {
        for (index, key) in self.assembled.iter().enumerate() {
            key.validate(&format!("{context}.assembled[{index}]"))?;
        }
        for (index, key) in self.deduped.iter().enumerate() {
            key.validate(&format!("{context}.deduped[{index}]"))?;
        }
        if self.deduped.len() != self.segments.len() {
            return Err(FoundationError::new(format!(
                "{context}.segments length must equal deduped length"
            )));
        }
        for (index, segment) in self.segments.iter().enumerate() {
            segment.validate(&format!("{context}.segments[{index}]"))?;
            if segment.diagnostic != self.deduped[index] {
                return Err(FoundationError::new(format!(
                    "{context}.segments[{index}].diagnostic must equal deduped[{index}]"
                )));
            }
        }
        let mut aggregate_offset = 0;
        for segment in &self.segments {
            if !self.aggregate_text[aggregate_offset..].starts_with(&segment.raw_text) {
                return Err(FoundationError::new(format!(
                    "{context}.aggregate_text must equal the exact concatenation of segment raw_text boundaries"
                )));
            }
            aggregate_offset += segment.raw_text.len();
        }
        if aggregate_offset != self.aggregate_text.len() {
            return Err(FoundationError::new(format!(
                "{context}.aggregate_text must equal the exact concatenation of segment raw_text boundaries"
            )));
        }
        Ok(())
    }

    fn validate_with_context(
        &self,
        case: &ValidatedCaseContext<'_>,
        context: &str,
    ) -> FoundationResult<()> {
        for (index, diagnostic) in self.assembled.iter().enumerate() {
            diagnostic.validate_with_context(case, &format!("{context}.assembled[{index}]"))?;
        }
        for (index, diagnostic) in self.deduped.iter().enumerate() {
            diagnostic.validate_with_context(case, &format!("{context}.deduped[{index}]"))?;
        }
        for (index, segment) in self.segments.iter().enumerate() {
            segment.validate_with_context(case, &format!("{context}.segments[{index}]"))?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompletedOutcome {
    pub diagnostics: Vec<DiagnosticRecord>,
    pub renderer: RendererObservation,
}

impl CompletedOutcome {
    pub fn validate(&self, context: &str) -> FoundationResult<()> {
        for (index, diagnostic) in self.diagnostics.iter().enumerate() {
            diagnostic.validate(&format!("{context}.diagnostics[{index}]"))?;
        }
        self.renderer.validate(&format!("{context}.renderer"))
    }

    pub fn validate_for_case(&self, case: &CaseSpec, context: &str) -> FoundationResult<()> {
        let validated = case.validated_context()?;
        self.validate_with_context(&validated, context)
    }

    fn validate_with_context(
        &self,
        case: &ValidatedCaseContext<'_>,
        context: &str,
    ) -> FoundationResult<()> {
        self.validate(context)?;
        for (index, diagnostic) in self.diagnostics.iter().enumerate() {
            diagnostic.validate_with_context(case, &format!("{context}.diagnostics[{index}]"))?;
        }
        self.renderer
            .validate_with_context(case, &format!("{context}.renderer"))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalPhase {
    Parse,
    Bind,
    Check,
    Format,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalKind {
    Panic,
    Crash,
    Timeout,
    Oom,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalBoundaryId {
    PhaseInvariant,
    ParserInvariant,
    RendererInvariant,
    RendererState,
    ProcessSignal,
    Deadline,
    AllocationLimit,
    FeatureGate,
}

impl TerminalBoundaryId {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::PhaseInvariant => "phase-invariant",
            Self::ParserInvariant => "parser-invariant",
            Self::RendererInvariant => "renderer-invariant",
            Self::RendererState => "renderer-state",
            Self::ProcessSignal => "process-signal",
            Self::Deadline => "deadline",
            Self::AllocationLimit => "allocation-limit",
            Self::FeatureGate => "feature-gate",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalOutcome {
    pub phase: TerminalPhase,
    pub kind: TerminalKind,
    /// Closed adapter-owned semantic boundary. Raw process text, paths,
    /// positions, seeds, timestamps, and hashes belong in `detail`.
    pub boundary_id: TerminalBoundaryId,
    /// Exact raw terminal text retained for replay and outcome hashing.
    pub detail: String,
}

impl TerminalOutcome {
    fn validate(&self, context: &str) -> FoundationResult<()> {
        if !terminal_boundary_is_valid(self.phase, self.kind, self.boundary_id) {
            return Err(FoundationError::new(format!(
                "{context}.boundary_id is not allowed for this terminal phase/kind"
            )));
        }
        if self.detail.is_empty() || self.detail.contains('\0') {
            return Err(FoundationError::new(format!(
                "{context}.detail must not be empty or contain NUL"
            )));
        }
        Ok(())
    }
}

pub(crate) const fn terminal_boundary_is_valid(
    phase: TerminalPhase,
    kind: TerminalKind,
    boundary_id: TerminalBoundaryId,
) -> bool {
    matches!(
        (phase, kind, boundary_id),
        (_, TerminalKind::Panic, TerminalBoundaryId::PhaseInvariant)
            | (
                TerminalPhase::Parse,
                TerminalKind::Panic,
                TerminalBoundaryId::ParserInvariant
            )
            | (
                TerminalPhase::Format,
                TerminalKind::Panic,
                TerminalBoundaryId::RendererInvariant | TerminalBoundaryId::RendererState
            )
            | (_, TerminalKind::Crash, TerminalBoundaryId::ProcessSignal)
            | (_, TerminalKind::Timeout, TerminalBoundaryId::Deadline)
            | (_, TerminalKind::Oom, TerminalBoundaryId::AllocationLimit)
            | (
                _,
                TerminalKind::Unsupported,
                TerminalBoundaryId::FeatureGate
            )
    )
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum EngineResult {
    Completed { outcome: CompletedOutcome },
    Terminal { outcome: TerminalOutcome },
}

impl EngineResult {
    pub fn validate(&self, context: &str) -> FoundationResult<()> {
        match self {
            Self::Completed { outcome } => outcome.validate(context),
            Self::Terminal { outcome } => outcome.validate(context),
        }
    }

    pub fn validate_for_case(&self, case: &CaseSpec, context: &str) -> FoundationResult<()> {
        let validated = case.validated_context()?;
        self.validate_with_context(&validated, context)
    }

    fn validate_with_context(
        &self,
        case: &ValidatedCaseContext<'_>,
        context: &str,
    ) -> FoundationResult<()> {
        match self {
            Self::Completed { outcome } => outcome.validate_with_context(case, context),
            Self::Terminal { outcome } => outcome.validate(context),
        }
    }

    pub fn canonical_bytes(&self, case: &CaseSpec) -> FoundationResult<Vec<u8>> {
        let validated = case.validated_context()?;
        self.validate_with_context(&validated, "engine_outcome")?;
        let envelope = EngineOutcomeEnvelopeRef {
            schema: ENGINE_OUTCOME_SCHEMA,
            case_sha256: sha256_hex(&case.canonical_bytes_after_validation()?),
            result: self,
        };
        serde_json::to_vec(&envelope).map_err(|error| {
            FoundationError::new(format!("cannot serialize engine outcome: {error}"))
        })
    }

    pub fn canonical_sha256(&self, case: &CaseSpec) -> FoundationResult<String> {
        Ok(sha256_hex(&self.canonical_bytes(case)?))
    }

    pub fn from_canonical_slice(case: &CaseSpec, bytes: &[u8]) -> FoundationResult<Self> {
        let validated = case.validated_context()?;
        let envelope: EngineOutcomeEnvelope = serde_json::from_slice(bytes).map_err(|error| {
            FoundationError::new(format!("invalid engine outcome JSON: {error}"))
        })?;
        if envelope.schema != ENGINE_OUTCOME_SCHEMA {
            return Err(FoundationError::new(format!(
                "unsupported engine outcome schema {}; expected {ENGINE_OUTCOME_SCHEMA}",
                envelope.schema
            )));
        }
        let expected_case = sha256_hex(&case.canonical_bytes_after_validation()?);
        if envelope.case_sha256 != expected_case {
            return Err(FoundationError::new(format!(
                "engine outcome case hash mismatch: expected {expected_case}, found {}",
                envelope.case_sha256
            )));
        }
        envelope
            .result
            .validate_with_context(&validated, "engine_outcome")?;
        if serde_json::to_vec(&envelope).map_err(|error| {
            FoundationError::new(format!("cannot reserialize engine outcome: {error}"))
        })? != bytes
        {
            return Err(FoundationError::new(
                "engine outcome input is valid JSON but not canonical compact schema-1 bytes",
            ));
        }
        Ok(envelope.result)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct EngineOutcomeEnvelope {
    schema: u32,
    case_sha256: String,
    result: EngineResult,
}

#[derive(Serialize)]
struct EngineOutcomeEnvelopeRef<'outcome> {
    schema: u32,
    case_sha256: String,
    result: &'outcome EngineResult,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProducerFailureKind {
    Generator,
    Domain,
    Harness,
    MalformedResponse,
    Controller,
    WorkerInterruption,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProducerFailureSource {
    Generator,
    DomainValidator,
    OracleAdapter,
    TsrsAdapter,
    Harness,
    Controller,
    Worker,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProducerFailure {
    pub source: ProducerFailureSource,
    pub kind: ProducerFailureKind,
    pub detail: String,
}

impl ProducerFailure {
    pub fn validate(&self) -> FoundationResult<()> {
        if self.detail.is_empty() || self.detail.contains('\0') {
            return Err(FoundationError::new(
                "producer failure detail must not be empty or contain NUL",
            ));
        }
        let coherent = matches!(
            (self.source, self.kind),
            (
                ProducerFailureSource::Generator,
                ProducerFailureKind::Generator
            ) | (
                ProducerFailureSource::DomainValidator,
                ProducerFailureKind::Domain
            ) | (ProducerFailureSource::Harness, ProducerFailureKind::Harness)
                | (
                    ProducerFailureSource::OracleAdapter | ProducerFailureSource::TsrsAdapter,
                    ProducerFailureKind::MalformedResponse
                )
                | (
                    ProducerFailureSource::Controller,
                    ProducerFailureKind::Controller
                )
                | (
                    ProducerFailureSource::Worker,
                    ProducerFailureKind::WorkerInterruption
                )
        );
        if !coherent {
            return Err(FoundationError::new(
                "producer failure source/kind combination is incoherent",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
#[allow(clippy::large_enum_variant)] // Keep the frozen flat serde shape; hot paths borrow this value.
pub enum CaseExecution {
    Compared {
        oracle: EngineResult,
        tsrs: EngineResult,
    },
    ProducerFailure {
        failure: ProducerFailure,
    },
}

impl CaseExecution {
    pub fn validate(&self) -> FoundationResult<()> {
        match self {
            Self::Compared { oracle, tsrs } => {
                oracle.validate("oracle")?;
                tsrs.validate("tsrs")
            }
            Self::ProducerFailure { failure } => failure.validate(),
        }
    }

    pub fn validate_for_case(&self, case: &CaseSpec) -> FoundationResult<()> {
        let validated = case.validated_context()?;
        self.validate_with_context(&validated)
    }

    pub(crate) fn validate_with_context(
        &self,
        case: &ValidatedCaseContext<'_>,
    ) -> FoundationResult<()> {
        match self {
            Self::Compared { oracle, tsrs } => {
                oracle.validate_with_context(case, "oracle")?;
                tsrs.validate_with_context(case, "tsrs")
            }
            Self::ProducerFailure { failure } => failure.validate(),
        }
    }

    pub fn canonical_bytes(&self, case: &CaseSpec) -> FoundationResult<Vec<u8>> {
        let validated = case.validated_context()?;
        self.canonical_bytes_with_context(&validated)
    }

    pub(crate) fn canonical_bytes_with_context(
        &self,
        case: &ValidatedCaseContext<'_>,
    ) -> FoundationResult<Vec<u8>> {
        self.validate_with_context(case)?;
        self.canonical_bytes_after_validation(case)
    }

    pub(crate) fn canonical_bytes_after_validation(
        &self,
        case: &ValidatedCaseContext<'_>,
    ) -> FoundationResult<Vec<u8>> {
        let envelope = CaseExecutionEnvelopeRef {
            schema: CASE_EXECUTION_SCHEMA,
            case_sha256: sha256_hex(&case.case().canonical_bytes_after_validation()?),
            execution: self,
        };
        serde_json::to_vec(&envelope).map_err(|error| {
            FoundationError::new(format!("cannot serialize case execution: {error}"))
        })
    }

    pub fn canonical_sha256(&self, case: &CaseSpec) -> FoundationResult<String> {
        Ok(sha256_hex(&self.canonical_bytes(case)?))
    }

    pub fn from_canonical_slice(case: &CaseSpec, bytes: &[u8]) -> FoundationResult<Self> {
        let validated = case.validated_context()?;
        let envelope: CaseExecutionEnvelope = serde_json::from_slice(bytes).map_err(|error| {
            FoundationError::new(format!("invalid case execution JSON: {error}"))
        })?;
        if envelope.schema != CASE_EXECUTION_SCHEMA {
            return Err(FoundationError::new(format!(
                "unsupported case execution schema {}; expected {CASE_EXECUTION_SCHEMA}",
                envelope.schema
            )));
        }
        let expected_case = sha256_hex(&case.canonical_bytes_after_validation()?);
        if envelope.case_sha256 != expected_case {
            return Err(FoundationError::new(format!(
                "case execution hash mismatch: expected {expected_case}, found {}",
                envelope.case_sha256
            )));
        }
        envelope.execution.validate_with_context(&validated)?;
        if serde_json::to_vec(&envelope).map_err(|error| {
            FoundationError::new(format!("cannot reserialize case execution: {error}"))
        })? != bytes
        {
            return Err(FoundationError::new(
                "case execution input is valid JSON but not canonical compact schema-1 bytes",
            ));
        }
        Ok(envelope.execution)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CaseExecutionEnvelope {
    schema: u32,
    case_sha256: String,
    execution: CaseExecution,
}

#[derive(Serialize)]
struct CaseExecutionEnvelopeRef<'execution> {
    schema: u32,
    case_sha256: String,
    execution: &'execution CaseExecution,
}

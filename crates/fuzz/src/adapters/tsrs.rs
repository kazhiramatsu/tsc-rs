//! In-process Rust checker adapter for the M9 production observation.
//!
//! The adapter consumes one validated/decoded program, preserves the
//! public getter occurrence seam, and returns the exact retained
//! assembled indices needed by the worker wire format.

use std::collections::{BTreeMap, BTreeSet};

use tsc_checker::{check_program_with_libs_at_observed, CheckPhase, CompilerOptions, InputFile};
use tsc_diagnostics::{
    compute_line_starts, format_sorted_diagnostics_with_context_raw,
    get_line_and_character_of_position, sort_and_dedupe_diagnostic_indices_with_context,
    Diagnostic, DiagnosticCategory as TsrsDiagnosticCategory, FormatDiagnosticsHost,
    MessageChain as TsrsMessageChain, RelatedInfo,
};
use tsc_harness::{try_compiler_options_from_options, OptionValue as HarnessOptionValue};

use crate::model::{
    AssembledDiagnostic, CanonicalHead, CompletedOutcome, DiagnosticCategory, DiagnosticFile,
    DiagnosticPass, DiagnosticRecord, EngineResult, MessageChain, OptionalBool, OptionalString,
    OptionalU32, RelatedDiagnostic, RenderSegment, RendererObservation, MAX_MESSAGE_CHAIN_DEPTH,
    MAX_MESSAGE_CHAIN_NODES,
};
use crate::schema::{
    validate_id, validate_public_file_name, validate_virtual_path, CompilerOptionValue,
    EncodedFile, OrderedSetting, ValidatedCaseContext,
};
use crate::worker_protocol::{WireProgram, WorkerPhase};
use crate::{FoundationError, FoundationResult};

#[derive(Clone, Debug)]
pub(crate) struct TsrsAdapterOutput {
    pub(crate) result: EngineResult,
    pub(crate) deduped_indices: Vec<u32>,
}

impl TsrsAdapterOutput {
    pub(crate) fn into_engine_result(self) -> EngineResult {
        self.result
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedProgram {
    cwd: String,
    options: CompilerOptions,
    libs: Vec<InputFile>,
    files: Vec<InputFile>,
    file_texts: BTreeMap<String, String>,
    sources: BTreeMap<String, SourceIndex>,
}

#[derive(Clone, Debug)]
struct SourceIndex {
    total_utf16: u32,
    line_starts: Vec<u32>,
    boundaries: Vec<u32>,
}

impl SourceIndex {
    fn new(text: &str) -> FoundationResult<Self> {
        let mut total_utf16 = 0_u32;
        let mut boundaries = vec![0];
        for character in text.chars() {
            total_utf16 = total_utf16
                .checked_add(
                    u32::try_from(character.len_utf16())
                        .expect("one scalar has at most two UTF-16 units"),
                )
                .ok_or_else(|| FoundationError::new("source UTF-16 length overflows u32"))?;
            boundaries.push(total_utf16);
        }
        Ok(Self {
            total_utf16,
            line_starts: compute_line_starts(text),
            boundaries,
        })
    }

    fn ensure_boundary(&self, position: u32, context: &str) -> FoundationResult<()> {
        if position > self.total_utf16 || self.boundaries.binary_search(&position).is_err() {
            return Err(FoundationError::new(format!(
                "{context} {position} is outside the source or splits a UTF-16 surrogate pair"
            )));
        }
        Ok(())
    }

    fn line_column(&self, position: u32, context: &str) -> FoundationResult<(u32, u32)> {
        self.ensure_boundary(position, context)?;
        let location = get_line_and_character_of_position(&self.line_starts, position);
        Ok((location.line, location.character))
    }
}

/// Execute a canonical CaseSpec through exactly one validation/decode
/// context and return the production Rust engine result.
pub fn execute_case(
    case: &crate::schema::CaseSpec,
    observe_phase: impl FnMut(WorkerPhase),
) -> FoundationResult<EngineResult> {
    let context = case.validated_context()?;
    execute_validated_case(&context, observe_phase).map(TsrsAdapterOutput::into_engine_result)
}

pub(crate) fn execute_validated_case(
    context: &ValidatedCaseContext<'_>,
    observe_phase: impl FnMut(WorkerPhase),
) -> FoundationResult<TsrsAdapterOutput> {
    let prepared = prepare_validated_case(context)?;
    execute_prepared_program(&prepared, observe_phase)
}

#[cfg(test)]
pub(crate) fn execute_wire_program(
    program: &WireProgram,
    observe_phase: impl FnMut(WorkerPhase),
) -> FoundationResult<TsrsAdapterOutput> {
    let prepared = prepare_wire_program(program)?;
    execute_prepared_program(&prepared, observe_phase)
}

pub(crate) fn prepare_wire_program(program: &WireProgram) -> FoundationResult<PreparedProgram> {
    validate_virtual_path(&program.cwd, "worker program cwd")?;
    let options = project_options(&program.options)?;
    if program.files.is_empty() {
        return Err(FoundationError::new(
            "worker program files must not be empty",
        ));
    }

    let mut all_public_names = BTreeSet::new();
    let mut all_resolved_names = BTreeSet::new();
    let mut file_texts = BTreeMap::new();
    let mut sources = BTreeMap::new();
    let libs = decode_wire_files(
        &program.libs,
        "worker program libs",
        &program.cwd,
        &mut all_public_names,
        &mut all_resolved_names,
        &mut file_texts,
        &mut sources,
    )?;
    let files = decode_wire_files(
        &program.files,
        "worker program files",
        &program.cwd,
        &mut all_public_names,
        &mut all_resolved_names,
        &mut file_texts,
        &mut sources,
    )?;

    Ok(PreparedProgram {
        cwd: program.cwd.clone(),
        options,
        libs,
        files,
        file_texts,
        sources,
    })
}

fn prepare_validated_case(context: &ValidatedCaseContext<'_>) -> FoundationResult<PreparedProgram> {
    let case = context.case();
    let options = project_options(&case.options)?;
    let mut file_texts = BTreeMap::new();
    let mut sources = BTreeMap::new();
    let mut collect = |encoded: &EncodedFile| -> FoundationResult<InputFile> {
        let text = context.source(&encoded.name)?.text().to_owned();
        sources.insert(encoded.name.clone(), SourceIndex::new(&text)?);
        file_texts.insert(encoded.name.clone(), text.clone());
        Ok(InputFile::new(encoded.name.clone(), text))
    };
    let libs = case
        .libs
        .iter()
        .map(&mut collect)
        .collect::<FoundationResult<Vec<_>>>()?;
    let files = case
        .files
        .iter()
        .map(&mut collect)
        .collect::<FoundationResult<Vec<_>>>()?;
    Ok(PreparedProgram {
        cwd: case.cwd.clone(),
        options,
        libs,
        files,
        file_texts,
        sources,
    })
}

fn project_options(settings: &[OrderedSetting]) -> FoundationResult<CompilerOptions> {
    let mut previous_name: Option<&str> = None;
    let mut folded_names = BTreeSet::new();
    let mut options = BTreeMap::new();
    for (index, setting) in settings.iter().enumerate() {
        if usize::try_from(setting.ordinal).ok() != Some(index) {
            return Err(FoundationError::new(format!(
                "compiler options[{index}].ordinal must be {index}, found {}",
                setting.ordinal
            )));
        }
        validate_id(&setting.name, &format!("compiler options[{index}].name"))?;
        if previous_name.is_some_and(|previous| previous.as_bytes() >= setting.name.as_bytes()) {
            return Err(FoundationError::new(
                "compiler options must be strictly sorted by UTF-8 name bytes",
            ));
        }
        previous_name = Some(&setting.name);
        if !folded_names.insert(setting.name.to_ascii_lowercase()) {
            return Err(FoundationError::new(format!(
                "compiler option name {:?} is ASCII-case-insensitively duplicated",
                setting.name
            )));
        }
        validate_option_text(&setting.value, &setting.name)?;
        if setting.name.eq_ignore_ascii_case("noLib") {
            match &setting.value {
                CompilerOptionValue::Boolean { value: true } | CompilerOptionValue::Null => {}
                _ => {
                    return Err(FoundationError::new(
                        "compiler option noLib must be true or null for the inline M9 host",
                    ));
                }
            }
            continue;
        }
        options.insert(setting.name.clone(), harness_option_value(&setting.value));
    }
    try_compiler_options_from_options(&options).map_err(|error| {
        FoundationError::new(format!("closed compiler option projection failed: {error}"))
    })
}

fn validate_option_text(value: &CompilerOptionValue, name: &str) -> FoundationResult<()> {
    match value {
        CompilerOptionValue::Text { value } => {
            if value.chars().any(char::is_control) {
                return Err(FoundationError::new(format!(
                    "compiler option {name:?} contains a control character"
                )));
            }
        }
        CompilerOptionValue::StringList { values } => {
            if values
                .iter()
                .any(|value| value.chars().any(char::is_control))
            {
                return Err(FoundationError::new(format!(
                    "compiler option {name:?} contains a control character"
                )));
            }
        }
        CompilerOptionValue::Boolean { .. }
        | CompilerOptionValue::Number { .. }
        | CompilerOptionValue::Null => {}
    }
    Ok(())
}

fn harness_option_value(value: &CompilerOptionValue) -> HarnessOptionValue {
    match value {
        CompilerOptionValue::Boolean { value } => HarnessOptionValue::Bool(*value),
        CompilerOptionValue::Number { value } => HarnessOptionValue::Number(*value),
        CompilerOptionValue::Text { value } => HarnessOptionValue::String(value.clone()),
        CompilerOptionValue::StringList { values } => {
            HarnessOptionValue::StringList(values.clone())
        }
        CompilerOptionValue::Null => HarnessOptionValue::Null,
    }
}

#[allow(clippy::too_many_arguments)]
fn decode_wire_files(
    files: &[EncodedFile],
    context: &str,
    cwd: &str,
    all_public_names: &mut BTreeSet<String>,
    all_resolved_names: &mut BTreeSet<String>,
    file_texts: &mut BTreeMap<String, String>,
    sources: &mut BTreeMap<String, SourceIndex>,
) -> FoundationResult<Vec<InputFile>> {
    let mut decoded = Vec::with_capacity(files.len());
    for (index, file) in files.iter().enumerate() {
        if usize::try_from(file.ordinal).ok() != Some(index) {
            return Err(FoundationError::new(format!(
                "{context}[{index}].ordinal must be {index}, found {}",
                file.ordinal
            )));
        }
        validate_public_file_name(&file.name, &format!("{context}[{index}].name"))?;
        if !all_public_names.insert(file.name.clone()) {
            return Err(FoundationError::new(format!(
                "worker program duplicates public file name {:?}",
                file.name
            )));
        }
        let resolved = resolve_public_name(cwd, &file.name);
        if resolved == cwd {
            return Err(FoundationError::new(format!(
                "worker file {:?} resolves to cwd {cwd:?}",
                file.name
            )));
        }
        if !all_resolved_names.insert(resolved.clone()) {
            return Err(FoundationError::new(format!(
                "worker file {:?} resolves to duplicate path {resolved:?}",
                file.name
            )));
        }
        let text = file.decoded_text()?;
        let source = SourceIndex::new(&text)?;
        file_texts.insert(file.name.clone(), text.clone());
        sources.insert(file.name.clone(), source);
        decoded.push(InputFile::new(file.name.clone(), text));
    }
    Ok(decoded)
}

fn resolve_public_name(cwd: &str, name: &str) -> String {
    if name.starts_with('/') {
        name.to_owned()
    } else if cwd == "/" {
        format!("/{name}")
    } else {
        format!("{cwd}/{name}")
    }
}

pub(crate) fn execute_prepared_program(
    prepared: &PreparedProgram,
    mut observe_phase: impl FnMut(WorkerPhase),
) -> FoundationResult<TsrsAdapterOutput> {
    let checked = check_program_with_libs_at_observed(
        &prepared.libs,
        &prepared.files,
        &prepared.options,
        &prepared.cwd,
        |phase| {
            observe_phase(match phase {
                CheckPhase::Parse => WorkerPhase::Parse,
                CheckPhase::Bind => WorkerPhase::Bind,
                CheckPhase::Check => WorkerPhase::Check,
            });
        },
    );
    observe_phase(WorkerPhase::Format);

    let mut raw_assembled = Vec::new();
    let mut assembled = Vec::new();
    for file in &checked.file_diagnostics {
        append_pass(
            DiagnosticPass::Syntactic,
            &file.syntactic,
            &prepared.sources,
            &mut raw_assembled,
            &mut assembled,
        )?;
        append_pass(
            DiagnosticPass::Semantic,
            &file.semantic,
            &prepared.sources,
            &mut raw_assembled,
            &mut assembled,
        )?;
        append_pass(
            DiagnosticPass::Suggestion,
            &file.suggestion,
            &prepared.sources,
            &mut raw_assembled,
            &mut assembled,
        )?;
    }

    let host = FormatDiagnosticsHost::new(&prepared.cwd, &prepared.file_texts);
    let retained = sort_and_dedupe_diagnostic_indices_with_context(&raw_assembled, &host);
    let mut deduped_indices = Vec::with_capacity(retained.len());
    let mut deduped = Vec::with_capacity(retained.len());
    let mut segments = Vec::with_capacity(retained.len());
    let mut aggregate_text = String::new();
    for index in retained {
        let wire_index = u32::try_from(index)
            .map_err(|_| FoundationError::new("assembled diagnostic index overflows u32"))?;
        let assembled_diagnostic = assembled
            .get(index)
            .expect("retained index comes from assembled diagnostics")
            .clone();
        let raw_text = format_sorted_diagnostics_with_context_raw(
            std::slice::from_ref(
                raw_assembled
                    .get(index)
                    .expect("retained index comes from raw diagnostics"),
            ),
            &host,
        )
        .map_err(|error| {
            FoundationError::new(format!(
                "Rust diagnostic formatter rejected an observation: {error}"
            ))
        })?;
        deduped_indices.push(wire_index);
        deduped.push(assembled_diagnostic.clone());
        aggregate_text.push_str(&raw_text);
        segments.push(RenderSegment {
            diagnostic: assembled_diagnostic,
            raw_text,
        });
    }

    let diagnostics = assembled
        .iter()
        .map(|entry| entry.diagnostic.clone())
        .collect();
    let outcome = CompletedOutcome {
        diagnostics,
        renderer: RendererObservation {
            assembled,
            deduped,
            segments,
            aggregate_text,
        },
    };
    outcome.validate("tsrs adapter")?;
    Ok(TsrsAdapterOutput {
        result: EngineResult::Completed { outcome },
        deduped_indices,
    })
}

fn append_pass(
    pass: DiagnosticPass,
    diagnostics: &[Diagnostic],
    sources: &BTreeMap<String, SourceIndex>,
    raw_assembled: &mut Vec<Diagnostic>,
    assembled: &mut Vec<AssembledDiagnostic>,
) -> FoundationResult<()> {
    for diagnostic in diagnostics {
        let record = diagnostic_record(pass, diagnostic, sources)?;
        let canonical_head = diagnostic
            .canonical_head
            .as_ref()
            .map_or_else(CanonicalHead::absent, |head| {
                CanonicalHead::present(head.code, head.text.clone())
            });
        raw_assembled.push(diagnostic.clone());
        assembled.push(AssembledDiagnostic {
            diagnostic: record,
            canonical_head,
        });
    }
    Ok(())
}

fn diagnostic_record(
    pass: DiagnosticPass,
    diagnostic: &Diagnostic,
    sources: &BTreeMap<String, SourceIndex>,
) -> FoundationResult<DiagnosticRecord> {
    let (file, start, length, line, column) = diagnostic_location(diagnostic, sources)?;
    let chain = message_chain(&diagnostic.message, "diagnostic chain")?;
    let related = diagnostic
        .related
        .iter()
        .enumerate()
        .map(|(index, related)| {
            related_diagnostic(related, sources, &format!("related diagnostic {index}"))
        })
        .collect::<FoundationResult<Vec<_>>>()?;
    let record = DiagnosticRecord {
        pass,
        file,
        code: diagnostic.code(),
        line,
        column,
        category: diagnostic_category(diagnostic.category()),
        start,
        length,
        chain,
        related_information_present: diagnostic.related_information_present
            || !diagnostic.related.is_empty(),
        related,
        reports_unnecessary: optional_bool(diagnostic.reports_unnecessary),
        reports_deprecated: optional_bool(diagnostic.reports_deprecated),
        source: diagnostic
            .source
            .as_ref()
            .map_or_else(OptionalString::absent, |source| {
                OptionalString::present(source.clone())
            }),
    };
    record.validate("tsrs diagnostic")?;
    Ok(record)
}

fn diagnostic_location(
    diagnostic: &Diagnostic,
    sources: &BTreeMap<String, SourceIndex>,
) -> FoundationResult<(
    DiagnosticFile,
    OptionalU32,
    OptionalU32,
    OptionalU32,
    OptionalU32,
)> {
    let Some(file_name) = diagnostic.file_name.as_ref() else {
        if diagnostic.start.is_some() || diagnostic.length.is_some() {
            return Err(FoundationError::new(
                "global Rust diagnostic unexpectedly carries a span",
            ));
        }
        return Ok((
            DiagnosticFile::Global,
            OptionalU32::Absent,
            OptionalU32::Absent,
            OptionalU32::Absent,
            OptionalU32::Absent,
        ));
    };
    let source = sources.get(file_name).ok_or_else(|| {
        FoundationError::new(format!(
            "Rust diagnostic path {file_name:?} is absent from the input program"
        ))
    })?;
    match (diagnostic.start, diagnostic.length) {
        (None, None) => Ok((
            DiagnosticFile::File {
                path: file_name.clone(),
            },
            OptionalU32::Absent,
            OptionalU32::Absent,
            OptionalU32::Absent,
            OptionalU32::Absent,
        )),
        (Some(start), Some(length)) => {
            let end = start.checked_add(length).ok_or_else(|| {
                FoundationError::new("Rust diagnostic start+length overflows u32")
            })?;
            source.ensure_boundary(end, "Rust diagnostic end")?;
            let (line, column) = source.line_column(start, "Rust diagnostic start")?;
            Ok((
                DiagnosticFile::File {
                    path: file_name.clone(),
                },
                OptionalU32::Present { value: start },
                OptionalU32::Present { value: length },
                OptionalU32::Present { value: line },
                OptionalU32::Present { value: column },
            ))
        }
        _ => Err(FoundationError::new(
            "Rust diagnostic has a partial source span",
        )),
    }
}

fn related_diagnostic(
    related: &RelatedInfo,
    sources: &BTreeMap<String, SourceIndex>,
    context: &str,
) -> FoundationResult<RelatedDiagnostic> {
    if related.file_name.is_none() && (related.start.is_some() || related.length.is_some()) {
        return Err(FoundationError::new(format!(
            "{context} is global but carries a span"
        )));
    }
    if related.start.is_some() != related.length.is_some() {
        return Err(FoundationError::new(format!(
            "{context} has a partial source span"
        )));
    }
    if let Some(file_name) = related.file_name.as_ref() {
        let source = sources.get(file_name).ok_or_else(|| {
            FoundationError::new(format!(
                "{context} path {file_name:?} is absent from the input program"
            ))
        })?;
        if let (Some(start), Some(length)) = (related.start, related.length) {
            source.ensure_boundary(start, &format!("{context} start"))?;
            let end = start.checked_add(length).ok_or_else(|| {
                FoundationError::new(format!("{context} start+length overflows u32"))
            })?;
            source.ensure_boundary(end, &format!("{context} end"))?;
        }
    }
    Ok(RelatedDiagnostic {
        file_present: related.file_name.is_some(),
        file: related.file_name.clone(),
        start_present: related.start.is_some(),
        start: related.start,
        length_present: related.length.is_some(),
        length: related.length,
        code: related.message.code,
        category: diagnostic_category(related.message.category),
        chain: message_chain(&related.message, &format!("{context} chain"))?,
    })
}

fn message_chain(root: &TsrsMessageChain, context: &str) -> FoundationResult<MessageChain> {
    preflight_message_chain(root, context)?;
    fn convert(node: &TsrsMessageChain) -> MessageChain {
        MessageChain {
            text: node.text.clone(),
            code: node.code,
            category: diagnostic_category(node.category),
            next_present: node.next_present,
            next: node.next.iter().map(convert).collect(),
        }
    }
    Ok(convert(root))
}

fn preflight_message_chain(root: &TsrsMessageChain, context: &str) -> FoundationResult<()> {
    let mut pending = vec![(root, 1_usize)];
    let mut nodes = 0_usize;
    while let Some((node, depth)) = pending.pop() {
        if depth > MAX_MESSAGE_CHAIN_DEPTH {
            return Err(FoundationError::new(format!(
                "{context} exceeds message-chain depth {MAX_MESSAGE_CHAIN_DEPTH}"
            )));
        }
        nodes = nodes
            .checked_add(1)
            .ok_or_else(|| FoundationError::new("message-chain node count overflows usize"))?;
        if nodes > MAX_MESSAGE_CHAIN_NODES {
            return Err(FoundationError::new(format!(
                "{context} exceeds message-chain nodes {MAX_MESSAGE_CHAIN_NODES}"
            )));
        }
        if !node.next_present && !node.next.is_empty() {
            return Err(FoundationError::new(format!(
                "{context} has children while next is absent"
            )));
        }
        let discovered = nodes
            .checked_add(pending.len())
            .and_then(|count| count.checked_add(node.next.len()))
            .ok_or_else(|| FoundationError::new("message-chain node count overflows usize"))?;
        if discovered > MAX_MESSAGE_CHAIN_NODES {
            return Err(FoundationError::new(format!(
                "{context} exceeds message-chain nodes {MAX_MESSAGE_CHAIN_NODES}"
            )));
        }
        pending.extend(node.next.iter().rev().map(|child| (child, depth + 1)));
    }
    Ok(())
}

fn diagnostic_category(category: TsrsDiagnosticCategory) -> DiagnosticCategory {
    match category {
        TsrsDiagnosticCategory::Warning => DiagnosticCategory::Warning,
        TsrsDiagnosticCategory::Error => DiagnosticCategory::Error,
        TsrsDiagnosticCategory::Suggestion => DiagnosticCategory::Suggestion,
        TsrsDiagnosticCategory::Message => DiagnosticCategory::Message,
    }
}

fn optional_bool(value: Option<bool>) -> OptionalBool {
    value.map_or_else(OptionalBool::absent, OptionalBool::present)
}

#[cfg(test)]
#[path = "../../tests/unit/adapters/tsrs/tests.rs"]
mod tests;

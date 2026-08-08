//! Hosted H1 acceptance projection sourced from the pinned `ts-tests` tree.
//!
//! This module deliberately executes only rows admitted by the frozen H1
//! suite classifications. Focused controls, evidence production, and phase
//! tests remain in the complete local gate.

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::Path;

use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tsc_compiler::{
    EmitArtifactKind, EmitOutcome, EmitWriteMetadata, MemoryOutputSink, ProgramSession,
};
use tsc_diagnostics::{
    compute_line_starts, get_line_and_character_of_position, Diagnostic, MessageChain,
};
use tsc_harness::upstream_suites::execution::{
    load_compiler_emit, load_compiler_no_emit, load_recorded_execution_plans,
    CompilerExecutionPlan, UpstreamExecutionInput, UpstreamExecutionPlan,
};
use tsc_program::ProgramLoadLimits;

const QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h1-emit-qualification.v1.json";
const EXPECTED_CASE_ID: &str =
    "typescript-6.0.3/compiler/esmNoSynthesizedDefault.ts#module%3Dpreserve";

fn failure(message: impl Into<String>) -> Box<dyn Error> {
    std::io::Error::other(message.into()).into()
}

fn limits() -> ProgramLoadLimits {
    ProgramLoadLimits::new(128, 1_024, 32, 8 * 1_024 * 1_024, 64 * 1_024 * 1_024)
}

fn compiler_plan<'a>(
    plans: &'a [UpstreamExecutionPlan],
    case_id: &str,
) -> Result<&'a CompilerExecutionPlan, Box<dyn Error>> {
    let plan = plans
        .iter()
        .find(|plan| plan.provenance.case_id.as_ref() == case_id)
        .ok_or_else(|| failure(format!("H1 acceptance plan is missing {case_id}")))?;
    match &plan.input {
        UpstreamExecutionInput::Compiler(plan) => Ok(plan),
        UpstreamExecutionInput::Project(_) => Err(failure(format!(
            "H1 acceptance row {case_id} is not a compiler case"
        ))),
    }
}

fn required_string<'a>(value: &'a Value, pointer: &str) -> Result<&'a str, Box<dyn Error>> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| failure(format!("H1 qualification field {pointer} is not a string")))
}

fn required_array<'a>(value: &'a Value, pointer: &str) -> Result<&'a [Value], Box<dyn Error>> {
    value
        .pointer(pointer)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| failure(format!("H1 qualification field {pointer} is not an array")))
}

fn optional_string(value: Option<&str>) -> Value {
    json!({"present": value.is_some(), "value": value})
}

fn optional_u32(value: Option<u32>) -> Value {
    json!({"present": value.is_some(), "value": value})
}

fn optional_bool(value: Option<bool>) -> Value {
    json!({"present": value.is_some(), "value": value})
}

fn normalize_chain(chain: &MessageChain) -> Value {
    json!({
        "text": chain.text,
        "code": chain.code,
        "category": chain.category.name(),
        "next_present": chain.next_present,
        "next": chain.next.iter().map(normalize_chain).collect::<Vec<_>>(),
    })
}

fn normalize_diagnostic(diagnostic: &Diagnostic, sources: &BTreeMap<String, String>) -> Value {
    let location = diagnostic
        .file_name
        .as_ref()
        .zip(diagnostic.start)
        .and_then(|(file_name, start)| {
            sources
                .get(file_name)
                .map(|text| get_line_and_character_of_position(&compute_line_starts(text), start))
        });
    json!({
        "file": optional_string(diagnostic.file_name.as_deref()),
        "start": optional_u32(diagnostic.start),
        "length": optional_u32(diagnostic.length),
        "line": optional_u32(location.map(|location| location.line)),
        "column": optional_u32(location.map(|location| location.character)),
        "code": diagnostic.code(),
        "category": diagnostic.category().name(),
        "chain": normalize_chain(&diagnostic.message),
        "related_information_present": diagnostic.related_information_present
            || !diagnostic.related.is_empty(),
        "related": diagnostic.related.iter().map(|related| {
            json!({
                "file": optional_string(related.file_name.as_deref()),
                "start": optional_u32(related.start),
                "length": optional_u32(related.length),
                "code": related.message.code,
                "category": related.message.category.name(),
                "chain": normalize_chain(&related.message),
            })
        }).collect::<Vec<_>>(),
        "reports_unnecessary": optional_bool(diagnostic.reports_unnecessary),
        "reports_deprecated": optional_bool(diagnostic.reports_deprecated),
        "source": optional_string(diagnostic.source.as_deref()),
    })
}

fn source_texts(case: &Value) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    required_array(case, "/virtual_files")?
        .iter()
        .map(|file| {
            let path = file["path"]
                .as_str()
                .ok_or_else(|| failure("H1 virtual file path is not a string"))?
                .to_owned();
            let encoded = file["utf8_base64"]
                .as_str()
                .ok_or_else(|| failure("H1 virtual file bytes are not base64"))?;
            let bytes = base64::engine::general_purpose::STANDARD.decode(encoded)?;
            let text = String::from_utf8(bytes)?;
            Ok((path, text))
        })
        .collect()
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn assert_diagnostics(
    case_id: &str,
    expected: &[Value],
    actual: impl IntoIterator<Item = Diagnostic>,
    sources: &BTreeMap<String, String>,
) -> Result<(), Box<dyn Error>> {
    let actual = actual
        .into_iter()
        .map(|diagnostic| normalize_diagnostic(&diagnostic, sources))
        .collect::<Vec<_>>();
    if actual != expected {
        return Err(failure(format!(
            "{case_id}: pre-emit diagnostics differ\nexpected={}\nactual={}",
            serde_json::to_string_pretty(expected)?,
            serde_json::to_string_pretty(&actual)?,
        )));
    }
    Ok(())
}

fn assert_outcome(
    case_id: &str,
    expected: &Value,
    actual: &EmitOutcome,
) -> Result<(), Box<dyn Error>> {
    if actual.emit_skipped() != expected["emit_skipped"].as_bool().unwrap_or(true) {
        return Err(failure(format!("{case_id}: emitSkipped differs")));
    }
    if !actual.diagnostics().is_empty()
        || !expected["emit_diagnostics"]
            .as_array()
            .is_some_and(Vec::is_empty)
    {
        return Err(failure(format!("{case_id}: emit diagnostics differ")));
    }
    if actual.emitted_files().is_some()
        != expected["emitted_files_present"].as_bool().unwrap_or(true)
    {
        return Err(failure(format!("{case_id}: emittedFiles presence differs")));
    }
    let actual_emitted = actual
        .emitted_files()
        .unwrap_or_default()
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let expected_emitted = expected["emitted_files"]
        .as_array()
        .ok_or_else(|| failure("H1 emitted_files is not an array"))?
        .iter()
        .map(|path| {
            path.as_str()
                .map(str::to_owned)
                .ok_or_else(|| failure("H1 emitted file path is not a string"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if actual_emitted != expected_emitted {
        return Err(failure(format!("{case_id}: emittedFiles differ")));
    }
    if actual.source_maps().is_some() != expected["source_maps_present"].as_bool().unwrap_or(true)
        || !actual.source_maps().unwrap_or_default().is_empty()
        || !expected["source_maps"]
            .as_array()
            .is_some_and(Vec::is_empty)
    {
        return Err(failure(format!("{case_id}: sourceMaps differ")));
    }
    Ok(())
}

fn assert_writes(
    case_id: &str,
    expected: &[Value],
    sink: &MemoryOutputSink,
) -> Result<(), Box<dyn Error>> {
    if sink.writes().len() != expected.len() {
        return Err(failure(format!(
            "{case_id}: expected {} writes, observed {}",
            expected.len(),
            sink.writes().len(),
        )));
    }
    for (index, (actual, expected)) in sink.writes().iter().zip(expected).enumerate() {
        let label = format!("{case_id} write {index}");
        let expected_path = expected["path"]
            .as_str()
            .ok_or_else(|| failure(format!("{label}: path is not a string")))?;
        if actual.path() != Path::new(expected_path)
            || actual.kind() != EmitArtifactKind::JavaScript
            || expected["kind"] != "javascript"
        {
            return Err(failure(format!("{label}: path or kind differs")));
        }
        let expected_text = expected["callback_text"]
            .as_str()
            .ok_or_else(|| failure(format!("{label}: callback text is not a string")))?;
        if actual.callback_text() != expected_text
            || sha256(actual.callback_text().as_bytes())
                != expected["callback_utf8_sha256"]
                    .as_str()
                    .unwrap_or_default()
            || actual.callback_text().len()
                != expected["callback_utf8_bytes"].as_u64().unwrap_or(u64::MAX) as usize
        {
            return Err(failure(format!("{label}: callback bytes differ")));
        }
        if actual.write_byte_order_mark()
            != expected["write_byte_order_mark"].as_bool().unwrap_or(true)
            || sha256(actual.materialized_bytes().as_ref())
                != expected["materialized_utf8_sha256"]
                    .as_str()
                    .unwrap_or_default()
            || actual.materialized_bytes().len()
                != expected["materialized_utf8_bytes"]
                    .as_u64()
                    .unwrap_or(u64::MAX) as usize
        {
            return Err(failure(format!(
                "{label}: materialized bytes or BOM differ"
            )));
        }
        let actual_sources = actual
            .source_files()
            .unwrap_or_default()
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let expected_sources = expected["source_files"]
            .as_array()
            .ok_or_else(|| failure(format!("{label}: source_files is not an array")))?
            .iter()
            .map(|path| path.as_str().unwrap_or_default().to_owned())
            .collect::<Vec<_>>();
        if actual.source_files().is_some()
            != expected["source_files_present"].as_bool().unwrap_or(false)
            || actual_sources != expected_sources
        {
            return Err(failure(format!("{label}: source provenance differs")));
        }
        let metadata = match actual.metadata() {
            Some(EmitWriteMetadata::Text(metadata)) => metadata,
            _ => return Err(failure(format!("{label}: text metadata is absent"))),
        };
        let expected_metadata = &expected["metadata"]["value"];
        if expected["metadata"]["present"] != true
            || expected_metadata["own_keys"] != json!(["diagnostics", "sourceMapUrlPos"])
            || !metadata.diagnostics().is_empty()
            || metadata.source_map_url_position().is_some()
            || expected["sink_disposition"] != "written"
        {
            return Err(failure(format!("{label}: callback metadata differs")));
        }
    }
    Ok(())
}

fn validate_qualification_header(artifact: &Value) -> Result<&Value, Box<dyn Error>> {
    if artifact["schema"] != 1
        || artifact["kind"] != "h1-emit-qualification"
        || artifact["status"] != "qualified"
        || artifact["phase"] != "H1.6"
        || artifact["upstream_closure"]["total_cases"] != 15_680
        || artifact["upstream_closure"]["compatible_cases"] != 1
        || artifact["upstream_closure"]["executed_cases"] != 1
        || artifact["upstream_closure"]["unexecuted_compatible_cases"] != 0
    {
        return Err(failure("H1 emit qualification is not closed"));
    }
    let cases = required_array(artifact, "/compatible_cases")?;
    if cases.len() != 1 || required_string(&cases[0], "/id")? != EXPECTED_CASE_ID {
        return Err(failure("H1 compatible-case inventory changed"));
    }
    Ok(&cases[0])
}

/// Execute every H1-compatible upstream row through both the diagnostic and
/// emitting program projections, comparing exact pinned observations twice.
pub fn run(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let artifact: Value =
        serde_json::from_slice(&fs::read(workspace.join(QUALIFICATION_RELATIVE_PATH))?)?;
    let case = validate_qualification_header(&artifact)?;
    let case_id = required_string(case, "/id")?;
    let sources = source_texts(case)?;
    let corpus = load_recorded_execution_plans(workspace)?;
    let plan = compiler_plan(&corpus.plans, case_id)?;

    let checked = ProgramSession::new(load_compiler_no_emit(workspace, plan, limits())?)
        .run()?
        .into_diagnostics();
    assert_diagnostics(
        case_id,
        required_array(case, "/observation/reported_diagnostics")?,
        checked,
        &sources,
    )?;

    let mut first_sink = MemoryOutputSink::new();
    let first = ProgramSession::new(load_compiler_emit(workspace, plan, limits())?)
        .emit(&mut first_sink)?;
    let mut second_sink = MemoryOutputSink::new();
    let second = ProgramSession::new(load_compiler_emit(workspace, plan, limits())?)
        .emit(&mut second_sink)?;
    if first != second || first_sink != second_sink {
        return Err(failure(format!(
            "{case_id}: repeated emit is not deterministic"
        )));
    }
    assert_outcome(case_id, &case["observation"]["emit_result"], &first)?;
    assert_writes(
        case_id,
        required_array(case, "/observation/writes")?,
        &first_sink,
    )?;

    println!(
        "H1 emit acceptance: compatible=1 executed=1 exact=1 diagnostics={} writes={}",
        case["observation"]["reported_diagnostics"]
            .as_array()
            .map_or(0, Vec::len),
        first_sink.writes().len(),
    );
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/h1_emit_acceptance/tests.rs"]
mod tests;

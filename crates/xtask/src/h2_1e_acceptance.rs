//! Hosted H2.1e acceptance projection over the source-dispositioned
//! compiler/conformance rows in the pinned `ts-tests` tree.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tsc_compiler::{
    EmitArtifact, EmitFailure, EmitIoError, EmitWriteDisposition, H2RuntimeSlice, MemoryOutputSink,
    OutputSink, ProgramSession,
};
use tsc_diagnostics::{Diagnostic, DiagnosticCategory, MessageChain};
use tsc_harness::upstream_suites::execution::load_qualified_compiler_emit;
use tsc_program::ProgramLoadLimits;

const QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-1e-qualification.v1.json";

fn failure(message: impl Into<String>) -> Box<dyn Error> {
    std::io::Error::other(message.into()).into()
}

fn limits() -> ProgramLoadLimits {
    ProgramLoadLimits::new(256, 2_048, 64, 16 * 1_024 * 1_024, 128 * 1_024 * 1_024)
}

fn sha256(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}

fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str, Box<dyn Error>> {
    value[field]
        .as_str()
        .ok_or_else(|| failure(format!("H2.1e field {field} is not a string")))
}

fn array<'a>(value: &'a Value, field: &str) -> Result<&'a [Value], Box<dyn Error>> {
    value[field]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| failure(format!("H2.1e field {field} is not an array")))
}

fn case_input(
    workspace: &Path,
    case: &Value,
) -> Result<tsc_program::PreparedProgram, Box<dyn Error>> {
    let input = &case["input"];
    let current_directory = string(input, "current_directory")?;
    let mut files = array(input, "files")?
        .iter()
        .map(|file| {
            let path = PathBuf::from(string(file, "path")?);
            let bytes =
                base64::engine::general_purpose::STANDARD.decode(string(file, "utf8_base64")?)?;
            if bytes.len() as u64 != file["utf8_bytes"].as_u64().unwrap_or(u64::MAX)
                || sha256(&bytes) != string(file, "utf8_sha256")?
            {
                return Err(failure(format!(
                    "{}: virtual input identity differs",
                    path.display()
                )));
            }
            Ok((path, bytes))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    if !input["virtual_config"].is_null() {
        let config = &input["virtual_config"];
        let path = PathBuf::from(string(config, "path")?);
        let bytes =
            base64::engine::general_purpose::STANDARD.decode(string(config, "utf8_base64")?)?;
        if bytes.len() as u64 != config["utf8_bytes"].as_u64().unwrap_or(u64::MAX)
            || sha256(&bytes) != string(config, "utf8_sha256")?
        {
            return Err(failure(format!(
                "{}: virtual config identity differs",
                path.display()
            )));
        }
        files.push((path, bytes));
    }
    let roots = array(input, "roots")?
        .iter()
        .map(|root| {
            root.as_str()
                .map(PathBuf::from)
                .ok_or_else(|| failure("H2.1e root is not a string"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let settings = array(input, "settings")?
        .iter()
        .map(|setting| {
            Ok((
                string(setting, "name")?.to_owned(),
                string(setting, "value")?.to_owned(),
            ))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    Ok(load_qualified_compiler_emit(
        workspace,
        current_directory,
        &files,
        &roots,
        &settings,
        limits(),
    )?)
}

fn assert_exact_writes(
    case_id: &str,
    expected: &[Value],
    actual: &MemoryOutputSink,
) -> Result<(), Box<dyn Error>> {
    if actual.writes().len() != expected.len() {
        return Err(failure(format!(
            "{case_id}: expected {} writes, observed {}",
            expected.len(),
            actual.writes().len()
        )));
    }
    for (index, (expected, actual)) in expected.iter().zip(actual.writes()).enumerate() {
        let expected_path = Path::new(string(expected, "path")?);
        let expected_bytes = base64::engine::general_purpose::STANDARD
            .decode(string(expected, "callback_utf8_base64")?)?;
        if actual.path() != expected_path
            || actual.callback_text().as_bytes() != expected_bytes
            || actual.callback_text().len() as u64
                != expected["callback_utf8_bytes"].as_u64().unwrap_or(u64::MAX)
            || sha256(actual.callback_text().as_bytes())
                != string(expected, "callback_utf8_sha256")?
            || actual.write_byte_order_mark()
                != expected["write_byte_order_mark"].as_bool().unwrap_or(true)
            || actual.materialized_bytes().len() as u64
                != expected["materialized_utf8_bytes"]
                    .as_u64()
                    .unwrap_or(u64::MAX)
            || sha256(actual.materialized_bytes()) != string(expected, "materialized_utf8_sha256")?
        {
            return Err(failure(format!(
                "{case_id}: write {index} path or exact bytes differ: expected_path={} actual_path={} expected_callback_sha256={} actual_callback_sha256={} expected_materialized_sha256={} actual_materialized_sha256={} expected_bom={} actual_bom={} expected_text={:?} actual_text={:?}",
                expected_path.display(),
                actual.path().display(),
                string(expected, "callback_utf8_sha256")?,
                sha256(actual.callback_text().as_bytes()),
                string(expected, "materialized_utf8_sha256")?,
                sha256(actual.materialized_bytes()),
                expected["write_byte_order_mark"].as_bool().unwrap_or(true),
                actual.write_byte_order_mark(),
                String::from_utf8_lossy(&expected_bytes),
                actual.callback_text(),
            )));
        }
        let expected_sources = array(expected, "source_files")?
            .iter()
            .map(|source| source.as_str().unwrap_or_default())
            .collect::<Vec<_>>();
        let actual_sources = actual
            .source_files()
            .unwrap_or_default()
            .iter()
            .map(|source| source.to_string_lossy())
            .collect::<Vec<_>>();
        if actual_sources
            .iter()
            .map(|source| source.as_ref())
            .collect::<Vec<_>>()
            != expected_sources
        {
            return Err(failure(format!(
                "{case_id}: write {index} source provenance differs"
            )));
        }
    }
    Ok(())
}

fn flatten_message_chain(chain: &MessageChain, indent: usize, output: &mut String) {
    if indent != 0 {
        output.push('\n');
        for _ in 0..indent {
            output.push_str("  ");
        }
    }
    output.push_str(&chain.text);
    for child in &chain.next {
        flatten_message_chain(child, indent + 1, output);
    }
}

fn diagnostic_category(category: DiagnosticCategory) -> &'static str {
    match category {
        DiagnosticCategory::Warning => "Warning",
        DiagnosticCategory::Error => "Error",
        DiagnosticCategory::Suggestion => "Suggestion",
        DiagnosticCategory::Message => "Message",
    }
}

fn normalize_diagnostic(diagnostic: &Diagnostic) -> Value {
    let mut message = String::new();
    flatten_message_chain(&diagnostic.message, 0, &mut message);
    json!({
        "code": diagnostic.code(),
        "category": diagnostic_category(diagnostic.category()),
        "file": diagnostic.file_name,
        "start": diagnostic.start,
        "length": diagnostic.length,
        "message": message,
    })
}

fn normalize_diagnostics(actual: &[Diagnostic]) -> Vec<Value> {
    actual.iter().map(normalize_diagnostic).collect()
}

fn assert_reported_diagnostics(
    case_id: &str,
    expected: &[Value],
    actual: &[Diagnostic],
) -> Result<(), Box<dyn Error>> {
    let actual = normalize_diagnostics(actual);
    if actual != expected {
        let expected_sha256 = sha256(serde_json::to_vec(expected)?);
        let actual_sha256 = sha256(serde_json::to_vec(&actual)?);
        let detail = if expected.len().max(actual.len()) <= 20 {
            format!(
                "\nexpected={}\nactual={}",
                serde_json::to_string_pretty(expected)?,
                serde_json::to_string_pretty(&actual)?,
            )
        } else {
            String::new()
        };
        return Err(failure(format!(
            "{case_id}: reported diagnostics differ: expected_count={} actual_count={} expected_sha256={expected_sha256} actual_sha256={actual_sha256}{detail}",
            expected.len(),
            actual.len(),
        )));
    }
    Ok(())
}

fn execute_observed(workspace: &Path, case: &Value) -> Result<(usize, usize), Box<dyn Error>> {
    let case_id = string(case, "case_id")?;
    let mut first_sink = MemoryOutputSink::new();
    let (first, first_reported) = ProgramSession::new(case_input(workspace, case)?)
        .emit_with_reported_diagnostics_for_harness(&mut first_sink)
        .map_err(|error| failure(format!("{case_id}: first Rust emit failed: {error}")))?;
    let mut second_sink = MemoryOutputSink::new();
    let (second, second_reported) = ProgramSession::new(case_input(workspace, case)?)
        .emit_with_reported_diagnostics_for_harness(&mut second_sink)
        .map_err(|error| failure(format!("{case_id}: second Rust emit failed: {error}")))?;
    if first != second || first_sink != second_sink || first_reported != second_reported {
        return Err(failure(format!(
            "{case_id}: repeated Rust emit is not deterministic"
        )));
    }
    let expected = &array(case, "typescript_runs")?[0];
    let expected_reported = array(expected, "reported_diagnostics")?;
    if case["diagnostic_disposition"]["state"] != "exact-required" {
        return Err(failure(format!(
            "{case_id}: exact case lacks its diagnostic disposition"
        )));
    }
    assert_reported_diagnostics(case_id, expected_reported, &first_reported)?;
    let actual_exit_code = if first.emit_skipped() && !first_reported.is_empty() {
        1
    } else if !first_reported.is_empty() {
        2
    } else {
        0
    };
    if first.emit_skipped()
        != expected["emit_result"]["emit_skipped"]
            .as_bool()
            .unwrap_or(true)
        || !first.diagnostics().is_empty()
        || !expected["emit_result"]["diagnostics"]
            .as_array()
            .is_some_and(Vec::is_empty)
        || first.emitted_files().is_some() == expected["emit_result"]["emitted_files"].is_null()
        || first.source_maps().is_some() == expected["emit_result"]["source_maps"].is_null()
        || !array(expected, "status_writes")?.is_empty()
        || expected["exit_code"].as_i64() != Some(actual_exit_code)
    {
        return Err(failure(format!(
            "{case_id}: exact Program.emit result differs"
        )));
    }
    assert_exact_writes(case_id, array(expected, "writes")?, &first_sink)?;
    let activity = first.h2_activity();
    let reached_sources = case["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter(|file| file["emit_eligible"] == true)
                .count() as u64
        })
        .unwrap_or(0);
    if activity.runtime_slice(H2RuntimeSlice::H2_1a) != reached_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_1b) != 0
        || activity.runtime_slice(H2RuntimeSlice::H2_1c) != 0
        || activity.runtime_slice(H2RuntimeSlice::H2_1d) != 0
        || activity.runtime_slice(H2RuntimeSlice::H2_1e) != reached_sources
    {
        return Err(failure(format!(
            "{case_id}: H2.1a/H2.1e activity does not match {reached_sources} reached sources"
        )));
    }
    for slice in H2RuntimeSlice::ALL {
        if !matches!(slice, H2RuntimeSlice::H2_1a | H2RuntimeSlice::H2_1e)
            && activity.runtime_slice(slice) != 0
        {
            return Err(failure(format!(
                "{case_id}: unadmitted {} activity",
                slice.name()
            )));
        }
    }
    Ok((first_sink.writes().len(), first_reported.len()))
}

#[derive(Default)]
struct CountingSink {
    writes: usize,
}

impl OutputSink for CountingSink {
    fn write(&mut self, _artifact: EmitArtifact) -> Result<EmitWriteDisposition, EmitIoError> {
        self.writes += 1;
        Ok(EmitWriteDisposition::Written)
    }
}

fn deferred_failure(workspace: &Path, case: &Value) -> Result<String, Box<dyn Error>> {
    let case_id = string(case, "case_id")?;
    let prepared = match case_input(workspace, case) {
        Ok(prepared) => prepared,
        Err(error) => return Ok(format!("load:{error}")),
    };
    let mut sink = CountingSink::default();
    let error = ProgramSession::new(prepared)
        .emit(&mut sink)
        .expect_err("source-deferred H2.1e case must fail closed");
    if sink.writes != 0 {
        return Err(failure(format!(
            "{case_id}: deferred case wrote {} artifacts",
            sink.writes
        )));
    }
    match &error {
        tsc_compiler::DriverError::Emit(EmitFailure::UnsupportedSourceExtension { .. })
        | tsc_compiler::DriverError::Emit(EmitFailure::Transform(_))
        | tsc_compiler::DriverError::IncompleteCheck { .. } => {}
        _ => {
            return Err(failure(format!(
                "{case_id}: deferred case returned an unowned failure: {error}"
            )))
        }
    }
    Ok(format!("emit:{error}"))
}

fn execute_deferred(workspace: &Path, case: &Value) -> Result<(), Box<dyn Error>> {
    let case_id = string(case, "case_id")?;
    let first = deferred_failure(workspace, case)?;
    let second = deferred_failure(workspace, case)?;
    if first != second {
        return Err(failure(format!(
            "{case_id}: deferred failure is not deterministic"
        )));
    }
    Ok(())
}

/// Execute all 6 H2.1e candidates. Fully admitted rows compare every
/// TypeScript observable twice, and source-deferred rows
/// prove deterministic typed failure before the first sink callback twice.
pub fn run(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let artifact: Value =
        serde_json::from_slice(&fs::read(workspace.join(QUALIFICATION_RELATIVE_PATH))?)?;
    if artifact["schema"] != 1
        || artifact["status"] != "qualified-typescript-oracle"
        || artifact["phase"] != "H2.1e-node-formats-source-and-emit"
        || artifact["summary"]["candidates"] != 6
        || artifact["summary"]["admitted_cases"] != 4
        || artifact["summary"]["deferred_cases"] != 2
        || artifact["summary"]["diagnostic_deferred_output_control_cases"] != 0
        || artifact["summary"]["source_deferred_cases"] != 2
        || artifact["summary"]["unexecuted_candidates"] != 0
        || artifact["summary"]["undispositioned_candidates"] != 0
    {
        return Err(failure("H2.1e qualification header is not closed"));
    }
    let cases = array(&artifact, "cases")?;
    if cases.len() != 6 {
        return Err(failure("H2.1e qualification case denominator changed"));
    }
    let mut admitted = 0;
    let mut source_deferred = 0;
    let mut writes = 0;
    let mut diagnostics = 0;
    for case in cases {
        match string(case, "disposition")? {
            "admitted-for-execution" => {
                admitted += 1;
                let (case_writes, case_diagnostics) = execute_observed(workspace, case)?;
                writes += case_writes;
                diagnostics += case_diagnostics;
            }
            "deferred-to-slices" => {
                source_deferred += 1;
                if case["diagnostic_disposition"]["state"] != "not-observed-source-deferred" {
                    return Err(failure(format!(
                        "{}: source-deferred case lacks its diagnostic disposition",
                        string(case, "case_id")?
                    )));
                }
                execute_deferred(workspace, case)?;
            }
            disposition => return Err(failure(format!("unknown H2.1e disposition {disposition}"))),
        }
    }
    if admitted != 4 || source_deferred != 2 || writes != 8 || diagnostics != 6 {
        return Err(failure(format!(
            "H2.1e execution totals differ: admitted={admitted} source_deferred={source_deferred} writes={writes} diagnostics={diagnostics}"
        )));
    }
    println!(
        "H2.1e emit acceptance: candidates=6 exact={admitted} source_deferred={source_deferred} exact_diagnostics={diagnostics} exact_writes={writes} repetitions=2"
    );
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/h2_1e_acceptance/tests.rs"]
mod tests;

//! Hosted H2.2c acceptance projection over the source-dispositioned
//! compiler/conformance rows in the pinned `ts-tests` tree.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tsc_compiler::{H2RuntimeSlice, MemoryOutputSink, ProgramSession};
use tsc_diagnostics::{Diagnostic, DiagnosticCategory, MessageChain};
use tsc_harness::upstream_suites::execution::load_qualified_compiler_emit;
use tsc_program::ProgramLoadLimits;

const QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-2c-qualification.v1.json";
const H2_4A_QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-4a-qualification.v1.json";
const H2_4B_QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-4b-qualification.v1.json";
const H2_5A_QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-5a-qualification.v1.json";

#[derive(Clone, Copy)]
enum AcceptanceSlice {
    H2_2c,
    H2_4a,
    H2_4b,
    H2_5a,
}

impl AcceptanceSlice {
    const fn label(self) -> &'static str {
        match self {
            Self::H2_2c => "H2.2c",
            Self::H2_4a => "H2.4a",
            Self::H2_4b => "H2.4b",
            Self::H2_5a => "H2.5a",
        }
    }
}

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
        .ok_or_else(|| failure(format!("H2.2c field {field} is not a string")))
}

fn array<'a>(value: &'a Value, field: &str) -> Result<&'a [Value], Box<dyn Error>> {
    value[field]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| failure(format!("H2.2c field {field} is not an array")))
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
                .ok_or_else(|| failure("H2.2c root is not a string"))
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

fn execute_slice_observed(
    workspace: &Path,
    case: &Value,
    accepted_slice: AcceptanceSlice,
) -> Result<(usize, usize), Box<dyn Error>> {
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
    let enum_sources = case["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter(|file| {
                    file["emit_eligible"] == true
                        && file["feature_roots"].as_array().is_some_and(|roots| {
                            roots.iter().any(|root| root["feature"] == "runtime-enums")
                        })
                })
                .count() as u64
        })
        .unwrap_or(0);
    let namespace_sources = case["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter(|file| {
                    file["emit_eligible"] == true
                        && file["feature_roots"].as_array().is_some_and(|roots| {
                            roots
                                .iter()
                                .any(|root| root["feature"] == "runtime-namespaces")
                        })
                })
                .count() as u64
        })
        .unwrap_or(0);
    let parameter_property_sources = case["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter(|file| {
                    file["emit_eligible"] == true
                        && file["feature_roots"].as_array().is_some_and(|roots| {
                            roots
                                .iter()
                                .any(|root| root["feature"] == "parameter-properties")
                        })
                })
                .count() as u64
        })
        .unwrap_or(0);
    let import_export_equals_sources = case["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter(|file| {
                    file["emit_eligible"] == true
                        && file["feature_roots"].as_array().is_some_and(|roots| {
                            roots.iter().any(|root| {
                                matches!(
                                    root["feature"].as_str(),
                                    Some("import-equals" | "export-equals")
                                )
                            })
                        })
                })
                .count() as u64
        })
        .unwrap_or(0);
    let decorator_sources = case["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter(|file| {
                    file["emit_eligible"] == true
                        && file["feature_roots"].as_array().is_some_and(|roots| {
                            roots.iter().any(|root| root["feature"] == "decorators")
                        })
                })
                .count() as u64
        })
        .unwrap_or(0);
    let legacy_decorator_sources = if array(&case["input"], "settings")?.iter().any(|setting| {
        setting["name"]
            .as_str()
            .is_some_and(|name| name.eq_ignore_ascii_case("experimentalDecorators"))
            && setting["value"]
                .as_str()
                .is_some_and(|value| value.eq_ignore_ascii_case("true"))
    }) {
        decorator_sources
    } else {
        0
    };
    let standard_decorator_sources = decorator_sources - legacy_decorator_sources;
    let assignment_field_mode = array(&case["input"], "settings")?
        .iter()
        .find(|setting| {
            setting["name"]
                .as_str()
                .is_some_and(|name| name.eq_ignore_ascii_case("useDefineForClassFields"))
        })
        .and_then(|setting| setting["value"].as_str())
        .map(|value| value.eq_ignore_ascii_case("false"))
        .unwrap_or(case["target_state"] == "ES2021(8)");
    let javascript_sources = case["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter(|file| {
                    file["emit_eligible"] == true
                        && matches!(file["script_kind"].as_str(), Some("JS" | "JSX"))
                })
                .count() as u64
        })
        .unwrap_or(0);
    let jsx_sources = case["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter(|file| {
                    file["emit_eligible"] == true
                        && matches!(file["script_kind"].as_str(), Some("TSX" | "JSX"))
                })
                .count() as u64
        })
        .unwrap_or(0);
    let automatic_jsx_sources = if array(&case["input"], "settings")?.iter().any(|setting| {
        setting["name"]
            .as_str()
            .is_some_and(|name| name.eq_ignore_ascii_case("jsx"))
            && setting["value"].as_str().is_some_and(|value| {
                matches!(
                    value.to_ascii_lowercase().as_str(),
                    "react-jsx" | "react-jsxdev"
                )
            })
    }) {
        jsx_sources
    } else {
        0
    };
    let json_sources = case["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter(|file| file["emit_eligible"] == true && file["script_kind"] == "JSON")
                .count() as u64
        })
        .unwrap_or(0);
    let transform_module_sources = case["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter(|file| {
                    file["emit_eligible"] == true
                        && matches!(file["emit_module_format"].as_i64(), Some(1..=3))
                })
                .count() as u64
        })
        .unwrap_or(0);
    let system_sources = case["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter(|file| file["emit_eligible"] == true && file["emit_module_format"] == 4)
                .count() as u64
        })
        .unwrap_or(0);
    let amd_umd_sources = case["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter(|file| {
                    file["emit_eligible"] == true
                        && matches!(file["emit_module_format"].as_i64(), Some(2 | 3))
                })
                .count() as u64
        })
        .unwrap_or(0);
    let node_format_sources = expected_node_format_sources(case)?;
    if activity.runtime_slice(H2RuntimeSlice::H2_1a) != reached_sources - system_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_1b) != transform_module_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_1c) != amd_umd_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_1d) != system_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_1e) != node_format_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_2a) != enum_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_2b) != namespace_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_2c) != parameter_property_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_2d) != import_export_equals_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_3a) != javascript_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_3b) != jsx_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_3c) != automatic_jsx_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_3d) != json_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_4a)
            != if matches!(
                accepted_slice,
                AcceptanceSlice::H2_4a | AcceptanceSlice::H2_4b | AcceptanceSlice::H2_5a
            ) {
                legacy_decorator_sources
            } else {
                0
            }
        || activity.runtime_slice(H2RuntimeSlice::H2_4b)
            != match accepted_slice {
                AcceptanceSlice::H2_4b => reached_sources,
                AcceptanceSlice::H2_5a if assignment_field_mode => reached_sources,
                AcceptanceSlice::H2_5a => standard_decorator_sources,
                AcceptanceSlice::H2_2c | AcceptanceSlice::H2_4a => 0,
            }
        || activity.runtime_slice(H2RuntimeSlice::H2_5a)
            != if matches!(accepted_slice, AcceptanceSlice::H2_5a) {
                reached_sources
            } else {
                0
            }
    {
        return Err(failure(format!(
            "{case_id}: {} activity does not match {reached_sources} reached, {node_format_sources} node-format, {enum_sources} enum, {namespace_sources} namespace, {parameter_property_sources} parameter-property, {import_export_equals_sources} import/export-equals, {javascript_sources} JavaScript, {jsx_sources} JSX, {automatic_jsx_sources} automatic-JSX, {json_sources} JSON, and {decorator_sources} decorator sources: actual H2.1a={} H2.1b={} H2.1c={} H2.1d={} H2.1e={} H2.2a={} H2.2b={} H2.2c={} H2.2d={} H2.3a={} H2.3b={} H2.3c={} H2.3d={} H2.4a={} H2.4b={} H2.5a={}",
            accepted_slice.label(),
            activity.runtime_slice(H2RuntimeSlice::H2_1a),
            activity.runtime_slice(H2RuntimeSlice::H2_1b),
            activity.runtime_slice(H2RuntimeSlice::H2_1c),
            activity.runtime_slice(H2RuntimeSlice::H2_1d),
            activity.runtime_slice(H2RuntimeSlice::H2_1e),
            activity.runtime_slice(H2RuntimeSlice::H2_2a),
            activity.runtime_slice(H2RuntimeSlice::H2_2b),
            activity.runtime_slice(H2RuntimeSlice::H2_2c),
            activity.runtime_slice(H2RuntimeSlice::H2_2d),
            activity.runtime_slice(H2RuntimeSlice::H2_3a),
            activity.runtime_slice(H2RuntimeSlice::H2_3b),
            activity.runtime_slice(H2RuntimeSlice::H2_3c),
            activity.runtime_slice(H2RuntimeSlice::H2_3d),
            activity.runtime_slice(H2RuntimeSlice::H2_4a),
            activity.runtime_slice(H2RuntimeSlice::H2_4b),
            activity.runtime_slice(H2RuntimeSlice::H2_5a),
        )));
    }
    for slice in H2RuntimeSlice::ALL {
        if !matches!(
            slice,
            H2RuntimeSlice::H2_1a
                | H2RuntimeSlice::H2_1b
                | H2RuntimeSlice::H2_1c
                | H2RuntimeSlice::H2_1d
                | H2RuntimeSlice::H2_1e
                | H2RuntimeSlice::H2_2a
                | H2RuntimeSlice::H2_2b
                | H2RuntimeSlice::H2_2c
                | H2RuntimeSlice::H2_2d
                | H2RuntimeSlice::H2_3a
                | H2RuntimeSlice::H2_3b
                | H2RuntimeSlice::H2_3c
                | H2RuntimeSlice::H2_3d
                | H2RuntimeSlice::H2_4a
                | H2RuntimeSlice::H2_4b
                | H2RuntimeSlice::H2_5a
        ) && activity.runtime_slice(slice) != 0
        {
            return Err(failure(format!(
                "{case_id}: unadmitted {} activity",
                slice.name()
            )));
        }
    }
    Ok((first_sink.writes().len(), first_reported.len()))
}

fn execute_observed(workspace: &Path, case: &Value) -> Result<(usize, usize), Box<dyn Error>> {
    execute_slice_observed(workspace, case, AcceptanceSlice::H2_2c)
}

fn execute_h2_4a_observed(
    workspace: &Path,
    case: &Value,
) -> Result<(usize, usize), Box<dyn Error>> {
    execute_slice_observed(workspace, case, AcceptanceSlice::H2_4a)
}

fn execute_h2_4b_observed(
    workspace: &Path,
    case: &Value,
) -> Result<(usize, usize), Box<dyn Error>> {
    execute_slice_observed(workspace, case, AcceptanceSlice::H2_4b)
}

fn execute_h2_5a_observed(
    workspace: &Path,
    case: &Value,
) -> Result<(usize, usize), Box<dyn Error>> {
    execute_slice_observed(workspace, case, AcceptanceSlice::H2_5a)
}

fn expected_node_format_sources(case: &Value) -> Result<u64, Box<dyn Error>> {
    let settings = array(&case["input"], "settings")?;
    let all_sources = settings.iter().any(|setting| {
        let name = setting["name"].as_str().unwrap_or_default();
        let value = &setting["value"];
        name == "rewriteRelativeImportExtensions" && value == true
            || name == "module"
                && value.as_str().is_some_and(|value| {
                    matches!(
                        value.to_ascii_lowercase().as_str(),
                        "node16" | "node18" | "node20" | "nodenext"
                    )
                })
    });
    let inputs = array(&case["input"], "files")?;
    let mut count = 0_u64;
    for file in array(case, "files")?
        .iter()
        .filter(|file| file["emit_eligible"] == true)
    {
        let path = string(file, "path")?;
        let owns_format = all_sources
            || path.to_ascii_lowercase().ends_with(".mts")
            || path.to_ascii_lowercase().ends_with(".cts")
            || inputs
                .iter()
                .find(|input| input["path"].as_str() == Some(path))
                .map(|input| {
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(string(input, "utf8_base64")?)?;
                    let text = String::from_utf8(bytes)?;
                    Ok::<_, Box<dyn Error>>(
                        text.contains(" with {")
                            || text.contains(" assert {")
                            || text.match_indices("import(").any(|(start, _)| {
                                text[start + "import(".len()..]
                                    .split_once(')')
                                    .is_some_and(|(arguments, _)| arguments.contains(','))
                            }),
                    )
                })
                .transpose()?
                .unwrap_or(false);
        if owns_format {
            count += 1;
        }
    }
    Ok(count)
}

/// Execute all six H2.2c candidates and compare every TypeScript observable
/// twice against two deterministic Rust executions.
pub fn run(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let artifact: Value =
        serde_json::from_slice(&fs::read(workspace.join(QUALIFICATION_RELATIVE_PATH))?)?;
    if artifact["schema"] != 1
        || artifact["status"] != "qualified-typescript-oracle"
        || artifact["phase"] != "H2.2c-parameter-properties"
        || artifact["summary"]["candidates"] != 6
        || artifact["summary"]["admitted_cases"] != 6
        || artifact["summary"]["deferred_cases"] != 0
        || artifact["summary"]["diagnostic_deferred_output_control_cases"] != 0
        || artifact["summary"]["source_deferred_cases"] != 0
        || artifact["summary"]["unexecuted_candidates"] != 0
        || artifact["summary"]["undispositioned_candidates"] != 0
    {
        return Err(failure("H2.2c qualification header is not closed"));
    }
    let cases = array(&artifact, "cases")?;
    if cases.len() != 6 {
        return Err(failure("H2.2c qualification case denominator changed"));
    }
    let mut admitted = 0;
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
            disposition => return Err(failure(format!("unknown H2.2c disposition {disposition}"))),
        }
    }
    if admitted != 6 || writes != 6 || diagnostics != 12 {
        return Err(failure(format!(
            "H2.2c execution totals differ: admitted={admitted} writes={writes} diagnostics={diagnostics}"
        )));
    }
    println!(
        "H2.2c emit acceptance: candidates=6 exact={admitted} source_deferred=0 exact_diagnostics={diagnostics} exact_writes={writes} repetitions=2"
    );
    Ok(())
}

/// Execute the nine admitted H2.4a legacy-decorator candidates and retain the
/// one parser-owned source deferral as an explicit H2.9 boundary.
pub fn run_h2_4a(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let artifact: Value = serde_json::from_slice(&fs::read(
        workspace.join(H2_4A_QUALIFICATION_RELATIVE_PATH),
    )?)?;
    if artifact["schema"] != 1
        || artifact["status"] != "qualified-typescript-oracle"
        || artifact["phase"] != "H2.4a-legacy-decorators"
        || artifact["selection_contract"]["global_h2_4a_rows"] != 418
        || artifact["selection_contract"]["candidate_denominator"] != 10
        || artifact["selection_contract"]["future_deferred_rows"] != 408
        || artifact["summary"]["candidates"] != 10
        || artifact["summary"]["admitted_cases"] != 9
        || artifact["summary"]["deferred_cases"] != 1
        || artifact["summary"]["source_deferred_cases"] != 1
        || artifact["summary"]["unexecuted_candidates"] != 0
        || artifact["summary"]["undispositioned_candidates"] != 0
    {
        return Err(failure("H2.4a qualification header is not closed"));
    }
    let cases = array(&artifact, "cases")?;
    if cases.len() != 10 {
        return Err(failure("H2.4a qualification case denominator changed"));
    }
    let mut admitted = 0;
    let mut source_deferred = 0;
    let mut writes = 0;
    let mut diagnostics = 0;
    for case in cases {
        match string(case, "disposition")? {
            "admitted-for-execution" => {
                admitted += 1;
                let (case_writes, case_diagnostics) = execute_h2_4a_observed(workspace, case)?;
                writes += case_writes;
                diagnostics += case_diagnostics;
            }
            "deferred-to-slices"
                if case["required_slices"]
                    .as_array()
                    .is_some_and(|slices| slices.len() == 1 && slices[0] == "H2.9")
                    && case["diagnostic_disposition"]["state"]
                        == "not-observed-source-deferred" =>
            {
                source_deferred += 1;
            }
            disposition => {
                return Err(failure(format!(
                    "unknown H2.4a disposition {disposition} for {}",
                    string(case, "case_id")?,
                )))
            }
        }
    }
    if admitted != 9 || source_deferred != 1 || writes != 9 || diagnostics != 8 {
        return Err(failure(format!(
            "H2.4a execution totals differ: admitted={admitted} source_deferred={source_deferred} writes={writes} diagnostics={diagnostics}"
        )));
    }
    println!(
        "H2.4a emit acceptance: candidates=10 exact={admitted} source_deferred={source_deferred} exact_diagnostics={diagnostics} exact_writes={writes} repetitions=2"
    );
    Ok(())
}

/// Execute the 42 dependency-closed H2.4b cases, including three immutable
/// H2.1a standard-decorator preservation promotions, and retain the two
/// parser-owned source deferrals as explicit H2.9 boundaries.
pub fn run_h2_4b(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let artifact: Value = serde_json::from_slice(&fs::read(
        workspace.join(H2_4B_QUALIFICATION_RELATIVE_PATH),
    )?)?;
    if artifact["schema"] != 1
        || artifact["status"] != "qualified-typescript-oracle"
        || artifact["phase"] != "H2.4b-standard-decorators-class-fields"
        || artifact["selection_contract"]["global_h2_4b_rows"] != 104
        || artifact["selection_contract"]["global_candidate_denominator"] != 41
        || artifact["selection_contract"]["historical_promotion_candidates"] != 3
        || artifact["selection_contract"]["candidate_denominator"] != 44
        || artifact["selection_contract"]["future_deferred_rows"] != 63
        || artifact["summary"]["candidates"] != 44
        || artifact["summary"]["admitted_cases"] != 42
        || artifact["summary"]["deferred_cases"] != 2
        || artifact["summary"]["source_deferred_cases"] != 2
        || artifact["summary"]["unexecuted_candidates"] != 0
        || artifact["summary"]["undispositioned_candidates"] != 0
    {
        return Err(failure("H2.4b qualification header is not closed"));
    }
    let cases = array(&artifact, "cases")?;
    if cases.len() != 44 {
        return Err(failure("H2.4b qualification case denominator changed"));
    }
    let mut admitted = 0;
    let mut source_deferred = 0;
    let mut writes = 0;
    let mut diagnostics = 0;
    for case in cases {
        match string(case, "disposition")? {
            "admitted-for-execution" => {
                admitted += 1;
                let (case_writes, case_diagnostics) = execute_h2_4b_observed(workspace, case)?;
                writes += case_writes;
                diagnostics += case_diagnostics;
            }
            "deferred-to-slices"
                if case["required_slices"]
                    .as_array()
                    .is_some_and(|slices| slices.len() == 1 && slices[0] == "H2.9")
                    && case["diagnostic_disposition"]["state"]
                        == "not-observed-source-deferred" =>
            {
                source_deferred += 1;
            }
            disposition => {
                return Err(failure(format!(
                    "unknown H2.4b disposition {disposition} for {}",
                    string(case, "case_id")?,
                )))
            }
        }
    }
    if admitted != 42 || source_deferred != 2 || writes != 56 || diagnostics != 150 {
        return Err(failure(format!(
            "H2.4b execution totals differ: admitted={admitted} source_deferred={source_deferred} writes={writes} diagnostics={diagnostics}"
        )));
    }
    println!(
        "H2.4b emit acceptance: candidates=44 exact={admitted} source_deferred={source_deferred} exact_diagnostics={diagnostics} exact_writes={writes} repetitions=2"
    );
    Ok(())
}

/// Execute every dependency-closed H2.5a target row twice and retain only the
/// explicitly owned parser/recovery and general comment-placement deferrals.
pub fn run_h2_5a(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let artifact: Value = serde_json::from_slice(&fs::read(
        workspace.join(H2_5A_QUALIFICATION_RELATIVE_PATH),
    )?)?;
    if artifact["schema"] != 1
        || artifact["status"] != "qualified-typescript-oracle"
        || artifact["phase"] != "H2.5a-esnext-target"
        || artifact["selection_contract"]["global_h2_5a_rows"] != 634
        || artifact["selection_contract"]["global_candidate_denominator"] != 172
        || artifact["selection_contract"]["candidate_denominator"] != 172
        || artifact["selection_contract"]["future_deferred_rows"] != 462
        || artifact["summary"]["candidates"] != 172
        || artifact["summary"]["admitted_cases"] != 167
        || artifact["summary"]["deferred_cases"] != 5
        || artifact["summary"]["source_deferred_cases"] != 5
        || artifact["summary"]["admitted_typescript_writes"] != 287
        || artifact["summary"]["admitted_typescript_diagnostics"] != 335
        || artifact["summary"]["unexecuted_candidates"] != 0
        || artifact["summary"]["undispositioned_candidates"] != 0
        || artifact["owner_closure"]
            .as_array()
            .is_none_or(|owners| owners.len() != 1 || owners[0]["key"] != "transform-esnext")
    {
        return Err(failure("H2.5a qualification header is not closed"));
    }
    let cases = array(&artifact, "cases")?;
    if cases.len() != 172 {
        return Err(failure("H2.5a qualification case denominator changed"));
    }
    let mut admitted = 0;
    let mut h2_8a_deferred = 0;
    let mut h2_9_deferred = 0;
    let mut writes = 0;
    let mut diagnostics = 0;
    for case in cases {
        match string(case, "disposition")? {
            "admitted-for-execution" => {
                admitted += 1;
                let (case_writes, case_diagnostics) = execute_h2_5a_observed(workspace, case)?;
                writes += case_writes;
                diagnostics += case_diagnostics;
            }
            "deferred-to-slices"
                if case["required_slices"]
                    .as_array()
                    .is_some_and(|slices| slices.len() == 1)
                    && case["diagnostic_disposition"]["state"]
                        == "not-observed-source-deferred" =>
            {
                match case["required_slices"][0].as_str() {
                    Some("H2.8a") => h2_8a_deferred += 1,
                    Some("H2.9") => h2_9_deferred += 1,
                    _ => {
                        return Err(failure(format!(
                            "unknown H2.5a deferred owner for {}",
                            string(case, "case_id")?,
                        )))
                    }
                }
            }
            disposition => {
                return Err(failure(format!(
                    "unknown H2.5a disposition {disposition} for {}",
                    string(case, "case_id")?,
                )))
            }
        }
    }
    if admitted != 167
        || h2_8a_deferred != 1
        || h2_9_deferred != 4
        || writes != 287
        || diagnostics != 335
    {
        return Err(failure(format!(
            "H2.5a execution totals differ: admitted={admitted} h2_8a_deferred={h2_8a_deferred} h2_9_deferred={h2_9_deferred} writes={writes} diagnostics={diagnostics}"
        )));
    }
    println!(
        "H2.5a emit acceptance: candidates=172 exact={admitted} h2_8a_deferred={h2_8a_deferred} h2_9_deferred={h2_9_deferred} exact_diagnostics={diagnostics} exact_writes={writes} repetitions=2"
    );
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/h2_2c_acceptance/tests.rs"]
mod tests;

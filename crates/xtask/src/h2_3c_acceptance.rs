//! Hosted H2.3c acceptance for automatic/development JSX runtimes and pinned owner controls.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tsc_compiler::{H2RuntimeSlice, MemoryOutputSink, ProgramSession};
use tsc_diagnostics::{Diagnostic, DiagnosticCategory, MessageChain};
use tsc_harness::upstream_suites::execution::load_qualified_compiler_emit;
use tsc_program::{
    CompilerOptions, PathContext, PreparedProgram, PreparedSourceFile, ProgramLoadLimits,
    ProgramPath,
};

const QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-3c-qualification.v1.json";
const OWNER_CONTROLS_RELATIVE_PATH: &str = "ratchets/h2-3c-owner-controls.v1.json";

const MINIMAL_GLOBALS: &str = r#"
interface IArguments { length: number; callee: Function; }
interface Array<T> { length: number; [index: number]: T; }
interface Object {}
interface Function {}
interface CallableFunction extends Function {}
interface NewableFunction extends Function {}
interface String {}
interface Number {}
interface Boolean {}
interface RegExp {}
"#;

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
        .ok_or_else(|| failure(format!("H2.3c field {field} is not a string")))
}

fn array<'a>(value: &'a Value, field: &str) -> Result<&'a [Value], Box<dyn Error>> {
    value[field]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| failure(format!("H2.3c field {field} is not an array")))
}

fn path(value: &str) -> ProgramPath {
    ProgramPath::from_trusted_parts(value, value).expect("trusted H2.3c acceptance path")
}

fn case_input(workspace: &Path, case: &Value) -> Result<PreparedProgram, Box<dyn Error>> {
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
                .ok_or_else(|| failure("H2.3c root is not a string"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let settings = array(input, "settings")?
        .iter()
        .filter(|setting| setting["name"] != "suppressOutputPathCheck")
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
    id: &str,
    expected: &[Value],
    actual: &MemoryOutputSink,
) -> Result<(), Box<dyn Error>> {
    if actual.writes().len() != expected.len() {
        return Err(failure(format!(
            "{id}: expected {} writes, observed {}",
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
                "{id}: write {index} differs: expected_path={} actual_path={} expected_sha256={} actual_sha256={} expected_text={:?} actual_text={:?}",
                expected_path.display(),
                actual.path().display(),
                string(expected, "callback_utf8_sha256")?,
                sha256(actual.callback_text().as_bytes()),
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
                "{id}: write {index} source provenance differs"
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

fn assert_reported_diagnostics(
    id: &str,
    expected: &[Value],
    actual: &[Diagnostic],
) -> Result<(), Box<dyn Error>> {
    let actual = actual.iter().map(normalize_diagnostic).collect::<Vec<_>>();
    if actual != expected {
        return Err(failure(format!(
            "{id}: reported diagnostics differ\nexpected={}\nactual={}",
            serde_json::to_string_pretty(expected)?,
            serde_json::to_string_pretty(&actual)?,
        )));
    }
    Ok(())
}

fn expected_activity_for_case(
    case: &Value,
    outcome: &tsc_compiler::EmitOutcome,
) -> Result<(), Box<dyn Error>> {
    let mut typescript_sources = 0_u64;
    let mut javascript_sources = 0_u64;
    let mut jsx_sources = 0_u64;
    for file in array(case, "files")?
        .iter()
        .filter(|file| file["emit_eligible"] == true)
    {
        let file_name = string(file, "path")?.to_ascii_lowercase();
        if file_name.ends_with(".js")
            || file_name.ends_with(".mjs")
            || file_name.ends_with(".cjs")
            || file_name.ends_with(".jsx")
        {
            javascript_sources += 1;
        } else {
            typescript_sources += 1;
        }
        if file_name.ends_with(".tsx") || file_name.ends_with(".jsx") {
            jsx_sources += 1;
        }
    }
    let activity = outcome.h2_activity();
    for slice in H2RuntimeSlice::ALL {
        let expected = match slice {
            H2RuntimeSlice::H2_1a => typescript_sources,
            H2RuntimeSlice::H2_3a => javascript_sources,
            H2RuntimeSlice::H2_3b => jsx_sources,
            H2RuntimeSlice::H2_3c => jsx_sources,
            _ => 0,
        };
        if activity.runtime_slice(slice) != expected {
            return Err(failure(format!(
                "{}: {} activity expected {expected}, observed {}",
                string(case, "case_id")?,
                slice.name(),
                activity.runtime_slice(slice),
            )));
        }
    }
    Ok(())
}

fn execute_exact_case(workspace: &Path, case: &Value) -> Result<(usize, usize), Box<dyn Error>> {
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
    assert_reported_diagnostics(
        case_id,
        array(expected, "reported_diagnostics")?,
        &first_reported,
    )?;
    let actual_exit_code = if first.emit_skipped() && !first_reported.is_empty() {
        1
    } else if !first_reported.is_empty() {
        2
    } else {
        0
    };
    if case["diagnostic_disposition"]["state"] != "exact-required"
        || first.emit_skipped()
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
    expected_activity_for_case(case, &first)?;
    Ok((first_sink.writes().len(), first_reported.len()))
}

/// Join a historically source-deferred row to the exact H2.3c admission that
/// supersedes its old fail-closed runtime expectation. Historical evidence
/// remains immutable; only the current acceptance view recognizes the later
/// exact owner.
pub(crate) fn promotes_historical_case(
    case: &Value,
    h2_3c_cases: &[Value],
) -> Result<bool, Box<dyn Error>> {
    if !array(case, "required_slices")?
        .iter()
        .any(|slice| slice.as_str() == Some("H2.3c"))
    {
        return Ok(false);
    }
    let case_id = string(case, "case_id")?;
    let promoted = h2_3c_cases
        .iter()
        .find(|candidate| candidate["case_id"].as_str() == Some(case_id))
        .ok_or_else(|| failure(format!("{case_id}: H2.3c promotion is not recorded")))?;
    if promoted["disposition"] != "admitted-for-execution"
        || promoted["diagnostic_disposition"]["state"] != "exact-required"
    {
        return Err(failure(format!(
            "{case_id}: H2.3c promotion is not an exact admission"
        )));
    }
    Ok(true)
}

fn owner_options(value: &Value) -> Result<CompilerOptions, Box<dyn Error>> {
    let optional_bool = |name: &str| value[name].as_bool();
    let optional_i32 = |name: &str| {
        value[name]
            .as_i64()
            .map(i32::try_from)
            .transpose()
            .map_err(|_| failure(format!("owner option {name} is outside i32")))
    };
    let optional_string = |name: &str| value[name].as_str().map(str::to_owned);
    Ok(CompilerOptions {
        allow_js: optional_bool("allowJs").unwrap_or(false),
        always_strict: optional_bool("alwaysStrict"),
        check_js: optional_bool("checkJs"),
        ignore_deprecations: optional_string("ignoreDeprecations"),
        jsx: optional_i32("jsx")?,
        jsx_factory: optional_string("jsxFactory"),
        jsx_fragment_factory: optional_string("jsxFragmentFactory"),
        jsx_import_source: optional_string("jsxImportSource"),
        module: optional_i32("module")?,
        module_detection: optional_i32("moduleDetection")?,
        new_line: optional_i32("newLine")?,
        out_dir: optional_string("outDir"),
        react_namespace: optional_string("reactNamespace"),
        strict: optional_bool("strict"),
        target: optional_i32("target")?,
        no_emit: Some(false),
        ..CompilerOptions::default()
    })
}

fn owner_input(control: &Value) -> Result<PreparedProgram, Box<dyn Error>> {
    let input = &control["input"];
    let mut builder = PreparedProgram::emitting_builder(
        PathContext::new(path(string(input, "current_directory")?), true),
        owner_options(&input["compiler_options"])?,
    );
    let library = builder
        .add_source_file(PreparedSourceFile::new(path("/lib.d.ts"), MINIMAL_GLOBALS))
        .map_err(|error| failure(format!("add owner library: {error}")))?;
    builder
        .add_library_file(library)
        .map_err(|error| failure(format!("register owner library: {error}")))?;
    for file in array(input, "files")? {
        let file_name = string(file, "path")?;
        let bytes =
            base64::engine::general_purpose::STANDARD.decode(string(file, "utf8_base64")?)?;
        if bytes.len() as u64 != file["utf8_bytes"].as_u64().unwrap_or(u64::MAX)
            || sha256(&bytes) != string(file, "utf8_sha256")?
        {
            return Err(failure(format!(
                "{file_name}: owner input identity differs"
            )));
        }
        let source = builder
            .add_source_file(PreparedSourceFile::new(
                path(file_name),
                String::from_utf8(bytes)?,
            ))
            .map_err(|error| failure(format!("add owner source: {error}")))?;
        if file["root"] == true {
            builder
                .add_root_file(source)
                .map_err(|error| failure(format!("add owner root: {error}")))?;
        }
    }
    builder
        .build()
        .map_err(|error| failure(format!("build owner program: {error}")))
}

fn expected_owner_activity(
    control: &Value,
    outcome: &tsc_compiler::EmitOutcome,
) -> Result<(), Box<dyn Error>> {
    let input = &control["input"];
    let options = &input["compiler_options"];
    let module = options["module"].as_i64().unwrap_or(200);
    let jsx_mode = options["jsx"].as_i64();
    let mut javascript = 0_u64;
    let mut jsx = 0_u64;
    let mut automatic = 0_u64;
    for source in array(input, "files")? {
        let file_name = string(source, "path")?.to_ascii_lowercase();
        if !file_name.ends_with(".tsx") && !file_name.ends_with(".jsx") {
            continue;
        }
        jsx += 1;
        javascript += u64::from(file_name.ends_with(".jsx"));
        let text = String::from_utf8(
            base64::engine::general_purpose::STANDARD.decode(string(source, "utf8_base64")?)?,
        )?;
        automatic += u64::from(
            !text.contains("@jsxRuntime classic")
                && (matches!(jsx_mode, Some(4 | 5))
                    || options["jsxImportSource"].is_string()
                    || text.contains("@jsxImportSource")
                    || text.contains("@jsxRuntime automatic")),
        );
    }
    for slice in H2RuntimeSlice::ALL {
        let expected = match slice {
            H2RuntimeSlice::H2_1a | H2RuntimeSlice::H2_1b if module == 1 => jsx,
            H2RuntimeSlice::H2_3a => javascript,
            H2RuntimeSlice::H2_3b => jsx,
            H2RuntimeSlice::H2_3c => automatic,
            _ => 0,
        };
        if outcome.h2_activity().runtime_slice(slice) != expected {
            return Err(failure(format!(
                "{}: {} owner activity expected {expected}, observed {}",
                string(control, "control_id")?,
                slice.name(),
                outcome.h2_activity().runtime_slice(slice),
            )));
        }
    }
    Ok(())
}

fn execute_owner_control(control: &Value) -> Result<usize, Box<dyn Error>> {
    let id = string(control, "control_id")?;
    let mut first_sink = MemoryOutputSink::new();
    let (first, first_reported) = ProgramSession::new(owner_input(control)?)
        .emit_with_reported_diagnostics_for_harness(&mut first_sink)
        .map_err(|error| failure(format!("{id}: first owner emit failed: {error}")))?;
    let mut second_sink = MemoryOutputSink::new();
    let (second, second_reported) = ProgramSession::new(owner_input(control)?)
        .emit_with_reported_diagnostics_for_harness(&mut second_sink)
        .map_err(|error| failure(format!("{id}: second owner emit failed: {error}")))?;
    if first != second || first_sink != second_sink || first_reported != second_reported {
        return Err(failure(format!(
            "{id}: repeated owner emit is not deterministic"
        )));
    }
    let expected = &control["observation"];
    assert_reported_diagnostics(
        id,
        array(expected, "reported_diagnostics")?,
        &first_reported,
    )?;
    if first.emit_skipped() != expected["emit_skipped"].as_bool().unwrap_or(true)
        || !first.diagnostics().is_empty()
        || !array(expected, "emit_diagnostics")?.is_empty()
    {
        return Err(failure(format!("{id}: owner emit result differs")));
    }
    assert_exact_writes(id, array(expected, "writes")?, &first_sink)?;
    expected_owner_activity(control, &first)?;
    Ok(first_sink.writes().len())
}

pub fn run(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let qualification: Value =
        serde_json::from_slice(&fs::read(workspace.join(QUALIFICATION_RELATIVE_PATH))?)?;
    if qualification["schema"] != 1
        || qualification["status"] != "qualified-typescript-oracle"
        || qualification["phase"] != "H2.3c-automatic-jsx-runtime"
        || qualification["summary"]["candidates"] != 4
        || qualification["summary"]["admitted_cases"] != 4
        || qualification["summary"]["deferred_cases"] != 0
        || qualification["summary"]["source_deferred_cases"] != 0
        || qualification["summary"]["unexecuted_candidates"] != 0
        || qualification["summary"]["undispositioned_candidates"] != 0
    {
        return Err(failure("H2.3c qualification header is not closed"));
    }

    let mut exact = 0;
    let mut writes = 0;
    let mut diagnostics = 0;
    for case in array(&qualification, "cases")? {
        match string(case, "disposition")? {
            "admitted-for-execution" => {
                exact += 1;
                let (case_writes, case_diagnostics) = execute_exact_case(workspace, case)?;
                writes += case_writes;
                diagnostics += case_diagnostics;
            }
            disposition => {
                return Err(failure(format!("unknown H2.3c disposition {disposition}")));
            }
        }
    }

    let owner_controls: Value =
        serde_json::from_slice(&fs::read(workspace.join(OWNER_CONTROLS_RELATIVE_PATH))?)?;
    if owner_controls["schema"] != 1
        || owner_controls["phase"] != "H2.3c-automatic-jsx-owner-controls"
        || owner_controls["status"] != "qualified"
        || owner_controls["summary"]["controls"] != 9
        || owner_controls["summary"]["exact_outputs"] != 9
        || owner_controls["summary"]["typescript_runs"] != 18
        || owner_controls["summary"]["reported_diagnostics"] != 0
    {
        return Err(failure("H2.3c owner-control header is not closed"));
    }
    let mut owner_writes = 0;
    for control in array(&owner_controls, "controls")? {
        owner_writes += execute_owner_control(control)?;
    }

    if exact != 4 || writes != 4 || diagnostics != 42 || owner_writes != 9 {
        return Err(failure(format!(
            "H2.3c totals differ: exact={exact} writes={writes} diagnostics={diagnostics} owner_writes={owner_writes}"
        )));
    }
    println!(
        "H2.3c emit acceptance: candidates=4 exact=4 source_deferred=0 exact_diagnostics=42 exact_writes=4 owner_controls=9 owner_writes=9 repetitions=2"
    );
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/h2_3c_acceptance/tests.rs"]
mod tests;

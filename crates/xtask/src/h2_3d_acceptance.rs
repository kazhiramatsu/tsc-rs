//! Hosted H2.3d acceptance for JSON source output and pinned owner controls.

use std::error::Error;
use std::fs;
use std::path::Path;

use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tsc_compiler::{H2RuntimeSlice, MemoryOutputSink, ProgramSession};
use tsc_diagnostics::{Diagnostic, DiagnosticCategory, MessageChain};
use tsc_program::{CompilerOptions, PathContext, PreparedProgram, PreparedSourceFile, ProgramPath};

const QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-3d-qualification.v1.json";
const OWNER_CONTROLS_RELATIVE_PATH: &str = "ratchets/h2-3d-owner-controls.v1.json";
const H2_4A_OWNER_CONTROLS_RELATIVE_PATH: &str = "ratchets/h2-4a-owner-controls.v1.json";
const H2_4B_OWNER_CONTROLS_RELATIVE_PATH: &str = "ratchets/h2-4b-owner-controls.v1.json";
const H2_5A_OWNER_CONTROLS_RELATIVE_PATH: &str = "ratchets/h2-5a-owner-controls.v1.json";

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

fn sha256(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}

fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str, Box<dyn Error>> {
    value[field]
        .as_str()
        .ok_or_else(|| failure(format!("H2.3d field {field} is not a string")))
}

fn array<'a>(value: &'a Value, field: &str) -> Result<&'a [Value], Box<dyn Error>> {
    value[field]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| failure(format!("H2.3d field {field} is not an array")))
}

fn path(value: &str) -> ProgramPath {
    ProgramPath::from_trusted_parts(value, value).expect("trusted H2.3d acceptance path")
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
        always_strict: optional_bool("alwaysStrict"),
        emit_decorator_metadata: optional_bool("emitDecoratorMetadata"),
        emit_bom: optional_bool("emitBOM"),
        experimental_decorators: optional_bool("experimentalDecorators").unwrap_or(false),
        ignore_deprecations: optional_string("ignoreDeprecations"),
        module: optional_i32("module")?,
        module_resolution: optional_i32("moduleResolution")?,
        new_line: optional_i32("newLine")?,
        no_emit_helpers: optional_bool("noEmitHelpers"),
        no_emit_on_error: optional_bool("noEmitOnError"),
        out_dir: optional_string("outDir"),
        resolve_json_module: optional_bool("resolveJsonModule"),
        target: optional_i32("target")?,
        use_define_for_class_fields: optional_bool("useDefineForClassFields"),
        no_emit: Some(false),
        ..CompilerOptions::default()
    })
}

fn expected_h2_4a_owner_activity(
    control: &Value,
    outcome: &tsc_compiler::EmitOutcome,
) -> Result<(), Box<dyn Error>> {
    let input = &control["input"];
    let module = input["compiler_options"]["module"].as_i64().unwrap_or(200);
    let output_units = array(&control["observation"], "writes")?.len() as u64;
    let namespace_sources = if string(control, "control_id")?
        == "qualified-value-reference-metadata"
        && output_units != 0
    {
        1
    } else {
        0
    };
    for slice in H2RuntimeSlice::ALL {
        let expected = match slice {
            H2RuntimeSlice::H2_1a if module != 4 && module != 200 => output_units,
            H2RuntimeSlice::H2_1b if matches!(module, 1..=3) => output_units,
            H2RuntimeSlice::H2_1d if module == 4 => output_units,
            H2RuntimeSlice::H2_2b => namespace_sources,
            H2RuntimeSlice::H2_4a => output_units,
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

type OwnerActivityExpectation =
    fn(&Value, &tsc_compiler::EmitOutcome) -> Result<(), Box<dyn Error>>;

fn execute_h2_owner_control(
    control: &Value,
    phase: &str,
    expected_activity: OwnerActivityExpectation,
) -> Result<(usize, usize), Box<dyn Error>> {
    let id = string(control, "control_id")?;
    let mut first_sink = MemoryOutputSink::new();
    let (first, first_reported) = ProgramSession::new(owner_input(control)?)
        .emit_with_reported_diagnostics_for_harness(&mut first_sink)
        .map_err(|error| failure(format!("{id}: first {phase} owner emit failed: {error}")))?;
    let mut second_sink = MemoryOutputSink::new();
    let (second, second_reported) = ProgramSession::new(owner_input(control)?)
        .emit_with_reported_diagnostics_for_harness(&mut second_sink)
        .map_err(|error| failure(format!("{id}: second {phase} owner emit failed: {error}")))?;
    if first != second || first_sink != second_sink || first_reported != second_reported {
        return Err(failure(format!(
            "{id}: repeated {phase} owner emit is not deterministic"
        )));
    }
    let expected = &control["observation"];
    assert_reported_diagnostics(
        id,
        array(expected, "reported_diagnostics")?,
        &first_reported,
    )?;
    let actual_emit_diagnostics = first
        .diagnostics()
        .iter()
        .map(normalize_diagnostic)
        .collect::<Vec<_>>();
    if actual_emit_diagnostics != array(expected, "emit_diagnostics")?
        || first.emit_skipped() != expected["emit_skipped"].as_bool().unwrap_or(true)
        || first.emitted_files().is_some()
        || first.source_maps().is_some()
    {
        return Err(failure(format!(
            "{id}: {phase} owner emit result differs\nexpected={}\nactual={}",
            serde_json::to_string_pretty(expected)?,
            serde_json::to_string_pretty(&actual_emit_diagnostics)?,
        )));
    }
    assert_exact_writes(id, array(expected, "writes")?, &first_sink)?;
    expected_activity(control, &first)?;
    Ok((first_sink.writes().len(), first_reported.len()))
}

fn execute_h2_4a_owner_control(control: &Value) -> Result<(usize, usize), Box<dyn Error>> {
    execute_h2_owner_control(control, "H2.4a", expected_h2_4a_owner_activity)
}

fn expected_h2_4b_owner_activity(
    control: &Value,
    outcome: &tsc_compiler::EmitOutcome,
) -> Result<(), Box<dyn Error>> {
    let input = &control["input"];
    let module = input["compiler_options"]["module"].as_i64().unwrap_or(200);
    let output_units = array(&control["observation"], "writes")?.len() as u64;
    let h2_4b_sources = control["runtime_expectation"]["h2_4b_sources"]
        .as_u64()
        .ok_or_else(|| failure("H2.4b owner control lacks its runtime expectation"))?;
    for slice in H2RuntimeSlice::ALL {
        let expected = match slice {
            H2RuntimeSlice::H2_1a if module != 4 && module != 200 => output_units,
            H2RuntimeSlice::H2_1b if matches!(module, 1..=3) => output_units,
            H2RuntimeSlice::H2_1d if module == 4 => output_units,
            H2RuntimeSlice::H2_4b => h2_4b_sources,
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

fn execute_h2_4b_owner_control(control: &Value) -> Result<(usize, usize), Box<dyn Error>> {
    execute_h2_owner_control(control, "H2.4b", expected_h2_4b_owner_activity)
}

fn expected_h2_5a_owner_activity(
    control: &Value,
    outcome: &tsc_compiler::EmitOutcome,
) -> Result<(), Box<dyn Error>> {
    let input = &control["input"];
    let module = input["compiler_options"]["module"].as_i64().unwrap_or(200);
    let output_units = array(&control["observation"], "writes")?.len() as u64;
    let runtime = &control["runtime_expectation"];
    let expected_sources = |name: &str| {
        runtime[name]
            .as_u64()
            .ok_or_else(|| failure(format!("H2.5a owner control lacks {name}")))
    };
    let h2_2b_sources = expected_sources("h2_2b_sources")?;
    let h2_2c_sources = expected_sources("h2_2c_sources")?;
    let h2_4b_sources = expected_sources("h2_4b_sources")?;
    let h2_5a_sources = expected_sources("h2_5a_sources")?;
    for slice in H2RuntimeSlice::ALL {
        let expected = match slice {
            H2RuntimeSlice::H2_1a if module != 4 && module != 200 => output_units,
            H2RuntimeSlice::H2_1b if matches!(module, 1..=3) => output_units,
            H2RuntimeSlice::H2_1d if module == 4 => output_units,
            H2RuntimeSlice::H2_2b => h2_2b_sources,
            H2RuntimeSlice::H2_2c => h2_2c_sources,
            H2RuntimeSlice::H2_4b => h2_4b_sources,
            H2RuntimeSlice::H2_5a => h2_5a_sources,
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

fn execute_h2_5a_owner_control(control: &Value) -> Result<(usize, usize), Box<dyn Error>> {
    execute_h2_owner_control(control, "H2.5a", expected_h2_5a_owner_activity)
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

fn expected_owner_activity(
    control: &Value,
    outcome: &tsc_compiler::EmitOutcome,
) -> Result<(), Box<dyn Error>> {
    let input = &control["input"];
    let module = input["compiler_options"]["module"].as_i64().unwrap_or(200);
    let output_units = array(&control["observation"], "writes")?.len() as u64;
    let json_sources = array(input, "files")?
        .iter()
        .filter(|source| {
            source["path"]
                .as_str()
                .is_some_and(|file_name| file_name.to_ascii_lowercase().ends_with(".json"))
        })
        .count() as u64;
    let eligible_json = if input["compiler_options"]["outDir"].is_string() {
        json_sources
    } else {
        0
    };

    for slice in H2RuntimeSlice::ALL {
        let expected = match slice {
            H2RuntimeSlice::H2_1a if module != 4 && module != 200 => output_units,
            H2RuntimeSlice::H2_1b if matches!(module, 1..=3) => output_units,
            H2RuntimeSlice::H2_1c if matches!(module, 2 | 3) => output_units,
            H2RuntimeSlice::H2_1d if module == 4 => output_units,
            H2RuntimeSlice::H2_1e if (100..=199).contains(&module) => output_units,
            H2RuntimeSlice::H2_3d => eligible_json,
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

fn execute_owner_control(control: &Value) -> Result<(usize, usize), Box<dyn Error>> {
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
    Ok((first_sink.writes().len(), first_reported.len()))
}

pub fn run(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let qualification: Value =
        serde_json::from_slice(&fs::read(workspace.join(QUALIFICATION_RELATIVE_PATH))?)?;
    if qualification["schema"] != 1
        || qualification["status"] != "qualified-typescript-oracle"
        || qualification["phase"] != "H2.3d-json-source-output"
        || qualification["selection_contract"]["global_h2_3d_rows"] != 695
        || qualification["selection_contract"]["candidate_denominator"] != 0
        || qualification["selection_contract"]["future_deferred_rows"] != 695
        || qualification["summary"]["candidates"] != 0
        || qualification["summary"]["admitted_cases"] != 0
        || qualification["summary"]["deferred_cases"] != 0
        || qualification["summary"]["source_deferred_cases"] != 0
        || qualification["summary"]["unexecuted_candidates"] != 0
        || qualification["summary"]["undispositioned_candidates"] != 0
        || !array(&qualification, "cases")?.is_empty()
    {
        return Err(failure("H2.3d qualification header is not closed"));
    }

    let owner_controls: Value =
        serde_json::from_slice(&fs::read(workspace.join(OWNER_CONTROLS_RELATIVE_PATH))?)?;
    if owner_controls["schema"] != 1
        || owner_controls["phase"] != "H2.3d-json-source-owner-controls"
        || owner_controls["status"] != "qualified"
        || owner_controls["summary"]["controls"] != 14
        || owner_controls["summary"]["exact_outputs"] != 13
        || owner_controls["summary"]["typescript_runs"] != 28
        || owner_controls["summary"]["reported_diagnostics"] != 2
    {
        return Err(failure("H2.3d owner-control header is not closed"));
    }
    let mut owner_writes = 0;
    let mut owner_diagnostics = 0;
    for control in array(&owner_controls, "controls")? {
        let (writes, diagnostics) = execute_owner_control(control)?;
        owner_writes += writes;
        owner_diagnostics += diagnostics;
    }
    if owner_writes != 13 || owner_diagnostics != 2 {
        return Err(failure(format!(
            "H2.3d totals differ: owner_writes={owner_writes} owner_diagnostics={owner_diagnostics}"
        )));
    }
    println!(
        "H2.3d emit acceptance: candidates=0 exact=0 future_deferred=695 source_deferred=0 owner_controls=14 owner_diagnostics=2 owner_writes=13 repetitions=2"
    );
    Ok(())
}

pub fn run_h2_4a_owner_controls(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let artifact: Value = serde_json::from_slice(&fs::read(
        workspace.join(H2_4A_OWNER_CONTROLS_RELATIVE_PATH),
    )?)?;
    if artifact["schema"] != 1
        || artifact["phase"] != "H2.4a-legacy-decorator-owner-controls"
        || artifact["status"] != "qualified"
        || artifact["summary"]["controls"] != 19
        || artifact["summary"]["exact_outputs"] != 18
        || artifact["summary"]["typescript_runs"] != 38
        || artifact["summary"]["reported_diagnostics"] != 2
        || artifact["summary"]["emit_diagnostics"] != 1
        || artifact["summary"]["no_emit_on_error_controls"] != 1
    {
        return Err(failure("H2.4a owner-control header is not closed"));
    }
    let controls = array(&artifact, "controls")?;
    if controls.len() != 19 {
        return Err(failure("H2.4a owner-control denominator changed"));
    }
    let mut writes = 0;
    let mut diagnostics = 0;
    for control in controls {
        let (control_writes, control_diagnostics) = execute_h2_4a_owner_control(control)?;
        writes += control_writes;
        diagnostics += control_diagnostics;
    }
    if writes != 18 || diagnostics != 2 {
        return Err(failure(format!(
            "H2.4a owner totals differ: writes={writes} diagnostics={diagnostics}"
        )));
    }
    println!(
        "H2.4a owner controls: controls=19 exact_writes={writes} reported_diagnostics={diagnostics} repetitions=2"
    );
    Ok(())
}

pub fn run_h2_4b_owner_controls(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let artifact: Value = serde_json::from_slice(&fs::read(
        workspace.join(H2_4B_OWNER_CONTROLS_RELATIVE_PATH),
    )?)?;
    if artifact["schema"] != 1
        || artifact["phase"] != "H2.4b-standard-decorator-class-fields-owner-controls"
        || artifact["status"] != "qualified"
        || artifact["summary"]["controls"] != 19
        || artifact["summary"]["exact_outputs"] != 18
        || artifact["summary"]["typescript_runs"] != 38
        || artifact["summary"]["reported_diagnostics"] != 3
        || artifact["summary"]["emit_diagnostics"] != 1
        || artifact["summary"]["no_emit_on_error_controls"] != 1
        || artifact["summary"]["define_fields_controls"] != 1
        || artifact["summary"]["assignment_fields_controls"] != 18
    {
        return Err(failure("H2.4b owner-control header is not closed"));
    }
    let controls = array(&artifact, "controls")?;
    if controls.len() != 19 {
        return Err(failure("H2.4b owner-control denominator changed"));
    }
    let mut writes = 0;
    let mut diagnostics = 0;
    for control in controls {
        let (control_writes, control_diagnostics) = execute_h2_4b_owner_control(control)?;
        writes += control_writes;
        diagnostics += control_diagnostics;
    }
    if writes != 18 || diagnostics != 3 {
        return Err(failure(format!(
            "H2.4b owner totals differ: writes={writes} diagnostics={diagnostics}"
        )));
    }
    println!(
        "H2.4b owner controls: controls=19 exact_writes={writes} reported_diagnostics={diagnostics} repetitions=2"
    );
    Ok(())
}

pub fn run_h2_5a_owner_controls(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let artifact: Value = serde_json::from_slice(&fs::read(
        workspace.join(H2_5A_OWNER_CONTROLS_RELATIVE_PATH),
    )?)?;
    if artifact["schema"] != 1
        || artifact["phase"] != "H2.5a-esnext-target-owner-controls"
        || artifact["status"] != "qualified"
        || artifact["summary"]["controls"] != 20
        || artifact["summary"]["exact_outputs"] != 19
        || artifact["summary"]["typescript_runs"] != 40
        || artifact["summary"]["reported_diagnostics"] != 1
        || artifact["summary"]["emit_diagnostics"] != 1
        || artifact["summary"]["no_emit_on_error_controls"] != 1
        || artifact["summary"]["no_emit_helpers_controls"] != 1
        || artifact["summary"]["es2021_controls"] != 4
        || artifact["summary"]["es2022_controls"] != 12
        || artifact["summary"]["later_standard_controls"] != 3
        || artifact["summary"]["esnext_controls"] != 1
        || artifact["summary"]["using_controls"] != 13
        || artifact["summary"]["await_using_controls"] != 2
        || artifact["summary"]["standard_decorator_controls"] != 2
        || artifact["summary"]["h2_5a_active_controls"] != 18
    {
        return Err(failure("H2.5a owner-control header is not closed"));
    }
    let controls = array(&artifact, "controls")?;
    if controls.len() != 20 {
        return Err(failure("H2.5a owner-control denominator changed"));
    }
    let mut writes = 0;
    let mut diagnostics = 0;
    for control in controls {
        let (control_writes, control_diagnostics) = execute_h2_5a_owner_control(control)?;
        writes += control_writes;
        diagnostics += control_diagnostics;
    }
    if writes != 19 || diagnostics != 1 {
        return Err(failure(format!(
            "H2.5a owner totals differ: writes={writes} diagnostics={diagnostics}"
        )));
    }
    println!(
        "H2.5a owner controls: controls=20 exact_writes={writes} reported_diagnostics={diagnostics} repetitions=2"
    );
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/h2_3d_acceptance/tests.rs"]
mod tests;

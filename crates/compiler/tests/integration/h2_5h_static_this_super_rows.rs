//! H2.5h `h2-5h-ca-2a-r1` rows (ES5 class wrapper: static `this`/`super`
//! and the class alias). Replays the seven owner rows of
//! `ratchets/h2-5h-known-divergences.v1.json` against their frozen
//! `ratchets/h2-5h-qualification.v1.json` TypeScript observations through
//! the same qualified-vfs route as the H2.5h acceptance runner: two
//! deterministic Rust runs per row, comparing every write (path, callback
//! bytes, byte-order mark, order), the reported diagnostics, the emit
//! result, and the exit code.
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde_json::{json, Value};
use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_diagnostics::{Diagnostic, MessageChain};
use tsc_harness::upstream_suites::execution::{
    load_qualified_compiler_emit_with_option_floor, EmitOptionFloor,
};
use tsc_program::ProgramLoadLimits;

const OWNER: &str = "h2-5h-ca-2a-r1";

/// The manifest rows owned by `h2-5h-ca-2a-r1` at the slice's start head
/// (main `0aaf808b`), in manifest order.
const ROWS: &[&str] = &[
    "typescript-6.0.3/conformance/classes/constructorDeclarations/superCalls/derivedClassSuperStatementPosition.ts#target%3Des5",
    "typescript-6.0.3/conformance/classes/members/instanceAndStaticMembers/superInStaticMembers1.ts#target%3Des5",
    "typescript-6.0.3/conformance/classes/members/instanceAndStaticMembers/thisAndSuperInStaticMembers3.ts#target%3Des5",
    "typescript-6.0.3/conformance/classes/members/instanceAndStaticMembers/thisAndSuperInStaticMembers4.ts#target%3Des5",
    "typescript-6.0.3/conformance/classes/members/instanceAndStaticMembers/typeOfThisInStaticMembers10.ts#target%3Des5",
    "typescript-6.0.3/conformance/classes/members/instanceAndStaticMembers/typeOfThisInStaticMembers11.ts#target%3Des5",
    "typescript-6.0.3/conformance/classes/propertyMemberDeclarations/autoAccessor5.ts#target%3Des5",
];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn limits() -> ProgramLoadLimits {
    ProgramLoadLimits::new(256, 2_048, 64, 16 * 1_024 * 1_024, 128 * 1_024 * 1_024)
}

#[test]
fn static_this_super_rows_match_frozen_h2_5h_observations() {
    let workspace = workspace();
    let artifact: Value = serde_json::from_slice(
        &std::fs::read(workspace.join("ratchets/h2-5h-qualification.v1.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(artifact["phase"], "H2.5h-es5-target");
    let cases = artifact["cases"].as_array().unwrap();
    let filter = std::env::var("TSC_RS_STATIC_THIS_SUPER_CASE_FILTER").ok();
    let mut failures = Vec::new();
    let mut executed = 0usize;
    for row in ROWS {
        if filter.as_deref().is_some_and(|filter| !row.contains(filter)) {
            continue;
        }
        executed += 1;
        let case = cases
            .iter()
            .find(|case| case["case_id"] == *row)
            .unwrap_or_else(|| panic!("{row}: absent from the H2.5h qualification artifact"));
        assert_eq!(case["disposition"], "admitted-for-execution", "{row}");
        assert_eq!(case["execution_route"], "qualified-vfs", "{row}");
        let fingerprints = case["typescript_run_fingerprints"].as_array().unwrap();
        let expected = &case["typescript_observation"];
        assert_eq!(fingerprints.len(), 2, "{row}");
        assert!(
            fingerprints
                .iter()
                .all(|fingerprint| fingerprint == &expected["run_fingerprint_sha256"]),
            "{row}: compact TypeScript repetition proof differs"
        );
        let mut outcomes = Vec::new();
        for attempt in 0..2 {
            let actual = run_case(&workspace, case);
            capture(row, attempt, &actual, expected);
            outcomes.push(actual);
        }
        assert_eq!(
            outcomes[0], outcomes[1],
            "{row}: repeated Rust emit is not deterministic"
        );
        let differences = compare(&outcomes[0], expected);
        if differences.is_empty() {
            eprintln!("static this/super rows EXACT x2 {row}");
        } else {
            eprintln!("static this/super rows DIVERGING {row}: {differences:#?}");
            failures.push(*row);
        }
    }
    assert!(executed > 0, "the case filter selected nothing");
    assert!(
        failures.is_empty(),
        "{OWNER} rows diverging from the frozen H2.5h observations: {failures:#?}"
    );
}

fn run_case(workspace: &Path, case: &Value) -> Value {
    let case_id = case["case_id"].as_str().unwrap();
    let input = &case["input"];
    let current_directory = input["current_directory"].as_str().unwrap();
    let mut files = input["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| {
            let path = PathBuf::from(file["path"].as_str().unwrap());
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(file["utf8_base64"].as_str().unwrap())
                .unwrap();
            assert_eq!(bytes.len() as u64, file["utf8_bytes"].as_u64().unwrap());
            (path, bytes)
        })
        .collect::<Vec<_>>();
    if !input["virtual_config"].is_null() {
        let config = &input["virtual_config"];
        files.push((
            PathBuf::from(config["path"].as_str().unwrap()),
            base64::engine::general_purpose::STANDARD
                .decode(config["utf8_base64"].as_str().unwrap())
                .unwrap(),
        ));
    }
    let roots = input["roots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|root| PathBuf::from(root.as_str().unwrap()))
        .collect::<Vec<_>>();
    let settings = input["settings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|setting| {
            (
                setting["name"].as_str().unwrap().to_owned(),
                setting["value"].as_str().unwrap().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    let prepared = load_qualified_compiler_emit_with_option_floor(
        workspace,
        current_directory,
        &files,
        &roots,
        &settings,
        limits(),
        EmitOptionFloor::Established,
    )
    .unwrap_or_else(|error| panic!("{case_id}: prepare failed: {error}"));
    let session = ProgramSession::new(prepared);
    let bundle = session
        .prepare_harness_lib_bundle()
        .unwrap_or_else(|error| panic!("{case_id}: lib bundle failed: {error}"));
    let mut sink = MemoryOutputSink::new();
    let (outcome, reported) = session
        .emit_with_reported_diagnostics_for_harness_with_lib_bundle(&mut sink, bundle.as_ref())
        .unwrap_or_else(|error| panic!("{case_id}: emit failed: {error}"));
    let exit_code = if outcome.emit_skipped() && !reported.is_empty() {
        1
    } else if !reported.is_empty() {
        2
    } else {
        0
    };
    let writes = sink
        .writes()
        .iter()
        .enumerate()
        .map(|(index, artifact)| {
            json!({
                "index": index,
                "path": artifact.path().to_string_lossy(),
                "callback_utf8_base64": base64::engine::general_purpose::STANDARD.encode(artifact.callback_bytes()),
                "callback_utf8_bytes": artifact.callback_bytes().len(),
                "write_byte_order_mark": artifact.write_byte_order_mark(),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "writes": writes,
        "reported_diagnostics": diagnostics(&reported),
        "emit_result": {
            "emit_skipped": outcome.emit_skipped(),
            "diagnostics": diagnostics(outcome.diagnostics()),
            "emitted_files_present": outcome.emitted_files().is_some(),
            "source_maps_present": outcome.source_maps().is_some(),
        },
        "exit_code": exit_code,
    })
}

fn message(chain: &MessageChain, indent: usize, text: &mut String) {
    if indent != 0 {
        text.push('\n');
        text.push_str(&"  ".repeat(indent));
    }
    text.push_str(&chain.text);
    for next in &chain.next {
        message(next, indent + 1, text);
    }
}

fn diagnostics(diagnostics: &[Diagnostic]) -> Value {
    json!(diagnostics
        .iter()
        .map(|diagnostic| {
            let mut text = String::new();
            message(&diagnostic.message, 0, &mut text);
            json!({
                "code": diagnostic.code(),
                "category": format!("{:?}", diagnostic.category()),
                "file": diagnostic.file_name,
                "start": diagnostic.start,
                "length": diagnostic.length,
                "message": text,
            })
        })
        .collect::<Vec<_>>())
}

fn project_expected_diagnostic(diagnostic: &Value) -> Value {
    json!({
        "code": diagnostic["code"],
        "category": diagnostic["category"],
        "file": diagnostic["file"],
        "start": diagnostic["start"],
        "length": diagnostic["length"],
        "message": diagnostic["message"],
    })
}

/// Typed differences between one Rust run and the frozen observation (the
/// H2.5h runner's comparisons, spelled out per field).
fn compare(actual: &Value, expected: &Value) -> Vec<String> {
    let mut differences = Vec::new();
    let expected_writes = expected["writes"].as_array().unwrap();
    let actual_writes = actual["writes"].as_array().unwrap();
    if expected_writes.len() != actual_writes.len() {
        differences.push(format!(
            "write count: expected {} observed {}",
            expected_writes.len(),
            actual_writes.len()
        ));
    }
    for (expected_write, actual_write) in expected_writes.iter().zip(actual_writes) {
        let path = expected_write["path"].as_str().unwrap();
        if actual_write["path"] != expected_write["path"] {
            differences.push(format!(
                "write path: expected {path} observed {}",
                actual_write["path"]
            ));
        }
        if actual_write["callback_utf8_base64"] != expected_write["callback_utf8_base64"] {
            differences.push(format!("write bytes differ: {path}"));
        }
        if actual_write["write_byte_order_mark"] != expected_write["write_byte_order_mark"] {
            differences.push(format!("write byte-order mark differs: {path}"));
        }
    }
    let expected_reported = expected["reported_diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(project_expected_diagnostic)
        .collect::<Vec<_>>();
    if actual["reported_diagnostics"].as_array().unwrap() != &expected_reported {
        differences.push(format!(
            "reported diagnostics: expected {expected_reported:?} observed {:?}",
            actual["reported_diagnostics"]
        ));
    }
    let expected_result = &expected["emit_result"];
    let actual_result = &actual["emit_result"];
    if actual_result["emit_skipped"] != expected_result["emit_skipped"] {
        differences.push("emit_skipped differs".to_owned());
    }
    let expected_emit_diagnostics = expected_result["diagnostics"]
        .as_array()
        .map(|list| list.iter().map(project_expected_diagnostic).collect::<Vec<_>>())
        .unwrap_or_default();
    if actual_result["diagnostics"].as_array().unwrap() != &expected_emit_diagnostics {
        differences.push("emit result diagnostics differ".to_owned());
    }
    if actual_result["emitted_files_present"].as_bool().unwrap()
        != !expected_result["emitted_files"].is_null()
    {
        differences.push("emitted_files presence differs".to_owned());
    }
    if actual_result["source_maps_present"].as_bool().unwrap()
        != !expected_result["source_maps"].is_null()
    {
        differences.push("source_maps presence differs".to_owned());
    }
    if !expected["status_writes"].as_array().unwrap().is_empty() {
        differences.push("expected status writes are not empty".to_owned());
    }
    if actual["exit_code"] != expected["exit_code"] {
        differences.push(format!(
            "exit code: expected {} observed {}",
            expected["exit_code"], actual["exit_code"]
        ));
    }
    differences
}

/// Retain each complete run (decoded texts included) when
/// `TSC_RS_H2_8A_CAPTURE_WRITES_DIR` names an absolute directory.
fn capture(row: &str, attempt: usize, actual: &Value, expected: &Value) {
    let Some(directory) = std::env::var_os("TSC_RS_H2_8A_CAPTURE_WRITES_DIR") else {
        return;
    };
    let directory = PathBuf::from(directory);
    assert!(directory.is_absolute());
    std::fs::create_dir_all(&directory).unwrap();
    let decode = |write: &Value| {
        base64::engine::general_purpose::STANDARD
            .decode(write["callback_utf8_base64"].as_str().unwrap())
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default()
    };
    let texts = |writes: &Value| {
        json!(writes
            .as_array()
            .unwrap()
            .iter()
            .map(|write| json!({"path": write["path"], "text": decode(write)}))
            .collect::<Vec<_>>())
    };
    let name = row
        .rsplit('/')
        .next()
        .unwrap()
        .replace(['#', '%'], "_");
    let value = json!({
        "case_id": row,
        "attempt": attempt,
        "actual": actual,
        "expected": {
            "writes": expected["writes"],
            "reported_diagnostics": expected["reported_diagnostics"],
            "emit_result": expected["emit_result"],
            "status_writes": expected["status_writes"],
            "exit_code": expected["exit_code"],
        },
        "actual_texts": texts(&actual["writes"]),
        "expected_texts": texts(&expected["writes"]),
    });
    std::fs::write(
        directory.join(format!("{name}-{attempt}.json")),
        serde_json::to_vec_pretty(&value).unwrap(),
    )
    .unwrap();
}

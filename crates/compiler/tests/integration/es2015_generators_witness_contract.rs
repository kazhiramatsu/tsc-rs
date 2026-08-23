//! B-5 witness-driven full-pipeline fixture gate (the CS-6 analog at
//! program level; packet h2-5h-b-b-5 §5).
//!
//! Every case of the frozen ES2015/Generators witness artifact drives
//! the port end to end — parse, bind, check with the PRODUCTION checker
//! resolver, and emit through the registered
//! `[transformES2015, transformGenerators]` pipeline — and the stored
//! oracle observation is the ENTIRE expectation: reported diagnostic
//! codes/positions, `emit_skipped`, write paths, exact output bytes, and
//! marker occurrence counts, all run twice per case (the artifact's
//! `repetitions: 2`). No expected value is authored here; a red case is
//! fixed in production under the frozen bytes' authority, never by
//! amending the witness.
//!
//! The oracle resolved default libraries from disk with plain
//! `ts.createCompilerHost` semantics (the artifact's `lib` inventory
//! record pins the census), so this gate preloads the complete vendored
//! `lib.*.d.ts` inventory into the memory host and lets
//! `LibraryCatalog::typescript_6_0_3` resolve identically.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_emitting_program, CompilerOptions, LibraryCatalog, ProgramLoadLimits, ProgramOptions,
};

const WITNESSES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h2-5h-a-es2015-generators-witnesses.v1.json"
));

const LIBRARY_HOST_DIRECTORY: &str = "/typescript/lib";

/// The four production gaps the fixture gate surfaced on its first run
/// (if any): each is a frozen oracle byte sequence the port does not yet
/// reproduce, owned by the B-5 train as production fixes cited by these
/// case ids. The list may only SHRINK: a case that starts passing must be
/// removed here, and any NEW divergence fails the suite immediately.
const KNOWN_DIVERGENCES: [&str; 0] = [];

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn decode_base64(text: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .expect("witness base64 payload")
}

fn vendored_library_files() -> Vec<(String, Vec<u8>)> {
    let directory =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/typescript-6.0.3/lib");
    let mut files = Vec::new();
    for entry in fs::read_dir(&directory).expect("vendored lib directory") {
        let entry = entry.expect("vendored lib entry");
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("lib.") && name.ends_with(".d.ts") {
            files.push((
                name.clone(),
                fs::read(entry.path()).expect("vendored lib bytes"),
            ));
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    assert!(!files.is_empty(), "vendored lib inventory is empty");
    files
}

/// The stored serialized options are exactly this ten-key set (base
/// options merged with the per-case overrides by the generator); anything
/// else fails closed — a silently defaulted option would break byte
/// parity (the CS-6 lesson).
fn case_compiler_options(serialized: &Value) -> CompilerOptions {
    let map = serialized.as_object().expect("serialized options object");
    let mut options = CompilerOptions::default();
    for (key, value) in map {
        match key.as_str() {
            "target" => options.target = Some(value.as_i64().expect("target") as i32),
            "module" => options.module = Some(value.as_i64().expect("module") as i32),
            "alwaysStrict" => {
                options.always_strict = Some(value.as_bool().expect("alwaysStrict"));
            }
            "downlevelIteration" => {
                options.downlevel_iteration = Some(value.as_bool().expect("downlevelIteration"));
            }
            "importHelpers" => {
                options.import_helpers = Some(value.as_bool().expect("importHelpers"));
            }
            "noEmitHelpers" => {
                options.no_emit_helpers = Some(value.as_bool().expect("noEmitHelpers"));
            }
            "newLine" => options.new_line = Some(value.as_i64().expect("newLine") as i32),
            "useDefineForClassFields" => {
                options.use_define_for_class_fields =
                    Some(value.as_bool().expect("useDefineForClassFields"));
            }
            "useUnknownInCatchVariables" => {
                options.use_unknown_in_catch_variables =
                    Some(value.as_bool().expect("useUnknownInCatchVariables"));
            }
            "ignoreDeprecations" => {
                options.ignore_deprecations =
                    Some(value.as_str().expect("ignoreDeprecations").to_owned());
            }
            other => panic!("unexpected stored compiler option {other}"),
        }
    }
    options
}

/// `(code, start, length, file_name)` of one reported diagnostic.
type ReportedDiagnostic = (u32, Option<u32>, Option<u32>, Option<String>);

struct DrivenCase {
    reported: Vec<ReportedDiagnostic>,
    emit_skipped: bool,
    emit_diagnostics: usize,
    writes: Vec<(String, Vec<u8>, bool, String)>,
}

fn drive_case(
    case_id: &str,
    case: &Value,
    library_files: &[(String, Vec<u8>)],
) -> Result<DrivenCase, String> {
    let input = &case["input"];
    assert_eq!(
        input["current_directory"].as_str(),
        Some("/project"),
        "{case_id}: current directory"
    );
    let mut builder = MemoryCompilerHost::builder("/project");
    for file in input["files"].as_array().expect("input files") {
        let path = file["path"].as_str().expect("input path");
        let bytes = decode_base64(file["utf8_base64"].as_str().expect("input base64"));
        assert_eq!(
            bytes.len() as u64,
            file["utf8_bytes"].as_u64().expect("input byte count"),
            "{case_id}: input byte count"
        );
        assert_eq!(
            sha256_hex(&bytes),
            file["utf8_sha256"].as_str().expect("input sha256"),
            "{case_id}: input identity"
        );
        builder = builder.file(path, bytes);
    }
    for (name, bytes) in library_files {
        builder = builder.file(format!("{LIBRARY_HOST_DIRECTORY}/{name}"), bytes.clone());
    }
    let host = builder.build().expect("build witness memory host");
    let catalog = LibraryCatalog::typescript_6_0_3(LIBRARY_HOST_DIRECTORY);
    let roots = input["roots"]
        .as_array()
        .expect("roots")
        .iter()
        .map(|root| PathBuf::from(root.as_str().expect("root path")))
        .collect::<Vec<_>>();
    let options = case_compiler_options(&input["compiler_options"]);
    let prepared = load_emitting_program(
        &host,
        &roots,
        options,
        ProgramOptions::default(),
        &catalog,
        ProgramLoadLimits::new(256, 2_048, 64, 16 * 1_024 * 1_024, 128 * 1_024 * 1_024),
    )
    .map_err(|error| format!("{case_id}: program load failed: {error:?}"))?;
    let second_program = prepared.clone();

    let mut first_sink = MemoryOutputSink::new();
    let (first, first_reported) = ProgramSession::new(prepared)
        .emit_with_reported_diagnostics_for_harness(&mut first_sink)
        .map_err(|error| format!("{case_id}: first emit failed: {error:?}"))?;
    let mut second_sink = MemoryOutputSink::new();
    let (second, second_reported) = ProgramSession::new(second_program)
        .emit_with_reported_diagnostics_for_harness(&mut second_sink)
        .map_err(|error| format!("{case_id}: second emit failed: {error:?}"))?;
    if first.emit_skipped() != second.emit_skipped()
        || first_sink != second_sink
        || first_reported != second_reported
    {
        return Err(format!("{case_id}: repeated emit is not deterministic"));
    }

    Ok(DrivenCase {
        reported: first_reported
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.code(),
                    diagnostic.start,
                    diagnostic.length,
                    diagnostic.file_name.clone(),
                )
            })
            .collect(),
        emit_skipped: first.emit_skipped(),
        emit_diagnostics: first.diagnostics().len(),
        writes: first_sink
            .writes()
            .iter()
            .map(|write| {
                (
                    write.path().to_string_lossy().into_owned(),
                    write.materialized_bytes().as_ref().to_vec(),
                    write.write_byte_order_mark(),
                    write.callback_text().to_owned(),
                )
            })
            .collect(),
    })
}

fn count_occurrences(text: &str, token: &str) -> u64 {
    if token.is_empty() {
        return 0;
    }
    let mut count = 0_u64;
    let mut rest = text;
    while let Some(index) = rest.find(token) {
        count += 1;
        rest = &rest[index + token.len()..];
    }
    count
}

fn first_divergence(expected: &str, actual: &str) -> String {
    let byte = expected
        .bytes()
        .zip(actual.bytes())
        .position(|(left, right)| left != right)
        .unwrap_or_else(|| expected.len().min(actual.len()));
    let start = byte.saturating_sub(40);
    format!(
        "first divergence at byte {byte}\n  expected …{:?}\n  actual   …{:?}",
        &expected[start..(byte + 40).min(expected.len())],
        &actual[start..(byte + 40).min(actual.len())],
    )
}

fn verify_case(case_id: &str, case: &Value, driven: &DrivenCase) -> Result<(), String> {
    let observation = &case["observation"];
    let expected_reported = observation["reported_diagnostics"]
        .as_array()
        .expect("reported diagnostics");
    let actual_reported = &driven.reported;
    if actual_reported.len() != expected_reported.len()
        || actual_reported
            .iter()
            .zip(expected_reported)
            .any(|(actual, expected)| {
                actual.0 != expected["code"].as_u64().expect("diagnostic code") as u32
                    || actual.1.map(u64::from) != expected["start"].as_u64()
                    || actual.2.map(u64::from) != expected["length"].as_u64()
                    || actual.3.as_deref() != expected["file"].as_str()
            })
    {
        return Err(format!(
            "{case_id}: reported diagnostics differ\n  expected {:?}\n  actual   {:?}",
            expected_reported
                .iter()
                .map(|diagnostic| {
                    (
                        diagnostic["code"].as_u64(),
                        diagnostic["start"].as_u64(),
                        diagnostic["length"].as_u64(),
                    )
                })
                .collect::<Vec<_>>(),
            actual_reported,
        ));
    }
    let expected_codes = case["expected_reported_codes"]
        .as_array()
        .expect("expected codes")
        .iter()
        .map(|code| code.as_u64().expect("code") as u32)
        .collect::<Vec<_>>();
    let actual_codes = actual_reported
        .iter()
        .map(|diagnostic| diagnostic.0)
        .collect::<Vec<_>>();
    if actual_codes != expected_codes {
        return Err(format!(
            "{case_id}: reported code sequence differs: expected {expected_codes:?}, got {actual_codes:?}"
        ));
    }
    if driven.emit_skipped != observation["emit_skipped"].as_bool().expect("emit_skipped") {
        return Err(format!("{case_id}: emit_skipped differs"));
    }
    let expected_emit_diagnostics = observation["emit_diagnostics"]
        .as_array()
        .expect("emit diagnostics")
        .len();
    if driven.emit_diagnostics != expected_emit_diagnostics {
        return Err(format!(
            "{case_id}: emit diagnostic count differs: expected {expected_emit_diagnostics}, got {}",
            driven.emit_diagnostics,
        ));
    }
    let expected_writes = observation["writes"].as_array().expect("writes");
    if driven.writes.len() != expected_writes.len() {
        return Err(format!(
            "{case_id}: expected {} writes, observed {}",
            expected_writes.len(),
            driven.writes.len()
        ));
    }
    let mut emitted_texts: BTreeMap<&str, &str> = BTreeMap::new();
    for ((path, bytes, bom, callback_text), expected) in driven.writes.iter().zip(expected_writes) {
        let expected_path = expected["path"].as_str().expect("write path");
        if path != expected_path {
            return Err(format!(
                "{case_id}: write path differs: expected {expected_path}, got {path}"
            ));
        }
        let expected_bytes =
            decode_base64(expected["callback_utf8_base64"].as_str().expect("output"));
        if callback_text.as_bytes() != expected_bytes.as_slice() {
            let expected_text = String::from_utf8_lossy(&expected_bytes).into_owned();
            return Err(format!(
                "{case_id}: write {path} callback bytes differ\n{}\n--- expected ---\n{expected_text}--- actual ---\n{callback_text}",
                first_divergence(&expected_text, callback_text),
            ));
        }
        let expected_materialized = expected["materialized_utf8_sha256"]
            .as_str()
            .expect("materialized sha");
        if sha256_hex(bytes) != expected_materialized {
            return Err(format!("{case_id}: write {path} materialized bytes differ"));
        }
        if *bom
            != expected["write_byte_order_mark"]
                .as_bool()
                .expect("BOM flag")
        {
            return Err(format!("{case_id}: write {path} BOM flag differs"));
        }
        emitted_texts.insert(expected_path, callback_text.as_str());
    }
    for marker in observation["marker_occurrences"]
        .as_array()
        .expect("marker occurrences")
    {
        let token = marker["token"].as_str().expect("marker token");
        let expected_count = marker["occurrences"].as_u64().expect("marker count");
        let actual_count = emitted_texts
            .values()
            .map(|text| count_occurrences(text, token))
            .sum::<u64>();
        if actual_count != expected_count {
            return Err(format!(
                "{case_id}: marker {token:?} occurs {actual_count} times, oracle observed {expected_count}"
            ));
        }
    }
    Ok(())
}

#[test]
fn every_frozen_witness_case_reproduces_the_oracle_observation_twice() {
    let artifact: Value = serde_json::from_slice(WITNESSES).expect("witness artifact JSON");
    let families = artifact["families"].as_array().expect("families");
    assert_eq!(families.len(), 9, "family census changed");
    let library_files = vendored_library_files();
    assert_eq!(
        artifact["typescript"]["lib"]["default_libraries"]
            .as_u64()
            .expect("artifact default-library census") as usize,
        library_files.len(),
        "vendored default-library census changed"
    );
    let mut cases_run = 0_usize;
    let mut role_census: BTreeMap<&str, usize> = BTreeMap::new();
    let mut failures: Vec<String> = Vec::new();
    let mut known_diverging: Vec<String> = Vec::new();
    for family in families {
        for case in family["cases"].as_array().expect("cases") {
            let case_id = case["case_id"].as_str().expect("case id");
            *role_census
                .entry(case["role"].as_str().expect("role"))
                .or_default() += 1;
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                drive_case(case_id, case, &library_files)
                    .and_then(|driven| verify_case(case_id, case, &driven))
            }))
            .unwrap_or_else(|panic| {
                let message = panic
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| panic.downcast_ref::<&str>().copied())
                    .unwrap_or("non-string panic payload");
                Err(format!("{case_id}: panicked: {message}"))
            });
            if let Err(failure) = outcome {
                if KNOWN_DIVERGENCES.contains(&case_id) {
                    known_diverging.push(case_id.to_owned());
                } else {
                    failures.push(failure);
                }
            }
            cases_run += 1;
        }
    }
    assert_eq!(cases_run, 32, "case census changed");
    assert_eq!(
        role_census.get("positive"),
        Some(&10),
        "positive census changed"
    );
    assert_eq!(
        role_census.get("adjacent-negative-control"),
        Some(&9),
        "adjacent-negative census changed"
    );
    assert_eq!(
        role_census.get("composition"),
        Some(&7),
        "composition census changed"
    );
    assert_eq!(role_census.get("fault"), Some(&6), "fault census changed");
    assert!(
        failures.is_empty(),
        "{} NEW frozen-case divergence(s) beyond the known set:\n\n{}",
        failures.len(),
        failures.join("\n=====\n"),
    );
    // Shrink-only: a known divergence that starts passing must leave the
    // list in the same change that fixes it.
    assert_eq!(
        known_diverging.len(),
        KNOWN_DIVERGENCES.len(),
        "known-divergence list is stale; now passing: {:?}",
        KNOWN_DIVERGENCES
            .iter()
            .filter(|id| !known_diverging.contains(&(**id).to_owned()))
            .collect::<Vec<_>>(),
    );
}

#[test]
fn witness_artifact_identity_is_the_frozen_authority() {
    let artifact: Value = serde_json::from_slice(WITNESSES).expect("witness artifact JSON");
    assert_eq!(artifact["schema"], 1);
    assert_eq!(artifact["slice_id"], "H2.5h-a");
    assert_eq!(artifact["summary"]["families"], 9);
    assert_eq!(artifact["summary"]["cases"], 32);
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ratchets/h2-5h-a-es2015-generators-witnesses.v1.json");
    let on_disk = fs::read(path).expect("witness artifact on disk");
    assert_eq!(
        sha256_hex(&on_disk),
        sha256_hex(WITNESSES),
        "compiled-in witness bytes drifted from the repository artifact"
    );
}

//! H2.6a / m-2 replay gate (packet §8): every parity-lane W-H2.6A witness
//! map reproduces byte-for-byte through the PRODUCTION pipeline — plan →
//! checker resolver → transforms → print with a `SourceMapRecording`
//! injected — via the §8-A.1 harness-print bridge. The frozen oracle
//! bytes are the entire expectation; the reconciliation rule for the
//! `.js` compare is the upstream URL-append (h2-6a.md §4.4), assembled
//! test-locally and never substituted inside printed bytes.

use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tsc_compiler::ProgramSession;
use tsc_emitter::SourceMapRecordingInputs;
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_emitting_program, CompilerOptions, LibraryCatalog, ProgramLoadLimits, ProgramOptions,
};

const WITNESSES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h2-6a-witnesses.v1.json"
));

const LIBRARY_HOST_DIRECTORY: &str = "/typescript/lib";

/// First-run divergences owned by this train as production fixes; the
/// list may only SHRINK (the es2015 witness-gate idiom), and any NEW
/// divergence fails the suite immediately.
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

/// Fail-closed option mapping over exactly the witness floor plus the
/// per-family overrides (packet §8.1's named extension set); an
/// unexpected key is a hard failure, never a silent default.
fn case_compiler_options(serialized: &Value) -> CompilerOptions {
    let map = serialized.as_object().expect("serialized options object");
    let mut options = CompilerOptions::default();
    for (key, value) in map {
        match key.as_str() {
            "target" => options.target = Some(value.as_i64().expect("target") as i32),
            "module" => options.module = Some(value.as_i64().expect("module") as i32),
            "newLine" => options.new_line = Some(value.as_i64().expect("newLine") as i32),
            "ignoreDeprecations" => {
                options.ignore_deprecations =
                    Some(value.as_str().expect("ignoreDeprecations").to_owned());
            }
            "sourceMap" => options.source_map = Some(value.as_bool().expect("sourceMap")),
            "removeComments" => {
                options.remove_comments = Some(value.as_bool().expect("removeComments"));
            }
            "outDir" => options.out_dir = Some(value.as_str().expect("outDir").to_owned()),
            "resolveJsonModule" => {
                options.resolve_json_module = Some(value.as_bool().expect("resolveJsonModule"));
            }
            "esModuleInterop" => {
                options.es_module_interop = Some(value.as_bool().expect("esModuleInterop"));
            }
            "listEmittedFiles" => {
                options.list_emitted_files = Some(value.as_bool().expect("listEmittedFiles"));
            }
            "emitBOM" => options.emit_bom = Some(value.as_bool().expect("emitBOM")),
            "noEmitOnError" => {
                options.no_emit_on_error = Some(value.as_bool().expect("noEmitOnError"));
            }
            "downlevelIteration" => {
                options.downlevel_iteration = Some(value.as_bool().expect("downlevelIteration"));
            }
            other => panic!("unexpected stored compiler option {other}"),
        }
    }
    options
}

fn write_entry<'a>(observation: &'a Value, suffix: &str) -> Option<&'a Value> {
    observation["writes"]
        .as_array()
        .expect("writes array")
        .iter()
        .find(|write| {
            write["path"]
                .as_str()
                .expect("write path")
                .ends_with(suffix)
        })
}

fn callback_text(write: &Value) -> String {
    String::from_utf8(decode_base64(
        write["callback_utf8_base64"]
            .as_str()
            .expect("write base64"),
    ))
    .expect("write UTF-8")
}

/// The test-local upstream URL-append rule (h2-6a.md §4.4): one newLine
/// when the printed text does not end at line start, then
/// `//# sourceMappingURL=` + the encodeURI'd map basename. The witness
/// set's only URI-escapable byte is the space.
fn expected_js_with_url(
    printed: &str,
    ends_at_line_start: bool,
    new_line: &str,
    map_basename: &str,
) -> String {
    let mut expected = printed.to_owned();
    if !ends_at_line_start {
        expected.push_str(new_line);
    }
    expected.push_str("//# sourceMappingURL=");
    expected.push_str(&map_basename.replace(' ', "%20"));
    expected
}

struct ReplayOutcome {
    js: String,
    map_json: String,
}

fn drive_case(case_id: &str, case: &Value, library_files: &[(String, Vec<u8>)]) -> ReplayOutcome {
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
    .unwrap_or_else(|error| panic!("{case_id}: program load failed: {error:?}"));

    let observation = &case["observation"];
    let js_path = write_entry(observation, ".js").expect("js write")["path"]
        .as_str()
        .expect("js path")
        .to_owned();
    let target_js = js_path.clone();
    let recording_target = PathBuf::from(&target_js);
    let file = target_js
        .rsplit('/')
        .next()
        .expect("js basename")
        .to_owned();
    let directory = target_js[..target_js.rfind('/').expect("js directory")].to_owned();

    let selector = move |unit_path: &Path| -> Option<SourceMapRecordingInputs> {
        (unit_path == recording_target).then(|| SourceMapRecordingInputs {
            file: file.clone().into(),
            source_root: "".into(),
            sources_directory_path: directory.clone().into(),
            current_directory: "/project".into(),
            use_case_sensitive_source_keys: true,
        })
    };
    let printed_units = ProgramSession::new(prepared)
        .print_units_with_source_map_recording_for_harness(&selector)
        .unwrap_or_else(|error| panic!("{case_id}: harness print failed: {error:?}"));
    let (_, printed) = printed_units
        .into_iter()
        .find(|(path, _)| path == Path::new(&js_path))
        .unwrap_or_else(|| panic!("{case_id}: no printed unit for {js_path}"));

    let ends_at_line_start = printed.end().column() == 0;
    let new_line = match case["input"]["compiler_options"]["newLine"].as_i64() {
        Some(0) => "\r\n",
        _ => "\n",
    };
    let map_basename = format!(
        "{}.map",
        js_path.rsplit('/').next().expect("js basename for url")
    );
    let js = expected_js_with_url(printed.text(), ends_at_line_start, new_line, &map_basename);
    let mut generator = printed
        .into_source_map()
        .unwrap_or_else(|| panic!("{case_id}: no source map recorded"));
    let map_json = generator.to_json_string();
    ReplayOutcome { js, map_json }
}

#[test]
fn every_parity_witness_map_reproduces_through_the_production_pipeline() {
    let artifact: Value = serde_json::from_slice(WITNESSES).expect("witness artifact JSON");
    let library_files = vendored_library_files();
    let mut replayed = 0usize;
    let mut divergences: Vec<String> = Vec::new();
    for family in artifact["families"].as_array().expect("families") {
        for case in family["cases"].as_array().expect("cases") {
            let case_id = case["case_id"].as_str().expect("case id");
            let lane = case["rust_lane"].as_str().expect("rust lane");
            if let Ok(only) = std::env::var("M2_CASE") {
                if case_id != only {
                    continue;
                }
            }
            let observation = &case["observation"];
            let has_map = write_entry(observation, ".js.map").is_some();
            if lane != "parity" || !has_map {
                continue;
            }
            if case_id == "edge-shapes--fault-parse-error" {
                // §8-A: files carrying parse diagnostics are typed-deferred
                // to H2.9 by the production transform path (the 5g/5h band
                // treatment); the frozen oracle bytes are that lane's
                // inheritance, not an m-2 replay target.
                continue;
            }
            let frozen_js = callback_text(write_entry(observation, ".js").expect("js write"));
            let frozen_map = callback_text(write_entry(observation, ".js.map").expect("map write"));
            let first = drive_case(case_id, case, &library_files);
            let second = drive_case(case_id, case, &library_files);
            assert_eq!(
                (&first.js, &first.map_json),
                (&second.js, &second.map_json),
                "{case_id}: repeated replay is not deterministic"
            );
            replayed += 1;
            let mut problems = Vec::new();
            if first.js != frozen_js {
                problems.push(format!(
                    "js diverges at byte {}",
                    first
                        .js
                        .bytes()
                        .zip(frozen_js.bytes())
                        .position(|(left, right)| left != right)
                        .unwrap_or_else(|| first.js.len().min(frozen_js.len()))
                ));
            }
            if first.map_json != frozen_map {
                problems.push(format!(
                    "map diverges at byte {}",
                    first
                        .map_json
                        .bytes()
                        .zip(frozen_map.bytes())
                        .position(|(left, right)| left != right)
                        .unwrap_or_else(|| first.map_json.len().min(frozen_map.len()))
                ));
            }
            if !problems.is_empty() {
                if std::env::var("M2_DUMP").is_ok() {
                    let slug = case_id.replace("--", "__");
                    std::fs::write(format!("/tmp/m2-{slug}.produced.json"), &first.map_json)
                        .expect("dump produced map");
                    std::fs::write(format!("/tmp/m2-{slug}.frozen.json"), &frozen_map)
                        .expect("dump frozen map");
                }
                if std::env::var("M2_DEBUG").is_ok() {
                    eprintln!("== {case_id}");
                    eprintln!("frozen js == produced js: {}", first.js == frozen_js);
                    if first.map_json != frozen_map {
                        let at = first
                            .map_json
                            .bytes()
                            .zip(frozen_map.bytes())
                            .position(|(left, right)| left != right)
                            .unwrap_or_else(|| first.map_json.len().min(frozen_map.len()));
                        let lo = at.saturating_sub(60);
                        eprintln!(
                            "frozen  : …{}…",
                            &frozen_map[lo..(at + 60).min(frozen_map.len())]
                        );
                        eprintln!(
                            "produced: …{}…",
                            &first.map_json[lo..(at + 60).min(first.map_json.len())]
                        );
                    }
                }
                divergences.push(format!("{case_id}: {}", problems.join("; ")));
            }
        }
    }
    if std::env::var("M2_CASE").is_err() {
        assert_eq!(
            replayed, 23,
            "parity replay census changed (parse-error is H2.9-deferred)"
        );
    }
    let known: Vec<&str> = KNOWN_DIVERGENCES.to_vec();
    let diverging_ids: Vec<&str> = divergences
        .iter()
        .map(|entry| entry.split(':').next().expect("case id prefix"))
        .collect();
    for divergence in &divergences {
        assert!(
            known.iter().any(|id| divergence.starts_with(id)),
            "NEW divergence: {divergence}"
        );
    }
    for id in &known {
        assert!(
            diverging_ids.contains(id),
            "stale KNOWN_DIVERGENCES entry: {id} now reproduces (shrink the list)"
        );
    }
}

#[test]
fn mapless_controls_print_identically_without_recording() {
    let artifact: Value = serde_json::from_slice(WITNESSES).expect("witness artifact JSON");
    let library_files = vendored_library_files();
    for case_id in [
        "plain-program--adjacent-negative-nomap",
        "edge-shapes--adjacent-negative-shebang-nomap",
    ] {
        let case = artifact["families"]
            .as_array()
            .expect("families")
            .iter()
            .flat_map(|family| family["cases"].as_array().expect("cases"))
            .find(|case| case["case_id"] == case_id)
            .unwrap_or_else(|| panic!("missing control case {case_id}"));
        let input = &case["input"];
        let mut builder = MemoryCompilerHost::builder("/project");
        for file in input["files"].as_array().expect("input files") {
            builder = builder.file(
                file["path"].as_str().expect("path"),
                decode_base64(file["utf8_base64"].as_str().expect("base64")),
            );
        }
        for (name, bytes) in &library_files {
            builder = builder.file(format!("{LIBRARY_HOST_DIRECTORY}/{name}"), bytes.clone());
        }
        let host = builder.build().expect("host");
        let catalog = LibraryCatalog::typescript_6_0_3(LIBRARY_HOST_DIRECTORY);
        let roots = input["roots"]
            .as_array()
            .expect("roots")
            .iter()
            .map(|root| PathBuf::from(root.as_str().expect("root")))
            .collect::<Vec<_>>();
        let prepared = load_emitting_program(
            &host,
            &roots,
            case_compiler_options(&input["compiler_options"]),
            ProgramOptions::default(),
            &catalog,
            ProgramLoadLimits::new(256, 2_048, 64, 16 * 1_024 * 1_024, 128 * 1_024 * 1_024),
        )
        .unwrap_or_else(|error| panic!("{case_id}: load failed: {error:?}"));
        let printed_units = ProgramSession::new(prepared)
            .print_units_with_source_map_recording_for_harness(&|_| None)
            .unwrap_or_else(|error| panic!("{case_id}: print failed: {error:?}"));
        let frozen_js = callback_text(write_entry(&case["observation"], ".js").expect("js write"));
        let (_, printed) = printed_units
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("{case_id}: no unit"));
        assert!(printed.source_map().is_none(), "{case_id}: unexpected map");
        assert_eq!(printed.text(), frozen_js, "{case_id}: mapless bytes");
    }
}

#[test]
fn parse_error_witness_is_typed_deferred_to_h2_9() {
    let artifact: Value = serde_json::from_slice(WITNESSES).expect("witness artifact JSON");
    let library_files = vendored_library_files();
    let case = artifact["families"]
        .as_array()
        .expect("families")
        .iter()
        .flat_map(|family| family["cases"].as_array().expect("cases"))
        .find(|case| case["case_id"] == "edge-shapes--fault-parse-error")
        .expect("parse-error case");
    let input = &case["input"];
    let mut builder = MemoryCompilerHost::builder("/project");
    for file in input["files"].as_array().expect("input files") {
        builder = builder.file(
            file["path"].as_str().expect("path"),
            decode_base64(file["utf8_base64"].as_str().expect("base64")),
        );
    }
    for (name, bytes) in &library_files {
        builder = builder.file(format!("{LIBRARY_HOST_DIRECTORY}/{name}"), bytes.clone());
    }
    let host = builder.build().expect("host");
    let catalog = LibraryCatalog::typescript_6_0_3(LIBRARY_HOST_DIRECTORY);
    let roots = input["roots"]
        .as_array()
        .expect("roots")
        .iter()
        .map(|root| PathBuf::from(root.as_str().expect("root")))
        .collect::<Vec<_>>();
    let prepared = load_emitting_program(
        &host,
        &roots,
        case_compiler_options(&input["compiler_options"]),
        ProgramOptions::default(),
        &catalog,
        ProgramLoadLimits::new(256, 2_048, 64, 16 * 1_024 * 1_024, 128 * 1_024 * 1_024),
    )
    .expect("load");
    let error = ProgramSession::new(prepared)
        .print_units_with_source_map_recording_for_harness(&|_| None)
        .expect_err("parse-diagnostic files are typed-deferred");
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("ParseDiagnosticsDeferred") && rendered.contains("H2.9"),
        "unexpected deferral shape: {rendered}"
    );
}

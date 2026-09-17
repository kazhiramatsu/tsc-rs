//! H2.8a-A-RES-BUNDLE-METADATA-T1: adjacent controls for the parse-node
//! `commentRange` a System/AMD bundle's JavaScript transform leaves on parsed
//! nodes (the class-fields private receiver, `_tsc.js:96407` / `96808`) and
//! that the same command's declaration transform must still see (a Bundle
//! root never disposes its annotated parse nodes, `_tsc.js:25302-25310`).
//!
//! Two tests over `fixtures/bundle-metadata-t1.json`
//! (`scripts/observe-bundle-metadata-t1.mjs`, inputs from
//! `scripts/generate-bundle-metadata-t1-inputs.mjs`):
//!
//! * every complete command (writes / diagnostics / emit result / status /
//!   exit) replayed twice with the shared exact comparator;
//! * for every ordinary bundle command that emits JavaScript and
//!   declarations, the parsed-metadata packet snapshotted after the
//!   JavaScript print and restored into a declaration arena mounted in the
//!   production order, projected node by node against the upstream
//!   `after_javascript` emitNode probe (flags, internal flags, comment-range
//!   endpoints in UTF-16, erased type node kind, constant presence).
//!
//! Independent integration target: `h2_7b_w4a_controls` is included only for
//! its exact complete-command comparator. Selection:
//! `TSC_RS_BUNDLE_METADATA_T1_CASE_SET` = `all` (default) or a comma-separated
//! list of case-id substrings.
#[path = "integration/h2_7b_w4a_controls.rs"]
#[allow(dead_code)]
mod h2_7b_w4a_controls;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde_json::{json, Value};
use sha2::Digest;
use tsc_diagnostics::JsString;

#[path = "../../program/tests/support/scalar_json.rs"]
mod utf16_scalar_json;
#[path = "support/witness_libraries.rs"]
mod witness_libraries;
use tsc_emitter::{
    create_printer, get_script_transformers_for_source, preflight_emit, transform_nodes,
    EmitArtifact, EmitHost, EmitIoError, EmitResolver, EmitRoot, EmitSelection,
    EmitWriteDisposition, NewLineKind, OutputSink, PrintRequest, PrinterOptions,
    SourceFileTextMode, TransformArena, TransformBundle, TransformNode, TransformRoot,
    TransformSourceId,
};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_emitting_program, CompilerOptions, LibraryCatalog, ProgramLoadLimits, ProgramOptions,
    SourceFileId,
};
use utf16_scalar_json::observe as scalar_json;

const EXPECTED_CASES: usize = 18;
const SELECTION_ENV: &str = "TSC_RS_BUNDLE_METADATA_T1_CASE_SET";
/// Set to a directory to write every native write of a replayed command
/// (`<sha256(case_id)>-<pass>-<index>-<basename>`) and, for every row that
/// differs from its frozen expectation, the native projection a known entry
/// is frozen from (`known-native-<key>.json` / `known-packet-<key>.json`).
/// Never inherited by the registered runner.
const DUMP_ENV: &str = "TSC_RS_BUNDLE_METADATA_T1_DUMP_DIR";
/// Complete-command rows that still differ from upstream for a reason outside
/// this slice, frozen with their owner (the R12 family: a System bundle's
/// exported class end-of-statement map). Each row's native projection (exit,
/// status writes, diagnostics, SHA-256 of every write) is asserted on both
/// passes and counted as `known`, never exact; a row that becomes exact fails
/// until it is retired here.
const KNOWN_NATIVE: &str = "bundle-metadata-t1-known-native.json";
/// Parse-node packet rows that still differ from the upstream probe for a
/// reason outside this slice (a Rust-native comment boundary flag on a lowered
/// private assignment's right operand, and the decorator expression's
/// upstream `NoComments` flag). Each entry freezes the exact symmetric
/// difference; anything else fails.
const KNOWN_PACKET: &str = "bundle-metadata-t1-known-packet.json";
const INPUTS: &str = "bundle-metadata-t1-inputs.json";
const OBSERVATIONS: &str = "bundle-metadata-t1.json";

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    let path = workspace()
        .join("crates/compiler/tests/fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", sha2::Sha256::digest(bytes))
}

struct RecordingSink {
    writes: Vec<EmitArtifact>,
}

impl OutputSink for RecordingSink {
    fn write(&mut self, artifact: EmitArtifact) -> Result<EmitWriteDisposition, EmitIoError> {
        self.writes.push(artifact);
        Ok(EmitWriteDisposition::Written)
    }
}

/// The frozen artifact, validated against its input manifest.
fn load_all_cases() -> Vec<Value> {
    let inputs_bytes = fixture_bytes(INPUTS);
    let inputs: Value = serde_json::from_slice(&inputs_bytes).unwrap();
    let artifact: Value = serde_json::from_slice(&fixture_bytes(OBSERVATIONS)).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    assert!(
        artifact["selection"].is_null(),
        "the frozen fixture is a complete observation, not a scratch selection"
    );
    assert_eq!(
        artifact["inputs"]["sha256"].as_str().unwrap(),
        sha256_hex(&inputs_bytes),
        "observations were taken from this exact input manifest"
    );
    let input_ids = inputs["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| case["case_id"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(input_ids.len(), EXPECTED_CASES);
    let cases = artifact["cases"].as_array().unwrap().clone();
    assert_eq!(
        cases
            .iter()
            .map(|case| case["case_id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>(),
        input_ids
    );
    for case in &cases {
        assert_eq!(
            case["typescript_observation"]["outcome"],
            Value::Null,
            "{}: complete upstream observation",
            case["case_id"]
        );
    }
    cases
}

/// The optional substring selection applied to the complete case list.
fn select_cases(mut cases: Vec<Value>) -> Vec<Value> {
    match std::env::var(SELECTION_ENV).as_deref() {
        Err(std::env::VarError::NotPresent) | Ok("all") => {}
        Ok(selection) => {
            let needles = selection.split(',').map(str::trim).collect::<Vec<_>>();
            assert!(needles.iter().all(|needle| !needle.is_empty()));
            cases.retain(|case| {
                let id = case["case_id"].as_str().unwrap();
                needles.iter().any(|needle| id.contains(needle))
            });
            eprintln!(
                "bundle metadata t1 SELECTION {selection:?} -> {} cases",
                cases.len()
            );
            assert!(!cases.is_empty(), "selection matched no case");
        }
        Err(error) => panic!("invalid case selection: {error}"),
    }
    cases
}

/// Frozen known rows keyed by case id; every key must be an observed case.
fn load_known(name: &str, route: &str, cases: &[Value]) -> BTreeMap<String, Value> {
    let known: Value = serde_json::from_slice(&fixture_bytes(name)).unwrap();
    assert_eq!(known["route"], route, "{name}: route");
    let rows = known["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| (row["case_id"].as_str().unwrap().to_owned(), row.clone()))
        .collect::<BTreeMap<_, _>>();
    let observed = cases
        .iter()
        .map(|case| case["case_id"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    for id in rows.keys() {
        assert!(
            observed.contains(id.as_str()),
            "{name}: {id} is not an observed case"
        );
    }
    rows
}

#[test]
fn bundle_metadata_t1_controls_match_complete_typescript_observations() {
    let all = load_all_cases();
    let known_native = load_known(KNOWN_NATIVE, "native-divergence-controls", &all);
    let cases = select_cases(all);
    let mut failures = Vec::new();
    let mut exact = 0usize;
    let mut known = 0usize;
    for case in &cases {
        let case_id = case["case_id"].as_str().unwrap();
        if let Some(record) = known_native.get(case_id) {
            let run = || {
                replay_known_case(case, record, 1);
                replay_known_case(case, record, 2);
            };
            if std::panic::catch_unwind(run).is_ok() {
                known += 1;
                eprintln!("bundle metadata t1 KNOWN x2 {case_id}");
            } else {
                failures.push(case_id);
                eprintln!("bundle metadata t1 KNOWN DIVERGENCE CHANGED {case_id}");
            }
            continue;
        }
        let run = || {
            // One complete Program and command comparison per pass, two
            // independent passes for each row.
            replay_case(case, 1);
            replay_case(case, 2);
        };
        if std::panic::catch_unwind(run).is_ok() {
            exact += 1;
            eprintln!("bundle metadata t1 EXACT x2 {case_id}");
        } else {
            assert!(
                std::panic::catch_unwind(run).is_err(),
                "{case_id}: comparison failed and then passed"
            );
            failures.push(case_id);
            eprintln!("bundle metadata t1 REPEATED FAILURE {case_id}");
        }
    }
    eprintln!(
        "bundle metadata t1 SUMMARY exact={exact} known={known} failed={} selected={}",
        failures.len(),
        cases.len()
    );
    assert!(
        failures.is_empty(),
        "complete command failures ({}): {failures:?}",
        failures.len()
    );
}

/// A frozen native divergence: the native projection must equal the record
/// on this pass and must still differ from the upstream observation.
fn replay_known_case(case: &Value, record: &Value, pass: usize) {
    let case_id = case["case_id"].as_str().unwrap();
    let expected = &case["typescript_observation"];
    let prepared = prepare_program(case);
    let mut sink = RecordingSink { writes: Vec::new() };
    let command = tsc_compiler::ProgramSession::new(prepared)
        .emit_command_for_harness(&mut sink)
        .unwrap_or_else(|error| panic!("{case_id}: production command completes: {error}"));
    dump_writes(case_id, pass, &sink.writes);
    let projection = completed_projection(&command, &sink.writes);
    assert!(
        !completed_projection_is_exact(&projection, expected),
        "{case_id}: the recorded native divergence is exact now; retire it from {KNOWN_NATIVE}"
    );
    assert_eq!(
        projection, record["native"],
        "{case_id} pass {pass}: the frozen native divergence changed ({})",
        record["owner"]
    );
}

/// The native outcome of a completed command: exit code, status writes,
/// diagnostic codes and the SHA-256 of every write's callback bytes.
fn completed_projection(
    command: &tsc_compiler::EmitCommandOutcome,
    writes: &[EmitArtifact],
) -> Value {
    json!({
        "outcome": "completed",
        "exit_code": command.exit_code(),
        "emit_refused": command.emit().emit_skipped(),
        "status_writes": scalar_json(command.status_writes()),
        "diagnostics": command.diagnostics().iter().map(|d| json!({"code": d.code(), "file": scalar_json(&d.file_name), "start": d.start})).collect::<Vec<_>>(),
        "writes": writes.iter().map(|artifact| json!({
            "path": artifact.path().to_string_lossy(),
            "sha256": sha256_hex(artifact.callback_bytes()),
            "bytes": artifact.callback_bytes().len(),
        })).collect::<Vec<_>>(),
    })
}

/// Whether a completed projection equals the upstream observation on every
/// compared surface (write bytes and paths, exit code, diagnostics).
fn completed_projection_is_exact(projection: &Value, expected: &Value) -> bool {
    let expected_writes = expected["writes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|write| {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(write["callback_utf8_base64"].as_str().unwrap())
                .unwrap();
            (
                write["path"].as_str().unwrap().to_owned(),
                sha256_hex(&bytes),
            )
        })
        .collect::<std::collections::BTreeSet<_>>();
    let actual_writes = projection["writes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|write| {
            (
                write["path"].as_str().unwrap().to_owned(),
                write["sha256"].as_str().unwrap().to_owned(),
            )
        })
        .collect::<std::collections::BTreeSet<_>>();
    let expected_diagnostics = expected["reported_diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| (d["code"].clone(), d["file"].clone(), d["start"].clone()))
        .collect::<Vec<_>>();
    let actual_diagnostics = projection["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| (d["code"].clone(), d["file"].clone(), d["start"].clone()))
        .collect::<Vec<_>>();
    expected_writes == actual_writes
        && expected["exit_code"] == projection["exit_code"]
        && expected_diagnostics == actual_diagnostics
}

fn dump_json(name: &str, value: &Value) {
    let Some(directory) = std::env::var_os(DUMP_ENV) else {
        return;
    };
    let directory = PathBuf::from(directory);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join(name),
        serde_json::to_vec_pretty(value).unwrap(),
    )
    .unwrap();
}

fn replay_case(case: &Value, pass: usize) {
    let case_id = case["case_id"].as_str().unwrap();
    let expected = &case["typescript_observation"];
    let prepared = prepare_program(case);
    let mut sink = RecordingSink { writes: Vec::new() };
    let command = tsc_compiler::ProgramSession::new(prepared)
        .emit_command_for_harness(&mut sink)
        .unwrap_or_else(|error| panic!("{case_id}: production command completes: {error}"));
    let outcome = command.emit().clone();
    dump_writes(case_id, pass, &sink.writes);
    let projection = completed_projection(&command, &sink.writes);
    if !completed_projection_is_exact(&projection, expected) {
        dump_json(
            &format!(
                "known-native-{}.json",
                &sha256_hex(case_id.as_bytes())[..16]
            ),
            &json!({"case_id": case_id, "native": projection}),
        );
    }
    h2_7b_w4a_controls::assert_completed_observation(
        case_id,
        &outcome,
        command.diagnostics(),
        Some((
            command
                .status_writes()
                .iter()
                .map(|text| text.as_str().expect("scalar fixture status").to_owned())
                .collect(),
            command.exit_code(),
        )),
        &sink.writes,
        expected,
        true,
    );
}

fn dump_writes(case_id: &str, pass: usize, writes: &[EmitArtifact]) {
    let Some(directory) = std::env::var_os(DUMP_ENV) else {
        return;
    };
    let directory = PathBuf::from(directory);
    std::fs::create_dir_all(&directory).unwrap();
    let key = &sha256_hex(case_id.as_bytes())[..16];
    for (index, artifact) in writes.iter().enumerate() {
        let path = artifact.path().to_string_lossy().into_owned();
        let name = path.rsplit('/').next().unwrap().to_owned();
        std::fs::write(
            directory.join(format!("{key}-{pass}-{index}-{name}")),
            artifact.callback_bytes(),
        )
        .unwrap();
    }
    std::fs::write(directory.join(format!("{key}.case_id")), case_id).unwrap();
}

/// Ordinary bundle commands that print JavaScript before their declaration
/// transform: the only route that snapshots parse-node metadata
/// (`crates/emitter/src/execute.rs`, `TransformRoot::Bundle` with a
/// declaration path).
fn carries_packet(case: &Value) -> bool {
    let options = &case["options"];
    options["outFile"].is_string()
        && options["declaration"] == true
        && options["emitDeclarationOnly"] != true
        && case["probe"]["after_javascript"].is_array()
}

#[test]
fn bundle_metadata_t1_parsed_packet_matches_typescript_after_javascript_probe() {
    let all = load_all_cases();
    let known_packet = load_known(KNOWN_PACKET, "packet-divergence-controls", &all);
    let cases = select_cases(all);
    let probed = cases
        .iter()
        .filter(|case| carries_packet(case))
        .collect::<Vec<_>>();
    assert!(
        !probed.is_empty(),
        "the selection has no packet-carrying bundle"
    );
    let mut failures = Vec::new();
    let mut known = 0usize;
    for case in &probed {
        let case_id = case["case_id"].as_str().unwrap();
        let record = known_packet.get(case_id);
        for repetition in 0..2 {
            let outcome = std::panic::catch_unwind(|| {
                let prepared = prepare_program(case);
                let (observed, checked) = tsc_compiler::ProgramSession::new(prepared)
                    .with_checked_emit_resolver_for_harness(|host, resolver, checked| {
                        assert!(checked.partial_checks.is_empty());
                        compare_packet(case, record, host, resolver);
                        Ok(())
                    })
                    .unwrap();
                assert!(observed.is_some());
                assert!(checked.partial_checks.is_empty());
            });
            if outcome.is_err() {
                failures.push(format!("{case_id} #{repetition}"));
            }
        }
        if !failures.iter().any(|failure| failure.starts_with(case_id)) {
            if record.is_some() {
                known += 1;
                eprintln!("bundle metadata t1 PACKET KNOWN x2 {case_id}");
            } else {
                eprintln!("bundle metadata t1 PACKET x2 {case_id}");
            }
        }
    }
    let failed = failures
        .iter()
        .map(|f| f.rsplit_once(" #").unwrap().0)
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    eprintln!(
        "bundle metadata t1 PACKET SUMMARY exact={} known={known} failed={failed} probed={}",
        probed.len() - known - failed,
        probed.len()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Mirror the production lifetime (`execute.rs` JavaScript unit, then
/// `declarations/orchestration.rs::emit_declaration_unit`): mount the
/// bundle's root sources, transform and print the JavaScript, snapshot the
/// parse-node packet, dispose, mount the declaration arena (non-JSON bundle
/// sources first, then every other Program source), restore, and project.
fn compare_packet(
    case: &Value,
    known: Option<&Value>,
    host: &dyn EmitHost,
    resolver: &dyn EmitResolver,
) {
    let case_id = case["case_id"].as_str().unwrap();
    let options = host.compiler_options();
    let preflight = preflight_emit(host, EmitSelection::WholeProgram).unwrap();
    assert_eq!(
        preflight.plan().units().len(),
        1,
        "{case_id}: one bundle unit"
    );
    let unit = &preflight.plan().units()[0];
    let EmitRoot::Bundle(root) = unit.root() else {
        panic!("{case_id}: bundle unit")
    };
    assert!(unit.paths().javascript_path().is_some() && unit.paths().declaration_path().is_some());
    let syntax = |id: SourceFileId| host.source_file(id).unwrap().syntax().unwrap();

    let mut arena = TransformArena::new();
    let sources = root
        .source_files()
        .iter()
        .map(|&id| arena.add_source(syntax(id), Some(id)))
        .collect::<Vec<_>>();
    let first = root.source_files()[0];
    let transformers = get_script_transformers_for_source(options, resolver, host, first).unwrap();
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::Bundle(TransformBundle::new(sources))],
        transformers,
        false,
    )
    .unwrap();
    assert!(
        result.diagnostics().is_empty(),
        "{case_id}: JavaScript transform diagnostics"
    );
    let TransformRoot::Bundle(bundle) = result.roots()[0].clone() else {
        panic!("{case_id}: bundle result")
    };
    let new_line = if options.new_line == Some(0) {
        NewLineKind::CarriageReturnLineFeed
    } else {
        NewLineKind::LineFeed
    };
    let mut printer = create_printer(
        PrinterOptions::new(new_line)
            .with_target(options.emit_script_target())
            .with_module_kind(options.emit_module_kind())
            .with_remove_comments(options.remove_comments == Some(true))
            .with_no_emit_helpers(options.no_emit_helpers == Some(true))
            .with_import_helpers(options.import_helpers == Some(true))
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    );
    printer
        .print(&mut result, PrintRequest::Bundle(bundle), None)
        .unwrap();
    let snapshot = result.arena().snapshot_parsed_emit_metadata(host).unwrap();
    result.dispose();

    let files_for_emit = root
        .source_files()
        .iter()
        .copied()
        .filter(|&id| {
            !host
                .source_file(id)
                .unwrap()
                .path()
                .to_string_lossy()
                .to_ascii_lowercase()
                .ends_with(".json")
        })
        .collect::<Vec<_>>();
    let mut arena = TransformArena::new();
    let mut mounted: BTreeMap<SourceFileId, TransformSourceId> = BTreeMap::new();
    for &id in &files_for_emit {
        mounted.insert(id, arena.add_source(syntax(id), Some(id)));
    }
    for &other in host.source_file_ids() {
        if !files_for_emit.contains(&other) {
            if let Some(syntax) = host.source_file(other).and_then(|source| source.syntax()) {
                mounted.insert(other, arena.add_source(syntax, Some(other)));
            }
        }
    }
    let javascript_order = root
        .source_files()
        .iter()
        .map(|id| {
            host.source_file(*id)
                .unwrap()
                .path()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    let declaration_order = files_for_emit
        .iter()
        .map(|id| {
            host.source_file(*id)
                .unwrap()
                .path()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    eprintln!("bundle metadata t1 MOUNT {case_id} javascript={javascript_order:?} declaration={declaration_order:?}");
    arena.restore_parsed_emit_metadata(&snapshot, host).unwrap();

    let program_of = mounted
        .iter()
        .map(|(&program, &source)| (source, program))
        .collect::<BTreeMap<_, _>>();
    let own_files = case["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["path"].as_str().unwrap().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    let mut actual = Vec::new();
    for (&program, &source) in &mounted {
        let parsed = syntax(program);
        let file = scalar_json(&parsed.file_name);
        if !own_files.contains(file.as_str().unwrap()) {
            continue;
        }
        for (offset, record) in parsed.arena.nodes().iter().enumerate() {
            let node = TransformNode::new(
                source,
                tsc_syntax::NodeId(parsed.arena.node_base() + offset as u32),
            );
            let Some(metadata) = arena.metadata(node) else {
                continue;
            };
            let comment_range = metadata.comment_range().map(|range| {
                let positions = syntax(program_of[&range.source()]).positions();
                let utf16 = |position: Option<tsc_emitter::SourceBytePosition>| {
                    position.map_or(-1, |position| {
                        i64::from(positions.byte_to_utf16(position.value()).unwrap())
                    })
                };
                json!({ "pos": utf16(range.range().start()), "end": utf16(range.range().end()) })
            });
            actual.push(
                json!({ "file": file, "kind": format!("{:?}", record.kind),
                    "pos": parsed.positions().byte_to_utf16(record.pos).unwrap(),
                    "end": parsed.positions().byte_to_utf16(record.end).unwrap(),
                    "flags": metadata.flags().bits(), "internal_flags": metadata.internal_flags().bits(),
                    "comment_range": comment_range,
                    "type_node": metadata.type_node().map(|node| format!("{:?}", arena.node(node).unwrap().kind)),
                    "constant_value": metadata.constant_value().is_some() })
                .to_string(),
            );
        }
    }
    let calls = case["probe"]["after_javascript"].as_array().unwrap();
    assert!(!calls.is_empty(), "{case_id}: upstream after hook ran");
    for call in calls {
        assert_eq!(
            call, &calls[0],
            "{case_id}: identical per-source projections of a bundle"
        );
    }
    let mut expected = Vec::new();
    for row in calls[0].as_array().unwrap() {
        assert_eq!(
            row["other_keys"].as_array().unwrap().len(),
            0,
            "{case_id}: an upstream parse node carries a field outside the portable packet: {row}"
        );
        // A SourceFile's emitNode may exist only to list its annotated nodes.
        if row["flags"] == 0
            && row["internal_flags"] == 0
            && row["comment_range"].is_null()
            && row["type_node"].is_null()
            && row["constant_value"] == false
        {
            continue;
        }
        let comment_range = row["comment_range"]
            .as_object()
            .map(|range| json!({ "pos": range["pos"], "end": range["end"] }));
        expected.push(
            json!({ "file": row["file"], "kind": row["kind"], "pos": row["pos"], "end": row["end"],
                "flags": row["flags"], "internal_flags": row["internal_flags"],
                "comment_range": comment_range, "type_node": row["type_node"],
                "constant_value": row["constant_value"] })
            .to_string(),
        );
    }
    actual.sort();
    expected.sort();
    let actual_set = actual
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let expected_set = expected
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let rust_only = actual_set
        .difference(&expected_set)
        .cloned()
        .collect::<Vec<_>>();
    let upstream_only = expected_set
        .difference(&actual_set)
        .cloned()
        .collect::<Vec<_>>();
    let rows = |strings: &[String]| {
        strings
            .iter()
            .map(|row| serde_json::from_str::<Value>(row).unwrap())
            .collect::<Vec<_>>()
    };
    if !rust_only.is_empty() || !upstream_only.is_empty() {
        dump_json(
            &format!(
                "known-packet-{}.json",
                &sha256_hex(case_id.as_bytes())[..16]
            ),
            &json!({"case_id": case_id, "rust_only": rows(&rust_only), "upstream_only": rows(&upstream_only)}),
        );
    }
    match known {
        None => assert_eq!(actual, expected, "{case_id}: restored parse-node packet"),
        Some(record) => {
            assert!(
                !rust_only.is_empty() || !upstream_only.is_empty(),
                "{case_id}: the recorded packet divergence is exact now; retire it from {KNOWN_PACKET}"
            );
            assert_eq!(
                json!({"rust_only": rows(&rust_only), "upstream_only": rows(&upstream_only)}),
                json!({"rust_only": record["rust_only"], "upstream_only": record["upstream_only"]}),
                "{case_id}: the frozen packet divergence changed ({})",
                record["owner"]
            );
        }
    }
    // Comment ranges are this slice's subject: they must match exactly even
    // where a frozen flag divergence remains.
    let ranges = |strings: &[String]| {
        strings
            .iter()
            .filter(|row| !row.contains("\"comment_range\":null"))
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ranges(&actual),
        ranges(&expected),
        "{case_id}: restored parse-node comment ranges"
    );
    let carried = ranges(&expected).len();
    eprintln!(
        "bundle metadata t1 PACKET {case_id} rows={} comment_ranges={carried}",
        expected.len()
    );
}

/// Program preparation for the manifest's direct-option cases (the same host,
/// library catalog, limits and option projection as the decorator-binding
/// pipeline comparator, restricted to the option keys this manifest uses).
fn prepare_program(case: &Value) -> tsc_program::PreparedProgram {
    let mut builder = MemoryCompilerHost::builder("/project").case_sensitive(
        case["use_case_sensitive_file_names"]
            .as_bool()
            .unwrap_or(true),
    );
    let mut roots = Vec::new();
    for file in case["files"].as_array().expect("files") {
        let path = PathBuf::from(file["path"].as_str().unwrap());
        builder = builder.file(path.clone(), file["text"].as_str().unwrap().as_bytes());
        roots.push(path);
    }
    if let Some(explicit_roots) = case["roots"].as_array() {
        roots = explicit_roots
            .iter()
            .map(|root| PathBuf::from(root.as_str().unwrap()))
            .collect();
    }
    for (path, bytes) in witness_libraries::files() {
        builder = builder.file(path.as_str(), bytes.as_slice());
    }
    assert!(
        case["config"].is_null(),
        "manifest cases use direct options"
    );
    let host = builder.build().expect("memory host");
    let mut options = CompilerOptions::default();
    let program_options = ProgramOptions::default();
    for (key, value) in case["options"].as_object().expect("options") {
        match key.as_str() {
            "target" => options.target = Some(value.as_i64().unwrap() as i32),
            "module" => options.module = Some(value.as_i64().unwrap() as i32),
            "moduleResolution" => options.module_resolution = Some(value.as_i64().unwrap() as i32),
            "newLine" => options.new_line = Some(value.as_i64().unwrap() as i32),
            "declaration" => options.declaration = value.as_bool(),
            "declarationMap" => options.declaration_map = value.as_bool(),
            "emitDeclarationOnly" => options.emit_declaration_only = value.as_bool(),
            "resolveJsonModule" => options.resolve_json_module = value.as_bool(),
            "outDir" => options.out_dir = value.as_str().map(JsString::from),
            "outFile" => options.out_file = value.as_str().map(JsString::from),
            "ignoreDeprecations" => {
                options.ignore_deprecations = value.as_str().map(JsString::from);
            }
            "allowJs" => options.allow_js = value.as_bool().unwrap(),
            "checkJs" => options.check_js = value.as_bool(),
            "removeComments" => options.remove_comments = value.as_bool(),
            "strict" => options.strict = value.as_bool(),
            "useDefineForClassFields" => options.use_define_for_class_fields = value.as_bool(),
            "sourceMap" => options.source_map = value.as_bool(),
            "skipDefaultLibCheck" => options.skip_default_lib_check = value.as_bool(),
            "noErrorTruncation" => options.no_error_truncation = value.as_bool(),
            other => panic!("unexpected option {other}"),
        }
    }
    let catalog = LibraryCatalog::typescript_6_0_3("/lib");
    let limits = ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024);
    load_emitting_program(&host, &roots, options, program_options, &catalog, limits)
        .expect("direct program")
}

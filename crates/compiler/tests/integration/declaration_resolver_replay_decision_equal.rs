use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tsc_checker::state::CheckerState;
use tsc_checker::{check_program_with_libs_at, CompilerOptions, InputFile};
use tsc_harness::upstream_suites::execution::{
    load_compiler_no_emit, load_project_no_emit, load_qualified_compiler_emit,
    load_recorded_execution_plans, UpstreamExecutionCorpus, UpstreamExecutionInput,
    UpstreamExecutionPlan,
};
use tsc_harness::upstream_suites::h1_conformance::ConformanceExpansionManifest;
use tsc_harness::upstream_suites::{SourceEncoding, UnitContent};
use tsc_program::{PreparedProgram, ProgramLoadLimits};

const WITNESSES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h2-7a-witnesses.v1.json"
));
const PROBE_TRACES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h2-7a-probe-traces.v1.json"
));
const CONFORMANCE_EXPANSION: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/typescript-6.0.3/conformance-suite-expansion.v1.json"
));

// Frozen OBSERVATION identity (register disposition 7): the case
// manifest fingerprint and the observation/trace content rolls pin
// the frozen evidence itself and are byte-stable across oracle-chain
// assembly re-mints (whole-artifact fingerprints move whenever any
// upstream ratchet re-pins its embedded hashes, so they are recorded
// in the register, not asserted here).
const EXPECTED_MANIFEST_FINGERPRINT: &str =
    "89bb0627cee58b5d12aeb6fd5e95a92d26e1bbb54fd592750b49a34b64a89efb";
const EXPECTED_WITNESS_OBSERVATION_ROLL: &str =
    "091cea9c5dd7a2c60a551d9292cd19832f9b71c77a28508eeae7252ccf556312";
const EXPECTED_PROBE_TRACE_ROLL: &str =
    "dcf1243f5b8f3187631349657ab07c05a5c52270fd564d415687a1ad76bcb6d9";

// Artifact upper-envelope constants transcribed from the FINAL E4 register.
const EXPECTED_EVENT_VOLUMES: &[(&str, u64)] = &[
    ("isDeclarationVisible", 2_036),
    ("isLiteralConstDeclaration", 612),
    ("isExpandoFunctionDeclaration", 456),
    ("isSymbolAccessible", 407),
    ("isOptionalParameter", 344),
    ("isImplementationOfOverload", 192),
    ("isEntityNameVisible", 195),
    ("requiresAddingImplicitUndefined", 133),
    ("isImportRequiredByAugmentation", 15),
    ("isDefinitelyReferenceToGlobalSymbolObject", 10),
    ("getPropertiesOfContainerFunction", 5),
    ("isLateBound", 4),
    ("getEnumMemberValue", 3),
    ("collectLinkedAliases", 12),
];

// Schema-2 remeasurement of the four packet §7.4 diagnostic edge families.
const EXPECTED_NESTED_EDGES: &[(&str, u64)] = &[
    (
        "resolver.isDeclarationVisible -> resolver.isDeclarationVisible",
        165,
    ),
    (
        "resolver.isEntityNameVisible -> resolver.isDeclarationVisible",
        100,
    ),
    (
        "resolver.isSymbolAccessible -> resolver.isDeclarationVisible",
        405,
    ),
    (
        "resolver.requiresAddingImplicitUndefined -> resolver.isOptionalParameter",
        244,
    ),
];

const EXPECTED_EXCLUDED_CAUSALITY_COUNTS: &[(&str, u64)] = &[
    ("root-result", 5),
    ("per-root-paint-set", 0),
    ("seed-entry", 4),
];

// Filled from the FINAL artifacts by this P4 harness and frozen here under the
// h2-7a-m-2 E4 register's "Harness constants for P4 transcription" row.
// Order: replayed, lib-target, synthetic-without-original, ambiguous-symbol,
// zero-declaration-symbol, shadow-string divergences.
const EXPECTED_MEMBER_COUNTS: &[(&str, [u64; 6])] = &[
    ("resolver.collectLinkedAliases", [12, 0, 0, 0, 0, 0]),
    ("resolver.getEnumMemberValue", [3, 0, 0, 0, 0, 0]),
    (
        "resolver.getPropertiesOfContainerFunction",
        [5, 0, 0, 0, 0, 0],
    ),
    ("resolver.isDeclarationVisible", [1_366, 0, 0, 0, 0, 0]),
    (
        "resolver.isDefinitelyReferenceToGlobalSymbolObject",
        [10, 0, 0, 0, 0, 0],
    ),
    ("resolver.isEntityNameVisible", [195, 0, 0, 0, 0, 0]),
    (
        "resolver.isExpandoFunctionDeclaration",
        [456, 0, 0, 0, 0, 0],
    ),
    ("resolver.isImplementationOfOverload", [192, 0, 0, 0, 0, 0]),
    (
        "resolver.isImportRequiredByAugmentation",
        [15, 0, 0, 0, 0, 0],
    ),
    ("resolver.isLateBound", [4, 0, 0, 0, 0, 0]),
    ("resolver.isLiteralConstDeclaration", [612, 0, 0, 0, 0, 0]),
    ("resolver.isOptionalParameter", [100, 0, 0, 0, 0, 0]),
    ("resolver.isSymbolAccessible", [295, 0, 112, 0, 0, 0]),
    (
        "resolver.requiresAddingImplicitUndefined",
        [47, 0, 86, 0, 0, 0],
    ),
];

#[derive(Debug, Deserialize)]
struct WitnessArtifact {
    observation_content_roll_sha256: String,
    case_manifest: CaseManifest,
}

#[derive(Debug, Deserialize)]
struct CaseManifest {
    case_manifest_fingerprint: String,
    cases: Vec<ManifestCase>,
}

#[derive(Debug, Deserialize)]
struct ManifestCase {
    case_id: String,
    suite: String,
    fixture_id: String,
    matrix: CaseMatrix,
    option_record: Value,
    input_files: Vec<ManifestInput>,
}

#[derive(Debug, Deserialize)]
struct CaseMatrix {
    configuration_index: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ManifestInput {
    path: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct ProbeArtifact {
    case_manifest_fingerprint: String,
    witnesses: PathHash,
    summary: ProbeSummary,
    cases: Vec<ProbeCase>,
}

#[derive(Debug, Deserialize)]
struct PathHash {
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct ProbeSummary {
    cases: u64,
    trace_content_roll: String,
    per_site_counts: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Deserialize)]
struct ProbeCase {
    case_id: String,
    #[serde(rename = "fileTable")]
    file_table: Value,
    trace_events: Value,
}

struct ProjectedInputs {
    libs: Vec<InputFile>,
    files: Vec<InputFile>,
    options: CompilerOptions,
    current_directory: String,
}

#[test]
fn declaration_resolver_replay_decision_equal() {
    let workspace = workspace_root();
    let witnesses: WitnessArtifact =
        serde_json::from_slice(WITNESSES).expect("witness artifact is valid JSON");
    let probes: ProbeArtifact =
        serde_json::from_slice(PROBE_TRACES).expect("probe artifact is valid JSON");
    let conformance: ConformanceExpansionManifest =
        serde_json::from_slice(CONFORMANCE_EXPANSION).expect("conformance expansion is valid JSON");

    assert_frozen_artifact_identity(&witnesses, &probes);
    assert_site_dispositions_and_volumes(&probes);
    let corpus = load_recorded_execution_plans(&workspace)
        .unwrap_or_else(|error| panic!("failed to load recorded execution plans: {error}"));
    let probe_by_case = probes
        .cases
        .iter()
        .map(|case| (case.case_id.as_str(), case))
        .collect::<BTreeMap<_, _>>();

    let first = run_full_pass(
        &workspace,
        &witnesses.case_manifest.cases,
        &probe_by_case,
        &corpus,
        &conformance,
    );
    let second = run_full_pass(
        &workspace,
        &witnesses.case_manifest.cases,
        &probe_by_case,
        &corpus,
        &conformance,
    );
    assert_eq!(first, second, "fresh-session replay passes diverged");

    eprintln!("P4_MEMBER_COUNTS={}", first["member_counts"]);
    eprintln!(
        "P4_NESTED_TOPOLOGY_DIVERGENCES={}",
        first["nested_topology_divergences"]
    );
    eprintln!("HREFINE_EXCLUDED_CAUSALITY={}", first["excluded_causality"]);
    assert_eq!(first["cases"], json!(112));
    assert_eq!(first["seed_checks"], json!(388));
    assert_eq!(
        first["traced_nested_edges"],
        json_object_from_pairs(EXPECTED_NESTED_EDGES)
    );
    let gating = first["gating_mismatches"]
        .as_array()
        .expect("gating mismatch array");
    if !gating.is_empty() {
        let mut summary = BTreeMap::<&str, u64>::new();
        let mut by_member = BTreeMap::<String, [u64; 2]>::new();
        let mut sample_by_member = BTreeMap::<String, &str>::new();
        let mut affected_cases = BTreeSet::new();
        for mismatch in gating {
            let mismatch = mismatch.as_str().expect("gating mismatch is a string");
            if let Some(case_id) = mismatch.split_whitespace().next() {
                affected_cases.insert(case_id);
            }
            let class = if mismatch.contains("visibility seed differs") {
                "seed"
            } else if mismatch.contains("paint set differs") {
                "paint"
            } else if mismatch.contains("result differs") {
                "result"
            } else if mismatch.contains("symbol input differs") {
                "symbol-input"
            } else {
                "resolution-or-shape"
            };
            *summary.entry(class).or_default() += 1;
            if matches!(class, "result" | "paint") {
                let member = mismatch
                    .split_once(": ")
                    .map(|(prefix, _)| prefix)
                    .and_then(|prefix| prefix.split_whitespace().next_back())
                    .expect("root mismatch carries a member");
                by_member.entry(member.to_owned()).or_default()[usize::from(class == "paint")] += 1;
                sample_by_member
                    .entry(member.to_owned())
                    .or_insert(mismatch);
            }
        }
        eprintln!("P4_GATING_SUMMARY={summary:?}");
        eprintln!("P4_GATING_BY_MEMBER={by_member:?}");
        eprintln!("P4_GATING_SAMPLES={sample_by_member:?}");
        eprintln!("P4_AFFECTED_CASES={}", affected_cases.len());
        eprintln!(
            "P4_OTHER_GATING={}",
            Value::Array(
                gating
                    .iter()
                    .filter(|mismatch| {
                        let mismatch = mismatch.as_str().expect("gating mismatch is a string");
                        !mismatch.contains("visibility seed differs")
                            && !mismatch.contains("paint set differs")
                            && !mismatch.contains("result differs")
                            && !mismatch.contains("symbol input differs")
                    })
                    .take(20)
                    .cloned()
                    .collect()
            )
        );
    }
    assert_member_counts(&first["member_counts"]);
    assert_excluded_causality(&first["excluded_causality"]);
    assert!(
        gating.is_empty(),
        "decision-equal replay has {} gating mismatches; first rows: {}",
        gating.len(),
        Value::Array(gating.iter().take(20).cloned().collect())
    );
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn limits() -> ProgramLoadLimits {
    ProgramLoadLimits::new(128, 1_024, 32, 8 * 1_024 * 1_024, 64 * 1_024 * 1_024)
}

fn assert_frozen_artifact_identity(witnesses: &WitnessArtifact, probes: &ProbeArtifact) {
    assert_eq!(
        witnesses.case_manifest.case_manifest_fingerprint,
        EXPECTED_MANIFEST_FINGERPRINT
    );
    assert_eq!(
        witnesses.observation_content_roll_sha256,
        EXPECTED_WITNESS_OBSERVATION_ROLL
    );
    assert_eq!(
        probes.case_manifest_fingerprint,
        EXPECTED_MANIFEST_FINGERPRINT
    );
    assert_eq!(probes.summary.trace_content_roll, EXPECTED_PROBE_TRACE_ROLL);
    // The probe->witness binding: the probe pins the exact witness
    // artifact bytes it observed against; assert the pin matches the
    // checked-in witness file so the pair cannot drift apart.
    assert_eq!(probes.witnesses.sha256, sha256(WITNESSES));
    assert_eq!(witnesses.case_manifest.cases.len(), 112);
    assert_eq!(probes.summary.cases, 112);
    assert_eq!(probes.cases.len(), 112);
}

fn assert_site_dispositions_and_volumes(probes: &ProbeArtifact) {
    let mut sites = BTreeSet::new();
    for case in &probes.cases {
        for event in case
            .trace_events
            .as_array()
            .expect("trace_events is an array")
        {
            let site = event["site_id"].as_str().expect("site_id is a string");
            sites.insert(site);
            assert!(
                site_disposition(site).is_some(),
                "undispositioned probe site {site}"
            );
        }
    }
    assert!(!sites.is_empty());
    assert_eq!(
        probes
            .summary
            .per_site_counts
            .get("probe.fallbackSweep")
            .copied()
            .unwrap_or(0),
        0
    );
    assert_eq!(probes.summary.per_site_counts["probe.checkSeed"], 194);
    assert_eq!(probes.summary.per_site_counts["probe.transformSeed"], 194);
    for &(member, count) in EXPECTED_EVENT_VOLUMES {
        let prefix = if member == "collectLinkedAliases" {
            "resolver.collectLinkedAliases".to_owned()
        } else {
            format!("resolver.{member}")
        };
        assert_eq!(
            probes.summary.per_site_counts[&format!("{prefix}.entry")],
            count,
            "{member} entry volume"
        );
        assert_eq!(
            probes.summary.per_site_counts[&format!("{prefix}.result")],
            count,
            "{member} result volume"
        );
    }
}

fn site_disposition(site: &str) -> Option<&'static str> {
    let base = site
        .strip_suffix(".entry")
        .or_else(|| site.strip_suffix(".result"))
        .unwrap_or(site);
    match base {
        "resolver.isDefinitelyReferenceToGlobalSymbolObject"
        | "resolver.isSymbolAccessible"
        | "resolver.isEntityNameVisible"
        | "resolver.isDeclarationVisible"
        | "resolver.collectLinkedAliases"
        | "resolver.isOptionalParameter"
        | "resolver.isImplementationOfOverload"
        | "resolver.requiresAddingImplicitUndefined"
        | "resolver.isExpandoFunctionDeclaration"
        | "resolver.getPropertiesOfContainerFunction"
        | "resolver.isLiteralConstDeclaration"
        | "resolver.isLateBound"
        | "resolver.isImportRequiredByAugmentation"
        | "isVisible.memo"
        | "isVisible.addVisibleAlias"
        | "isVisible.collectLinkedAliases"
        | "probe.checkSeed"
        | "probe.transformSeed"
        | "probe.bootstrap" => Some("h2-7a-m-2"),
        "resolver.getEnumMemberValue" => Some("existing-resolver-api"),
        "resolver.createTypeOfDeclaration"
        | "resolver.createReturnTypeOfSignatureDeclaration"
        | "resolver.createTypeOfExpression"
        | "resolver.createLiteralConstValue"
        | "resolver.getDeclarationStatementsForSourceFile"
        | "resolver.createLateBoundIndexSignatures" => Some("h2-7a-m-3"),
        "resolver.hasGlobalName" | "probe.fallbackSweep" => Some("h2-7b"),
        _ if base.starts_with("nodebuilder.")
            || base.starts_with("syntactic.")
            || base.starts_with("tracker.") =>
        {
            Some("h2-7a-m-3")
        }
        _ if base.starts_with("declarations.") => Some("h2-7a-m-4"),
        _ => None,
    }
}

fn run_full_pass(
    workspace: &Path,
    manifest: &[ManifestCase],
    probes: &BTreeMap<&str, &ProbeCase>,
    corpus: &UpstreamExecutionCorpus,
    conformance: &ConformanceExpansionManifest,
) -> Value {
    let mut reports = Vec::with_capacity(manifest.len());
    let mut gating_mismatches = Vec::new();
    let mut excluded_causality_rows = Vec::new();
    let mut seed_checks = 0_u64;
    let mut member_counts = BTreeMap::<String, [u64; 6]>::new();
    let mut traced_nested_edges = BTreeMap::<String, u64>::new();
    let mut replayed_nested_edges = BTreeMap::<String, u64>::new();
    let mut rust_nested_edges = BTreeMap::<String, u64>::new();
    let mut nested_topology_divergences = 0_u64;

    for case in manifest {
        let probe = probes
            .get(case.case_id.as_str())
            .unwrap_or_else(|| panic!("{}: missing probe case", case.case_id));
        let (prepared, source_paths) = expand_case(workspace, case, corpus, conformance);
        verify_expanded_inputs(case, &prepared);
        let (replay_source_paths, replay_file_table, replay_trace_events) = project_replay_request(
            &prepared,
            &source_paths,
            &probe.file_table,
            &probe.trace_events,
            &case.case_id,
        );
        let projected = project_checker_inputs(&prepared, case);
        let request = json!({
            "case_id": case.case_id,
            // fileTable src keys are assigned from the probe control's VFS
            // insertion order.  The witness manifest deliberately sorts its
            // input_files rows, so recover the mint-side order from the
            // re-expanded, hash-verified Program instead of that presentation
            // order.
            "source_paths": replay_source_paths,
            "file_table": replay_file_table,
            "trace_events": replay_trace_events,
        });
        let (checked, mut report) =
            CheckerState::with_declaration_emit_replay_observer_for_harness(request, || {
                check_program_with_libs_at(
                    &projected.libs,
                    &projected.files,
                    &projected.options,
                    &projected.current_directory,
                )
            })
            .unwrap_or_else(|error| panic!("{}: replay hook failed: {error}", case.case_id));
        assert!(
            checked.partial_checks.is_empty(),
            "{}: checker reported partial checks",
            case.case_id
        );
        demote_excluded_causality(&mut report, &replay_trace_events, &case.case_id).unwrap_or_else(
            |error| {
                panic!(
                    "{}: excluded-causality analysis failed: {error}",
                    case.case_id
                )
            },
        );

        seed_checks += report["seed_checks"].as_u64().expect("seed count");
        gating_mismatches.extend(
            report["gating_mismatches"]
                .as_array()
                .expect("gating mismatch rows")
                .iter()
                .cloned(),
        );
        excluded_causality_rows.extend(
            report["excluded_causality"]["rows"]
                .as_array()
                .expect("excluded-causality rows")
                .iter()
                .cloned(),
        );
        aggregate_member_counts(&mut member_counts, &report["member_counts"]);
        aggregate_count_object(&mut traced_nested_edges, &report["traced_nested_edges"]);
        aggregate_count_object(&mut replayed_nested_edges, &report["replayed_nested_edges"]);
        aggregate_count_object(&mut rust_nested_edges, &report["rust_nested_edges"]);
        nested_topology_divergences += report["nested_topology_divergences"]
            .as_u64()
            .expect("nested divergence count");
        reports.push(report);
    }
    assert_eq!(reports.len(), probes.len());

    json!({
        "cases": reports.len(),
        "seed_checks": seed_checks,
        "member_counts": member_counts_json(&member_counts),
        "gating_mismatches": gating_mismatches,
        "excluded_causality": {
            "count": excluded_causality_rows.len(),
            "rows": excluded_causality_rows,
        },
        "traced_nested_edges": count_map_json(&traced_nested_edges),
        "replayed_nested_edges": count_map_json(&replayed_nested_edges),
        "rust_nested_edges": count_map_json(&rust_nested_edges),
        "nested_topology_divergences": nested_topology_divergences,
        "case_reports": reports,
    })
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct TraceCoordinate {
    file_tag: u64,
    kind: u64,
    pos: u64,
    end: u64,
}

impl TraceCoordinate {
    fn json(self) -> Value {
        json!([self.file_tag, self.kind, self.pos, self.end])
    }
}

#[derive(Clone)]
struct ExcludedCausalWriter {
    root_id: i64,
    root_entry_event_index: u64,
    root_result_event_index: u64,
    writer_event_index: u64,
    node_ref: Value,
    coordinate: TraceCoordinate,
    value: bool,
}

struct PendingCausalWriter {
    writer_event_index: u64,
    node_ref: Value,
    coordinate: TraceCoordinate,
    value: bool,
}

struct PendingCausalRoot {
    entry_event_index: u64,
    result_event_index: Option<u64>,
    writers: Vec<PendingCausalWriter>,
}

struct CausalityFrame {
    call_id: i64,
    member: String,
    maximal_domain_call: Option<i64>,
}

struct ExcludedCausalityTrace {
    entries_by_call: BTreeMap<i64, Value>,
    events_by_index: BTreeMap<u64, Value>,
    writers: Vec<ExcludedCausalWriter>,
}

fn demote_excluded_causality(
    report: &mut Value,
    trace_events: &Value,
    case_id: &str,
) -> Result<(), String> {
    let trace = build_excluded_causality_trace(trace_events, case_id)?;
    let mismatches = report
        .get("gating_mismatches")
        .and_then(Value::as_array)
        .ok_or_else(|| "report lacks gating_mismatches".to_owned())?;
    let mut remaining = Vec::new();
    let mut demoted = Vec::new();
    for mismatch in mismatches {
        let row = mismatch
            .as_str()
            .ok_or_else(|| "gating mismatch is not a string".to_owned())?;
        if let Some(justification) = prove_excluded_causality(row, case_id, &trace) {
            demoted.push(justification);
        } else {
            remaining.push(mismatch.clone());
        }
    }

    let report = report
        .as_object_mut()
        .ok_or_else(|| "replay report is not an object".to_owned())?;
    report.insert("gating_mismatches".to_owned(), Value::Array(remaining));
    report.insert(
        "excluded_causality".to_owned(),
        json!({
            "count": demoted.len(),
            "rows": demoted,
        }),
    );
    Ok(())
}

fn build_excluded_causality_trace(
    trace_events: &Value,
    case_id: &str,
) -> Result<ExcludedCausalityTrace, String> {
    let events = trace_events
        .as_array()
        .ok_or_else(|| "trace_events is not an array".to_owned())?;
    let mut entries_by_call = BTreeMap::new();
    let mut events_by_index = BTreeMap::new();
    let mut roots = BTreeMap::<i64, PendingCausalRoot>::new();
    let mut stack = Vec::<CausalityFrame>::new();

    for event in events {
        let event_index = trace_u64(event, "event_seq")?;
        if events_by_index.insert(event_index, event.clone()).is_some() {
            return Err(format!("duplicate trace event index {event_index}"));
        }
        let site = trace_str(event, "site_id")?;
        let call_id = trace_i64(event, "call_id")?;

        if call_id >= 0 && site.ends_with(".entry") {
            let member = site.trim_end_matches(".entry").to_owned();
            if entries_by_call.insert(call_id, event.clone()).is_some() {
                return Err(format!("duplicate trace call id {call_id}"));
            }
            let inherited_root = stack.last().and_then(|frame| frame.maximal_domain_call);
            let root = replay_domain_member(&member) && inherited_root.is_none();
            let maximal_domain_call = if root { Some(call_id) } else { inherited_root };
            if root && synthetic_without_original_symbol_enclosing(event)? {
                roots.insert(
                    call_id,
                    PendingCausalRoot {
                        entry_event_index: event_index,
                        result_event_index: None,
                        writers: Vec::new(),
                    },
                );
            }
            stack.push(CausalityFrame {
                call_id,
                member,
                maximal_domain_call,
            });
            continue;
        }

        if call_id >= 0 && site.ends_with(".result") {
            let frame = stack
                .pop()
                .ok_or_else(|| format!("result {site} call {call_id} has no entry"))?;
            if frame.call_id != call_id || frame.member != site.trim_end_matches(".result") {
                return Err(format!(
                    "trace call stack mismatch at {site} call {call_id}"
                ));
            }
            if frame.maximal_domain_call == Some(call_id) {
                if let Some(root) = roots.get_mut(&call_id) {
                    root.result_event_index = Some(event_index);
                }
            }
            continue;
        }

        if site.starts_with("isVisible.") {
            let Some(root_id) = stack.last().and_then(|frame| frame.maximal_domain_call) else {
                continue;
            };
            let Some(root) = roots.get_mut(&root_id) else {
                continue;
            };
            let args = trace_args(event)?;
            let node_ref = args
                .get(1)
                .ok_or_else(|| format!("writer {site} lacks a node ref"))?
                .clone();
            let coordinate = node_reference_coordinate(&node_ref)?
                .ok_or_else(|| format!("writer {site} uses a sentinel node"))?;
            let value = args
                .get(2)
                .and_then(Value::as_bool)
                .ok_or_else(|| format!("writer {site} lacks a boolean value"))?;
            root.writers.push(PendingCausalWriter {
                writer_event_index: event_index,
                node_ref,
                coordinate,
                value,
            });
        }
    }

    if !stack.is_empty() {
        return Err(format!("{case_id}: trace ended with open call frames"));
    }

    let mut writers = Vec::new();
    for (root_id, root) in roots {
        let root_result_event_index = root
            .result_event_index
            .ok_or_else(|| format!("synthetic root {root_id} has no result"))?;
        writers.extend(root.writers.into_iter().map(|writer| ExcludedCausalWriter {
            root_id,
            root_entry_event_index: root.entry_event_index,
            root_result_event_index,
            writer_event_index: writer.writer_event_index,
            node_ref: writer.node_ref,
            coordinate: writer.coordinate,
            value: writer.value,
        }));
    }
    writers.sort_by_key(|writer| (writer.writer_event_index, writer.root_id));

    Ok(ExcludedCausalityTrace {
        entries_by_call,
        events_by_index,
        writers,
    })
}

fn replay_domain_member(member: &str) -> bool {
    EXPECTED_MEMBER_COUNTS
        .iter()
        .any(|(expected, _)| *expected == member)
}

fn synthetic_without_original_symbol_enclosing(event: &Value) -> Result<bool, String> {
    if trace_str(event, "site_id")? != "resolver.isSymbolAccessible.entry" {
        return Ok(false);
    }
    let enclosing = trace_args(event)?
        .get(3)
        .ok_or_else(|| "isSymbolAccessible entry lacks enclosing node".to_owned())?;
    Ok(node_reference_coordinate(enclosing)?.is_none())
}

fn prove_excluded_causality(
    row: &str,
    case_id: &str,
    trace: &ExcludedCausalityTrace,
) -> Option<Value> {
    let (event_index, site, detail) = split_gating_row(row, case_id)?;
    if detail.contains("; ") {
        return None;
    }
    let (gate, proofs) =
        if site == "resolver.isDeclarationVisible" && detail.starts_with("result differs: ") {
            (
                "root-result",
                prove_root_result(event_index, site, detail, trace)?,
            )
        } else if site.starts_with("resolver.") && detail.starts_with("paint set differs: ") {
            (
                "per-root-paint-set",
                prove_paint_set(event_index, detail, trace)?,
            )
        } else if matches!(site, "probe.checkSeed" | "probe.transformSeed")
            && detail.starts_with("visibility seed differs: ")
        {
            ("seed-entry", prove_seed_entry(event_index, detail, trace)?)
        } else {
            return None;
        };

    Some(json!({
        "case_id": case_id,
        "gate": gate,
        "gate_event_index": event_index,
        "site": site,
        "proofs": proofs.into_iter().map(causal_writer_json).collect::<Vec<_>>(),
    }))
}

fn prove_root_result(
    event_index: u64,
    site: &str,
    detail: &str,
    trace: &ExcludedCausalityTrace,
) -> Option<Vec<ExcludedCausalWriter>> {
    let (expected, actual) = parse_expected_actual(detail, "result differs: expected ")?;
    if expected.get("kind")?.as_str()? != "boolean" || actual.get("kind")?.as_str()? != "boolean" {
        return None;
    }
    let expected_value = expected.get("value")?.as_bool()?;
    let actual_value = actual.get("value")?.as_bool()?;
    if expected_value == actual_value {
        return None;
    }

    let result_event = trace.events_by_index.get(&event_index)?;
    if trace_str(result_event, "site_id").ok()? != format!("{site}.result") {
        return None;
    }
    let result_args = trace_args(result_event).ok()?;
    if result_args.get(1)?.as_array()?.get(3)?.as_bool()? != expected_value {
        return None;
    }
    let call_id = trace_i64(result_event, "call_id").ok()?;
    let entry = trace.entries_by_call.get(&call_id)?;
    if trace_str(entry, "site_id").ok()? != format!("{site}.entry") {
        return None;
    }
    let node = trace_args(entry).ok()?.get(3)?;
    let coordinate = node_reference_coordinate(node).ok()??;
    Some(vec![find_causal_writer(
        trace,
        coordinate,
        expected_value,
        event_index,
    )?
    .clone()])
}

fn prove_seed_entry(
    event_index: u64,
    detail: &str,
    trace: &ExcludedCausalityTrace,
) -> Option<Vec<ExcludedCausalWriter>> {
    let (expected, actual) = parse_expected_actual(detail, "visibility seed differs: expected ")?;
    let expected = visibility_map(&expected)?;
    let actual = visibility_map(&actual)?;
    prove_visibility_difference(&expected, &actual, event_index, trace)
}

fn prove_paint_set(
    event_index: u64,
    detail: &str,
    trace: &ExcludedCausalityTrace,
) -> Option<Vec<ExcludedCausalWriter>> {
    let (expected, actual) = parse_expected_actual(detail, "paint set differs: expected ")?;
    let expected = visibility_set(&expected)?;
    let actual = visibility_set(&actual)?;
    let difference = expected
        .symmetric_difference(&actual)
        .copied()
        .collect::<Vec<_>>();
    if difference.is_empty() {
        return None;
    }
    difference
        .into_iter()
        .map(|(coordinate, value)| {
            find_causal_writer(trace, coordinate, value, event_index).cloned()
        })
        .collect()
}

fn prove_visibility_difference(
    expected: &BTreeMap<TraceCoordinate, bool>,
    actual: &BTreeMap<TraceCoordinate, bool>,
    event_index: u64,
    trace: &ExcludedCausalityTrace,
) -> Option<Vec<ExcludedCausalWriter>> {
    let coordinates = expected
        .keys()
        .chain(actual.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    let mut proofs = Vec::new();
    for coordinate in coordinates {
        if expected.get(&coordinate) == actual.get(&coordinate) {
            continue;
        }
        let expected_value = *expected.get(&coordinate)?;
        proofs.push(find_causal_writer(trace, coordinate, expected_value, event_index)?.clone());
    }
    (!proofs.is_empty()).then_some(proofs)
}

fn find_causal_writer(
    trace: &ExcludedCausalityTrace,
    coordinate: TraceCoordinate,
    value: bool,
    before_event_index: u64,
) -> Option<&ExcludedCausalWriter> {
    trace
        .writers
        .iter()
        .filter(|writer| {
            writer.coordinate == coordinate
                && writer.value == value
                && writer.root_result_event_index < before_event_index
        })
        .max_by_key(|writer| (writer.root_result_event_index, writer.writer_event_index))
}

fn causal_writer_json(writer: ExcludedCausalWriter) -> Value {
    json!({
        "excluded_class": "synthetic-without-original-enclosing",
        "excluded_root_member": "resolver.isSymbolAccessible",
        "root_id": writer.root_id,
        "root_entry_event_index": writer.root_entry_event_index,
        "root_result_event_index": writer.root_result_event_index,
        "node_ref": writer.node_ref,
        "node": writer.coordinate.json(),
        "expected_value": writer.value,
        "traced_writer_event_index": writer.writer_event_index,
    })
}

fn split_gating_row<'a>(row: &'a str, case_id: &str) -> Option<(u64, &'a str, &'a str)> {
    let row = row.strip_prefix(&format!("{case_id} event "))?;
    let (event_index, row) = row.split_once(' ')?;
    let (site, detail) = row.split_once(": ")?;
    Some((event_index.parse().ok()?, site, detail))
}

fn parse_expected_actual(detail: &str, prefix: &str) -> Option<(Value, Value)> {
    let detail = detail.strip_prefix(prefix)?;
    let (expected, actual) = detail.split_once(", actual ")?;
    Some((
        serde_json::from_str(expected).ok()?,
        serde_json::from_str(actual).ok()?,
    ))
}

fn visibility_map(value: &Value) -> Option<BTreeMap<TraceCoordinate, bool>> {
    let mut rows = BTreeMap::new();
    for row in value.as_array()? {
        let row = row.as_array().filter(|row| row.len() == 2)?;
        let coordinate = projected_coordinate(&row[0])?;
        let value = row[1].as_bool()?;
        if rows.insert(coordinate, value).is_some() {
            return None;
        }
    }
    Some(rows)
}

fn visibility_set(value: &Value) -> Option<BTreeSet<(TraceCoordinate, bool)>> {
    let mut rows = BTreeSet::new();
    for row in value.as_array()? {
        let row = row.as_array().filter(|row| row.len() == 2)?;
        if !rows.insert((projected_coordinate(&row[0])?, row[1].as_bool()?)) {
            return None;
        }
    }
    Some(rows)
}

fn projected_coordinate(value: &Value) -> Option<TraceCoordinate> {
    let values = value.as_array().filter(|values| values.len() == 4)?;
    Some(TraceCoordinate {
        file_tag: values[0].as_u64()?,
        kind: values[1].as_u64()?,
        pos: values[2].as_u64()?,
        end: values[3].as_u64()?,
    })
}

fn node_reference_coordinate(value: &Value) -> Result<Option<TraceCoordinate>, String> {
    let values = value
        .as_array()
        .filter(|values| values.len() == 8)
        .ok_or_else(|| "node reference is not an eight-element array".to_owned())?;
    let numbers = values
        .iter()
        .map(|value| {
            value
                .as_i64()
                .ok_or_else(|| "node reference contains a non-integer".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let offset = if numbers[0] >= 0 {
        0
    } else if numbers[4] >= 0 {
        4
    } else {
        return Ok(None);
    };
    Ok(Some(TraceCoordinate {
        file_tag: u64::try_from(numbers[offset])
            .map_err(|_| "node file tag is negative".to_owned())?,
        kind: u64::try_from(numbers[offset + 1]).map_err(|_| "node kind is negative".to_owned())?,
        pos: u64::try_from(numbers[offset + 2]).map_err(|_| "node pos is negative".to_owned())?,
        end: u64::try_from(numbers[offset + 3]).map_err(|_| "node end is negative".to_owned())?,
    }))
}

fn trace_str<'a>(event: &'a Value, field: &str) -> Result<&'a str, String> {
    event
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("trace event lacks string field {field}"))
}

fn trace_u64(event: &Value, field: &str) -> Result<u64, String> {
    event
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("trace event lacks unsigned field {field}"))
}

fn trace_i64(event: &Value, field: &str) -> Result<i64, String> {
    event
        .get(field)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("trace event lacks integer field {field}"))
}

fn trace_args(event: &Value) -> Result<&[Value], String> {
    event
        .get("args")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| "trace event lacks args".to_owned())
}

fn expand_case(
    workspace: &Path,
    case: &ManifestCase,
    corpus: &UpstreamExecutionCorpus,
    conformance: &ConformanceExpansionManifest,
) -> (PreparedProgram, Vec<String>) {
    match case.suite.as_str() {
        "compiler" => {
            let tail = case
                .fixture_id
                .strip_prefix("typescript-6.0.3/compiler/")
                .expect("compiler fixture prefix");
            let configuration = case
                .matrix
                .configuration_index
                .expect("compiler configuration index");
            let plan = unique_plan(&corpus.plans, |plan| {
                plan.provenance.suite.as_str() == "compiler"
                    && plan.provenance.upstream_path.ends_with(tail)
                    && matches!(
                        &plan.input,
                        UpstreamExecutionInput::Compiler(compiler)
                            if compiler.variant.configuration_index == configuration
                    )
            });
            let UpstreamExecutionInput::Compiler(plan) = &plan.input else {
                unreachable!("filtered compiler plan")
            };
            let source_paths = compiler_control_source_paths(plan, case);
            let prepared =
                load_compiler_no_emit(workspace, plan, limits()).unwrap_or_else(|error| {
                    panic!("{}: compiler expansion failed: {error}", case.case_id)
                });
            (prepared, source_paths)
        }
        "project" => {
            let plan = unique_plan(&corpus.plans, |plan| {
                plan.provenance.case_id.as_ref() == case.case_id
            });
            let UpstreamExecutionInput::Project(plan) = &plan.input else {
                panic!("{}: expected a project plan", case.case_id)
            };
            // The no-emit loader owns this suite's mount/root expansion, but
            // deliberately refuses emit-only descriptor properties. Those
            // properties cannot alter this frozen family's VFS membership;
            // the exact artifact option_record is projected into the checker
            // after structure loading.
            let mut no_emit_plan = plan.clone();
            let mut fixture = (*plan.fixture).clone();
            fixture.properties = Arc::from(
                plan.fixture
                    .properties
                    .iter()
                    .filter(|property| !project_emit_only_property(&property.name))
                    .cloned()
                    .collect::<Vec<_>>(),
            );
            no_emit_plan.fixture = Arc::new(fixture);
            let prepared = load_project_no_emit(workspace, &no_emit_plan, limits())
                .unwrap_or_else(|error| {
                    panic!("{}: project expansion failed: {error}", case.case_id)
                })
                .prepared_program;
            let source_paths = prepared
                .source_files()
                .iter()
                .skip(prepared.library_files().len())
                .map(|source| source.path().display().to_string_lossy().into_owned())
                .collect();
            (prepared, source_paths)
        }
        "conformance" => expand_conformance_case(workspace, case, conformance),
        suite => panic!("{}: unsupported suite {suite}", case.case_id),
    }
}

fn compiler_control_source_paths(
    plan: &tsc_harness::upstream_suites::execution::CompilerExecutionPlan,
    case: &ManifestCase,
) -> Vec<String> {
    let vfs_write_order = match &plan.root_selection {
        tsc_harness::upstream_suites::execution::CompilerRootSelection::Explicit {
            vfs_write_order,
            ..
        }
        | tsc_harness::upstream_suites::execution::CompilerRootSelection::Config {
            vfs_write_order,
            ..
        } => vfs_write_order,
    };
    let mut claimed = BTreeSet::new();
    vfs_write_order
        .iter()
        .filter_map(|unit_id| {
            let unit = &plan.fixture.units[unit_id.0 as usize];
            let content = unit.content.as_ref()?;
            let digest = sha256(content.as_bytes());
            let candidates = case
                .input_files
                .iter()
                .enumerate()
                .filter(|(index, input)| !claimed.contains(index) && input.sha256 == digest)
                .collect::<Vec<_>>();
            assert_eq!(
                candidates.len(),
                1,
                "{}: cannot uniquely recover probe VFS order for unit {}",
                case.case_id,
                unit.name
            );
            let (index, input) = candidates[0];
            claimed.insert(index);
            Some(input.path.clone())
        })
        .collect()
}

fn project_emit_only_property(name: &str) -> bool {
    matches!(
        name,
        "declaration"
            | "sourceMap"
            | "sourceRoot"
            | "mapRoot"
            | "outDir"
            | "outFile"
            | "declarationDir"
            | "resolveSourceRoot"
            | "resolveMapRoot"
            | "emittedFiles"
    )
}

fn unique_plan(
    plans: &[UpstreamExecutionPlan],
    predicate: impl Fn(&UpstreamExecutionPlan) -> bool,
) -> &UpstreamExecutionPlan {
    let matches = plans
        .iter()
        .filter(|plan| predicate(plan))
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), 1, "execution-plan match is not unique");
    matches[0]
}

fn expand_conformance_case(
    workspace: &Path,
    case: &ManifestCase,
    manifest: &ConformanceExpansionManifest,
) -> (PreparedProgram, Vec<String>) {
    let relative = case
        .fixture_id
        .strip_prefix("typescript-6.0.3/conformance/")
        .expect("conformance fixture prefix");
    let (source_index, source) = manifest
        .sources
        .iter()
        .enumerate()
        .find(|(_, source)| source.path == relative)
        .unwrap_or_else(|| panic!("{}: conformance source is absent", case.case_id));
    let raw_path = workspace
        .join("ts-tests/tests/cases/conformance")
        .join(relative);
    let raw = fs::read(&raw_path).unwrap_or_else(|error| {
        panic!(
            "{}: failed to read {}: {error}",
            case.case_id,
            raw_path.display()
        )
    });
    assert_eq!(raw.len() as u64, source.bytes);
    assert_eq!(sha256(&raw), source.sha256);
    let fixture = manifest
        .fixtures
        .iter()
        .find(|fixture| fixture.source as usize == source_index)
        .unwrap_or_else(|| panic!("{}: conformance fixture is absent", case.case_id));
    assert_eq!(fixture.encoding, SourceEncoding::Utf8);
    let decoded = String::from_utf8(raw).expect("pinned conformance fixture is UTF-8");
    assert_eq!(decoded.len() as u64, fixture.decoded_utf8_bytes);
    assert_eq!(sha256(decoded.as_bytes()), fixture.decoded_sha256);
    assert!(fixture.virtual_config.is_none());
    assert!(fixture.links.is_empty());
    let units = split_conformance_units(&decoded, relative);
    assert_eq!(units.len(), fixture.normal_units.len());
    let files = units
        .into_iter()
        .zip(&fixture.normal_units)
        .map(|((name, content), recorded)| {
            assert_eq!(name, recorded.name);
            let UnitContent::Present {
                utf8_bytes,
                sha256: expected,
            } = &recorded.content
            else {
                panic!("{}: conformance unit is unexpectedly missing", case.case_id)
            };
            assert_eq!(content.len() as u64, *utf8_bytes);
            assert_eq!(sha256(content.as_bytes()), *expected);
            (PathBuf::from("/.src").join(name), content.into_bytes())
        })
        .collect::<Vec<_>>();
    let roots = files
        .iter()
        .map(|(path, _)| path.clone())
        .collect::<Vec<_>>();
    let configuration = case
        .matrix
        .configuration_index
        .expect("conformance configuration index") as usize;
    let mut settings = fixture
        .settings
        .iter()
        .filter(|setting| setting.name != "filename")
        .cloned()
        .collect::<Vec<_>>();
    for override_setting in &fixture.configurations[configuration].settings {
        if let Some(existing) = settings
            .iter_mut()
            .find(|setting| setting.name == override_setting.name)
        {
            existing.value.clone_from(&override_setting.value);
        } else {
            settings.push(override_setting.clone());
        }
    }
    let settings = settings
        .into_iter()
        .map(|setting| (setting.name, setting.value))
        .collect::<Vec<_>>();
    let source_paths = files
        .iter()
        .map(|(path, _)| path.to_string_lossy().into_owned())
        .collect();
    let prepared =
        load_qualified_compiler_emit(workspace, "/.src", &files, &roots, &settings, limits())
            .unwrap_or_else(|error| {
                panic!("{}: conformance expansion failed: {error}", case.case_id)
            });
    (prepared, source_paths)
}

fn split_conformance_units(decoded: &str, fixture: &str) -> Vec<(String, String)> {
    let mut units = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_content: Option<String> = None;
    for line in decoded
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
    {
        if let Some((name, value)) = parse_directive(line) {
            if name.eq_ignore_ascii_case("filename") {
                if let Some(previous) = current_name.replace(value.to_owned()) {
                    units.push((previous, current_content.take().unwrap_or_default()));
                }
                current_content = Some(String::new());
            }
            continue;
        }
        let content = current_content.get_or_insert_with(String::new);
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str(line);
    }
    let name = current_name.unwrap_or_else(|| {
        fixture
            .rsplit('/')
            .next()
            .expect("fixture basename")
            .to_owned()
    });
    units.push((name, current_content.unwrap_or_default()));
    units
}

fn parse_directive(line: &str) -> Option<(&str, &str)> {
    let directive = line.trim_start().strip_prefix("//")?.trim_start();
    let directive = directive.strip_prefix('@')?;
    let (name, value) = directive.split_once(':')?;
    let name = name.trim();
    (!name.is_empty()).then_some((name, value.trim()))
}

fn verify_expanded_inputs(case: &ManifestCase, prepared: &PreparedProgram) {
    for expected in &case.input_files {
        let matches = prepared
            .source_files()
            .iter()
            .filter(|source| source.path().display().to_string_lossy() == expected.path)
            .collect::<Vec<_>>();
        assert_eq!(
            matches.len(),
            1,
            "{}: expanded input {} is not unique",
            case.case_id,
            expected.path
        );
        assert_eq!(
            sha256(matches[0].text().as_bytes()),
            expected.sha256,
            "{}: expanded VFS input {} differs from the manifest",
            case.case_id,
            expected.path
        );
    }
}

fn project_replay_request(
    prepared: &PreparedProgram,
    source_paths: &[String],
    file_table: &Value,
    trace_events: &Value,
    case_id: &str,
) -> (Vec<String>, Value, Value) {
    let mut replay_source_paths = source_paths.to_vec();
    let mut replay_file_table = file_table.clone();
    let mut lib_position_maps = BTreeMap::new();
    let rows = replay_file_table
        .as_array_mut()
        .expect("fileTable is an array");

    for (file_tag, row) in rows.iter_mut().enumerate() {
        let fields = row
            .as_array()
            .filter(|fields| fields.len() == 2)
            .unwrap_or_else(|| panic!("{case_id}: fileTable row {file_tag} is malformed"));
        if fields[0].as_str() != Some("lib") {
            continue;
        }
        let basename = fields[1]
            .as_str()
            .unwrap_or_else(|| panic!("{case_id}: fileTable row {file_tag} lacks a lib name"))
            .to_owned();
        let matches = prepared
            .library_files()
            .iter()
            .copied()
            .filter_map(|source_file| prepared.source_file(source_file))
            .filter(|source| {
                replay_path_basename(&source.path().display().to_string_lossy()) == basename
            })
            .collect::<Vec<_>>();

        // A non-unique basename remains a lib descriptor, preserving the
        // observer's fail-closed lib-target disposition.  A unique match is
        // admitted through the source path table while retaining the frozen
        // file tag; the observer still requires exactly one (kind,pos,end)
        // parse-tree match before invoking the root.
        let [source] = matches.as_slice() else {
            continue;
        };
        let source_index = replay_source_paths.len();
        replay_source_paths.push(source.path().display().to_string_lossy().into_owned());
        lib_position_maps.insert(file_tag, utf16_to_utf8_boundaries(source.text()));
        *row = json!(["src", source_index]);
    }

    let mut replay_trace_events = trace_events.clone();
    project_lib_node_positions(&mut replay_trace_events, &lib_position_maps, case_id);
    (replay_source_paths, replay_file_table, replay_trace_events)
}

fn replay_path_basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn utf16_to_utf8_boundaries(text: &str) -> Vec<Option<u32>> {
    let mut boundaries = vec![None; text.encode_utf16().count() + 1];
    let mut utf16_offset = 0;
    for (byte_offset, character) in text.char_indices() {
        boundaries[utf16_offset] = Some(u32::try_from(byte_offset).expect("lib offset fits u32"));
        utf16_offset += character.len_utf16();
    }
    boundaries[utf16_offset] = Some(u32::try_from(text.len()).expect("lib length fits u32"));
    boundaries
}

fn project_lib_node_positions(
    value: &mut Value,
    position_maps: &BTreeMap<usize, Vec<Option<u32>>>,
    case_id: &str,
) {
    match value {
        Value::Array(values)
            if values.len() == 8 && values.iter().all(|value| value.as_i64().is_some()) =>
        {
            for offset in [0, 4] {
                let Some(file_tag) = values[offset]
                    .as_i64()
                    .and_then(|value| usize::try_from(value).ok())
                else {
                    continue;
                };
                let Some(boundaries) = position_maps.get(&file_tag) else {
                    continue;
                };
                for position_index in [offset + 2, offset + 3] {
                    let utf16_position = values[position_index]
                        .as_u64()
                        .and_then(|value| usize::try_from(value).ok())
                        .unwrap_or_else(|| {
                            panic!(
                                "{case_id}: lib node ref at file tag {file_tag} has an invalid position"
                            )
                        });
                    let utf8_position = boundaries
                        .get(utf16_position)
                        .and_then(|position| *position)
                        .unwrap_or_else(|| {
                            panic!(
                                "{case_id}: lib node ref at file tag {file_tag} is not on a UTF-16 character boundary"
                            )
                        });
                    values[position_index] = json!(utf8_position);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                project_lib_node_positions(value, position_maps, case_id);
            }
        }
        Value::Object(fields) => {
            for value in fields.values_mut() {
                project_lib_node_positions(value, position_maps, case_id);
            }
        }
        _ => {}
    }
}

fn project_checker_inputs(prepared: &PreparedProgram, case: &ManifestCase) -> ProjectedInputs {
    let library_ids = prepared.library_files();
    let mut libs = Vec::with_capacity(library_ids.len());
    for (position, source_file) in library_ids.iter().copied().enumerate() {
        assert_eq!(
            source_file.index(),
            position,
            "{}: library prefix is not dense",
            case.case_id
        );
        let source = prepared
            .source_file(source_file)
            .expect("library source exists");
        libs.push(InputFile::from_snapshot(
            source.path().display().to_string_lossy().into_owned(),
            source.snapshot().clone(),
        ));
    }
    let files = prepared
        .source_files()
        .iter()
        .skip(library_ids.len())
        .map(|source| {
            InputFile::from_snapshot(
                source.path().display().to_string_lossy().into_owned(),
                source.snapshot().clone(),
            )
        })
        .collect();
    ProjectedInputs {
        libs,
        files,
        options: options_from_record(&case.option_record),
        current_directory: prepared
            .current_directory()
            .display()
            .to_str()
            .expect("current directory is Unicode")
            .to_owned(),
    }
}

fn options_from_record(record: &Value) -> CompilerOptions {
    let object = record.as_object().expect("option_record is an object");
    let mut options = CompilerOptions::default();
    for (name, value) in object {
        match name.as_str() {
            "allowJs" => options.allow_js = bool_value(value),
            "checkJs" => options.check_js = Some(bool_value(value)),
            "declaration" => options.declaration = Some(bool_value(value)),
            "emitDeclarationOnly" => options.emit_declaration_only = Some(bool_value(value)),
            "isolatedDeclarations" => options.isolated_declarations = Some(bool_value(value)),
            "lib" => {
                options.lib = Some(
                    value
                        .as_array()
                        .expect("lib is an array")
                        .iter()
                        .map(|value| value.as_str().expect("lib entry is a string").to_owned())
                        .collect(),
                )
            }
            "mapRoot" => options.map_root = Some(string_value(value)),
            "module" => options.module = Some(i32_value(value)),
            "moduleResolution" => options.module_resolution = Some(i32_value(value)),
            "newLine" => options.new_line = Some(i32_value(value)),
            "noErrorTruncation" => options.no_error_truncation = Some(bool_value(value)),
            "noResolve" => options.no_resolve = Some(bool_value(value)),
            "outDir" => options.out_dir = Some(string_value(value)),
            "outFile" => options.out_file = Some(string_value(value)),
            "removeComments" => options.remove_comments = Some(bool_value(value)),
            "skipDefaultLibCheck" => options.skip_default_lib_check = Some(bool_value(value)),
            "sourceMap" => options.source_map = Some(bool_value(value)),
            "sourceRoot" => options.source_root = Some(string_value(value)),
            "strict" => options.strict = Some(bool_value(value)),
            "target" => options.target = Some(i32_value(value)),
            other => panic!("unprojected option_record key {other}"),
        }
    }
    options
}

fn bool_value(value: &Value) -> bool {
    value.as_bool().expect("option is a boolean")
}

fn i32_value(value: &Value) -> i32 {
    i32::try_from(value.as_i64().expect("option is an integer")).expect("option fits i32")
}

fn string_value(value: &Value) -> String {
    value.as_str().expect("option is a string").to_owned()
}

fn aggregate_member_counts(target: &mut BTreeMap<String, [u64; 6]>, counts: &Value) {
    for (member, row) in counts.as_object().expect("member counts object") {
        let excluded = row["excluded"].as_object().expect("excluded counts object");
        let values = target.entry(member.clone()).or_default();
        values[0] += row["replayed"].as_u64().expect("replayed count");
        values[1] += excluded["lib-target"].as_u64().expect("lib count");
        values[2] += excluded["synthetic-without-original"]
            .as_u64()
            .expect("synthetic count");
        values[3] += excluded["ambiguous-symbol"]
            .as_u64()
            .expect("ambiguous count");
        values[4] += excluded["zero-declaration-symbol"]
            .as_u64()
            .expect("zero-declaration count");
        values[5] += row["shadow"].as_u64().expect("shadow count");
    }
}

fn aggregate_count_object(target: &mut BTreeMap<String, u64>, counts: &Value) {
    for (key, value) in counts.as_object().expect("count object") {
        *target.entry(key.clone()).or_default() += value.as_u64().expect("count value");
    }
}

fn member_counts_json(counts: &BTreeMap<String, [u64; 6]>) -> Value {
    Value::Object(
        counts
            .iter()
            .map(|(member, values)| (member.clone(), json!(values)))
            .collect(),
    )
}

fn count_map_json(counts: &BTreeMap<String, u64>) -> Value {
    Value::Object(
        counts
            .iter()
            .map(|(key, value)| (key.clone(), json!(value)))
            .collect(),
    )
}

fn json_object_from_pairs(rows: &[(&str, u64)]) -> Value {
    Value::Object(
        rows.iter()
            .map(|(key, value)| ((*key).to_owned(), json!(value)))
            .collect(),
    )
}

fn assert_member_counts(actual: &Value) {
    let expected = Value::Object(
        EXPECTED_MEMBER_COUNTS
            .iter()
            .map(|(member, counts)| ((*member).to_owned(), json!(counts)))
            .collect(),
    );
    assert_eq!(actual, &expected, "frozen per-member replay counts drifted");
}

fn assert_excluded_causality(actual: &Value) {
    let count = actual["count"].as_u64().expect("excluded-causality count");
    let rows = actual["rows"].as_array().expect("excluded-causality rows");
    assert_eq!(count, rows.len() as u64);

    let mut counts = EXPECTED_EXCLUDED_CAUSALITY_COUNTS
        .iter()
        .map(|(gate, _)| ((*gate).to_owned(), 0_u64))
        .collect::<BTreeMap<_, _>>();
    let mut row_ids = BTreeSet::new();
    for row in rows {
        let case_id = row["case_id"].as_str().expect("excluded-causality case id");
        let gate = row["gate"].as_str().expect("excluded-causality gate");
        let gate_event_index = row["gate_event_index"]
            .as_u64()
            .expect("excluded-causality gate event index");
        let site = row["site"].as_str().expect("excluded-causality site");
        assert!(
            row_ids.insert((case_id, gate_event_index, site)),
            "duplicate excluded-causality row"
        );
        *counts
            .get_mut(gate)
            .unwrap_or_else(|| panic!("unknown excluded-causality gate {gate}")) += 1;

        let proofs = row["proofs"].as_array().expect("excluded-causality proofs");
        assert!(!proofs.is_empty(), "excluded-causality row has no proof");
        for proof in proofs {
            assert_eq!(
                proof["excluded_class"],
                json!("synthetic-without-original-enclosing")
            );
            assert_eq!(
                proof["excluded_root_member"],
                json!("resolver.isSymbolAccessible")
            );
            assert!(proof["root_id"].as_i64().is_some_and(|root| root >= 0));
            let entry = proof["root_entry_event_index"]
                .as_u64()
                .expect("excluded root entry event index");
            let writer = proof["traced_writer_event_index"]
                .as_u64()
                .expect("traced writer event index");
            let result = proof["root_result_event_index"]
                .as_u64()
                .expect("excluded root result event index");
            assert!(entry < writer && writer < result && result < gate_event_index);
            assert!(proof["expected_value"].as_bool().is_some());
            let node_ref = node_reference_coordinate(&proof["node_ref"])
                .expect("machine proof node ref is valid")
                .expect("machine proof node ref is concrete");
            assert_eq!(projected_coordinate(&proof["node"]), Some(node_ref));
        }
    }

    assert_eq!(
        count_map_json(&counts),
        json_object_from_pairs(EXPECTED_EXCLUDED_CAUSALITY_COUNTS),
        "frozen excluded-causality counts drifted"
    );
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

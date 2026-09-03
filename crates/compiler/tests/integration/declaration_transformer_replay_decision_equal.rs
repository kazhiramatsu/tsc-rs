//! Transformer-driven H2.7a declaration replay over the frozen witness and
//! probe artifacts. The four later-owned option cases are excluded by name;
//! every remaining transform window is consumed independently.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tsc_checker::emit::CheckerSession;
use tsc_checker::state::CheckerState;
use tsc_checker::{
    check_program_with_authoritative_modules_at_for_emit, check_program_with_libs_at,
    AuthoritativeModuleLookupFailure, AuthoritativeModuleProvider, AuthoritativeModuleRequest,
    AuthoritativeModuleResolution, AuthoritativeModuleResolutionDiagnostic,
    AuthoritativeNotFoundModule, AuthoritativePackageId, AuthoritativeResolutionDiagnosticModule,
    AuthoritativeResolutionMode, AuthoritativeResolvedModule, AuthoritativeSourceMetadata,
    AuthoritativeSourceToken, AuthoritativeUntypedModule, ProgramSnapshot,
    UnsupportedAuthoritativeResolution,
};
use tsc_diagnostics::{Diagnostic, DiagnosticCategory};
use tsc_emitter::{
    create_printer, preflight_emit, transform_declaration_unit_with_observer_for_harness,
    BoundaryEvent, DeclarationPathResolver, EmitEnumMemberValue, EmitFunctionProperty, EmitHost,
    EmitInternalNodeBuilderFlags, EmitNodeBuilderFlags, EmitOutputPaths, EmitPreflight,
    EmitResolver, EmitResolverError, EmitResolverNode, EmitResolverSymbol,
    EmitSymbolAccessibilityResult, EmitSymbolMeaning, EmitSymbolTracker, NewLineKind, PrintRequest,
    PrinterOptions, SourceFileId, SourceFileTextMode, TransformArena, TransformNode, TransformRoot,
    TransformSourceId, TransformationResult,
};
use tsc_harness::upstream_suites::execution::{
    load_recorded_execution_plans, UpstreamExecutionCorpus,
};
use tsc_harness::upstream_suites::h1_conformance::ConformanceExpansionManifest;
use tsc_program::{
    plan_source_requests, ModuleExtension, PreparedProgram, PreparedSourceFile, ResolutionKey,
    ResolutionMode, ResolutionOutcome, ResolvedModuleTarget, SourceRequestPlan,
    UnloadedModuleReason,
};

use super::declaration_resolver_replay_decision_equal::{
    assert_frozen_artifact_identity, expand_case, project_checker_inputs, project_replay_request,
    verify_expanded_inputs, workspace_root, ManifestCase, ProbeArtifact, ProbeCase,
    ProjectedInputs, WitnessArtifact,
};

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

const EXCLUDED_CASES: &[(&str, &str)] = &[
    ("h2-7a/F6/references-first", "H2.7d outFile"),
    ("h2-7a/S2/entityname-1", "H2.7d outFile"),
    ("h2-7a/S3/typeofexpr-1", "H2.7d outFile"),
    ("h2-7a/S2/latebound-1", "H2.7c isolatedDeclarations"),
];

const EXPECTED_TRACKER_SITES: &[(&str, u64)] = &[
    ("tracker.trackSymbol", 533),
    ("tracker.reportInferenceFallback", 362),
    ("tracker.reportInaccessibleUniqueSymbolError", 1),
    ("tracker.reportLikelyUnsafeImportRequiredError", 1),
    ("tracker.reportPrivateInBaseOfClassExpression", 0),
    ("tracker.reportCyclicStructureError", 0),
    ("tracker.reportInaccessibleThisError", 0),
    ("tracker.reportTruncationError", 0),
    ("tracker.reportNonlocalAugmentation", 0),
    ("tracker.reportNonSerializableProperty", 0),
];

#[derive(Debug, Deserialize)]
struct WitnessObservations {
    observations: Vec<WitnessCaseObservation>,
}

#[derive(Debug, Deserialize)]
struct WitnessCaseObservation {
    case_id: String,
    observation: WitnessObservation,
}

#[derive(Debug, Deserialize)]
struct WitnessObservation {
    writes: Vec<WitnessWrite>,
    emit_result: WitnessEmitResult,
}

#[derive(Clone, Debug, Deserialize)]
struct WitnessWrite {
    path: String,
    kind: String,
    write_byte_order_mark: bool,
    declaration_callback_base64: Option<String>,
    declaration_materialized_base64: Option<String>,
    source_files: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct WitnessEmitResult {
    diagnostics: Vec<WitnessDiagnostic>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct WitnessDiagnostic {
    code: u32,
    category: String,
    message: String,
    file: Option<String>,
    start: Option<u32>,
    length: Option<u32>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct SiteCounts {
    expected: u64,
    actual: u64,
}

#[derive(Clone, Debug, Default)]
struct PassReport {
    cases: u64,
    windows: u64,
    unblocked: u64,
    blocked: u64,
    writes: u64,
    diagnostics: u64,
    actual_roots: u64,
    actual_unblocked: u64,
    actual_blocked: u64,
    site_counts: BTreeMap<String, SiteCounts>,
    divergences: Vec<String>,
    mismatches: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DeterministicCounts {
    cases: u64,
    windows: u64,
    unblocked: u64,
    blocked: u64,
    writes: u64,
    diagnostics: u64,
    actual_roots: u64,
    actual_unblocked: u64,
    actual_blocked: u64,
    site_counts: BTreeMap<String, SiteCounts>,
    divergences: usize,
    mismatches: usize,
}

impl PassReport {
    fn deterministic_counts(&self) -> DeterministicCounts {
        DeterministicCounts {
            cases: self.cases,
            windows: self.windows,
            unblocked: self.unblocked,
            blocked: self.blocked,
            writes: self.writes,
            diagnostics: self.diagnostics,
            actual_roots: self.actual_roots,
            actual_unblocked: self.actual_unblocked,
            actual_blocked: self.actual_blocked,
            site_counts: self.site_counts.clone(),
            divergences: self.divergences.len(),
            mismatches: self.mismatches.len(),
        }
    }

    fn count_expected(&mut self, site: &str) {
        self.site_counts
            .entry(site.to_owned())
            .or_default()
            .expected += 1;
    }

    fn count_actual(&mut self, site: &str) {
        self.site_counts.entry(site.to_owned()).or_default().actual += 1;
    }
}

#[test]
fn declaration_transformer_replay_decision_equal() {
    let workspace = workspace_root();
    let witnesses: WitnessArtifact =
        serde_json::from_slice(WITNESSES).expect("witness artifact is valid JSON");
    let witness_observations: WitnessObservations =
        serde_json::from_slice(WITNESSES).expect("witness observations are valid JSON");
    let probes: ProbeArtifact =
        serde_json::from_slice(PROBE_TRACES).expect("probe artifact is valid JSON");
    let conformance: ConformanceExpansionManifest =
        serde_json::from_slice(CONFORMANCE_EXPANSION).expect("conformance expansion is valid JSON");

    assert_frozen_artifact_identity(&witnesses, &probes);
    let corpus = load_recorded_execution_plans(&workspace)
        .unwrap_or_else(|error| panic!("failed to load recorded execution plans: {error}"));
    let probe_by_case = probes
        .cases
        .iter()
        .map(|case| (case.case_id.as_str(), case))
        .collect::<BTreeMap<_, _>>();
    let observation_by_case = witness_observations
        .observations
        .iter()
        .map(|case| (case.case_id.as_str(), case))
        .collect::<BTreeMap<_, _>>();

    let first = run_full_pass(
        &workspace,
        &witnesses.case_manifest.cases,
        &probe_by_case,
        &observation_by_case,
        &corpus,
        &conformance,
    );
    let second = run_full_pass(
        &workspace,
        &witnesses.case_manifest.cases,
        &probe_by_case,
        &observation_by_case,
        &corpus,
        &conformance,
    );
    assert_eq!(
        first.deterministic_counts(),
        second.deterministic_counts(),
        "fresh-session transformer passes produced different counts"
    );

    println!(
        "declaration transformer per-site summary: {}",
        site_counts_json(&first.site_counts)
    );
    println!(
        "declaration transformer replay summary: cases={} windows={} unblocked={} blocked={} writes={} diagnostics={} actual_roots={} actual_unblocked={} actual_blocked={} divergences={} first_20={}",
        first.cases,
        first.windows,
        first.unblocked,
        first.blocked,
        first.writes,
        first.diagnostics,
        first.actual_roots,
        first.actual_unblocked,
        first.actual_blocked,
        first.divergences.len(),
        Value::Array(first.divergences.iter().take(20).cloned().map(Value::String).collect())
    );
    println!(
        "declaration transformer byte summary: mismatches={} first_10:\n{}",
        first.mismatches.len(),
        first
            .mismatches
            .iter()
            .take(10)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );

    assert_eq!(first.cases, 116, "eligible case denominator");
    assert_eq!(first.windows, 202, "transform window denominator");
    assert_eq!(first.unblocked, 199, "unblocked window denominator");
    assert_eq!(first.blocked, 3, "blocked window denominator");
    assert_eq!(first.writes, 199, "declaration write denominator");
    assert_eq!(first.diagnostics, 3, "emit diagnostic denominator");
    assert_site_denominator(
        &first.site_counts,
        "declarations.transformTopLevelDeclaration.changed",
        742,
    );
    assert_site_denominator(
        &first.site_counts,
        "declarations.visitDeclarationSubtree.changed",
        496,
    );
    assert_site_denominator(&first.site_counts, "declarations.declBlocked", 202);
    for &(site, expected) in EXPECTED_TRACKER_SITES {
        assert_site_denominator(&first.site_counts, site, expected);
    }
    assert!(
        first.divergences.is_empty(),
        "transformer replay has {} divergences; first rows: {}",
        first.divergences.len(),
        Value::Array(
            first
                .divergences
                .iter()
                .take(20)
                .cloned()
                .map(Value::String)
                .collect()
        )
    );
    assert!(
        first.mismatches.is_empty(),
        "declaration byte lane has {} mismatches; first rows:\n{}",
        first.mismatches.len(),
        first
            .mismatches
            .iter()
            .take(10)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn assert_site_denominator(counts: &BTreeMap<String, SiteCounts>, site: &str, expected: u64) {
    assert_eq!(
        counts.get(site).map_or(0, |row| row.expected),
        expected,
        "{site} frozen expected denominator"
    );
}

fn site_counts_json(counts: &BTreeMap<String, SiteCounts>) -> Value {
    Value::Object(
        counts
            .iter()
            .map(|(site, counts)| {
                (
                    site.clone(),
                    json!({"expected": counts.expected, "actual": counts.actual}),
                )
            })
            .collect(),
    )
}

fn excluded_case(case_id: &str) -> bool {
    EXCLUDED_CASES
        .iter()
        .any(|(excluded, _)| *excluded == case_id)
}

struct TransformWindow<'a> {
    index: usize,
    start_sequence: u64,
    end_sequence: u64,
    events: Vec<&'a Value>,
}

fn transform_windows<'a>(case_id: &str, trace_events: &'a Value) -> Vec<TransformWindow<'a>> {
    let events = trace_events
        .as_array()
        .unwrap_or_else(|| panic!("{case_id}: trace_events is not an array"));
    let mut windows = Vec::new();
    let mut current: Option<Vec<&Value>> = None;
    for event in events {
        match event["site_id"].as_str() {
            Some("probe.transformSeed") => {
                assert!(current.is_none(), "{case_id}: nested transform window");
                current = Some(vec![event]);
            }
            Some("declarations.declBlocked") => {
                let mut window = current
                    .take()
                    .unwrap_or_else(|| panic!("{case_id}: declBlocked outside a window"));
                window.push(event);
                let index = windows.len();
                windows.push(TransformWindow {
                    index,
                    start_sequence: event_sequence(window[0]),
                    end_sequence: event_sequence(event),
                    events: window,
                });
            }
            _ => {
                if let Some(window) = current.as_mut() {
                    window.push(event);
                }
            }
        }
    }
    assert!(
        current.is_none(),
        "{case_id}: unterminated transform window"
    );
    windows
}

fn event_sequence(event: &Value) -> u64 {
    event["event_seq"].as_u64().expect("event_seq is unsigned")
}

fn window_index_for_sequence(windows: &[TransformWindow<'_>], sequence: u64) -> Option<usize> {
    windows
        .iter()
        .find(|window| window.start_sequence <= sequence && sequence <= window.end_sequence)
        .map(|window| window.index)
}

fn run_full_pass(
    workspace: &Path,
    manifest: &[ManifestCase],
    probes: &BTreeMap<&str, &ProbeCase>,
    observations: &BTreeMap<&str, &WitnessCaseObservation>,
    corpus: &UpstreamExecutionCorpus,
    conformance: &ConformanceExpansionManifest,
) -> PassReport {
    let mut pass = PassReport::default();
    for &(site, _) in EXPECTED_TRACKER_SITES {
        pass.site_counts.entry(site.to_owned()).or_default();
    }

    for case in manifest {
        if excluded_case(&case.case_id) {
            continue;
        }
        pass.cases += 1;
        let probe = probes
            .get(case.case_id.as_str())
            .unwrap_or_else(|| panic!("{}: missing probe case", case.case_id));
        let witness = observations
            .get(case.case_id.as_str())
            .unwrap_or_else(|| panic!("{}: missing witness observation", case.case_id));
        let (prepared, source_paths) = expand_case(workspace, case, corpus, conformance);
        verify_expanded_inputs(case, &prepared);
        let (replay_source_paths, replay_file_table, replay_trace_events) = project_replay_request(
            &prepared,
            &source_paths,
            &probe.file_table,
            &probe.trace_events,
            &case.case_id,
        );
        let windows = transform_windows(&case.case_id, &replay_trace_events);
        pass.windows += windows.len() as u64;
        record_expected_site_counts(&mut pass, &windows);

        let projected = project_checker_inputs(&prepared, case);
        let exact_root_windows = run_transform_case(
            &case.case_id,
            &prepared,
            &projected,
            &replay_source_paths,
            &replay_file_table,
            &windows,
            &witness.observation,
            &mut pass,
        );
        run_m3_comparison_engine(
            &case.case_id,
            &projected,
            &replay_source_paths,
            &replay_file_table,
            &probe.printed_results,
            &windows,
            &exact_root_windows,
            &mut pass,
        );
    }
    pass
}

fn record_expected_site_counts(pass: &mut PassReport, windows: &[TransformWindow<'_>]) {
    for window in windows {
        for event in &window.events {
            let site = event["site_id"].as_str().expect("site_id is a string");
            if is_surface_expected_event(event) || site.starts_with("tracker.") {
                pass.count_expected(site);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_m3_comparison_engine(
    case_id: &str,
    projected: &ProjectedInputs,
    source_paths: &[String],
    file_table: &Value,
    printed_results: &Value,
    windows: &[TransformWindow<'_>],
    exact_root_windows: &BTreeSet<usize>,
    pass: &mut PassReport,
) {
    let transform_trace = Value::Array(
        windows
            .iter()
            .flat_map(|window| window.events.iter().copied().cloned())
            .collect(),
    );
    let request = json!({
        "case_id": case_id,
        "source_paths": source_paths,
        "file_table": file_table,
        "trace_events": transform_trace,
        "printed_results": printed_results,
    });
    let (checked, report) =
        CheckerState::with_declaration_emit_replay_observer_for_harness(request, || {
            check_program_with_libs_at(
                &projected.libs,
                &projected.files,
                &projected.options,
                &projected.current_directory,
            )
        })
        .unwrap_or_else(|error| panic!("{case_id}: replay hook failed: {error}"));
    assert!(
        checked.partial_checks.is_empty(),
        "{case_id}: checker reported partial checks"
    );
    let divergence_start = pass.divergences.len();
    for mismatch in report["gating_mismatches"]
        .as_array()
        .expect("gating mismatch rows")
    {
        let detail = mismatch.as_str().expect("gating mismatch is a string");
        pass.divergences
            .push(annotate_m3_row(case_id, detail, windows));
    }
    for mismatch in report["printed_mismatches"]
        .as_array()
        .expect("printed mismatch rows")
    {
        let detail = mismatch.as_str().expect("printed mismatch is a string");
        pass.divergences
            .push(annotate_m3_row(case_id, detail, windows));
    }
    for (member, counts) in report["member_counts"]
        .as_object()
        .expect("member count rows")
    {
        for (class, count) in counts["excluded"].as_object().expect("exclusion count row") {
            let count = count.as_u64().expect("exclusion count");
            if count != 0 {
                pass.divergences.push(format!(
                    "{case_id}/window=all/event_seq=all: m-3 engine excluded {count} {member} root(s) as {class}"
                ));
            }
        }
    }
    if pass.divergences.len() == divergence_start {
        for window in windows {
            if !exact_root_windows.contains(&window.index) {
                continue;
            }
            for event in &window.events {
                let site = event["site_id"].as_str().expect("site_id is a string");
                if site.starts_with("tracker.") {
                    pass.count_actual(site);
                }
            }
        }
    }
    // Nested resolver topology is the m-3 lane's explicitly non-gating
    // measurement; root results and the complete decision payload remain
    // gating through the two mismatch arrays above.
}

fn annotate_m3_row(case_id: &str, detail: &str, windows: &[TransformWindow<'_>]) -> String {
    let sequence = detail
        .strip_prefix(&format!("{case_id} event "))
        .and_then(|tail| tail.split_once(' '))
        .and_then(|(sequence, _)| sequence.parse::<u64>().ok());
    let window = sequence.and_then(|sequence| window_index_for_sequence(windows, sequence));
    format!(
        "{case_id}/window={}/event_seq={}: {detail}",
        window.map_or_else(|| "outside".to_owned(), |value| value.to_string()),
        sequence.map_or_else(|| "unknown".to_owned(), |value| value.to_string())
    )
}

#[derive(Clone, Debug)]
enum ActualEvent {
    Projected { site: &'static str, payload: Value },
    Boundary(BoundaryEvent),
}

#[derive(Clone, Debug)]
struct ComparableEvent {
    site: String,
    payload: Value,
    event_sequence: Option<u64>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct OutputKey {
    path: String,
    source_files: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
fn run_transform_case(
    case_id: &str,
    prepared: &PreparedProgram,
    projected: &ProjectedInputs,
    source_paths: &[String],
    file_table: &Value,
    windows: &[TransformWindow<'_>],
    witness: &WitnessObservation,
    pass: &mut PassReport,
) -> BTreeSet<usize> {
    let (lib_metadata, file_metadata) = authoritative_metadata(prepared, projected);
    let provider = PreparedModuleProvider::new(prepared, &projected.options);
    let file_tags = source_file_tags(prepared, source_paths, file_table, case_id);
    let mut expected_writes = witness
        .writes
        .iter()
        .filter(|write| write.kind == "declaration")
        .map(|write| {
            let key = OutputKey {
                path: normalize_output_path(&write.path, &projected.current_directory),
                source_files: write
                    .source_files
                    .iter()
                    .map(|path| normalize_path(path))
                    .collect(),
            };
            (key, write.clone())
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        expected_writes.len(),
        witness
            .writes
            .iter()
            .filter(|write| write.kind == "declaration")
            .count(),
        "{case_id}: declaration-write keys are unique"
    );
    pass.writes += expected_writes.len() as u64;
    for window in windows {
        if expected_decl_blocked(window) {
            pass.blocked += 1;
        } else {
            pass.unblocked += 1;
        }
    }
    let mut actual_diagnostics = Vec::new();
    let mut callback_ran = false;
    let mut exact_root_windows = BTreeSet::new();

    let checked = check_program_with_authoritative_modules_at_for_emit(
        &projected.libs,
        &projected.files,
        &lib_metadata,
        &file_metadata,
        &projected.options,
        &projected.current_directory,
        &provider,
        |snapshot, checker, checked| {
            assert!(!callback_ran, "{case_id}: checked callback ran twice");
            callback_ran = true;
            assert!(
                checked.partial_checks.is_empty(),
                "{case_id}: authoritative checker reported partial checks"
            );
            let host = HarnessEmitHost::new(prepared, snapshot, &projected.options)
                .unwrap_or_else(|detail| panic!("{case_id}: {detail}"));
            let preflight = preflight_emit(&host, tsc_emitter::EmitSelection::WholeProgram)
                .unwrap_or_else(|error| panic!("{case_id}: preflight failed: {error}"));
            let paths = PlanDeclarationPaths::new(&host, &preflight);
            let units = declaration_units(&preflight);
            assert_eq!(
                units.len(),
                windows.len(),
                "{case_id}: declaration plan/window bijection"
            );
            let recorded = Rc::new(RefCell::new(Vec::<ActualEvent>::new()));
            let resolver = RecordingResolver {
                inner: checker,
                host: &host,
                file_tags: &file_tags,
                recorded: Rc::clone(&recorded),
            };

            for ((source, declaration_path), window) in units.into_iter().zip(windows) {
                recorded.borrow_mut().clear();
                let boundary_recorded = Rc::clone(&recorded);
                let mut observer = move |event| {
                    boundary_recorded
                        .borrow_mut()
                        .push(ActualEvent::Boundary(event));
                };
                let transformed = transform_declaration_unit_with_observer_for_harness(
                    &resolver,
                    &host,
                    &preflight,
                    &paths,
                    source,
                    &mut observer,
                );
                match transformed {
                    Ok((outcome, mut result)) => {
                        pass.actual_roots += 1;
                        if outcome.decl_blocked {
                            pass.actual_blocked += 1;
                        } else {
                            pass.actual_unblocked += 1;
                        }
                        let block_payload = json!([
                            outcome.decl_blocked_inputs.diagnostics_len,
                            outcome.decl_blocked_inputs.is_emit_blocked_evaluated,
                            outcome.decl_blocked_inputs.is_emit_blocked,
                            outcome.decl_blocked_inputs.no_emit == Some(true),
                            outcome.decl_blocked_inputs.decl_blocked,
                        ]);
                        recorded.borrow_mut().push(ActualEvent::Projected {
                            site: "declarations.declBlocked",
                            payload: block_payload,
                        });
                        if compare_window_surface(
                            case_id,
                            window,
                            &recorded.borrow(),
                            Some(&result),
                            &file_tags,
                            pass,
                        ) {
                            exact_root_windows.insert(window.index);
                        }
                        actual_diagnostics.extend(outcome.diagnostics.iter().cloned());
                        compare_window_bytes(
                            case_id,
                            window,
                            source,
                            &declaration_path,
                            &host,
                            &outcome.root,
                            outcome.decl_blocked,
                            &projected.current_directory,
                            &mut result,
                            &mut expected_writes,
                            pass,
                        );
                    }
                    Err(error) => {
                        if compare_window_surface(
                            case_id,
                            window,
                            &recorded.borrow(),
                            None,
                            &file_tags,
                            pass,
                        ) {
                            exact_root_windows.insert(window.index);
                        }
                        pass.mismatches.push(format!(
                            "{case_id}/window={}/event_seq={}: transform failed\n{}",
                            window.index,
                            window.start_sequence,
                            unified_diff(
                                "expected root",
                                "actual transform",
                                "transformed source-file root\n",
                                &format!("error: {error}\n"),
                            )
                        ));
                    }
                }
            }
        },
    )
    .unwrap_or_else(|error| panic!("{case_id}: authoritative check failed: {error}"));
    assert!(callback_ran, "{case_id}: checked callback did not run");
    assert!(
        checked.partial_checks.is_empty(),
        "{case_id}: authoritative check is partial"
    );

    for (key, write) in expected_writes {
        pass.mismatches.push(format!(
            "{case_id}/window=missing/event_seq=missing: declaration write was not consumed\n{}",
            unified_diff(
                "expected",
                "actual",
                &format!("{} {:?}\n", write.path, write.source_files),
                &format!("missing key {} {:?}\n", key.path, key.source_files),
            )
        ));
    }

    let actual_diagnostics = actual_diagnostics
        .iter()
        .map(normalize_diagnostic)
        .collect::<Vec<_>>();
    if actual_diagnostics != witness.emit_result.diagnostics {
        let expected = serde_json::to_string_pretty(&witness.emit_result.diagnostics)
            .expect("expected diagnostics serialize");
        let actual = serde_json::to_string_pretty(&actual_diagnostics)
            .expect("actual diagnostics serialize");
        pass.mismatches.push(format!(
            "{case_id}/window=all/event_seq=all: emit diagnostics differ\n{}",
            unified_diff(
                "expected diagnostics",
                "actual diagnostics",
                &expected,
                &actual
            )
        ));
    }
    pass.diagnostics += witness.emit_result.diagnostics.len() as u64;
    exact_root_windows
}

fn declaration_units(preflight: &EmitPreflight) -> Vec<(SourceFileId, PathBuf)> {
    preflight
        .plan()
        .units()
        .iter()
        .filter_map(|unit| {
            let path = unit.paths().declaration_path()?.to_path_buf();
            let tsc_emitter::EmitRoot::SourceFile(source) = unit.root() else {
                panic!("eligible declaration plan contains a bundle")
            };
            Some((*source, path))
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn compare_window_bytes(
    case_id: &str,
    window: &TransformWindow<'_>,
    source: SourceFileId,
    declaration_path: &Path,
    host: &HarnessEmitHost<'_, '_, '_>,
    root: &TransformRoot,
    decl_blocked: bool,
    current_directory: &str,
    result: &mut TransformationResult<'_>,
    expected_writes: &mut BTreeMap<OutputKey, WitnessWrite>,
    pass: &mut PassReport,
) {
    let source_path = host
        .source_file(source)
        .expect("planned declaration source exists")
        .path()
        .to_string_lossy()
        .into_owned();
    let key = OutputKey {
        path: normalize_output_path(&declaration_path.to_string_lossy(), current_directory),
        source_files: vec![normalize_path(&source_path)],
    };
    let expected_blocked = expected_decl_blocked(window);
    if decl_blocked != expected_blocked {
        pass.mismatches.push(format!(
            "{case_id}/window={}/event_seq={}: decl_blocked expected {expected_blocked}, actual {decl_blocked}",
            window.index, window.end_sequence
        ));
    }
    if expected_blocked {
        if expected_writes.contains_key(&key) {
            pass.mismatches.push(format!(
                "{case_id}/window={}/event_seq={}: blocked root unexpectedly has frozen write {} {:?}",
                window.index, window.end_sequence, key.path, key.source_files
            ));
        }
        return;
    }

    let Some(write) = expected_writes.remove(&key) else {
        pass.mismatches.push(format!(
            "{case_id}/window={}/event_seq={}: no frozen declaration write for {} {:?}",
            window.index, window.end_sequence, key.path, key.source_files
        ));
        return;
    };
    let TransformRoot::SourceFile(root_source) = root else {
        pass.mismatches.push(format!(
            "{case_id}/window={}/event_seq={}: transformed root is a bundle",
            window.index, window.end_sequence
        ));
        return;
    };
    let options = host.compiler_options();
    let new_line = match options.new_line {
        Some(0) => NewLineKind::CarriageReturnLineFeed,
        None | Some(1) => NewLineKind::LineFeed,
        Some(value) => panic!("{case_id}: unsupported frozen newLine value {value}"),
    };
    let printer_options = PrinterOptions::new(new_line)
        .with_remove_comments(options.remove_comments == Some(true))
        .with_no_emit_helpers(true)
        .with_declaration_syntax(true)
        .with_only_print_js_doc_style(true)
        .with_omit_brace_source_map_positions(true)
        .with_target(options.emit_script_target())
        .with_source_file_text_mode(SourceFileTextMode::Canonical);
    let actual = match create_printer(printer_options).print(
        result,
        PrintRequest::SourceFile(*root_source),
        None,
    ) {
        Ok(printed) => printed.text().as_bytes().to_vec(),
        Err(error) => {
            pass.mismatches.push(format!(
                "{case_id}/window={}/event_seq={}: declaration print failed: {error}",
                window.index, window.end_sequence
            ));
            return;
        }
    };
    let expected_callback = write
        .declaration_callback_base64
        .as_deref()
        .map(decode_base64)
        .unwrap_or_else(|| panic!("{case_id}: declaration write lacks callback bytes"));
    if actual != expected_callback {
        pass.mismatches.push(format!(
            "{case_id}/window={}/event_seq={}: declaration callback bytes differ for {}\n{}",
            window.index,
            window.end_sequence,
            write.path,
            unified_diff(
                "expected",
                "actual",
                &String::from_utf8_lossy(&expected_callback),
                &String::from_utf8_lossy(&actual),
            )
        ));
    }
    let actual_bom = options.emit_bom == Some(true);
    if actual_bom != write.write_byte_order_mark {
        pass.mismatches.push(format!(
            "{case_id}/window={}/event_seq={}: BOM flag expected {}, actual {}",
            window.index, window.end_sequence, write.write_byte_order_mark, actual_bom
        ));
    }
    let mut materialized = Vec::with_capacity(actual.len() + usize::from(actual_bom) * 3);
    if actual_bom {
        materialized.extend_from_slice(&[0xef, 0xbb, 0xbf]);
    }
    materialized.extend_from_slice(&actual);
    let expected_materialized = write
        .declaration_materialized_base64
        .as_deref()
        .map(decode_base64)
        .unwrap_or_else(|| panic!("{case_id}: declaration write lacks materialized bytes"));
    if materialized != expected_materialized {
        pass.mismatches.push(format!(
            "{case_id}/window={}/event_seq={}: materialized declaration bytes differ for {}\n{}",
            window.index,
            window.end_sequence,
            write.path,
            unified_diff(
                "expected materialized",
                "actual materialized",
                &String::from_utf8_lossy(&expected_materialized),
                &String::from_utf8_lossy(&materialized),
            )
        ));
    }
}

fn decode_base64(encoded: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .expect("frozen declaration base64 is valid")
}

fn expected_decl_blocked(window: &TransformWindow<'_>) -> bool {
    let event = window.events.last().expect("window has declBlocked");
    assert_eq!(event["site_id"], "declarations.declBlocked");
    event["args"][5]
        .as_bool()
        .expect("declBlocked result is boolean")
}

fn normalize_diagnostic(diagnostic: &Diagnostic) -> WitnessDiagnostic {
    WitnessDiagnostic {
        code: diagnostic.code(),
        category: match diagnostic.category() {
            DiagnosticCategory::Warning => "Warning",
            DiagnosticCategory::Error => "Error",
            DiagnosticCategory::Suggestion => "Suggestion",
            DiagnosticCategory::Message => "Message",
        }
        .to_owned(),
        message: diagnostic.message_text().to_owned(),
        file: diagnostic.file_name.as_deref().map(normalize_path),
        start: diagnostic.start,
        length: diagnostic.length,
    }
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn normalize_output_path(path: &str, current_directory: &str) -> String {
    // The probe records the writeFile callback's path verbatim, while Rust's
    // preflight exposes the same output as an absolute path that may retain a
    // `.` component. Probe runtime lines 130-160 normalize node/source
    // coordinates only; lines 495-505 consume declarationFilePath without
    // rewriting the callback key. Compare the frozen and planned paths in the
    // same lexical, current-directory-relative domain.
    fn collapse(path: &str) -> String {
        let absolute = path.starts_with('/');
        let mut components = Vec::new();
        for component in path.split('/') {
            match component {
                "" | "." => {}
                ".." if components.last().is_some_and(|last| *last != "..") => {
                    components.pop();
                }
                ".." if !absolute => components.push(component),
                ".." => {}
                _ => components.push(component),
            }
        }
        let joined = components.join("/");
        if absolute {
            format!("/{joined}")
        } else {
            joined
        }
    }

    let current_directory = collapse(&normalize_path(current_directory));
    let path = normalize_path(path);
    let absolute = if path.starts_with('/') {
        collapse(&path)
    } else {
        collapse(&format!("{current_directory}/{path}"))
    };
    if absolute == current_directory {
        return ".".to_owned();
    }
    let prefix = if current_directory == "/" {
        "/".to_owned()
    } else {
        format!("{current_directory}/")
    };
    absolute
        .strip_prefix(&prefix)
        .map_or(absolute.clone(), str::to_owned)
}

fn unified_diff(expected_name: &str, actual_name: &str, expected: &str, actual: &str) -> String {
    if expected == actual {
        return String::new();
    }
    let expected_lines = expected.split_inclusive('\n').collect::<Vec<_>>();
    let actual_lines = actual.split_inclusive('\n').collect::<Vec<_>>();
    let first = expected_lines
        .iter()
        .zip(&actual_lines)
        .position(|(expected, actual)| expected != actual)
        .unwrap_or_else(|| expected_lines.len().min(actual_lines.len()));
    let start = first.saturating_sub(3);
    let expected_end = (first + 4).min(expected_lines.len());
    let actual_end = (first + 4).min(actual_lines.len());
    let mut diff = format!(
        "--- {expected_name}\n+++ {actual_name}\n@@ -{},{} +{},{} @@\n",
        start + 1,
        expected_end.saturating_sub(start),
        start + 1,
        actual_end.saturating_sub(start)
    );
    for line in &expected_lines[start..first.min(expected_end)] {
        diff.push(' ');
        diff.push_str(line);
        if !line.ends_with('\n') {
            diff.push('\n');
        }
    }
    for line in &expected_lines[first.min(expected_end)..expected_end] {
        diff.push('-');
        diff.push_str(line);
        if !line.ends_with('\n') {
            diff.push('\n');
        }
    }
    for line in &actual_lines[first.min(actual_end)..actual_end] {
        diff.push('+');
        diff.push_str(line);
        if !line.ends_with('\n') {
            diff.push('\n');
        }
    }
    diff
}

fn is_surface_expected_event(event: &Value) -> bool {
    let site = event["site_id"].as_str().expect("site_id is a string");
    matches!(
        site,
        "declarations.visitDeclarationSubtree.changed"
            | "declarations.transformTopLevelDeclaration.changed"
            | "declarations.declBlocked"
    ) || (event["depth"].as_u64() == Some(1)
        && site.starts_with("resolver.")
        && site.ends_with(".entry")
        && site != "resolver.hasGlobalName.entry")
}

fn compare_window_surface(
    case_id: &str,
    window: &TransformWindow<'_>,
    actual: &[ActualEvent],
    result: Option<&TransformationResult<'_>>,
    file_tags: &BTreeMap<SourceFileId, u64>,
    pass: &mut PassReport,
) -> bool {
    let expected = window
        .events
        .iter()
        .filter(|event| is_surface_expected_event(event))
        .map(|event| expected_surface_event(event))
        .collect::<Vec<_>>();
    let actual = actual
        .iter()
        .filter_map(|event| actual_surface_event(event, result, file_tags))
        .collect::<Vec<_>>();
    for event in &actual {
        pass.count_actual(&event.site);
    }
    let expected_roots = expected
        .iter()
        .filter(|event| is_root_resolver_entry(&event.site))
        .map(|event| (&event.site, &event.payload))
        .collect::<Vec<_>>();
    let actual_roots = actual
        .iter()
        .filter(|event| is_root_resolver_entry(&event.site))
        .map(|event| (&event.site, &event.payload))
        .collect::<Vec<_>>();
    let roots_exact = expected_roots == actual_roots;

    let compared = expected.len().max(actual.len());
    for index in 0..compared {
        match (expected.get(index), actual.get(index)) {
            (Some(expected), Some(actual))
                if expected.site == actual.site && expected.payload == actual.payload => {}
            (Some(expected), Some(actual)) => pass.divergences.push(format!(
                "{case_id}/window={}/event_seq={}: expected {} {}, actual {} {}",
                window.index,
                expected.event_sequence.expect("expected sequence"),
                expected.site,
                expected.payload,
                actual.site,
                actual.payload
            )),
            (Some(expected), None) => pass.divergences.push(format!(
                "{case_id}/window={}/event_seq={}: unmatched expected {} {}",
                window.index,
                expected.event_sequence.expect("expected sequence"),
                expected.site,
                expected.payload
            )),
            (None, Some(actual)) => pass.divergences.push(format!(
                "{case_id}/window={}/event_seq=after-{}: unmatched actual {} {}",
                window.index, window.end_sequence, actual.site, actual.payload
            )),
            (None, None) => unreachable!(),
        }
    }
    roots_exact
}

fn is_root_resolver_entry(site: &str) -> bool {
    site.starts_with("resolver.") && site.ends_with(".entry")
}

fn expected_surface_event(event: &Value) -> ComparableEvent {
    let site = event["site_id"]
        .as_str()
        .expect("site_id is a string")
        .to_owned();
    let args = event["args"].as_array().expect("event args are an array");
    let payload = match site.as_str() {
        "declarations.visitDeclarationSubtree.changed"
        | "declarations.transformTopLevelDeclaration.changed" => json!([
            normalize_expected_node_ref(&args[1]),
            normalize_expected_node_ref(&args[2]),
            args[3],
            args[4],
        ]),
        "declarations.declBlocked" => Value::Array(args[1..].to_vec()),
        "resolver.isEntityNameVisible.entry" => json!([
            args[1],
            normalize_expected_node_ref(&args[2]),
            normalize_expected_node_ref(&args[3]),
            args[4],
            args[5],
        ]),
        "resolver.isSymbolAccessible.entry" => json!([
            args[1],
            args[2],
            normalize_expected_node_ref(&args[3]),
            args[4],
            args[5],
        ]),
        "resolver.collectLinkedAliases.entry" => {
            json!([normalize_expected_node_ref(&args[1]), args[2],])
        }
        site if site.starts_with("resolver.") && site.ends_with(".entry") => json!([
            args[1],
            args[2],
            normalize_expected_node_ref(&args[3]),
            normalize_expected_node_ref(&args[4]),
            args[5],
            args[6],
        ]),
        _ => unreachable!("surface filter admitted {site}"),
    };
    ComparableEvent {
        site,
        payload,
        event_sequence: Some(event_sequence(event)),
    }
}

fn normalize_expected_node_ref(value: &Value) -> Value {
    let values = value
        .as_array()
        .filter(|values| values.len() == 8)
        .expect("node ref is an eight-element array");
    if values[0].as_i64().is_some_and(|value| value >= 0) {
        json!(["parse-own", [values[0], values[1], values[2], values[3]]])
    } else if values[4].as_i64().is_some_and(|value| value >= 0) {
        json!([
            "original-projected",
            [values[4], values[5], values[6], values[7]]
        ])
    } else {
        json!(["synthetic-without-original", Value::Null])
    }
}

fn actual_surface_event(
    event: &ActualEvent,
    result: Option<&TransformationResult<'_>>,
    file_tags: &BTreeMap<SourceFileId, u64>,
) -> Option<ComparableEvent> {
    match event {
        ActualEvent::Projected { site, payload } => Some(ComparableEvent {
            site: (*site).to_owned(),
            payload: payload.clone(),
            event_sequence: None,
        }),
        ActualEvent::Boundary(event) => {
            let result = result?;
            let arena = result.arena();
            // Probe transform wrappers are attached to the two functions, not
            // inferred from AST ancestry (probe runtime :151, edit table
            // :461-474). Nested module statements are a frozen counterexample.
            let site = if event.is_top_level {
                "declarations.transformTopLevelDeclaration.changed"
            } else {
                "declarations.visitDeclarationSubtree.changed"
            };
            Some(ComparableEvent {
                site: site.to_owned(),
                payload: json!([
                    project_transform_node(arena, Some(event.input_ref), file_tags),
                    project_transform_node(arena, event.output_ref, file_tags),
                    event.has_original,
                    event.transform_flags.bits(),
                ]),
                event_sequence: None,
            })
        }
    }
}

fn project_transform_node(
    arena: &TransformArena,
    node: Option<TransformNode>,
    file_tags: &BTreeMap<SourceFileId, u64>,
) -> Value {
    let Some(mut node) = node else {
        return json!(["synthetic-without-original", Value::Null]);
    };
    if arena.is_parsed_node(node).unwrap_or(false) {
        return json!(["parse-own", transform_coordinate(arena, node, file_tags)]);
    }
    let mut remaining = 1 + arena.sources().len() + 64;
    while remaining != 0 {
        let Some(original) = arena
            .metadata(node)
            .and_then(tsc_emitter::EmitMetadata::original)
        else {
            return json!(["synthetic-without-original", Value::Null]);
        };
        if arena.is_parsed_node(original).unwrap_or(false) {
            return json!([
                "original-projected",
                transform_coordinate(arena, original, file_tags)
            ]);
        }
        if original == node {
            break;
        }
        node = original;
        remaining -= 1;
    }
    json!(["synthetic-without-original", Value::Null])
}

fn transform_coordinate(
    arena: &TransformArena,
    node: TransformNode,
    file_tags: &BTreeMap<SourceFileId, u64>,
) -> Value {
    let source = arena
        .source(node.source())
        .expect("transform source exists");
    let program_source = source
        .program_source()
        .expect("declaration transform source has Program identity");
    let record = arena.node(node).expect("transform node exists");
    json!([
        file_tags[&program_source],
        record.kind as u16,
        record.pos,
        record.end
    ])
}

struct RecordingResolver<'resolver, 'program, 'prepared, 'snapshot, 'options> {
    inner: &'resolver CheckerSession<'program>,
    host: &'resolver HarnessEmitHost<'prepared, 'snapshot, 'options>,
    file_tags: &'resolver BTreeMap<SourceFileId, u64>,
    recorded: Rc<RefCell<Vec<ActualEvent>>>,
}

impl RecordingResolver<'_, '_, '_, '_, '_> {
    fn resolver_node_ref(&self, node: EmitResolverNode) -> Value {
        let source = self
            .host
            .source_file(node.source())
            .expect("resolver source is present");
        let syntax = source.syntax().expect("checked source syntax is present");
        let record = syntax.arena.node(node.node());
        json!([
            "parse-own",
            [
                self.file_tags[&node.source()],
                record.kind as u16,
                record.pos,
                record.end
            ]
        ])
    }

    fn absent_ref() -> Value {
        json!(["synthetic-without-original", Value::Null])
    }

    fn object_scalar() -> Value {
        json!(["object", "", 0, false])
    }

    fn undefined_scalar() -> Value {
        json!(["undefined", "", 0, false])
    }

    fn number_scalar(value: u32) -> Value {
        json!(["number", "", value, false])
    }

    fn record(&self, site: &'static str, payload: Value) {
        self.recorded
            .borrow_mut()
            .push(ActualEvent::Projected { site, payload });
    }

    fn record_unary(&self, site: &'static str, node: EmitResolverNode) {
        self.record(
            site,
            json!([
                1,
                "",
                self.resolver_node_ref(node),
                Self::absent_ref(),
                Self::object_scalar(),
                Self::undefined_scalar(),
            ]),
        );
    }

    fn record_node_pair(
        &self,
        site: &'static str,
        arity: u32,
        first: EmitResolverNode,
        second: EmitResolverNode,
    ) {
        self.record(
            site,
            json!([
                arity,
                "",
                self.resolver_node_ref(first),
                self.resolver_node_ref(second),
                Self::object_scalar(),
                Self::object_scalar(),
            ]),
        );
    }

    fn record_node_and_absent_object(
        &self,
        site: &'static str,
        arity: u32,
        first: EmitResolverNode,
    ) {
        self.record(
            site,
            json!([
                arity,
                "",
                self.resolver_node_ref(first),
                Self::absent_ref(),
                Self::object_scalar(),
                Self::object_scalar(),
            ]),
        );
    }
}

impl EmitResolver for RecordingResolver<'_, '_, '_, '_, '_> {
    fn get_enum_member_value(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitEnumMemberValue>, EmitResolverError> {
        self.record_unary("resolver.getEnumMemberValue.entry", node);
        self.inner.get_enum_member_value(node)
    }

    fn is_definitely_reference_to_global_symbol_object(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        self.record_unary(
            "resolver.isDefinitelyReferenceToGlobalSymbolObject.entry",
            node,
        );
        self.inner
            .is_definitely_reference_to_global_symbol_object(node)
    }

    fn is_symbol_accessible(
        &self,
        symbol: EmitResolverSymbol,
        enclosing_declaration: EmitResolverNode,
        meaning: EmitSymbolMeaning,
        should_compute_aliases: bool,
    ) -> Result<EmitSymbolAccessibilityResult, EmitResolverError> {
        self.record(
            "resolver.isSymbolAccessible.entry",
            json!([
                4,
                [format!("<symbol:{}>", symbol.symbol_index), 0, []],
                self.resolver_node_ref(enclosing_declaration),
                meaning.0,
                should_compute_aliases,
            ]),
        );
        self.inner.is_symbol_accessible(
            symbol,
            enclosing_declaration,
            meaning,
            should_compute_aliases,
        )
    }

    fn is_entity_name_visible(
        &self,
        entity_name: EmitResolverNode,
        enclosing_declaration: EmitResolverNode,
    ) -> Result<EmitSymbolAccessibilityResult, EmitResolverError> {
        self.record(
            "resolver.isEntityNameVisible.entry",
            json!([
                2,
                self.resolver_node_ref(entity_name),
                self.resolver_node_ref(enclosing_declaration),
                // The probe wrapper passes the defaulted third parameter into
                // `__h27aEntryArgs`; a two-argument upstream call therefore
                // records `true` here. See h2-7a-probe-traces.mjs:141, :242
                // and the frozen `inaccessible-substitution` entry.
                true,
                false,
            ]),
        );
        self.inner
            .is_entity_name_visible(entity_name, enclosing_declaration)
    }

    fn is_declaration_visible(&self, node: EmitResolverNode) -> Result<bool, EmitResolverError> {
        self.record_unary("resolver.isDeclarationVisible.entry", node);
        self.inner.is_declaration_visible(node)
    }

    fn is_optional_parameter(&self, node: EmitResolverNode) -> Result<bool, EmitResolverError> {
        self.record_unary("resolver.isOptionalParameter.entry", node);
        self.inner.is_optional_parameter(node)
    }

    fn is_implementation_of_overload(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        self.record_unary("resolver.isImplementationOfOverload.entry", node);
        self.inner.is_implementation_of_overload(node)
    }

    fn requires_adding_implicit_undefined(
        &self,
        parameter: EmitResolverNode,
        enclosing_declaration: Option<EmitResolverNode>,
    ) -> Result<bool, EmitResolverError> {
        let second_ref = enclosing_declaration
            .map(|node| self.resolver_node_ref(node))
            .unwrap_or_else(Self::absent_ref);
        let second_scalar = if enclosing_declaration.is_some() {
            Self::object_scalar()
        } else {
            Self::undefined_scalar()
        };
        self.record(
            "resolver.requiresAddingImplicitUndefined.entry",
            json!([
                if enclosing_declaration.is_some() {
                    2
                } else {
                    1
                },
                "",
                self.resolver_node_ref(parameter),
                second_ref,
                Self::object_scalar(),
                second_scalar,
            ]),
        );
        self.inner
            .requires_adding_implicit_undefined(parameter, enclosing_declaration)
    }

    fn is_expando_function_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        self.record_unary("resolver.isExpandoFunctionDeclaration.entry", node);
        self.inner.is_expando_function_declaration(node)
    }

    fn get_properties_of_container_function(
        &self,
        node: EmitResolverNode,
    ) -> Result<Vec<EmitFunctionProperty>, EmitResolverError> {
        self.record_unary("resolver.getPropertiesOfContainerFunction.entry", node);
        self.inner.get_properties_of_container_function(node)
    }

    fn is_literal_const_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        self.record_unary("resolver.isLiteralConstDeclaration.entry", node);
        self.inner.is_literal_const_declaration(node)
    }

    fn is_late_bound(&self, node: EmitResolverNode) -> Result<bool, EmitResolverError> {
        self.record_unary("resolver.isLateBound.entry", node);
        self.inner.is_late_bound(node)
    }

    fn is_import_required_by_augmentation(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        self.record_unary("resolver.isImportRequiredByAugmentation.entry", node);
        self.inner.is_import_required_by_augmentation(node)
    }

    fn create_type_of_declaration(
        &self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        declaration: EmitResolverNode,
        enclosing_declaration: EmitResolverNode,
        flags: EmitNodeBuilderFlags,
        internal_flags: EmitInternalNodeBuilderFlags,
        tracker: &mut dyn EmitSymbolTracker,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        self.record_node_pair(
            "resolver.createTypeOfDeclaration.entry",
            5,
            declaration,
            enclosing_declaration,
        );
        self.inner.create_type_of_declaration(
            arena,
            target,
            declaration,
            enclosing_declaration,
            flags,
            internal_flags,
            tracker,
        )
    }

    fn create_type_of_declaration_in_expando_scope(
        &self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        declaration: EmitResolverNode,
        function: EmitResolverNode,
        enclosing_declaration: EmitResolverNode,
        flags: EmitNodeBuilderFlags,
        internal_flags: EmitInternalNodeBuilderFlags,
        tracker: &mut dyn EmitSymbolTracker,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        self.record(
            "resolver.createTypeOfDeclaration.entry",
            json!([
                5,
                "",
                self.resolver_node_ref(declaration),
                Self::absent_ref(),
                Self::object_scalar(),
                Self::object_scalar(),
            ]),
        );
        self.inner.create_type_of_declaration_in_expando_scope(
            arena,
            target,
            declaration,
            function,
            enclosing_declaration,
            flags,
            internal_flags,
            tracker,
        )
    }

    fn is_last_bodiless_overload_of_symbol(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        self.inner.is_last_bodiless_overload_of_symbol(node)
    }

    fn is_first_declaration_of_symbol(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        self.inner.is_first_declaration_of_symbol(node)
    }

    fn create_return_type_of_signature_declaration(
        &self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        signature_declaration: EmitResolverNode,
        enclosing_declaration: EmitResolverNode,
        flags: EmitNodeBuilderFlags,
        internal_flags: EmitInternalNodeBuilderFlags,
        tracker: &mut dyn EmitSymbolTracker,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        self.record_node_pair(
            "resolver.createReturnTypeOfSignatureDeclaration.entry",
            5,
            signature_declaration,
            enclosing_declaration,
        );
        self.inner.create_return_type_of_signature_declaration(
            arena,
            target,
            signature_declaration,
            enclosing_declaration,
            flags,
            internal_flags,
            tracker,
        )
    }

    fn create_type_of_expression(
        &self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        expression: EmitResolverNode,
        enclosing_declaration: EmitResolverNode,
        flags: EmitNodeBuilderFlags,
        internal_flags: EmitInternalNodeBuilderFlags,
        tracker: &mut dyn EmitSymbolTracker,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        self.record_node_pair(
            "resolver.createTypeOfExpression.entry",
            5,
            expression,
            enclosing_declaration,
        );
        self.inner.create_type_of_expression(
            arena,
            target,
            expression,
            enclosing_declaration,
            flags,
            internal_flags,
            tracker,
        )
    }

    fn create_literal_const_value(
        &self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        node: EmitResolverNode,
        tracker: &mut dyn EmitSymbolTracker,
    ) -> Result<TransformNode, EmitResolverError> {
        self.record_node_and_absent_object("resolver.createLiteralConstValue.entry", 2, node);
        self.inner
            .create_literal_const_value(arena, target, node, tracker)
    }

    fn get_declaration_statements_for_source_file(
        &self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        node: EmitResolverNode,
        flags: EmitNodeBuilderFlags,
        internal_flags: EmitInternalNodeBuilderFlags,
        tracker: &mut dyn EmitSymbolTracker,
    ) -> Result<Option<Vec<TransformNode>>, EmitResolverError> {
        self.record(
            "resolver.getDeclarationStatementsForSourceFile.entry",
            json!([
                3,
                "",
                self.resolver_node_ref(node),
                Self::absent_ref(),
                Self::object_scalar(),
                Self::number_scalar(flags.0),
            ]),
        );
        self.inner.get_declaration_statements_for_source_file(
            arena,
            target,
            node,
            flags,
            internal_flags,
            tracker,
        )
    }

    fn create_late_bound_index_signatures(
        &self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        container: EmitResolverNode,
        enclosing_declaration: EmitResolverNode,
        flags: EmitNodeBuilderFlags,
        internal_flags: EmitInternalNodeBuilderFlags,
        tracker: &mut dyn EmitSymbolTracker,
    ) -> Result<Option<Vec<TransformNode>>, EmitResolverError> {
        self.record_node_pair(
            "resolver.createLateBoundIndexSignatures.entry",
            4,
            container,
            enclosing_declaration,
        );
        self.inner.create_late_bound_index_signatures(
            arena,
            target,
            container,
            enclosing_declaration,
            flags,
            internal_flags,
            tracker,
        )
    }
}

struct HarnessEmitHost<'prepared, 'snapshot, 'options> {
    prepared: &'prepared PreparedProgram,
    snapshot: &'snapshot ProgramSnapshot,
    options: &'options tsc_checker::CompilerOptions,
    source_files: Vec<SourceFileId>,
    common_source_directory: PathBuf,
}

impl<'prepared, 'snapshot, 'options> HarnessEmitHost<'prepared, 'snapshot, 'options> {
    fn new(
        prepared: &'prepared PreparedProgram,
        snapshot: &'snapshot ProgramSnapshot,
        options: &'options tsc_checker::CompilerOptions,
    ) -> Result<Self, String> {
        let source_files = prepared
            .source_files()
            .iter()
            .map(|source| {
                prepared
                    .source_id(source.path().canonical())
                    .ok_or_else(|| {
                        format!(
                            "prepared source {} lacks stable identity",
                            source.path().display().display()
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let common_source_directory =
            common_emit_source_directory(prepared, options, &source_files);
        Ok(Self {
            prepared,
            snapshot,
            options,
            source_files,
            common_source_directory,
        })
    }
}

impl EmitHost for HarnessEmitHost<'_, '_, '_> {
    fn compiler_options(&self) -> &tsc_checker::CompilerOptions {
        self.options
    }

    fn current_directory(&self) -> &Path {
        self.prepared.current_directory().display()
    }

    fn common_source_directory(&self) -> &Path {
        &self.common_source_directory
    }

    fn config_file_path(&self) -> Option<&Path> {
        self.prepared
            .program_options()
            .config_file_path()
            .map(|path| path.display())
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.prepared.path_context().use_case_sensitive_file_names()
    }

    fn source_file_ids(&self) -> &[SourceFileId] {
        &self.source_files
    }

    fn source_file(&self, id: SourceFileId) -> Option<tsc_emitter::EmitSource<'_>> {
        let source = self.prepared.source_file(id)?;
        let expected_name = source.path().display().to_string_lossy();
        let syntax = self
            .snapshot
            .documents()
            .get(id.index())
            .filter(|document| document.source().file_name == expected_name)
            .or_else(|| {
                self.snapshot
                    .documents()
                    .iter()
                    .find(|document| document.source().file_name == expected_name)
            })
            .map(|document| document.source());
        Some(tsc_emitter::EmitSource::new(
            id,
            source.path().display(),
            source.path().canonical().as_path(),
            source.may_be_emitted(),
            source.implied_node_format_for_emit(),
            syntax,
        ))
    }
}

fn common_emit_source_directory(
    prepared: &PreparedProgram,
    options: &tsc_checker::CompilerOptions,
    source_files: &[SourceFileId],
) -> PathBuf {
    if let Some(root_dir) = options.root_dir.as_deref() {
        let root = Path::new(root_dir);
        return if root.is_absolute() {
            root.to_path_buf()
        } else {
            prepared.current_directory().display().join(root)
        };
    }
    let mut directories = source_files.iter().filter_map(|id| {
        let source = prepared.source_file(*id)?;
        (source.may_be_emitted() && !is_declaration_file_name(source.path().display()))
            .then(|| source.path().display().parent().map(Path::to_path_buf))
            .flatten()
    });
    let Some(mut common) = directories.next() else {
        return prepared.current_directory().display().to_path_buf();
    };
    let case_sensitive = prepared.path_context().use_case_sensitive_file_names();
    for directory in directories {
        while !path_starts_with(&directory, &common, case_sensitive) {
            if !common.pop() {
                return prepared.current_directory().display().to_path_buf();
            }
        }
    }
    common
}

fn path_starts_with(path: &Path, prefix: &Path, case_sensitive: bool) -> bool {
    if case_sensitive {
        path.starts_with(prefix)
    } else {
        path.to_string_lossy()
            .to_lowercase()
            .starts_with(&prefix.to_string_lossy().to_lowercase())
    }
}

struct PlanDeclarationPaths {
    paths: BTreeMap<SourceFileId, EmitOutputPaths>,
    source_paths: BTreeMap<SourceFileId, PathBuf>,
}

impl PlanDeclarationPaths {
    fn new(host: &dyn EmitHost, preflight: &EmitPreflight) -> Self {
        let paths = preflight
            .plan()
            .units()
            .iter()
            .filter_map(|unit| {
                let tsc_emitter::EmitRoot::SourceFile(source) = unit.root() else {
                    return None;
                };
                Some((*source, unit.paths().clone()))
            })
            .collect();
        let source_paths = host
            .source_file_ids()
            .iter()
            .filter_map(|&source| {
                host.source_file(source)
                    .map(|emit_source| (source, emit_source.path().to_path_buf()))
            })
            .collect();
        Self {
            paths,
            source_paths,
        }
    }
}

impl DeclarationPathResolver for PlanDeclarationPaths {
    fn declaration_file_path(&self, source: SourceFileId) -> Option<PathBuf> {
        self.paths
            .get(&source)
            .and_then(EmitOutputPaths::declaration_path)
            .map(Path::to_path_buf)
    }

    fn reference_target_path(&self, source: SourceFileId) -> Option<PathBuf> {
        self.paths
            .get(&source)
            .and_then(|paths| paths.declaration_path().or_else(|| paths.javascript_path()))
            .map(Path::to_path_buf)
            .or_else(|| self.source_paths.get(&source).cloned())
    }
}

fn source_file_tags(
    prepared: &PreparedProgram,
    source_paths: &[String],
    file_table: &Value,
    case_id: &str,
) -> BTreeMap<SourceFileId, u64> {
    let mut tags = BTreeMap::new();
    for (tag, row) in file_table
        .as_array()
        .expect("fileTable is an array")
        .iter()
        .enumerate()
    {
        let row = row
            .as_array()
            .filter(|row| row.len() == 2)
            .unwrap_or_else(|| panic!("{case_id}: malformed fileTable row {tag}"));
        if row[0].as_str() != Some("src") {
            continue;
        }
        let source_index = row[1]
            .as_u64()
            .and_then(|index| usize::try_from(index).ok())
            .unwrap_or_else(|| panic!("{case_id}: invalid source index in fileTable row {tag}"));
        let path = source_paths
            .get(source_index)
            .unwrap_or_else(|| panic!("{case_id}: source index {source_index} is out of range"));
        let matching = prepared
            .source_files()
            .iter()
            .filter(|source| source.path().display().to_string_lossy() == path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            matching.len(),
            1,
            "{case_id}: fileTable source {path} is not unique"
        );
        let source = prepared
            .source_id(matching[0].path().canonical())
            .expect("matched source has identity");
        assert!(
            tags.insert(source, tag as u64).is_none(),
            "{case_id}: source has duplicate file tags"
        );
    }
    tags
}

fn authoritative_metadata(
    prepared: &PreparedProgram,
    projected: &ProjectedInputs,
) -> (
    Vec<AuthoritativeSourceMetadata>,
    Vec<AuthoritativeSourceMetadata>,
) {
    let library_ids = prepared.library_files();
    let lib_metadata = library_ids
        .iter()
        .copied()
        .map(|source| project_source_metadata(prepared, source))
        .collect::<Vec<_>>();
    let file_metadata = prepared
        .source_files()
        .iter()
        .skip(library_ids.len())
        .map(|source| {
            let source = prepared
                .source_id(source.path().canonical())
                .expect("prepared source has identity");
            project_source_metadata(prepared, source)
        })
        .collect::<Vec<_>>();
    assert_eq!(lib_metadata.len(), projected.libs.len());
    assert_eq!(file_metadata.len(), projected.files.len());
    (lib_metadata, file_metadata)
}

fn project_source_metadata(
    prepared: &PreparedProgram,
    source_file: SourceFileId,
) -> AuthoritativeSourceMetadata {
    let source = prepared
        .source_file(source_file)
        .expect("prepared source exists");
    AuthoritativeSourceMetadata {
        token: AuthoritativeSourceToken(source_file.raw()),
        file_name: source.path().display().to_string_lossy().into_owned(),
        may_be_emitted: source.may_be_emitted(),
        implied_node_format: source.implied_node_format().map(checker_resolution_mode),
        implied_node_format_for_emit: source
            .implied_node_format_for_emit()
            .map(checker_resolution_mode),
    }
}

const fn checker_resolution_mode(mode: ResolutionMode) -> AuthoritativeResolutionMode {
    match mode {
        ResolutionMode::CommonJs => AuthoritativeResolutionMode::CommonJs,
        ResolutionMode::EsNext => AuthoritativeResolutionMode::EsNext,
        ResolutionMode::Unspecified => AuthoritativeResolutionMode::Unspecified,
    }
}

struct PreparedModuleProvider<'a> {
    prepared: &'a PreparedProgram,
    options: &'a tsc_checker::CompilerOptions,
    request_plans: RefCell<BTreeMap<SourceFileId, SourceRequestPlan>>,
}

impl<'a> PreparedModuleProvider<'a> {
    fn new(prepared: &'a PreparedProgram, options: &'a tsc_checker::CompilerOptions) -> Self {
        Self {
            prepared,
            options,
            request_plans: RefCell::new(BTreeMap::new()),
        }
    }

    fn source_request_plan(
        &self,
        source_file: SourceFileId,
        source: &PreparedSourceFile,
    ) -> Result<SourceRequestPlan, AuthoritativeModuleLookupFailure> {
        if let Some(plan) = self.request_plans.borrow().get(&source_file) {
            return Ok(plan.clone());
        }
        let plan = plan_source_requests(source, self.options).map_err(|_| {
            AuthoritativeModuleLookupFailure::Unsupported(
                UnsupportedAuthoritativeResolution::UnloadedTargetAdmission,
            )
        })?;
        self.request_plans
            .borrow_mut()
            .insert(source_file, plan.clone());
        Ok(plan)
    }

    fn module_request_loads_source(
        &self,
        source_file: SourceFileId,
        source: &PreparedSourceFile,
        key: &ResolutionKey,
    ) -> Result<bool, AuthoritativeModuleLookupFailure> {
        if let Some(plan) = self.request_plans.borrow().get(&source_file) {
            return plan.module_request_loads_source(key).ok_or(
                AuthoritativeModuleLookupFailure::Unsupported(
                    UnsupportedAuthoritativeResolution::UnloadedTargetAdmission,
                ),
            );
        }
        self.source_request_plan(source_file, source)?
            .module_request_loads_source(key)
            .ok_or(AuthoritativeModuleLookupFailure::Unsupported(
                UnsupportedAuthoritativeResolution::UnloadedTargetAdmission,
            ))
    }
}

impl AuthoritativeModuleProvider for PreparedModuleProvider<'_> {
    fn resolve_module(
        &self,
        request: AuthoritativeModuleRequest<'_>,
    ) -> Result<AuthoritativeModuleResolution, AuthoritativeModuleLookupFailure> {
        let source_file = SourceFileId::from_raw(request.source_token.0);
        let Some(source) = self.prepared.source_file(source_file) else {
            return Err(AuthoritativeModuleLookupFailure::InvalidSourceToken);
        };
        let key = ResolutionKey::new(
            source.path().canonical().clone(),
            request.specifier,
            program_resolution_mode(request.mode),
        );
        let resolution = match self.prepared.resolutions().require_module(&key) {
            Ok(resolution) => resolution,
            Err(_) => {
                let plan = self.source_request_plan(source_file, source)?;
                if plan
                    .unpreprocessed_module_requests()
                    .any(|unpreprocessed| unpreprocessed == &key)
                {
                    return Ok(AuthoritativeModuleResolution::NotFound(
                        AuthoritativeNotFoundModule {
                            alternate_result: None,
                        },
                    ));
                }
                return Err(AuthoritativeModuleLookupFailure::Missing);
            }
        };
        if !resolution.diagnostics().is_empty() {
            return Err(AuthoritativeModuleLookupFailure::Unsupported(
                UnsupportedAuthoritativeResolution::ResolutionDiagnostics,
            ));
        }
        let ResolutionOutcome::Resolved(module) = resolution.outcome() else {
            let alternate_result = resolution
                .alternate_result()
                .map(|path| {
                    path.display().to_str().map(str::to_owned).ok_or(
                        AuthoritativeModuleLookupFailure::Unsupported(
                            UnsupportedAuthoritativeResolution::ResolvedFileIdentity,
                        ),
                    )
                })
                .transpose()?;
            return Ok(AuthoritativeModuleResolution::NotFound(
                AuthoritativeNotFoundModule { alternate_result },
            ));
        };
        if let ResolvedModuleTarget::Unloaded {
            resolved_file,
            reason,
        } = module.target()
        {
            let arbitrary_declaration = matches!(
                module.extension(),
                ModuleExtension::Arbitrary(extension)
                    if extension.starts_with(".d.") && extension.ends_with(".ts")
            );
            let jsx_syntax_extension = matches!(
                module.extension(),
                ModuleExtension::Tsx | ModuleExtension::Jsx
            );
            if !module.extension().is_javascript()
                && !jsx_syntax_extension
                && !arbitrary_declaration
                && !matches!(reason, UnloadedModuleReason::NoResolve)
            {
                return Err(AuthoritativeModuleLookupFailure::Unsupported(
                    UnsupportedAuthoritativeResolution::UnloadedTargetExtension,
                ));
            }
            if jsx_syntax_extension
                && self.options.jsx.unwrap_or(0) == 0
                && !matches!(reason, UnloadedModuleReason::JsxWithoutJsxOption)
            {
                return Err(AuthoritativeModuleLookupFailure::Unsupported(
                    UnsupportedAuthoritativeResolution::UnloadedJsxWithoutJsxOption,
                ));
            }
            let loads_source = self.module_request_loads_source(source_file, source, &key)?;
            if arbitrary_declaration
                && matches!(reason, UnloadedModuleReason::ResolutionOnly)
                && !loads_source
                && (is_declaration_file_name(source.path().display())
                    || self.options.allow_arbitrary_extensions == Some(true))
            {
                let alternate_result = resolution
                    .alternate_result()
                    .map(|path| {
                        path.display().to_str().map(str::to_owned).ok_or(
                            AuthoritativeModuleLookupFailure::Unsupported(
                                UnsupportedAuthoritativeResolution::ResolvedFileIdentity,
                            ),
                        )
                    })
                    .transpose()?;
                return Ok(AuthoritativeModuleResolution::NotFound(
                    AuthoritativeNotFoundModule { alternate_result },
                ));
            }
            let node_modules_depth_applies = module.is_external_library_import()
                && (module.original_path().is_none()
                    || path_contains_node_modules(resolved_file.canonical().as_path()));
            let first_node_modules_javascript_layer_is_admitted =
                !self.options.node_modules_depth_exceeds_limit(1);
            let resolution_diagnostic = match reason {
                UnloadedModuleReason::NoResolve if self.options.no_resolve == Some(true) => None,
                UnloadedModuleReason::JsxWithoutJsxOption
                    if jsx_syntax_extension && self.options.jsx.unwrap_or(0) == 0 =>
                {
                    Some(AuthoritativeModuleResolutionDiagnostic::JsxWithoutJsxOption)
                }
                UnloadedModuleReason::ArbitraryExtensionWithoutOption
                    if arbitrary_declaration
                        && loads_source
                        && self.options.allow_arbitrary_extensions != Some(true)
                        && !is_declaration_file_name(source.path().display()) =>
                {
                    Some(AuthoritativeModuleResolutionDiagnostic::ArbitraryExtensionWithoutOption)
                }
                UnloadedModuleReason::ResolutionOnly if !loads_source => (arbitrary_declaration
                    && self.options.allow_arbitrary_extensions != Some(true)
                    && !is_declaration_file_name(source.path().display()))
                .then_some(
                    AuthoritativeModuleResolutionDiagnostic::ArbitraryExtensionWithoutOption,
                ),
                UnloadedModuleReason::NodeModulesDepth
                    if module.extension().is_javascript()
                        && loads_source
                        && node_modules_depth_applies =>
                {
                    None
                }
                UnloadedModuleReason::JavaScriptNotAdmitted
                    if module.extension().is_javascript()
                        && loads_source
                        && !self.options.allow_js
                        && (!node_modules_depth_applies
                            || first_node_modules_javascript_layer_is_admitted) =>
                {
                    None
                }
                _ => {
                    return Err(AuthoritativeModuleLookupFailure::Unsupported(
                        UnsupportedAuthoritativeResolution::UnloadedTargetAdmission,
                    ));
                }
            };
            let resolved_file_name = resolved_file
                .display()
                .to_str()
                .ok_or(AuthoritativeModuleLookupFailure::Unsupported(
                    UnsupportedAuthoritativeResolution::ResolvedFileIdentity,
                ))?
                .to_owned();
            let alternate_result = resolution
                .alternate_result()
                .map(|path| {
                    path.display().to_str().map(str::to_owned).ok_or(
                        AuthoritativeModuleLookupFailure::Unsupported(
                            UnsupportedAuthoritativeResolution::ResolvedFileIdentity,
                        ),
                    )
                })
                .transpose()?;
            if let Some(diagnostic) = resolution_diagnostic {
                return Ok(AuthoritativeModuleResolution::ResolutionDiagnostic(
                    AuthoritativeResolutionDiagnosticModule {
                        resolved_file_name,
                        diagnostic,
                    },
                ));
            }
            return Ok(AuthoritativeModuleResolution::Untyped(
                AuthoritativeUntypedModule {
                    resolved_file_name,
                    package_name: module
                        .package_id()
                        .map(|package_id| package_id.name().to_owned()),
                    alternate_result,
                    types_package_exists: resolution.types_package_exists(),
                    package_bundles_types: resolution.package_bundles_types(),
                },
            ));
        }
        let ResolvedModuleTarget::Source {
            source,
            resolved_file,
        } = module.target()
        else {
            unreachable!("unloaded target returned above")
        };
        if self.prepared.source_file(*source).is_none() {
            return Err(AuthoritativeModuleLookupFailure::InvalidSourceToken);
        }
        Ok(AuthoritativeModuleResolution::Resolved(
            AuthoritativeResolvedModule {
                target_token: AuthoritativeSourceToken(source.raw()),
                resolved_file_name: resolved_file
                    .display()
                    .to_str()
                    .ok_or(AuthoritativeModuleLookupFailure::Unsupported(
                        UnsupportedAuthoritativeResolution::ResolvedFileIdentity,
                    ))?
                    .to_owned(),
                resolved_using_ts_extension: module.resolved_using_ts_extension(),
                is_tsx: matches!(
                    module.extension(),
                    ModuleExtension::Tsx | ModuleExtension::Jsx
                ),
                is_arbitrary_extension: matches!(module.extension(), ModuleExtension::Arbitrary(_)),
                is_external_library_import: module.is_external_library_import(),
                package_id: module
                    .package_id()
                    .map(|package_id| AuthoritativePackageId {
                        name: package_id.name().to_owned(),
                        submodule_name: package_id.submodule_name().to_owned(),
                        version: package_id.version().to_owned(),
                        peer_dependencies: package_id.peer_dependencies().map(str::to_owned),
                    }),
                alternate_result: resolution
                    .alternate_result()
                    .map(|path| {
                        path.display().to_str().map(str::to_owned).ok_or(
                            AuthoritativeModuleLookupFailure::Unsupported(
                                UnsupportedAuthoritativeResolution::ResolvedFileIdentity,
                            ),
                        )
                    })
                    .transpose()?,
                types_package_exists: resolution.types_package_exists(),
                package_bundles_types: resolution.package_bundles_types(),
            },
        ))
    }
}

fn path_contains_node_modules(path: &Path) -> bool {
    path.to_str()
        .is_some_and(|path| path.split('/').any(|component| component == "node_modules"))
}

fn is_declaration_file_name(path: &Path) -> bool {
    path.to_str().is_some_and(|file_name| {
        if file_name.ends_with(".d.ts")
            || file_name.ends_with(".d.cts")
            || file_name.ends_with(".d.mts")
        {
            return true;
        }
        let base_name = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
        base_name.ends_with(".ts") && base_name.contains(".d.")
    })
}

const fn program_resolution_mode(mode: AuthoritativeResolutionMode) -> ResolutionMode {
    match mode {
        AuthoritativeResolutionMode::CommonJs => ResolutionMode::CommonJs,
        AuthoritativeResolutionMode::EsNext => ResolutionMode::EsNext,
        AuthoritativeResolutionMode::Unspecified => ResolutionMode::Unspecified,
    }
}

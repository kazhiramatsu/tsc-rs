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

const EXPECTED_WITNESS_FINGERPRINT: &str =
    "5f669ada78346bf938eb3da23de871a4d6426dab401e5ad5274839ca65beca8d";
const EXPECTED_MANIFEST_FINGERPRINT: &str =
    "89bb0627cee58b5d12aeb6fd5e95a92d26e1bbb54fd592750b49a34b64a89efb";
const EXPECTED_PROBE_FINGERPRINT: &str =
    "34a0e69d990022b0a4ecc08e3415261587b66b5968b572bc7f796897de39b9df";
const EXPECTED_WITNESS_FILE_SHA256: &str =
    "e81e14e6e8de86460d569a0d3b7a8df95be94f18e596e5f79bb8d571ac5a602f";

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
    ("resolver.isDeclarationVisible", [1_361, 5, 0, 0, 0, 0]),
    (
        "resolver.isDefinitelyReferenceToGlobalSymbolObject",
        [10, 0, 0, 0, 0, 0],
    ),
    ("resolver.isEntityNameVisible", [190, 5, 0, 0, 0, 0]),
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
    ("resolver.isOptionalParameter", [97, 3, 0, 0, 0, 0]),
    ("resolver.isSymbolAccessible", [293, 20, 94, 0, 0, 0]),
    (
        "resolver.requiresAddingImplicitUndefined",
        [47, 7, 79, 0, 0, 0],
    ),
];

#[derive(Debug, Deserialize)]
struct WitnessArtifact {
    witnesses_fingerprint_sha256: String,
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
    probe_traces_fingerprint_sha256: String,
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
    assert_eq!(first["cases"], json!(112));
    assert_eq!(first["seed_checks"], json!(388));
    assert_eq!(
        first["traced_nested_edges"],
        json_object_from_pairs(EXPECTED_NESTED_EDGES)
    );
    assert_member_counts(&first["member_counts"]);
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
        witnesses.witnesses_fingerprint_sha256,
        EXPECTED_WITNESS_FINGERPRINT
    );
    assert_eq!(
        witnesses.case_manifest.case_manifest_fingerprint,
        EXPECTED_MANIFEST_FINGERPRINT
    );
    assert_eq!(
        probes.probe_traces_fingerprint_sha256,
        EXPECTED_PROBE_FINGERPRINT
    );
    assert_eq!(
        probes.case_manifest_fingerprint,
        EXPECTED_MANIFEST_FINGERPRINT
    );
    assert_eq!(probes.witnesses.sha256, EXPECTED_WITNESS_FILE_SHA256);
    assert_eq!(sha256(WITNESSES), EXPECTED_WITNESS_FILE_SHA256);
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
        let projected = project_checker_inputs(&prepared, case);
        let request = json!({
            "case_id": case.case_id,
            // fileTable src keys are assigned from the probe control's VFS
            // insertion order.  The witness manifest deliberately sorts its
            // input_files rows, so recover the mint-side order from the
            // re-expanded, hash-verified Program instead of that presentation
            // order.
            "source_paths": source_paths,
            "file_table": probe.file_table,
            "trace_events": probe.trace_events,
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
            .unwrap_or_else(|error| panic!("{}: replay hook failed: {error}", case.case_id));
        assert!(
            checked.partial_checks.is_empty(),
            "{}: checker reported partial checks",
            case.case_id
        );

        seed_checks += report["seed_checks"].as_u64().expect("seed count");
        gating_mismatches.extend(
            report["gating_mismatches"]
                .as_array()
                .expect("gating mismatch rows")
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
        "traced_nested_edges": count_map_json(&traced_nested_edges),
        "replayed_nested_edges": count_map_json(&replayed_nested_edges),
        "rust_nested_edges": count_map_json(&rust_nested_edges),
        "nested_topology_divergences": nested_topology_divergences,
        "case_reports": reports,
    })
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

fn unique_plan<'a>(
    plans: &'a [UpstreamExecutionPlan],
    predicate: impl Fn(&UpstreamExecutionPlan) -> bool,
) -> &'a UpstreamExecutionPlan {
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

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

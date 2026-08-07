use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tsc_harness::upstream_suites::{
    CaseConfiguration, ExecutionState, ExpansionManifest, SuiteName,
};

const RECORDED: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/typescript-6.0.3/compiler-profile-classification.v1.json"
));
const EXPANSION: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/typescript-6.0.3/test-suite-expansion.v1.json"
));
const MANIFEST_SHA256: &str = "fbe4d05310edca95c2aa52cdfa0c08b39725745d93846f037e674d803d5e452a";
const GENERATOR_PATH: &str = "crates/oracle/h1-compiler-classification.mjs";
const GENERATOR_SHA256: &str = "a366d8f2e0043e9fe568230785e5cb83f8d893803b7e22799736caf942f5e638";
const CONTRACT_PATH: &str = ".github/ci/contracts/h1-compiler-classification.schema.json";
const CONTRACT_SHA256: &str = "ff70a6c653368f687e9282f008a71f65361c30f410671a07bfe2d77471e52b0c";
const EXPANSION_PATH: &str = "vendor/typescript-6.0.3/test-suite-expansion.v1.json";
const EXPANSION_SHA256: &str = "9c6e991103b571f7a8800dc5e1ef66088017689f3769de8fcdb408b2dc125188";
const CONFIG_PLANS_PATH: &str = "vendor/typescript-6.0.3/compiler-config-plans.v1.json";
const CONFIG_PLANS_SHA256: &str =
    "d19356ed235fd32579f8688be44ee2f57dd7965cf45ccf172e7f01cd95050453";
const PROFILE_PATH: &str = "ratchets/h1-emit-profile.v1.json";
const PROFILE_SHA256: &str = "501c363f2ea6c626d46b195daab949886cc9bacb1314f3c6584a1f82bd76ef8f";
const TYPESCRIPT_BUNDLE_PATH: &str = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_BUNDLE_SHA256: &str =
    "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39";
const REFERENCE_BASELINE_STATE: &str = "content-not-vendored-or-compared";

const IMPLEMENTATION_SOURCES: [(&str, &str); 4] = [
    (
        "src/testRunner/compilerRunner.ts",
        "aed00f47656b316f3f20c913e2408a128d4671cb",
    ),
    (
        "src/harness/harnessIO.ts",
        "a06bde1c95182ea1bfad0ddf9af73053501a6dc7",
    ),
    (
        "src/harness/harnessUtils.ts",
        "f768325897167ad793eeff9ced7763e12f9aa154",
    ),
    (
        "src/harness/vfsUtil.ts",
        "b217fb57bba950c13d5d2e821b0652eacce0e65f",
    ),
];

const REJECTED_FEATURE_ROOTS: [&str; 7] = [
    "decorators",
    "export-equals",
    "import-equals",
    "jsx",
    "parameter-properties",
    "runtime-enums",
    "runtime-namespaces",
];

static PARSED: OnceLock<Manifest> = OnceLock::new();
static PARSED_EXPANSION: OnceLock<ExpansionManifest> = OnceLock::new();

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    status: String,
    phase: String,
    typescript: TypeScriptIdentity,
    generator: PathHash,
    contract: PathHash,
    inputs: Inputs,
    classification_contract: ClassificationContract,
    config_classification: ConfigClassification,
    analyses: Vec<Analysis>,
    cases: Vec<Case>,
    summary: Summary,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TypeScriptIdentity {
    version: String,
    source_repository: String,
    source_commit: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PathHash {
    path: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GitSourceIdentity {
    source_path: String,
    git_blob_sha1: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Inputs {
    suite_expansion: PathHash,
    compiler_config_plans: PathHash,
    h1_profile: PathHash,
    typescript_bundle: PathHash,
    implementation_sources: Vec<GitSourceIdentity>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassificationContract {
    effective_option_order: String,
    source_analysis_gate: String,
    source_analysis_scope: String,
    required_options: Vec<String>,
    admitted_products: Vec<String>,
    execution_state: String,
    reference_baseline_state: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigClassification {
    fixtures: u64,
    cases: u64,
    diagnostics: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum Origin {
    VirtualConfig,
    HarnessSetting,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
enum EnumProjection {
    Absent,
    Set {
        name: String,
        value: i32,
        origin: Origin,
    },
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
enum BooleanProjection {
    Absent,
    Set { value: bool, origin: Origin },
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct RejectedOption {
    name: String,
    value: Value,
    origin: Origin,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct EffectiveProfile {
    target: EnumProjection,
    module: EnumProjection,
    use_define_for_class_fields: BooleanProjection,
    no_emit: BooleanProjection,
    rejected_when_effective: Vec<RejectedOption>,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
enum SourceAnalysis {
    NotRequiredEffectiveOptions,
    Analyzed { analysis: usize },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    expansion_case: usize,
    id: String,
    source: u32,
    configuration: u32,
    effective_profile: EffectiveProfile,
    source_analysis: SourceAnalysis,
    bootstrap_profile_admitted: bool,
    disposition: String,
    decisive_blocker: Option<String>,
    profile_blockers: Vec<String>,
    execution_state: String,
    reference_baseline_state: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Analysis {
    expansion_case: usize,
    source: u32,
    configuration: u32,
    current_directory: String,
    root_unit_ids: Vec<u32>,
    other_unit_ids: Vec<u32>,
    program_root_unit_ids: Vec<u32>,
    reached_units: Vec<ReachedUnit>,
    profile_blockers: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReachedUnit {
    unit: u32,
    name: String,
    source_kind: String,
    rejected_feature_roots: Vec<String>,
    parse_diagnostic_codes: Vec<u32>,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct OptionCount {
    name: String,
    cases: u64,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct ValueCount {
    value: String,
    cases: u64,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct Summary {
    fixtures: u64,
    virtual_config_fixtures: u64,
    cases: u64,
    required_target_module_matches: u64,
    effective_option_clear_cases: u64,
    source_analyzed_cases: u64,
    source_profile_blocked_cases: u64,
    cases_with_target_blocker: u64,
    cases_with_module_blocker: u64,
    cases_with_use_define_for_class_fields_blocker: u64,
    cases_with_no_emit_route: u64,
    cases_with_rejected_effective_options: u64,
    rejected_option_cases: Vec<OptionCount>,
    rejected_feature_cases: Vec<OptionCount>,
    decisive_blockers: Vec<ValueCount>,
    target_states: Vec<ValueCount>,
    module_states: Vec<ValueCount>,
    dispositions: Vec<ValueCount>,
    bootstrap_profile_admitted_cases: u64,
    not_run_cases: u64,
    reference_baselines_compared: u64,
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("harness crate must be inside workspace")
        .to_path_buf()
}

fn parsed() -> &'static Manifest {
    PARSED.get_or_init(|| {
        serde_json::from_slice(RECORDED)
            .expect("H1 compiler profile classification must be strict, valid JSON")
    })
}

fn expansion() -> &'static ExpansionManifest {
    PARSED_EXPANSION.get_or_init(|| {
        serde_json::from_slice(EXPANSION)
            .expect("upstream suite expansion must be strict, valid JSON")
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn verify_path_hash(workspace: &Path, pin: &PathHash, path: &str, expected_hash: &str) {
    assert_eq!(pin.path, path);
    assert_eq!(pin.sha256, expected_hash);
    let bytes = fs::read(workspace.join(path))
        .unwrap_or_else(|error| panic!("failed to read {path}: {error}"));
    assert_eq!(sha256_hex(&bytes), expected_hash, "{path} hash");
}

fn profile_lists(workspace: &Path) -> (Vec<String>, Vec<String>) {
    let bytes = fs::read(workspace.join(PROFILE_PATH)).expect("failed to read H1 profile");
    assert_eq!(sha256_hex(&bytes), PROFILE_SHA256, "H1 profile hash");
    let profile: Value = serde_json::from_slice(&bytes).expect("H1 profile must be valid JSON");
    let strings = |field: &Value, label: &str| {
        field
            .as_array()
            .unwrap_or_else(|| panic!("{label} must be an array"))
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .unwrap_or_else(|| panic!("{label} entry must be a string"))
                    .to_owned()
            })
            .collect()
    };
    (
        strings(
            &profile["emit_active_options"]["rejected_when_effective"],
            "rejected options",
        ),
        strings(
            &profile["source_profile"]["rejected_feature_roots"],
            "rejected features",
        ),
    )
}

#[test]
fn recorded_classification_is_bound_to_the_exact_runner_profile_and_schema() {
    assert_eq!(sha256_hex(RECORDED), MANIFEST_SHA256, "manifest hash");
    assert_eq!(sha256_hex(EXPANSION), EXPANSION_SHA256, "expansion hash");

    let manifest = parsed();
    let workspace = workspace();
    assert_eq!(manifest.schema, 1);
    assert_eq!(manifest.status, "classified-not-run");
    assert_eq!(manifest.phase, "H1.0a-compiler-profile-classification");
    assert_eq!(manifest.typescript.version, "6.0.3");
    assert_eq!(
        manifest.typescript.source_repository,
        "https://github.com/microsoft/TypeScript.git"
    );
    assert_eq!(
        manifest.typescript.source_commit,
        "050880ce59e30b356b686bd3144efe24f875ebc8"
    );
    verify_path_hash(
        &workspace,
        &manifest.generator,
        GENERATOR_PATH,
        GENERATOR_SHA256,
    );
    verify_path_hash(
        &workspace,
        &manifest.contract,
        CONTRACT_PATH,
        CONTRACT_SHA256,
    );
    verify_path_hash(
        &workspace,
        &manifest.inputs.suite_expansion,
        EXPANSION_PATH,
        EXPANSION_SHA256,
    );
    verify_path_hash(
        &workspace,
        &manifest.inputs.compiler_config_plans,
        CONFIG_PLANS_PATH,
        CONFIG_PLANS_SHA256,
    );
    verify_path_hash(
        &workspace,
        &manifest.inputs.h1_profile,
        PROFILE_PATH,
        PROFILE_SHA256,
    );
    verify_path_hash(
        &workspace,
        &manifest.inputs.typescript_bundle,
        TYPESCRIPT_BUNDLE_PATH,
        TYPESCRIPT_BUNDLE_SHA256,
    );
    assert_eq!(
        manifest.inputs.implementation_sources.len(),
        IMPLEMENTATION_SOURCES.len()
    );
    for (actual, expected) in manifest
        .inputs
        .implementation_sources
        .iter()
        .zip(IMPLEMENTATION_SOURCES)
    {
        assert_eq!(actual.source_path, expected.0);
        assert_eq!(actual.git_blob_sha1, expected.1);
    }

    let schema: Value = serde_json::from_slice(
        &fs::read(workspace.join(CONTRACT_PATH)).expect("failed to read classification schema"),
    )
    .expect("classification schema must be valid JSON");
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(schema["additionalProperties"], false);

    let contract = &manifest.classification_contract;
    assert_eq!(
        contract.effective_option_order,
        "virtual tsconfig parse, Compiler.compileFiles defaults, then harness settings with matrix overrides"
    );
    assert_eq!(
        contract.source_analysis_gate,
        "construct a vendored TypeScript Program only when effective options have no bootstrap blocker"
    );
    assert_eq!(
        contract.source_analysis_scope,
        "fixture VFS program roots plus module-resolved fixture source dependencies"
    );
    assert_eq!(
        contract.required_options,
        ["target=ESNext(99)", "module=Preserve(200)"]
    );
    assert_eq!(contract.admitted_products, ["javascript"]);
    assert_eq!(contract.execution_state, "not-run");
    assert_eq!(contract.reference_baseline_state, REFERENCE_BASELINE_STATE);

    let (rejected_options, rejected_features) = profile_lists(&workspace);
    assert_eq!(rejected_options.len(), 22);
    assert_eq!(rejected_features, REJECTED_FEATURE_ROOTS.map(str::to_owned));
}

#[test]
fn every_compiler_expansion_row_has_an_independently_recomputed_disposition() {
    let manifest = parsed();
    let expansion = expansion();
    let (rejected_options, rejected_features) = profile_lists(&workspace());

    assert_eq!(expansion.compiler_fixtures.len(), 6_537);
    assert_eq!(manifest.cases.len(), 7_276);
    assert_eq!(
        manifest.cases.len(),
        expansion.summary.compiler_cases as usize
    );
    assert_eq!(
        expansion
            .compiler_fixtures
            .iter()
            .enumerate()
            .map(|(index, fixture)| {
                assert_eq!(fixture.source as usize, index);
                fixture.virtual_config.is_some()
            })
            .filter(|present| *present)
            .count(),
        manifest.config_classification.fixtures as usize
    );
    assert_eq!(manifest.config_classification.fixtures, 103);
    assert_eq!(
        expansion
            .compiler_fixtures
            .iter()
            .filter(|fixture| fixture.virtual_config.is_some())
            .map(|fixture| fixture.configurations.len())
            .sum::<usize>(),
        manifest.config_classification.cases as usize
    );
    assert_eq!(manifest.config_classification.cases, 106);
    assert_eq!(manifest.config_classification.diagnostics, 0);

    let mut analyzed = Vec::new();
    let mut admitted = Vec::new();
    let mut required_pair = Vec::new();
    for (index, (case, expanded)) in manifest
        .cases
        .iter()
        .zip(&expansion.cases[..manifest.cases.len()])
        .enumerate()
    {
        assert_eq!(case.expansion_case, index);
        assert_eq!(case.id, expanded.id);
        assert_eq!(case.source, expanded.source);
        assert_eq!(expanded.suite, SuiteName::Compiler);
        assert_eq!(expanded.initial_execution_state, ExecutionState::NotRun);
        let CaseConfiguration::Compiler { configuration } = expanded.configuration else {
            panic!("compiler case {index} has a project configuration");
        };
        assert_eq!(case.configuration, configuration);
        assert_eq!(case.execution_state, "not-run");
        assert_eq!(case.reference_baseline_state, REFERENCE_BASELINE_STATE);

        verify_enum_projection(&case.effective_profile.target, canonical_target_name);
        verify_enum_projection(&case.effective_profile.module, canonical_module_name);
        let option_blockers = derive_option_blockers(&case.effective_profile, &rejected_options);
        let expected_blockers = match case.source_analysis {
            SourceAnalysis::NotRequiredEffectiveOptions => {
                assert!(!option_blockers.is_empty(), "{} skipped analysis", case.id);
                option_blockers
            }
            SourceAnalysis::Analyzed { analysis } => {
                assert!(
                    option_blockers.is_empty(),
                    "{} analyzed with option blockers",
                    case.id
                );
                let analysis_row = &manifest.analyses[analysis];
                assert_eq!(analysis_row.expansion_case, index);
                assert_eq!(analysis_row.source, case.source);
                assert_eq!(analysis_row.configuration, case.configuration);
                analyzed.push((index, analysis));
                analysis_row.profile_blockers.clone()
            }
        };
        assert_eq!(case.profile_blockers, expected_blockers, "{}", case.id);
        assert_eq!(
            case.profile_blockers.len(),
            case.profile_blockers.iter().collect::<BTreeSet<_>>().len(),
            "{} has duplicate blockers",
            case.id
        );
        assert_eq!(
            case.decisive_blocker.as_ref(),
            case.profile_blockers.first(),
            "{} decisive blocker",
            case.id
        );
        assert_eq!(
            case.bootstrap_profile_admitted,
            case.profile_blockers.is_empty(),
            "{} admission",
            case.id
        );
        let disposition = if case.bootstrap_profile_admitted {
            "bootstrap-candidate-not-run"
        } else if case.profile_blockers == ["route:noEmit=true"] {
            "h0-no-emit"
        } else {
            "deferred-profile"
        };
        assert_eq!(case.disposition, disposition, "{} disposition", case.id);

        if enum_value(&case.effective_profile.target) == Some(99)
            && enum_value(&case.effective_profile.module) == Some(200)
        {
            required_pair.push(case.id.as_str());
        }
        if case.bootstrap_profile_admitted {
            admitted.push(case.id.as_str());
        }
    }
    assert_eq!(analyzed, [(2_794, 0), (4_825, 1)]);
    assert_eq!(required_pair.len(), 7);
    assert_eq!(
        admitted,
        ["typescript-6.0.3/compiler/esmNoSynthesizedDefault.ts#module%3Dpreserve"]
    );
    assert_eq!(rejected_features, REJECTED_FEATURE_ROOTS.map(str::to_owned));

    let derived = derive_summary(manifest, &rejected_options, &rejected_features);
    assert_eq!(manifest.summary, derived);
    assert_frozen_summary(&manifest.summary);
}

#[test]
fn option_clear_program_analyses_pin_roots_reachability_and_syntax_blockers() {
    let manifest = parsed();
    let expansion = expansion();
    assert_eq!(manifest.analyses.len(), 2);

    for analysis in &manifest.analyses {
        let case = &manifest.cases[analysis.expansion_case];
        let fixture = &expansion.compiler_fixtures[analysis.source as usize];
        assert_eq!(case.source, analysis.source);
        assert_eq!(case.configuration, analysis.configuration);
        assert_eq!(analysis.current_directory, "/.src");
        for ids in [
            &analysis.root_unit_ids,
            &analysis.other_unit_ids,
            &analysis.program_root_unit_ids,
        ] {
            assert_eq!(ids.len(), ids.iter().collect::<BTreeSet<_>>().len());
            assert!(ids
                .iter()
                .all(|unit| (*unit as usize) < fixture.normal_units.len()));
        }
        assert!(analysis
            .program_root_unit_ids
            .iter()
            .all(|unit| analysis.root_unit_ids.contains(unit)));
        assert_eq!(
            analysis.program_root_unit_ids,
            analysis
                .root_unit_ids
                .iter()
                .copied()
                .filter(|unit| !fixture.normal_units[*unit as usize].name.ends_with(".json"))
                .collect::<Vec<_>>()
        );
        for reached in &analysis.reached_units {
            assert_eq!(
                reached.name, fixture.normal_units[reached.unit as usize].name,
                "{} reached unit {}",
                case.id, reached.unit
            );
            let expected_kind = if reached.name.to_ascii_lowercase().ends_with(".d.ts") {
                "declaration-dependency"
            } else if reached.name.to_ascii_lowercase().ends_with(".ts") {
                "javascript-emit-input"
            } else {
                "unsupported-extension"
            };
            assert_eq!(reached.source_kind, expected_kind);
            assert!(reached.parse_diagnostic_codes.is_empty());
            assert_eq!(
                reached.rejected_feature_roots.len(),
                reached
                    .rejected_feature_roots
                    .iter()
                    .collect::<BTreeSet<_>>()
                    .len()
            );
        }
    }

    let admitted = &manifest.analyses[0];
    assert_eq!((admitted.expansion_case, admitted.source), (2_794, 2_436));
    assert_eq!(admitted.root_unit_ids, [0, 1, 2]);
    assert!(admitted.other_unit_ids.is_empty());
    assert_eq!(admitted.program_root_unit_ids, [1, 2]);
    assert_eq!(
        admitted
            .reached_units
            .iter()
            .map(|unit| (unit.unit, unit.name.as_str(), unit.source_kind.as_str()))
            .collect::<Vec<_>>(),
        [
            (
                1,
                "/node_modules/mdast-util-to-string/index.d.ts",
                "declaration-dependency",
            ),
            (2, "/index.ts", "javascript-emit-input"),
        ]
    );
    assert!(admitted
        .reached_units
        .iter()
        .all(|unit| unit.rejected_feature_roots.is_empty()));
    assert!(admitted.profile_blockers.is_empty());

    let blocked = &manifest.analyses[1];
    assert_eq!((blocked.expansion_case, blocked.source), (4_825, 4_259));
    assert_eq!(blocked.root_unit_ids, [2]);
    assert_eq!(blocked.other_unit_ids, [0, 1]);
    assert_eq!(blocked.program_root_unit_ids, [2]);
    assert_eq!(
        blocked
            .reached_units
            .iter()
            .map(|unit| {
                (
                    unit.unit,
                    unit.name.as_str(),
                    unit.rejected_feature_roots.as_slice(),
                )
            })
            .collect::<Vec<_>>(),
        [
            (0, "/a.ts", &[] as &[String]),
            (1, "/b.ts", &["export-equals".to_owned()]),
            (2, "/main.ts", &["import-equals".to_owned()]),
        ]
    );
    assert_eq!(
        blocked.profile_blockers,
        [
            "rejected-feature:export-equals",
            "rejected-feature:import-equals",
        ]
    );
}

fn enum_value(projection: &EnumProjection) -> Option<i32> {
    match projection {
        EnumProjection::Absent => None,
        EnumProjection::Set { value, .. } => Some(*value),
    }
}

fn enum_display(projection: &EnumProjection) -> String {
    match projection {
        EnumProjection::Absent => "absent".to_owned(),
        EnumProjection::Set { name, value, .. } => format!("{name}({value})"),
    }
}

fn boolean_value(projection: &BooleanProjection) -> Option<bool> {
    match projection {
        BooleanProjection::Absent => None,
        BooleanProjection::Set { value, origin } => {
            let _ = origin;
            Some(*value)
        }
    }
}

fn canonical_target_name(value: i32) -> Option<&'static str> {
    Some(match value {
        0 => "ES3",
        1 => "ES5",
        2 => "ES2015",
        3 => "ES2016",
        4 => "ES2017",
        5 => "ES2018",
        6 => "ES2019",
        7 => "ES2020",
        8 => "ES2021",
        9 => "ES2022",
        10 => "ES2023",
        11 => "ES2024",
        12 => "ES2025",
        99 => "ESNext",
        100 => "JSON",
        _ => return None,
    })
}

fn canonical_module_name(value: i32) -> Option<&'static str> {
    Some(match value {
        0 => "None",
        1 => "CommonJS",
        2 => "AMD",
        3 => "UMD",
        4 => "System",
        5 => "ES2015",
        6 => "ES2020",
        7 => "ES2022",
        99 => "ESNext",
        100 => "Node16",
        101 => "Node18",
        102 => "Node20",
        199 => "NodeNext",
        200 => "Preserve",
        _ => return None,
    })
}

fn verify_enum_projection(
    projection: &EnumProjection,
    canonical_name: fn(i32) -> Option<&'static str>,
) {
    if let EnumProjection::Set {
        name,
        value,
        origin,
    } = projection
    {
        let _ = origin;
        assert_eq!(Some(name.as_str()), canonical_name(*value));
    }
}

fn derive_option_blockers(profile: &EffectiveProfile, rejected_options: &[String]) -> Vec<String> {
    let mut blockers = Vec::new();
    if enum_value(&profile.target) != Some(99) {
        blockers.push(format!(
            "required-option:target={}",
            enum_display(&profile.target)
        ));
    }
    if enum_value(&profile.module) != Some(200) {
        blockers.push(format!(
            "required-option:module={}",
            enum_display(&profile.module)
        ));
    }
    if boolean_value(&profile.use_define_for_class_fields) == Some(false) {
        blockers.push("required-option:useDefineForClassFields=false".to_owned());
    }
    if boolean_value(&profile.no_emit) == Some(true) {
        blockers.push("route:noEmit=true".to_owned());
    }

    let rejected_indexes = rejected_options
        .iter()
        .enumerate()
        .map(|(index, name)| (name.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    let mut previous = None;
    for rejected in &profile.rejected_when_effective {
        let index = *rejected_indexes
            .get(rejected.name.as_str())
            .unwrap_or_else(|| panic!("unknown rejected option {}", rejected.name));
        if let Some(previous) = previous {
            assert!(
                previous < index,
                "rejected options are out of profile order"
            );
        }
        previous = Some(index);
        assert!(!rejected.value.is_null());
        assert_ne!(rejected.value, Value::Bool(false));
        let _ = rejected.origin;
        blockers.push(format!("rejected-option:{}", rejected.name));
    }
    blockers
}

fn count_values(values: impl Iterator<Item = String>) -> Vec<ValueCount> {
    let mut counts = BTreeMap::new();
    for value in values {
        *counts.entry(value).or_insert(0_u64) += 1;
    }
    let mut rows = counts
        .into_iter()
        .map(|(value, cases)| ValueCount { value, cases })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .cases
            .cmp(&left.cases)
            .then_with(|| left.value.cmp(&right.value))
    });
    rows
}

fn derive_summary(
    manifest: &Manifest,
    rejected_options: &[String],
    rejected_features: &[String],
) -> Summary {
    let cases = &manifest.cases;
    Summary {
        fixtures: expansion().compiler_fixtures.len() as u64,
        virtual_config_fixtures: expansion()
            .compiler_fixtures
            .iter()
            .filter(|fixture| fixture.virtual_config.is_some())
            .count() as u64,
        cases: cases.len() as u64,
        required_target_module_matches: cases
            .iter()
            .filter(|case| {
                enum_value(&case.effective_profile.target) == Some(99)
                    && enum_value(&case.effective_profile.module) == Some(200)
            })
            .count() as u64,
        effective_option_clear_cases: cases
            .iter()
            .filter(|case| matches!(case.source_analysis, SourceAnalysis::Analyzed { .. }))
            .count() as u64,
        source_analyzed_cases: manifest.analyses.len() as u64,
        source_profile_blocked_cases: cases
            .iter()
            .filter(|case| {
                matches!(case.source_analysis, SourceAnalysis::Analyzed { .. })
                    && !case.profile_blockers.is_empty()
            })
            .count() as u64,
        cases_with_target_blocker: cases
            .iter()
            .filter(|case| {
                case.profile_blockers
                    .iter()
                    .any(|blocker| blocker.starts_with("required-option:target="))
            })
            .count() as u64,
        cases_with_module_blocker: cases
            .iter()
            .filter(|case| {
                case.profile_blockers
                    .iter()
                    .any(|blocker| blocker.starts_with("required-option:module="))
            })
            .count() as u64,
        cases_with_use_define_for_class_fields_blocker: cases
            .iter()
            .filter(|case| {
                case.profile_blockers
                    .iter()
                    .any(|blocker| blocker == "required-option:useDefineForClassFields=false")
            })
            .count() as u64,
        cases_with_no_emit_route: cases
            .iter()
            .filter(|case| {
                case.profile_blockers
                    .iter()
                    .any(|blocker| blocker == "route:noEmit=true")
            })
            .count() as u64,
        cases_with_rejected_effective_options: cases
            .iter()
            .filter(|case| !case.effective_profile.rejected_when_effective.is_empty())
            .count() as u64,
        rejected_option_cases: rejected_options
            .iter()
            .map(|name| OptionCount {
                name: name.clone(),
                cases: cases
                    .iter()
                    .filter(|case| {
                        case.effective_profile
                            .rejected_when_effective
                            .iter()
                            .any(|option| option.name == *name)
                    })
                    .count() as u64,
            })
            .collect(),
        rejected_feature_cases: rejected_features
            .iter()
            .map(|name| OptionCount {
                name: name.clone(),
                cases: cases
                    .iter()
                    .filter(|case| {
                        case.profile_blockers
                            .iter()
                            .any(|blocker| blocker == &format!("rejected-feature:{name}"))
                    })
                    .count() as u64,
            })
            .collect(),
        decisive_blockers: count_values(
            cases
                .iter()
                .filter_map(|case| case.decisive_blocker.clone()),
        ),
        target_states: count_values(
            cases
                .iter()
                .map(|case| enum_display(&case.effective_profile.target)),
        ),
        module_states: count_values(
            cases
                .iter()
                .map(|case| enum_display(&case.effective_profile.module)),
        ),
        dispositions: count_values(cases.iter().map(|case| case.disposition.clone())),
        bootstrap_profile_admitted_cases: cases
            .iter()
            .filter(|case| case.bootstrap_profile_admitted)
            .count() as u64,
        not_run_cases: cases
            .iter()
            .filter(|case| case.execution_state == "not-run")
            .count() as u64,
        reference_baselines_compared: 0,
    }
}

fn assert_frozen_summary(summary: &Summary) {
    assert_eq!(summary.fixtures, 6_537);
    assert_eq!(summary.virtual_config_fixtures, 103);
    assert_eq!(summary.cases, 7_276);
    assert_eq!(summary.required_target_module_matches, 7);
    assert_eq!(summary.effective_option_clear_cases, 2);
    assert_eq!(summary.source_analyzed_cases, 2);
    assert_eq!(summary.source_profile_blocked_cases, 1);
    assert_eq!(summary.cases_with_target_blocker, 7_056);
    assert_eq!(summary.cases_with_module_blocker, 7_255);
    assert_eq!(summary.cases_with_use_define_for_class_fields_blocker, 13);
    assert_eq!(summary.cases_with_no_emit_route, 510);
    assert_eq!(summary.cases_with_rejected_effective_options, 2_094);
    assert_eq!(summary.rejected_option_cases.len(), 22);
    assert_eq!(summary.rejected_feature_cases.len(), 7);
    assert_eq!(summary.decisive_blockers.len(), 22);
    assert_eq!(summary.target_states.len(), 13);
    assert_eq!(summary.module_states.len(), 15);
    assert_eq!(summary.dispositions.len(), 3);
    assert_eq!(summary.bootstrap_profile_admitted_cases, 1);
    assert_eq!(summary.not_run_cases, 7_276);
    assert_eq!(summary.reference_baselines_compared, 0);
    assert_eq!(
        summary
            .rejected_option_cases
            .iter()
            .map(|row| (row.name.as_str(), row.cases))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            ("allowImportingTsExtensions", 0),
            ("allowJs", 346),
            ("composite", 9),
            ("declaration", 1_030),
            ("declarationDir", 9),
            ("declarationMap", 6),
            ("emitDeclarationOnly", 88),
            ("experimentalDecorators", 117),
            ("importHelpers", 110),
            ("incremental", 7),
            ("inlineSourceMap", 10),
            ("isolatedModules", 71),
            ("jsx", 191),
            ("noCheck", 3),
            ("noEmitHelpers", 114),
            ("outDir", 185),
            ("outFile", 115),
            ("rewriteRelativeImportExtensions", 1),
            ("resolveJsonModule", 39),
            ("sourceMap", 192),
            ("tsBuildInfoFile", 3),
            ("verbatimModuleSyntax", 12),
        ])
    );
    assert_eq!(
        summary
            .rejected_feature_cases
            .iter()
            .map(|row| (row.name.as_str(), row.cases))
            .collect::<Vec<_>>(),
        [
            ("decorators", 0),
            ("export-equals", 1),
            ("import-equals", 1),
            ("jsx", 0),
            ("parameter-properties", 0),
            ("runtime-enums", 0),
            ("runtime-namespaces", 0),
        ]
    );
    assert_eq!(
        summary
            .dispositions
            .iter()
            .map(|row| (row.value.as_str(), row.cases))
            .collect::<Vec<_>>(),
        [
            ("deferred-profile", 7_273),
            ("h0-no-emit", 2),
            ("bootstrap-candidate-not-run", 1),
        ]
    );
}

#[test]
fn node_reconstruction_matches_every_recorded_compiler_classification_row() {
    let output = Command::new("node")
        .arg(GENERATOR_PATH)
        .arg("--check")
        .current_dir(workspace())
        .output()
        .expect("failed to run H1 compiler classification generator");
    assert!(
        output.status.success(),
        "Node classification check failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("cases=7276 analyzed=2 admitted=1 status=not-run"));
}

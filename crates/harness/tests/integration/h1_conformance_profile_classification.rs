use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tsc_harness::upstream_suites::h1_conformance::{
    ConformanceExpansionManifest, ReferenceBaselineState,
};

const RECORDED: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/typescript-6.0.3/conformance-profile-classification.v1.json"
));
const EXPANSION: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/typescript-6.0.3/conformance-suite-expansion.v1.json"
));
const MANIFEST_SHA256: &str = "ac879a75c8ca9accf39e41dfa37b26fa2359e91398217da339686c88a493eeea";
const GENERATOR_PATH: &str = "crates/oracle/h1-conformance-classification.mjs";
const GENERATOR_SHA256: &str = "8cbb8433acdd5ab580d9fefaea4a1678981674a7e0e8af2b917f5ca7d5fa6a6c";
const CONTRACT_PATH: &str = ".github/ci/contracts/h1-conformance-classification.schema.json";
const CONTRACT_SHA256: &str = "d3e054d5634e08ea967e20204039c6af3083f0259c22c3f383978c887a16273e";
const EXPANSION_PATH: &str = "vendor/typescript-6.0.3/conformance-suite-expansion.v1.json";
const EXPANSION_SHA256: &str = "924d4007b3ac93a3ee57032ea6089b649bab2902e30ee64cff02f4c9404b7bbd";
const PROFILE_PATH: &str = "ratchets/h1-emit-profile.v1.json";
const PROFILE_SHA256: &str = "568a61c410284f01239476fcbc48f29556193c5aa8daecf4a39c307666e1bde8";
const TYPESCRIPT_BUNDLE_PATH: &str = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_BUNDLE_SHA256: &str =
    "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39";
const REFERENCE_BASELINE_STATE: &str = "content-not-vendored-or-compared";

const IMPLEMENTATION_SOURCES: [(&str, &str); 3] = [
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
];

static PARSED: OnceLock<Manifest> = OnceLock::new();
static PARSED_EXPANSION: OnceLock<ConformanceExpansionManifest> = OnceLock::new();

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
    fixtures: Vec<Fixture>,
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
    conformance_expansion: PathHash,
    h1_profile: PathHash,
    typescript_bundle: PathHash,
    implementation_sources: Vec<GitSourceIdentity>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassificationContract {
    effective_option_order: String,
    admission_proof: String,
    required_options: Vec<String>,
    javascript_observation_index: u32,
    non_javascript_observations: String,
    syntax_classification: String,
    reference_baseline_state: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    source: u32,
    javascript_observation_applicable: bool,
    virtual_config: VirtualConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VirtualConfig {
    present: bool,
    diagnostic_codes: Vec<u32>,
    file_names: Vec<String>,
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct JavascriptObservation {
    index: u32,
    applicable: bool,
    execution_state: String,
    reference_baseline_state: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    expansion_case: usize,
    id: String,
    source: u32,
    configuration: u32,
    effective_profile: EffectiveProfile,
    javascript_observation: JavascriptObservation,
    bootstrap_profile_admitted: bool,
    disposition: String,
    decisive_blocker: String,
    profile_blockers: Vec<String>,
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
    virtual_config_diagnostic_fixtures: u64,
    cases: u64,
    javascript_observation_applicable_cases: u64,
    required_target_module_matches: u64,
    cases_with_target_blocker: u64,
    cases_with_module_blocker: u64,
    cases_with_use_define_for_class_fields_blocker: u64,
    cases_with_no_emit_route: u64,
    cases_with_rejected_effective_options: u64,
    rejected_option_cases: Vec<OptionCount>,
    decisive_blockers: Vec<ValueCount>,
    target_states: Vec<ValueCount>,
    module_states: Vec<ValueCount>,
    bootstrap_profile_admitted_cases: u64,
    deferred_effective_option_cases: u64,
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
            .expect("H1 conformance profile classification must be strict, valid JSON")
    })
}

fn expansion() -> &'static ConformanceExpansionManifest {
    PARSED_EXPANSION.get_or_init(|| {
        serde_json::from_slice(EXPANSION)
            .expect("H1 conformance expansion must be strict, valid JSON")
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

fn profile_rejected_options(workspace: &Path) -> Vec<String> {
    let bytes = fs::read(workspace.join(PROFILE_PATH)).expect("failed to read H1 profile");
    assert_eq!(sha256_hex(&bytes), PROFILE_SHA256, "H1 profile hash");
    let profile: Value = serde_json::from_slice(&bytes).expect("H1 profile must be valid JSON");
    profile["emit_active_options"]["rejected_when_effective"]
        .as_array()
        .expect("H1 profile must list rejected effective options")
        .iter()
        .map(|value| {
            value
                .as_str()
                .expect("rejected option name must be a string")
                .to_owned()
        })
        .collect()
}

#[test]
fn recorded_classification_is_bound_to_the_exact_runner_and_profile() {
    assert_eq!(sha256_hex(RECORDED), MANIFEST_SHA256, "manifest hash");
    assert_eq!(sha256_hex(EXPANSION), EXPANSION_SHA256, "expansion hash");

    let manifest = parsed();
    let workspace = workspace();
    assert_eq!(manifest.schema, 1);
    assert_eq!(manifest.status, "classified-not-run");
    assert_eq!(manifest.phase, "H1.0a-conformance-profile-classification");
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
        &manifest.inputs.conformance_expansion,
        EXPANSION_PATH,
        EXPANSION_SHA256,
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
        contract.admission_proof,
        "every case has at least one effective-option blocker before source reachability or syntax classification"
    );
    assert_eq!(
        contract.required_options,
        ["target=ESNext(99)", "module=Preserve(200)"]
    );
    assert_eq!(contract.javascript_observation_index, 3);
    assert_eq!(
        contract.non_javascript_observations,
        "remain deferred and not-run outside bootstrap JavaScript acceptance"
    );
    assert_eq!(
        contract.syntax_classification,
        "not required for the zero-admission proof and not claimed by this artifact"
    );
    assert_eq!(contract.reference_baseline_state, REFERENCE_BASELINE_STATE);
}

#[test]
fn every_expansion_row_has_an_independently_recomputed_option_disposition() {
    let manifest = parsed();
    let expansion = expansion();
    let rejected_options = profile_rejected_options(&workspace());
    assert_eq!(manifest.fixtures.len(), expansion.fixtures.len());
    assert_eq!(manifest.cases.len(), expansion.cases.len());

    let mut applicability = BTreeMap::new();
    let mut diagnostic_paths = Vec::new();
    for (fixture, expanded) in manifest.fixtures.iter().zip(&expansion.fixtures) {
        assert_eq!(fixture.source, expanded.source);
        let derived_applicability = expanded
            .normal_units
            .iter()
            .any(|unit| !unit.name.ends_with(".d.ts"));
        assert_eq!(
            fixture.javascript_observation_applicable, derived_applicability,
            "JavaScript observation applicability changed for {}",
            expansion.sources[fixture.source as usize].path
        );
        assert_eq!(
            fixture.virtual_config.present,
            expanded.virtual_config.is_some()
        );
        if fixture.virtual_config.present {
            assert!(!fixture.virtual_config.file_names.is_empty());
        } else {
            assert!(fixture.virtual_config.file_names.is_empty());
            assert!(fixture.virtual_config.diagnostic_codes.is_empty());
        }
        if !fixture.virtual_config.diagnostic_codes.is_empty() {
            diagnostic_paths.push((
                expansion.sources[fixture.source as usize].path.clone(),
                fixture.virtual_config.diagnostic_codes.clone(),
                fixture.virtual_config.file_names.clone(),
            ));
        }
        assert!(applicability
            .insert(fixture.source, derived_applicability)
            .is_none());
    }
    assert_eq!(
        diagnostic_paths,
        [
            (
                "typings/typingsLookup1.ts".to_owned(),
                vec![5024],
                vec!["/a.ts".to_owned()],
            ),
            (
                "typings/typingsLookup3.ts".to_owned(),
                vec![5024],
                vec!["/a.ts".to_owned()],
            ),
        ]
    );

    let mut matched_required_pair = Vec::new();
    for (index, (case, expanded)) in manifest.cases.iter().zip(&expansion.cases).enumerate() {
        assert_eq!(case.expansion_case, index);
        assert_eq!(case.id, expanded.id);
        assert_eq!(case.source, expanded.source);
        assert_eq!(case.configuration, expanded.configuration);
        assert_eq!(expanded.observations, [0, 1, 2, 3, 4, 5]);
        assert_eq!(
            expanded.reference_baseline_state,
            ReferenceBaselineState::ContentNotVendoredOrCompared
        );

        assert_eq!(case.javascript_observation.index, 3);
        assert_eq!(
            case.javascript_observation.applicable,
            applicability[&case.source]
        );
        assert_eq!(case.javascript_observation.execution_state, "not-run");
        assert_eq!(
            case.javascript_observation.reference_baseline_state,
            REFERENCE_BASELINE_STATE
        );
        assert!(!case.bootstrap_profile_admitted);
        assert_eq!(case.disposition, "deferred-effective-options");

        verify_enum_projection(&case.effective_profile.target, canonical_target_name);
        verify_enum_projection(&case.effective_profile.module, canonical_module_name);
        let expected_blockers = derive_blockers(&case.effective_profile, &rejected_options);
        assert!(!expected_blockers.is_empty());
        assert_eq!(
            case.profile_blockers, expected_blockers,
            "{} blockers",
            case.id
        );
        assert_eq!(case.decisive_blocker, expected_blockers[0]);

        if enum_value(&case.effective_profile.target) == Some(99)
            && enum_value(&case.effective_profile.module) == Some(200)
        {
            matched_required_pair.push(case.id.as_str());
        }
    }
    assert_eq!(
        matched_required_pair,
        [
            "typescript-6.0.3/conformance/externalModules/rewriteRelativeImportExtensions/emit.ts#jsx%3Dreact",
            "typescript-6.0.3/conformance/externalModules/rewriteRelativeImportExtensions/emit.ts#jsx%3Dpreserve",
            "typescript-6.0.3/conformance/externalModules/verbatimModuleSyntaxAmbientConstEnum.ts#default",
        ]
    );

    let derived = derive_summary(manifest, &rejected_options);
    assert_eq!(manifest.summary, derived);
    assert_frozen_summary(&manifest.summary);
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
        BooleanProjection::Set { value, .. } => Some(*value),
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
    if let EnumProjection::Set { name, value, .. } = projection {
        assert_eq!(Some(name.as_str()), canonical_name(*value));
    }
}

fn derive_blockers(profile: &EffectiveProfile, rejected_options: &[String]) -> Vec<String> {
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
            .unwrap_or_else(|| panic!("unknown rejected effective option {}", rejected.name));
        if let Some(previous) = previous {
            assert!(previous < index, "rejected option order changed");
        }
        previous = Some(index);
        assert!(!matches!(rejected.value, Value::Null | Value::Bool(false)));
        blockers.push(format!("rejected-option:{}", rejected.name));
    }
    blockers
}

fn count_values(values: impl IntoIterator<Item = String>) -> Vec<ValueCount> {
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

fn derive_summary(manifest: &Manifest, rejected_options: &[String]) -> Summary {
    let cases = &manifest.cases;
    Summary {
        fixtures: manifest.fixtures.len() as u64,
        virtual_config_fixtures: manifest
            .fixtures
            .iter()
            .filter(|fixture| fixture.virtual_config.present)
            .count() as u64,
        virtual_config_diagnostic_fixtures: manifest
            .fixtures
            .iter()
            .filter(|fixture| !fixture.virtual_config.diagnostic_codes.is_empty())
            .count() as u64,
        cases: cases.len() as u64,
        javascript_observation_applicable_cases: cases
            .iter()
            .filter(|case| case.javascript_observation.applicable)
            .count() as u64,
        required_target_module_matches: cases
            .iter()
            .filter(|case| {
                enum_value(&case.effective_profile.target) == Some(99)
                    && enum_value(&case.effective_profile.module) == Some(200)
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
                            .any(|rejected| rejected.name == *name)
                    })
                    .count() as u64,
            })
            .collect(),
        decisive_blockers: count_values(cases.iter().map(|case| case.decisive_blocker.clone())),
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
        bootstrap_profile_admitted_cases: cases
            .iter()
            .filter(|case| case.bootstrap_profile_admitted)
            .count() as u64,
        deferred_effective_option_cases: cases
            .iter()
            .filter(|case| case.disposition == "deferred-effective-options")
            .count() as u64,
        not_run_cases: cases
            .iter()
            .filter(|case| case.javascript_observation.execution_state == "not-run")
            .count() as u64,
        reference_baselines_compared: 0,
    }
}

fn assert_frozen_summary(summary: &Summary) {
    assert_eq!(summary.fixtures, 5_907);
    assert_eq!(summary.virtual_config_fixtures, 27);
    assert_eq!(summary.virtual_config_diagnostic_fixtures, 2);
    assert_eq!(summary.cases, 7_697);
    assert_eq!(summary.javascript_observation_applicable_cases, 7_655);
    assert_eq!(summary.required_target_module_matches, 3);
    assert_eq!(summary.cases_with_target_blocker, 7_152);
    assert_eq!(summary.cases_with_module_blocker, 7_678);
    assert_eq!(summary.cases_with_use_define_for_class_fields_blocker, 91);
    assert_eq!(summary.cases_with_no_emit_route, 547);
    assert_eq!(summary.cases_with_rejected_effective_options, 2_483);
    assert_eq!(summary.rejected_option_cases.len(), 22);
    assert_eq!(summary.decisive_blockers.len(), 24);
    assert_eq!(summary.target_states.len(), 13);
    assert_eq!(summary.module_states.len(), 15);
    assert_eq!(summary.bootstrap_profile_admitted_cases, 0);
    assert_eq!(summary.deferred_effective_option_cases, 7_697);
    assert_eq!(summary.not_run_cases, 7_697);
    assert_eq!(summary.reference_baselines_compared, 0);
    assert_eq!(
        summary
            .rejected_option_cases
            .iter()
            .map(|row| (row.name.as_str(), row.cases))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            ("allowImportingTsExtensions", 10),
            ("allowJs", 694),
            ("composite", 0),
            ("declaration", 861),
            ("declarationDir", 1),
            ("declarationMap", 5),
            ("emitDeclarationOnly", 49),
            ("experimentalDecorators", 291),
            ("importHelpers", 74),
            ("incremental", 0),
            ("inlineSourceMap", 0),
            ("isolatedModules", 17),
            ("jsx", 257),
            ("noCheck", 0),
            ("noEmitHelpers", 630),
            ("outDir", 431),
            ("outFile", 26),
            ("rewriteRelativeImportExtensions", 16),
            ("resolveJsonModule", 24),
            ("sourceMap", 32),
            ("tsBuildInfoFile", 0),
            ("verbatimModuleSyntax", 22),
        ])
    );
}

#[test]
fn node_reconstruction_matches_every_recorded_classification_row() {
    let output = Command::new("node")
        .arg(GENERATOR_PATH)
        .arg("--check")
        .current_dir(workspace())
        .output()
        .expect("failed to run H1 conformance classification generator");
    assert!(
        output.status.success(),
        "Node classification check failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("cases=7697 admitted=0 deferred=7697 status=not-run"));
}

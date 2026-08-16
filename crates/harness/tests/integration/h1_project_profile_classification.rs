use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tsc_harness::upstream_suites::{
    CaseConfiguration, ExecutionState, ExpansionManifest, ProjectInputFiles, ProjectInputPresence,
    ProjectModule, SuiteName,
};

const RECORDED: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/typescript-6.0.3/project-profile-classification.v1.json"
));
const EXPANSION: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/typescript-6.0.3/test-suite-expansion.v1.json"
));
const MANIFEST_SHA256: &str = "b89589c1372a2c2bb4d8415f8f5b3168605fd11cb43d5b9b55828d834f54342a";
const GENERATOR_PATH: &str = "crates/oracle/h1-project-classification.mjs";
const GENERATOR_SHA256: &str = "af6c84fe62e55bc317a76b12f8ecedd41b5746a8f5566e91ad542e520be37659";
const CONTRACT_PATH: &str = ".github/ci/contracts/h1-project-classification.schema.json";
const CONTRACT_SHA256: &str = "d742abcbec9c8f5eabe911a31be5e08eb478c93c0684495f9523549022073632";
const EXPANSION_PATH: &str = "vendor/typescript-6.0.3/test-suite-expansion.v1.json";
const EXPANSION_SHA256: &str = "9c6e991103b571f7a8800dc5e1ef66088017689f3769de8fcdb408b2dc125188";
const PROFILE_PATH: &str = "ratchets/h1-emit-profile.v1.json";
const PROFILE_SHA256: &str = "d7a7d212780ef94cb9675c104ec8d2ca28af95764fa78f8aeb8c7c25885fa7db";
const TYPESCRIPT_BUNDLE_PATH: &str = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_BUNDLE_SHA256: &str =
    "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39";
const FOCUSED_ORACLE_PATH: &str = "vendor/typescript-6.0.3/project-node-modules-search.v1.json";
const FOCUSED_ORACLE_SHA256: &str =
    "daa2afdd72235612b0a6d27ab50de709a9f62095c226dfcb0020222e005ed2c1";
const REFERENCE_BASELINE_STATE: &str = "content-not-vendored-or-compared";
const PROJECT_CASE_OFFSET: usize = 7_276;

const IMPLEMENTATION_SOURCES: [(&str, &str); 4] = [
    (
        "src/testRunner/projectsRunner.ts",
        "5befdf497dff2accd67e08c3c51100b66f1b14b5",
    ),
    (
        "src/compiler/commandLineParser.ts",
        "c17cc4ef9ca01cedd915a7040efb248aa19d2e18",
    ),
    (
        "src/harness/harnessIO.ts",
        "a06bde1c95182ea1bfad0ddf9af73053501a6dc7",
    ),
    (
        "src/harness/vfsUtil.ts",
        "b217fb57bba950c13d5d2e821b0652eacce0e65f",
    ),
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
    h1_profile: PathHash,
    typescript_bundle: PathHash,
    focused_project_oracle: PathHash,
    implementation_sources: Vec<GitSourceIdentity>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassificationContract {
    runner_option_order: String,
    root_selection_order: String,
    admission_proof: String,
    required_options: Vec<String>,
    admitted_products: Vec<String>,
    source_analysis: String,
    execution_state: String,
    reference_baseline_state: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum Origin {
    RunnerDefault,
    Descriptor,
    VirtualConfig,
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
enum RootPresence {
    Present { source: u32 },
    Missing,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct ExplicitRoot {
    requested: String,
    path: String,
    presence: RootPresence,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct SourceIdentity {
    path: String,
    source: u32,
    sha256: String,
    git_blob_sha1: String,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct ConfigRoot {
    path: String,
    source: u32,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
enum RootSelection {
    ExplicitInputs {
        roots: Vec<ExplicitRoot>,
    },
    ProjectConfig {
        config: SourceIdentity,
        roots: Vec<ConfigRoot>,
        diagnostic_codes: Vec<u32>,
    },
    DiscoveredConfig {
        config: SourceIdentity,
        roots: Vec<ConfigRoot>,
        diagnostic_codes: Vec<u32>,
    },
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct ModuleVariant {
    name: String,
    value: i32,
    baseline_folder: String,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
enum SourceAnalysis {
    NotRequiredEffectiveOptions,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct JavascriptObservation {
    applicable: bool,
    execution_state: String,
    reference_baseline_state: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    project_case: usize,
    expansion_case: usize,
    id: String,
    source: u32,
    descriptor_path: String,
    module_variant: ModuleVariant,
    current_directory: String,
    root_selection: RootSelection,
    effective_profile: EffectiveProfile,
    source_analysis: SourceAnalysis,
    javascript_observation: JavascriptObservation,
    bootstrap_profile_admitted: bool,
    disposition: String,
    decisive_blocker: Option<String>,
    profile_blockers: Vec<String>,
    execution_state: String,
    reference_baseline_state: String,
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
    explicit_input_fixtures: u64,
    project_config_fixtures: u64,
    discovered_config_fixtures: u64,
    cases: u64,
    explicit_input_cases: u64,
    config_cases: u64,
    explicit_declared_roots: u64,
    explicit_missing_roots: u64,
    config_roots: u64,
    javascript_observation_applicable_cases: u64,
    required_target_module_matches: u64,
    effective_option_clear_cases: u64,
    cases_with_target_blocker: u64,
    cases_with_module_blocker: u64,
    cases_with_use_define_for_class_fields_blocker: u64,
    cases_with_no_emit_route: u64,
    cases_with_rejected_effective_options: u64,
    rejected_option_cases: Vec<OptionCount>,
    decisive_blockers: Vec<ValueCount>,
    target_states: Vec<ValueCount>,
    module_states: Vec<ValueCount>,
    root_modes: Vec<ValueCount>,
    dispositions: Vec<ValueCount>,
    config_diagnostic_cases: u64,
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
            .expect("H1 project profile classification must be strict, valid JSON")
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

fn rejected_options(workspace: &Path) -> Vec<String> {
    let bytes = fs::read(workspace.join(PROFILE_PATH)).expect("failed to read H1 profile");
    assert_eq!(sha256_hex(&bytes), PROFILE_SHA256, "H1 profile hash");
    let profile: Value = serde_json::from_slice(&bytes).expect("H1 profile must be valid JSON");
    profile["emit_active_options"]["rejected_when_effective"]
        .as_array()
        .expect("rejected options must be an array")
        .iter()
        .map(|value| {
            value
                .as_str()
                .expect("rejected option must be a string")
                .to_owned()
        })
        .collect()
}

#[test]
fn recorded_classification_is_bound_to_the_exact_runner_profile_and_schema() {
    assert_eq!(sha256_hex(RECORDED), MANIFEST_SHA256, "manifest hash");
    assert_eq!(sha256_hex(EXPANSION), EXPANSION_SHA256, "expansion hash");

    let manifest = parsed();
    let workspace = workspace();
    assert_eq!(manifest.schema, 1);
    assert_eq!(manifest.status, "classified-not-run");
    assert_eq!(manifest.phase, "H1.0a-project-profile-classification");
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
    verify_path_hash(
        &workspace,
        &manifest.inputs.focused_project_oracle,
        FOCUSED_ORACLE_PATH,
        FOCUSED_ORACLE_SHA256,
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
    assert_eq!(schema["properties"]["cases"]["minItems"], 632);
    assert_eq!(schema["properties"]["cases"]["maxItems"], 632);

    let contract = &manifest.classification_contract;
    assert_eq!(
        contract.runner_option_order,
        "ProjectRunner defaults, descriptor compiler options, then virtual tsconfig with existing runner options winning"
    );
    assert_eq!(
        contract.root_selection_order,
        "project option, else nonempty inputFiles, else discover tsconfig.json"
    );
    assert_eq!(
        contract.admission_proof,
        "every CommonJS/AMD project-runner row has required target and module blockers before source reachability"
    );
    assert_eq!(
        contract.required_options,
        ["target=ESNext(99)", "module=Preserve(200)"]
    );
    assert_eq!(contract.admitted_products, ["javascript"]);
    assert_eq!(
        contract.source_analysis,
        "not required by the zero-admission option proof"
    );
    assert_eq!(contract.execution_state, "not-run");
    assert_eq!(contract.reference_baseline_state, REFERENCE_BASELINE_STATE);
    assert_eq!(rejected_options(&workspace).len(), 22);
}

#[test]
fn every_project_expansion_row_has_exact_roots_and_an_independent_disposition() {
    let manifest = parsed();
    let expansion = expansion();
    let workspace = workspace();
    let rejected_options = rejected_options(&workspace);

    assert_eq!(expansion.project_fixtures.len(), 316);
    assert_eq!(manifest.cases.len(), 632);
    assert_eq!(expansion.summary.project_cases, 632);

    let mut fixture_modes = BTreeMap::new();
    for (index, (case, expanded)) in manifest
        .cases
        .iter()
        .zip(&expansion.cases[PROJECT_CASE_OFFSET..])
        .enumerate()
    {
        assert_eq!(case.project_case, index);
        assert_eq!(case.expansion_case, PROJECT_CASE_OFFSET + index);
        assert_eq!(case.id, expanded.id);
        assert_eq!(case.source, expanded.source);
        assert_eq!(expanded.suite, SuiteName::Project);
        assert_eq!(expanded.initial_execution_state, ExecutionState::NotRun);

        let CaseConfiguration::Project {
            module,
            baseline_folder,
        } = &expanded.configuration
        else {
            panic!("project case {index} has a compiler configuration");
        };
        let expected_variant = match module {
            ProjectModule::Commonjs => ("commonjs", 1, "node"),
            ProjectModule::Amd => ("amd", 2, "amd"),
        };
        assert_eq!(
            (
                case.module_variant.name.as_str(),
                case.module_variant.value,
                case.module_variant.baseline_folder.as_str(),
            ),
            expected_variant
        );
        assert_eq!(baseline_folder, expected_variant.2);

        let fixture = expansion
            .project_fixtures
            .get(case.source as usize - expansion.summary.compiler_sources as usize)
            .unwrap_or_else(|| panic!("missing project fixture for {}", case.id));
        assert_eq!(fixture.source, case.source);
        assert_eq!(
            case.current_directory,
            format!("/.src/{}", fixture.project_root)
        );
        let descriptor_source = &expansion.sources[case.source as usize];
        assert_eq!(descriptor_source.suite, SuiteName::Project);
        assert_eq!(case.descriptor_path, descriptor_source.path);
        let descriptor_bytes = fs::read(
            workspace
                .join("ts-tests/tests/cases/project")
                .join(&case.descriptor_path),
        )
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", case.descriptor_path));
        assert_eq!(sha256_hex(&descriptor_bytes), descriptor_source.sha256);
        let descriptor: Value = serde_json::from_slice(&descriptor_bytes)
            .unwrap_or_else(|error| panic!("invalid {}: {error}", case.descriptor_path));
        assert_eq!(
            descriptor["scenario"].as_str(),
            Some(fixture.scenario.as_str())
        );
        assert_eq!(
            descriptor["projectRoot"].as_str(),
            Some(fixture.project_root.as_str())
        );

        let expected_mode = descriptor_mode(&descriptor);
        assert_eq!(root_mode(&case.root_selection), expected_mode);
        if let Some(previous) = fixture_modes.insert(case.source, expected_mode) {
            assert_eq!(previous, expected_mode, "{} changed root mode", case.id);
        }
        verify_roots(case, fixture, expansion);
        assert_eq!(
            case.javascript_observation.applicable,
            descriptor["baselineCheck"].as_bool() == Some(true),
            "{} baselineCheck",
            case.id
        );
        assert_eq!(case.javascript_observation.execution_state, "not-run");
        assert_eq!(
            case.javascript_observation.reference_baseline_state,
            REFERENCE_BASELINE_STATE
        );

        verify_enum_projection(&case.effective_profile.target, canonical_target_name);
        verify_enum_projection(&case.effective_profile.module, canonical_module_name);
        let blockers = derive_option_blockers(&case.effective_profile, &rejected_options);
        assert_eq!(case.profile_blockers, blockers, "{} blockers", case.id);
        assert_eq!(
            case.profile_blockers.len(),
            case.profile_blockers.iter().collect::<BTreeSet<_>>().len(),
            "{} duplicate blockers",
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
        assert!(
            !case.bootstrap_profile_admitted,
            "{} unexpectedly admitted",
            case.id
        );
        assert_eq!(case.disposition, "deferred-profile");
        assert_eq!(
            case.source_analysis,
            SourceAnalysis::NotRequiredEffectiveOptions
        );
        assert_eq!(case.execution_state, "not-run");
        assert_eq!(case.reference_baseline_state, REFERENCE_BASELINE_STATE);
    }

    assert_eq!(fixture_modes.len(), 316);
    let derived = derive_summary(manifest, &fixture_modes, &rejected_options);
    assert_eq!(manifest.summary, derived);
    assert_frozen_summary(&manifest.summary);
}

fn descriptor_mode(descriptor: &Value) -> &'static str {
    if descriptor["project"]
        .as_str()
        .is_some_and(|project| !project.is_empty())
    {
        "project-config"
    } else if descriptor["inputFiles"]
        .as_array()
        .is_some_and(|inputs| !inputs.is_empty())
    {
        "explicit-inputs"
    } else {
        "discovered-config"
    }
}

fn root_mode(selection: &RootSelection) -> &'static str {
    match selection {
        RootSelection::ExplicitInputs { .. } => "explicit-inputs",
        RootSelection::ProjectConfig { .. } => "project-config",
        RootSelection::DiscoveredConfig { .. } => "discovered-config",
    }
}

fn verify_roots(
    case: &Case,
    fixture: &tsc_harness::upstream_suites::ProjectFixtureExpansion,
    expansion: &ExpansionManifest,
) {
    match (&case.root_selection, &fixture.input_files) {
        (RootSelection::ExplicitInputs { roots }, ProjectInputFiles::Present { inputs }) => {
            assert_eq!(roots.len(), inputs.len(), "{} root count", case.id);
            for (root, input) in roots.iter().zip(inputs) {
                assert_eq!(root.requested, input.path);
                assert_eq!(
                    root.path,
                    format!("/.src/tests/cases/projects/{}", input.resolved_backing_path)
                );
                match (&root.presence, &input.presence) {
                    (
                        RootPresence::Present { source },
                        ProjectInputPresence::Present {
                            source: input_source,
                        },
                    ) => assert_eq!(source, input_source),
                    (RootPresence::Missing, ProjectInputPresence::Missing) => {}
                    _ => panic!("{} root presence differs", case.id),
                }
            }
        }
        (RootSelection::ProjectConfig { .. }, ProjectInputFiles::Absent)
        | (RootSelection::DiscoveredConfig { .. }, ProjectInputFiles::Absent) => {
            let (config, roots, diagnostics) = config_parts(&case.root_selection)
                .expect("config selection must expose config parts");
            verify_source_identity(config, expansion);
            assert!(diagnostics.is_empty(), "{} config diagnostics", case.id);
            assert!(!roots.is_empty(), "{} config roots", case.id);
            assert_eq!(
                roots.len(),
                roots
                    .iter()
                    .map(|root| root.source)
                    .collect::<BTreeSet<_>>()
                    .len(),
                "{} duplicate config roots",
                case.id
            );
            for root in roots {
                let source = &expansion.sources[root.source as usize];
                assert_eq!(source.suite, SuiteName::Projects);
                assert_eq!(
                    root.path,
                    format!("/.src/tests/cases/projects/{}", source.path)
                );
            }
        }
        _ => panic!("{} expansion and classified root modes differ", case.id),
    }
}

fn config_parts(selection: &RootSelection) -> Option<(&SourceIdentity, &[ConfigRoot], &[u32])> {
    match selection {
        RootSelection::ProjectConfig {
            config,
            roots,
            diagnostic_codes,
        }
        | RootSelection::DiscoveredConfig {
            config,
            roots,
            diagnostic_codes,
        } => Some((config, roots, diagnostic_codes)),
        RootSelection::ExplicitInputs { .. } => None,
    }
}

fn verify_source_identity(identity: &SourceIdentity, expansion: &ExpansionManifest) {
    let source = &expansion.sources[identity.source as usize];
    assert_eq!(source.suite, SuiteName::Projects);
    assert_eq!(
        identity.path,
        format!("/.src/tests/cases/projects/{}", source.path)
    );
    assert_eq!(identity.sha256, source.sha256);
    assert_eq!(identity.git_blob_sha1, source.git_blob_sha1);
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
    if let EnumProjection::Set {
        name,
        value,
        origin: _,
    } = projection
    {
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

    let positions = rejected_options
        .iter()
        .enumerate()
        .map(|(index, name)| (name.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    let mut previous = None;
    for rejected in &profile.rejected_when_effective {
        let position = *positions
            .get(rejected.name.as_str())
            .unwrap_or_else(|| panic!("unknown rejected option {}", rejected.name));
        if let Some(previous) = previous {
            assert!(
                previous < position,
                "rejected options are out of profile order"
            );
        }
        previous = Some(position);
        assert!(!rejected.value.is_null());
        assert_ne!(rejected.value, Value::Bool(false));
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
    fixture_modes: &BTreeMap<u32, &str>,
    rejected_options: &[String],
) -> Summary {
    let cases = &manifest.cases;
    let explicit_cases = cases
        .iter()
        .filter(|case| matches!(&case.root_selection, RootSelection::ExplicitInputs { .. }))
        .collect::<Vec<_>>();
    let config_cases = cases
        .iter()
        .filter(|case| !matches!(&case.root_selection, RootSelection::ExplicitInputs { .. }))
        .collect::<Vec<_>>();
    Summary {
        fixtures: fixture_modes.len() as u64,
        explicit_input_fixtures: fixture_modes
            .values()
            .filter(|mode| **mode == "explicit-inputs")
            .count() as u64,
        project_config_fixtures: fixture_modes
            .values()
            .filter(|mode| **mode == "project-config")
            .count() as u64,
        discovered_config_fixtures: fixture_modes
            .values()
            .filter(|mode| **mode == "discovered-config")
            .count() as u64,
        cases: cases.len() as u64,
        explicit_input_cases: explicit_cases.len() as u64,
        config_cases: config_cases.len() as u64,
        explicit_declared_roots: explicit_cases
            .iter()
            .map(|case| match &case.root_selection {
                RootSelection::ExplicitInputs { roots } => roots.len(),
                _ => 0,
            })
            .sum::<usize>() as u64,
        explicit_missing_roots: explicit_cases
            .iter()
            .map(|case| match &case.root_selection {
                RootSelection::ExplicitInputs { roots } => roots
                    .iter()
                    .filter(|root| matches!(&root.presence, RootPresence::Missing))
                    .count(),
                _ => 0,
            })
            .sum::<usize>() as u64,
        config_roots: config_cases
            .iter()
            .map(|case| config_parts(&case.root_selection).unwrap().1.len())
            .sum::<usize>() as u64,
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
        effective_option_clear_cases: cases
            .iter()
            .filter(|case| case.profile_blockers.is_empty())
            .count() as u64,
        cases_with_target_blocker: count_blocker(cases, "required-option:target="),
        cases_with_module_blocker: count_blocker(cases, "required-option:module="),
        cases_with_use_define_for_class_fields_blocker: count_exact_blocker(
            cases,
            "required-option:useDefineForClassFields=false",
        ),
        cases_with_no_emit_route: count_exact_blocker(cases, "route:noEmit=true"),
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
        root_modes: count_values(
            cases
                .iter()
                .map(|case| root_mode(&case.root_selection).to_owned()),
        ),
        dispositions: count_values(cases.iter().map(|case| case.disposition.clone())),
        config_diagnostic_cases: config_cases
            .iter()
            .filter(|case| !config_parts(&case.root_selection).unwrap().2.is_empty())
            .count() as u64,
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

fn count_blocker(cases: &[Case], prefix: &str) -> u64 {
    cases
        .iter()
        .filter(|case| {
            case.profile_blockers
                .iter()
                .any(|blocker| blocker.starts_with(prefix))
        })
        .count() as u64
}

fn count_exact_blocker(cases: &[Case], expected: &str) -> u64 {
    cases
        .iter()
        .filter(|case| {
            case.profile_blockers
                .iter()
                .any(|blocker| blocker == expected)
        })
        .count() as u64
}

fn assert_frozen_summary(summary: &Summary) {
    assert_eq!(summary.fixtures, 316);
    assert_eq!(summary.explicit_input_fixtures, 285);
    assert_eq!(summary.project_config_fixtures, 16);
    assert_eq!(summary.discovered_config_fixtures, 15);
    assert_eq!(summary.cases, 632);
    assert_eq!(summary.explicit_input_cases, 570);
    assert_eq!(summary.config_cases, 62);
    assert_eq!(summary.explicit_declared_roots, 604);
    assert_eq!(summary.explicit_missing_roots, 6);
    assert_eq!(summary.config_roots, 74);
    assert_eq!(summary.javascript_observation_applicable_cases, 572);
    assert_eq!(summary.required_target_module_matches, 0);
    assert_eq!(summary.effective_option_clear_cases, 0);
    assert_eq!(summary.cases_with_target_blocker, 632);
    assert_eq!(summary.cases_with_module_blocker, 632);
    assert_eq!(summary.cases_with_use_define_for_class_fields_blocker, 0);
    assert_eq!(summary.cases_with_no_emit_route, 0);
    assert_eq!(summary.cases_with_rejected_effective_options, 556);
    assert_eq!(summary.config_diagnostic_cases, 0);
    assert_eq!(summary.bootstrap_profile_admitted_cases, 0);
    assert_eq!(summary.not_run_cases, 632);
    assert_eq!(summary.reference_baselines_compared, 0);
    assert_eq!(summary.rejected_option_cases.len(), 22);
    assert_eq!(
        summary
            .rejected_option_cases
            .iter()
            .map(|row| (row.name.as_str(), row.cases))
            .collect::<BTreeMap<_, _>>(),
        BTreeMap::from([
            ("allowImportingTsExtensions", 0),
            ("allowJs", 26),
            ("composite", 0),
            ("declaration", 528),
            ("declarationDir", 6),
            ("declarationMap", 0),
            ("emitDeclarationOnly", 0),
            ("experimentalDecorators", 10),
            ("importHelpers", 0),
            ("incremental", 0),
            ("inlineSourceMap", 0),
            ("isolatedModules", 8),
            ("jsx", 0),
            ("noCheck", 0),
            ("noEmitHelpers", 0),
            ("outDir", 188),
            ("outFile", 174),
            ("rewriteRelativeImportExtensions", 0),
            ("resolveJsonModule", 0),
            ("sourceMap", 404),
            ("tsBuildInfoFile", 0),
            ("verbatimModuleSyntax", 0),
        ])
    );
    assert_eq!(
        summary
            .decisive_blockers
            .iter()
            .map(|row| (row.value.as_str(), row.cases))
            .collect::<Vec<_>>(),
        [
            ("required-option:target=absent", 620),
            ("required-option:target=ES5(1)", 12),
        ]
    );
    assert_eq!(
        summary
            .root_modes
            .iter()
            .map(|row| (row.value.as_str(), row.cases))
            .collect::<Vec<_>>(),
        [
            ("explicit-inputs", 570),
            ("project-config", 32),
            ("discovered-config", 30),
        ]
    );
    assert_eq!(
        summary
            .dispositions
            .iter()
            .map(|row| (row.value.as_str(), row.cases))
            .collect::<Vec<_>>(),
        [("deferred-profile", 632)]
    );
}

#[test]
fn missing_roots_and_focused_project_oracle_are_retained_as_canaries() {
    let manifest = parsed();
    let missing = manifest
        .cases
        .iter()
        .filter(|case| match &case.root_selection {
            RootSelection::ExplicitInputs { roots } => roots
                .iter()
                .any(|root| matches!(&root.presence, RootPresence::Missing)),
            _ => false,
        })
        .collect::<Vec<_>>();
    assert_eq!(missing.len(), 2);
    for case in missing {
        assert!(case.id.contains("invalidRootFile.json"));
        let RootSelection::ExplicitInputs { roots } = &case.root_selection else {
            unreachable!();
        };
        assert_eq!(
            roots
                .iter()
                .map(|root| (root.requested.as_str(), root.path.as_str()))
                .collect::<Vec<_>>(),
            [
                ("a", "/.src/tests/cases/projects/InvalidRootFile/a",),
                ("a.t", "/.src/tests/cases/projects/InvalidRootFile/a.t",),
                ("a.ts", "/.src/tests/cases/projects/InvalidRootFile/a.ts",),
            ]
        );
        assert!(roots
            .iter()
            .all(|root| matches!(&root.presence, RootPresence::Missing)));
    }

    let focused: Value = serde_json::from_slice(
        &fs::read(workspace().join(FOCUSED_ORACLE_PATH))
            .expect("failed to read focused project oracle"),
    )
    .expect("focused project oracle must be valid JSON");
    let focused_cases = focused["cases"]
        .as_array()
        .expect("focused cases must be an array");
    assert_eq!(focused_cases.len(), 6);
    for oracle in focused_cases {
        let id = oracle["case_id"].as_str().expect("focused case id");
        let case = manifest
            .cases
            .iter()
            .find(|candidate| candidate.id == id)
            .unwrap_or_else(|| panic!("focused case {id} absent"));
        let (config, roots, diagnostics) =
            config_parts(&case.root_selection).expect("focused case must use project config");
        assert!(matches!(
            &case.root_selection,
            RootSelection::ProjectConfig { .. }
        ));
        assert_eq!(
            relative_virtual_path(&case.current_directory, &config.path),
            oracle["config"]["path"].as_str().unwrap()
        );
        assert_eq!(config.sha256, oracle["config"]["sha256"].as_str().unwrap());
        assert_eq!(
            config.git_blob_sha1,
            oracle["config"]["git_blob_sha1"].as_str().unwrap()
        );
        assert_eq!(
            roots
                .iter()
                .map(|root| relative_virtual_path(&case.current_directory, &root.path))
                .collect::<Vec<_>>(),
            oracle["config"]["root_names"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            diagnostics,
            oracle["config"]["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .map(|diagnostic| diagnostic["code"].as_u64().unwrap() as u32)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            case.module_variant.value as i64,
            oracle["module"]["value"].as_i64().unwrap()
        );
    }
}

fn relative_virtual_path(base: &str, path: &str) -> String {
    Path::new(path)
        .strip_prefix(base)
        .unwrap_or_else(|_| panic!("{path} is not below {base}"))
        .to_string_lossy()
        .into_owned()
}

#[test]
fn node_reconstruction_matches_every_recorded_project_classification_row() {
    let output = Command::new("node")
        .arg(GENERATOR_PATH)
        .arg("--check")
        .current_dir(workspace())
        .output()
        .expect("failed to run H1 project classification generator");
    assert!(
        output.status.success(),
        "Node classification check failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("cases=632 configs=62 admitted=0 status=not-run"));
}

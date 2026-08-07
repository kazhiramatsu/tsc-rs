use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

const MANIFEST: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/typescript-6.0.3/transpile-suite-inventory.v1.json"
));
const MANIFEST_SHA256: &str = "e8992cf7d0a22dc55a9a17c0c52cc06f848970be5e99c7dbdc6f156af4ae7beb";
const GENERATOR_PATH: &str = "crates/oracle/h1-transpile-inventory.mjs";
const GENERATOR_SHA256: &str = "46346c955f480524679112c253352efeb9f91bdf997384514441952a5fdddbfe";
const CONTRACT_PATH: &str = ".github/ci/contracts/h1-transpile-inventory.schema.json";
const CONTRACT_SHA256: &str = "67518769af8865462a66619c193b4e011d898b892b25ed55140ede8e70f5c6d7";
const SUITE_PIN_PATH: &str = "vendor/typescript-6.0.3/test-suites-pin.v2.json";
const SUITE_PIN_SHA256: &str = "83f8edbb6f4535a19e61cf872532a46722f8cedbd2d746a0922dc507addc0879";
const PROFILE_PATH: &str = "ratchets/h1-emit-profile.v1.json";
const PROFILE_SHA256: &str = "2edf0ec23a59cef953bf3322397c642fb5e38b5a33bd98310349ca16188ee6be";
const TYPESCRIPT_BUNDLE_PATH: &str = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_BUNDLE_SHA256: &str =
    "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39";
const SOURCE_ROOT: &str = "ts-tests/tests/cases/transpile";
const VARY_BY: [&str; 3] = ["declarationMap", "sourceMap", "inlineSourceMap"];
const FEATURE_ROOTS: [&str; 7] = [
    "decorators",
    "export-equals",
    "import-equals",
    "jsx",
    "parameter-properties",
    "runtime-enums",
    "runtime-namespaces",
];
const RELEVANT_REJECTED_OPTIONS: [&str; 5] = [
    "declaration",
    "declarationMap",
    "emitDeclarationOnly",
    "inlineSourceMap",
    "sourceMap",
];
const IMPLEMENTATION_SOURCES: [(&str, &str); 3] = [
    (
        "src/testRunner/transpileRunner.ts",
        "3926aa9b7d88e953163ed1fee843d273783be131",
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
    runner_contract: RunnerContract,
    source_inventory_sha256: String,
    sources: Vec<SourceIdentity>,
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
struct Inputs {
    suite_pin: PathHash,
    transpile_suite: SuiteIdentity,
    h1_profile: PathHash,
    typescript_bundle: PathHash,
    implementation_sources: Vec<GitSourceIdentity>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SuiteIdentity {
    name: String,
    source_path: String,
    vendored_path: String,
    git_tree_sha1: String,
    blob_inventory_sha256: String,
    files: u64,
    bytes: u64,
    unique_blobs: usize,
    executable_paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GitSourceIdentity {
    source_path: String,
    git_blob_sha1: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunnerContract {
    enumeration: String,
    vary_by: Vec<String>,
    variation_limit: u64,
    configuration_order: String,
    unit_partition: String,
    unit_execution: String,
    run_kinds: Vec<String>,
    reference_baseline_state: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceIdentity {
    path: String,
    mode: String,
    bytes: u64,
    sha256: String,
    git_blob_sha1: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct Setting {
    name: String,
    value: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    source: usize,
    encoding: String,
    decoded_utf8_bytes: u64,
    decoded_sha256: String,
    settings: Vec<Setting>,
    units: Vec<Unit>,
    configurations: Vec<Configuration>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Unit {
    name: String,
    file_options: Vec<Setting>,
    content: ContentIdentity,
    rejected_feature_roots: Vec<String>,
    parse_diagnostic_codes: Vec<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContentIdentity {
    utf8_bytes: u64,
    sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct Configuration {
    variant: String,
    runner_name: String,
    overrides: Vec<Setting>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    source: usize,
    configuration: usize,
    kind: String,
    api: String,
    baseline_path: String,
    unit_outputs: Vec<UnitOutput>,
    report_diagnostics: bool,
    execution_state: String,
    reference_baseline_state: String,
    component_disposition: String,
    whole_program_equivalence: String,
    bootstrap_profile_admitted: bool,
    profile_blockers: Vec<String>,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct UnitOutput {
    unit: String,
    output_path: String,
    products: Vec<String>,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct Summary {
    source_files: u64,
    source_bytes: u64,
    unique_blobs: usize,
    fixtures: u64,
    configurations: u64,
    fixture_units: u64,
    cases: u64,
    module_cases: u64,
    declaration_cases: u64,
    unit_operations: u64,
    javascript_transform_printer_controls: u64,
    deferred_source_map_controls: u64,
    deferred_declaration_controls: u64,
    deferred_declaration_map_controls: u64,
    report_diagnostics_cases: u64,
    bootstrap_profile_admitted_cases: u64,
    not_run_cases: u64,
    reference_baselines_compared: u64,
}

#[derive(Debug)]
struct ParsedUnit {
    name: String,
    file_options: Vec<Setting>,
    content: String,
}

#[test]
fn transpile_runner_inventory_reconstructs_every_not_run_case() {
    assert_eq!(sha256_hex(MANIFEST), MANIFEST_SHA256, "manifest hash");
    let manifest: Manifest =
        serde_json::from_slice(MANIFEST).expect("transpile inventory must be valid JSON");
    let workspace = workspace();

    assert_eq!(manifest.schema, 1);
    assert_eq!(manifest.status, "classified-not-run");
    assert_eq!(manifest.phase, "H1.0a-transpile-runner-inventory");
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
        &manifest.inputs.suite_pin,
        SUITE_PIN_PATH,
        SUITE_PIN_SHA256,
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
    verify_suite_identity(&manifest.inputs.transpile_suite);
    verify_implementation_sources(&manifest.inputs.implementation_sources);
    verify_runner_contract(&manifest.runner_contract);

    let raw_sources = verify_sources(&workspace, &manifest);
    verify_fixtures_and_cases(&manifest, &raw_sources);
    assert_eq!(manifest.summary, expected_summary());
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("harness crate must be inside workspace")
        .to_path_buf()
}

fn verify_path_hash(workspace: &Path, pin: &PathHash, path: &str, expected_hash: &str) {
    assert_eq!(pin.path, path);
    assert_eq!(pin.sha256, expected_hash);
    let bytes = fs::read(workspace.join(path))
        .unwrap_or_else(|error| panic!("failed to read {path}: {error}"));
    assert_eq!(sha256_hex(&bytes), expected_hash, "{path} hash");
}

fn verify_suite_identity(suite: &SuiteIdentity) {
    assert_eq!(suite.name, "transpile");
    assert_eq!(suite.source_path, "tests/cases/transpile");
    assert_eq!(suite.vendored_path, SOURCE_ROOT);
    assert_eq!(
        suite.git_tree_sha1,
        "e457f4923a084d10e9902ab311f640f02467e20d"
    );
    assert_eq!(
        suite.blob_inventory_sha256,
        "d07d1ac154da492d5d1d5a01fd00eea830f9d372aff03215eda1baad8b2c12ac"
    );
    assert_eq!(suite.files, 22);
    assert_eq!(suite.bytes, 13_480);
    assert_eq!(suite.unique_blobs, 22);
    assert!(suite.executable_paths.is_empty());
}

fn verify_implementation_sources(sources: &[GitSourceIdentity]) {
    assert_eq!(sources.len(), IMPLEMENTATION_SOURCES.len());
    for (source, (path, blob)) in sources.iter().zip(IMPLEMENTATION_SOURCES) {
        assert_eq!(source.source_path, path);
        assert_eq!(source.git_blob_sha1, blob);
        assert_lower_hex(&source.git_blob_sha1, 40);
    }
}

fn verify_runner_contract(contract: &RunnerContract) {
    assert_eq!(
        contract.enumeration,
        "recursive files matching /\\.[cm]?[tj]sx?/i in tests/cases/transpile"
    );
    assert!(contract.vary_by.iter().map(String::as_str).eq(VARY_BY));
    assert_eq!(contract.variation_limit, 25);
    assert_eq!(
        contract.configuration_order,
        "vary_by order, then each comma-separated value order"
    );
    assert_eq!(
        contract.unit_partition,
        "harnessIO.makeUnitsFromTest with CRLF/LF line splitting and @filename metadata"
    );
    assert_eq!(
        contract.unit_execution,
        "each configuration runs each parsed unit independently in source order"
    );
    assert_eq!(contract.run_kinds, ["module", "declaration"]);
    assert_eq!(
        contract.reference_baseline_state,
        "path-pinned-content-not-vendored-or-compared"
    );
}

fn verify_sources(workspace: &Path, manifest: &Manifest) -> Vec<String> {
    assert_eq!(manifest.sources.len(), 22);
    let mut previous = None;
    let mut inventory = Vec::new();
    let mut total_bytes = 0_u64;
    let mut blobs = BTreeSet::new();
    let mut raw_sources = Vec::new();
    for source in &manifest.sources {
        assert_eq!(source.mode, "100644");
        assert_relative_path(&source.path);
        if let Some(previous) = previous {
            assert!(
                previous < source.path.as_str(),
                "sources must be path sorted"
            );
        }
        previous = Some(source.path.as_str());
        assert_lower_hex(&source.sha256, 64);
        assert_lower_hex(&source.git_blob_sha1, 40);
        let raw = fs::read(workspace.join(SOURCE_ROOT).join(&source.path))
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", source.path));
        assert_eq!(raw.len() as u64, source.bytes, "{} bytes", source.path);
        assert_eq!(sha256_hex(&raw), source.sha256, "{} SHA-256", source.path);
        let decoded = std::str::from_utf8(&raw)
            .unwrap_or_else(|error| panic!("{} is not UTF-8: {error}", source.path));
        assert!(!decoded.starts_with('\u{feff}'));
        raw_sources.push(decoded.to_owned());
        total_bytes += source.bytes;
        blobs.insert(source.git_blob_sha1.as_str());
        inventory.extend_from_slice(source.path.as_bytes());
        inventory.push(0);
        inventory.extend_from_slice(source.mode.as_bytes());
        inventory.push(0);
        inventory.extend_from_slice(source.bytes.to_string().as_bytes());
        inventory.push(0);
        inventory.extend_from_slice(source.sha256.as_bytes());
        inventory.push(0);
        inventory.extend_from_slice(source.git_blob_sha1.as_bytes());
        inventory.push(b'\n');
    }
    assert_eq!(total_bytes, 13_480);
    assert_eq!(blobs.len(), 22);
    assert_eq!(sha256_hex(&inventory), manifest.source_inventory_sha256);
    raw_sources
}

fn verify_fixtures_and_cases(manifest: &Manifest, raw_sources: &[String]) {
    assert_eq!(manifest.fixtures.len(), manifest.sources.len());
    let mut expected_cases = Vec::new();
    for (source_index, ((source, raw), fixture)) in manifest
        .sources
        .iter()
        .zip(raw_sources)
        .zip(&manifest.fixtures)
        .enumerate()
    {
        assert_eq!(fixture.source, source_index);
        assert_eq!(fixture.encoding, "utf-8");
        assert_eq!(fixture.decoded_utf8_bytes, raw.len() as u64);
        assert_eq!(fixture.decoded_sha256, sha256_hex(raw.as_bytes()));
        let settings = extract_settings(raw);
        assert_eq!(fixture.settings, settings, "{} settings", source.path);
        let units = make_units(raw, &source.path);
        assert_eq!(
            fixture.units.len(),
            units.len(),
            "{} unit count",
            source.path
        );
        for (recorded, parsed) in fixture.units.iter().zip(&units) {
            assert_eq!(recorded.name, parsed.name);
            assert_eq!(recorded.file_options, parsed.file_options);
            assert_eq!(recorded.content.utf8_bytes, parsed.content.len() as u64);
            assert_eq!(
                recorded.content.sha256,
                sha256_hex(parsed.content.as_bytes())
            );
            assert_ordered_subset(&recorded.rejected_feature_roots, &FEATURE_ROOTS);
            assert!(recorded
                .parse_diagnostic_codes
                .windows(2)
                .all(|pair| pair[0] < pair[1]));
        }
        let configurations = expand_configurations(&settings, &source.path);
        assert_eq!(fixture.configurations, configurations);
        for (configuration_index, configuration) in configurations.iter().enumerate() {
            expected_cases.extend(expected_cases_for(
                source_index,
                source,
                fixture,
                configuration_index,
                configuration,
            ));
        }
    }

    assert_eq!(manifest.cases.len(), expected_cases.len());
    let mut ids = BTreeSet::new();
    let mut baselines = BTreeSet::new();
    for (case, expected) in manifest.cases.iter().zip(expected_cases) {
        verify_case(case, &expected);
        assert!(
            ids.insert(case.id.as_str()),
            "duplicate case ID {}",
            case.id
        );
        assert!(
            baselines.insert(case.baseline_path.as_str()),
            "duplicate baseline {}",
            case.baseline_path
        );
    }
    assert_eq!(derive_summary(manifest), expected_summary());
}

#[derive(Debug)]
struct ExpectedCase {
    id: String,
    source: usize,
    configuration: usize,
    kind: &'static str,
    api: &'static str,
    baseline_path: String,
    unit_outputs: Vec<UnitOutput>,
    report_diagnostics: bool,
    component_disposition: &'static str,
    profile_blockers: Vec<String>,
}

fn expected_cases_for(
    source_index: usize,
    source: &SourceIdentity,
    fixture: &Fixture,
    configuration_index: usize,
    configuration: &Configuration,
) -> Vec<ExpectedCase> {
    let settings = merge_settings(&fixture.settings, &configuration.overrides);
    let mut kinds = Vec::new();
    if setting(&settings, "emitDeclarationOnly").is_none() {
        kinds.push("module");
    }
    if setting(&settings, "declaration").is_some() {
        kinds.push("declaration");
    }
    assert!(!kinds.is_empty());
    let extension = Path::new(&source.path)
        .extension()
        .and_then(|extension| extension.to_str())
        .expect("transpile fixture extension");
    kinds
        .into_iter()
        .map(|kind| {
            let api = if kind == "module" {
                "transpileModule"
            } else {
                "transpileDeclaration"
            };
            let baseline_extension = if kind == "module" { ".js" } else { ".d.ts" };
            let baseline_path = format!(
                "tests/baselines/reference/transpile/{}{}",
                configuration.runner_name, baseline_extension
            );
            ExpectedCase {
                id: format!("transpile:{}#{}#{kind}", source.path, configuration.variant),
                source: source_index,
                configuration: configuration_index,
                kind,
                api,
                baseline_path,
                unit_outputs: expected_unit_outputs(kind, fixture, &settings),
                report_diagnostics: setting(&settings, "reportDiagnostics") == Some("true"),
                component_disposition: expected_disposition(kind, &settings),
                profile_blockers: expected_profile_blockers(kind, api, fixture, &settings),
            }
        })
        .inspect(|_| assert_eq!(extension, "ts"))
        .collect()
}

fn verify_case(case: &Case, expected: &ExpectedCase) {
    assert_eq!(case.id, expected.id);
    assert_eq!(case.source, expected.source);
    assert_eq!(case.configuration, expected.configuration);
    assert_eq!(case.kind, expected.kind);
    assert_eq!(case.api, expected.api);
    assert_eq!(case.baseline_path, expected.baseline_path);
    assert_eq!(case.unit_outputs, expected.unit_outputs);
    assert_eq!(case.report_diagnostics, expected.report_diagnostics);
    assert_eq!(case.execution_state, "not-run");
    assert_eq!(
        case.reference_baseline_state,
        "path-pinned-content-not-vendored-or-compared"
    );
    assert_eq!(case.component_disposition, expected.component_disposition);
    assert_eq!(case.whole_program_equivalence, "unproven");
    assert!(!case.bootstrap_profile_admitted);
    assert_eq!(case.profile_blockers, expected.profile_blockers);
}

fn expected_unit_outputs(kind: &str, fixture: &Fixture, settings: &[Setting]) -> Vec<UnitOutput> {
    fixture
        .units
        .iter()
        .map(|unit| {
            let (output_path, mut products) = if kind == "module" {
                (
                    change_extension(&unit.name, ".js"),
                    vec!["javascript".to_owned()],
                )
            } else {
                (
                    change_extension(&unit.name, ".d.ts"),
                    vec!["declaration".to_owned()],
                )
            };
            if kind == "module" && setting(settings, "sourceMap") == Some("true") {
                products.push("javascript-map".to_owned());
            }
            if kind == "module" && setting(settings, "inlineSourceMap") == Some("true") {
                products.push("javascript-inline-map".to_owned());
            }
            if kind == "declaration" && setting(settings, "declarationMap") == Some("true") {
                products.push("declaration-map".to_owned());
            }
            UnitOutput {
                unit: unit.name.clone(),
                output_path,
                products,
            }
        })
        .collect()
}

fn expected_disposition(kind: &str, settings: &[Setting]) -> &'static str {
    if kind == "module" {
        if setting(settings, "sourceMap") == Some("true")
            || setting(settings, "inlineSourceMap") == Some("true")
        {
            "deferred-source-map-control"
        } else {
            "javascript-transform-printer-control"
        }
    } else if setting(settings, "declarationMap") == Some("true") {
        "deferred-declaration-map-control"
    } else {
        "deferred-declaration-control"
    }
}

fn expected_profile_blockers(
    kind: &str,
    api: &str,
    fixture: &Fixture,
    settings: &[Setting],
) -> Vec<String> {
    let mut blockers = vec![format!("api:component-only:{api}")];
    if setting(settings, "target")
        .map(str::to_ascii_lowercase)
        .as_deref()
        != Some("esnext")
    {
        blockers.push(format!(
            "required-option:target={}",
            setting(settings, "target").unwrap_or("absent")
        ));
    }
    if setting(settings, "module")
        .map(str::to_ascii_lowercase)
        .as_deref()
        != Some("preserve")
    {
        blockers.push(format!(
            "required-option:module={}",
            setting(settings, "module").unwrap_or("absent")
        ));
    }
    for name in RELEVANT_REJECTED_OPTIONS {
        if setting(settings, name) == Some("true") {
            blockers.push(format!("rejected-option:{name}"));
        }
    }
    let features = fixture
        .units
        .iter()
        .flat_map(|unit| unit.rejected_feature_roots.iter().map(String::as_str))
        .collect::<BTreeSet<_>>();
    for feature in FEATURE_ROOTS {
        if features.contains(feature) {
            blockers.push(format!("rejected-feature:{feature}"));
        }
    }
    if kind == "declaration" {
        blockers.push("product:declaration".to_owned());
    }
    blockers
}

fn derive_summary(manifest: &Manifest) -> Summary {
    let count_disposition = |disposition: &str| {
        manifest
            .cases
            .iter()
            .filter(|case| case.component_disposition == disposition)
            .count() as u64
    };
    Summary {
        source_files: manifest.sources.len() as u64,
        source_bytes: manifest.sources.iter().map(|source| source.bytes).sum(),
        unique_blobs: manifest
            .sources
            .iter()
            .map(|source| source.git_blob_sha1.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        fixtures: manifest.fixtures.len() as u64,
        configurations: manifest
            .fixtures
            .iter()
            .map(|fixture| fixture.configurations.len() as u64)
            .sum(),
        fixture_units: manifest
            .fixtures
            .iter()
            .map(|fixture| fixture.units.len() as u64)
            .sum(),
        cases: manifest.cases.len() as u64,
        module_cases: manifest
            .cases
            .iter()
            .filter(|case| case.kind == "module")
            .count() as u64,
        declaration_cases: manifest
            .cases
            .iter()
            .filter(|case| case.kind == "declaration")
            .count() as u64,
        unit_operations: manifest
            .cases
            .iter()
            .map(|case| case.unit_outputs.len() as u64)
            .sum(),
        javascript_transform_printer_controls: count_disposition(
            "javascript-transform-printer-control",
        ),
        deferred_source_map_controls: count_disposition("deferred-source-map-control"),
        deferred_declaration_controls: count_disposition("deferred-declaration-control"),
        deferred_declaration_map_controls: count_disposition("deferred-declaration-map-control"),
        report_diagnostics_cases: manifest
            .cases
            .iter()
            .filter(|case| case.report_diagnostics)
            .count() as u64,
        bootstrap_profile_admitted_cases: manifest
            .cases
            .iter()
            .filter(|case| case.bootstrap_profile_admitted)
            .count() as u64,
        not_run_cases: manifest
            .cases
            .iter()
            .filter(|case| case.execution_state == "not-run")
            .count() as u64,
        reference_baselines_compared: 0,
    }
}

const fn expected_summary() -> Summary {
    Summary {
        source_files: 22,
        source_bytes: 13_480,
        unique_blobs: 22,
        fixtures: 22,
        configurations: 25,
        fixture_units: 42,
        cases: 37,
        module_cases: 16,
        declaration_cases: 21,
        unit_operations: 79,
        javascript_transform_printer_controls: 14,
        deferred_source_map_controls: 2,
        deferred_declaration_controls: 20,
        deferred_declaration_map_controls: 1,
        report_diagnostics_cases: 2,
        bootstrap_profile_admitted_cases: 0,
        not_run_cases: 37,
        reference_baselines_compared: 0,
    }
}

fn extract_settings(content: &str) -> Vec<Setting> {
    let mut settings = Vec::new();
    for line in content.split('\n') {
        if let Some((name, value)) = parse_option_line(line.strip_suffix('\r').unwrap_or(line)) {
            set_ordered(&mut settings, name, value);
        }
    }
    settings
}

fn make_units(content: &str, fixture_path: &str) -> Vec<ParsedUnit> {
    let mut units = Vec::new();
    let mut current_content: Option<String> = None;
    let mut current_options = Vec::new();
    let mut current_name: Option<String> = None;
    for line in content.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if let Some((name, value)) = parse_option_line(line) {
            set_ordered(&mut current_options, name.clone(), value.clone());
            if !name.eq_ignore_ascii_case("filename") {
                continue;
            }
            if current_name.as_deref().is_some_and(|name| !name.is_empty()) {
                units.push(ParsedUnit {
                    name: current_name.take().expect("current unit name"),
                    file_options: std::mem::take(&mut current_options),
                    content: current_content.take().expect("intermediate unit content"),
                });
                current_name = Some(value);
            } else {
                current_name = Some(value);
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
    let name = if !units.is_empty()
        || current_name
            .as_deref()
            .is_some_and(|current_name| !current_name.is_empty())
    {
        current_name.unwrap_or_default()
    } else {
        Path::new(fixture_path)
            .file_name()
            .and_then(|name| name.to_str())
            .expect("fixture file name")
            .to_owned()
    };
    units.push(ParsedUnit {
        name,
        file_options: current_options,
        content: current_content.unwrap_or_default(),
    });
    units
}

fn parse_option_line(line: &str) -> Option<(String, String)> {
    let mut rest = line.strip_prefix("//")?;
    rest = rest.trim_start();
    rest = rest.strip_prefix('@')?;
    let name_end = rest
        .char_indices()
        .find_map(|(index, ch)| (!ch.is_ascii_alphanumeric() && ch != '_').then_some(index))
        .unwrap_or(rest.len());
    if name_end == 0 {
        return None;
    }
    let name = rest[..name_end].to_owned();
    rest = rest[name_end..].trim_start();
    rest = rest.strip_prefix(':')?.trim_start();
    Some((name, rest.trim().to_owned()))
}

fn set_ordered(settings: &mut Vec<Setting>, name: String, value: String) {
    if let Some(existing) = settings.iter_mut().find(|setting| setting.name == name) {
        existing.value = value;
    } else {
        settings.push(Setting { name, value });
    }
}

fn expand_configurations(settings: &[Setting], fixture_path: &str) -> Vec<Configuration> {
    let mut dimensions = Vec::new();
    let mut count = 1_usize;
    for name in VARY_BY {
        let Some(value) = setting(settings, name) else {
            continue;
        };
        let entries = value
            .split(',')
            .map(|entry| entry.trim().to_ascii_lowercase())
            .filter(|entry| !entry.is_empty())
            .collect::<Vec<_>>();
        if entries.len() <= 1 {
            continue;
        }
        count *= entries.len();
        assert!(count <= 25, "{fixture_path} exceeds variation limit");
        dimensions.push((name, entries));
    }
    if dimensions.is_empty() {
        let stem = file_stem(fixture_path);
        return vec![Configuration {
            variant: "default".to_owned(),
            runner_name: stem.to_owned(),
            overrides: Vec::new(),
        }];
    }
    let mut states = vec![Vec::new()];
    for (name, entries) in dimensions {
        let mut next = Vec::new();
        for state in states {
            for value in &entries {
                let mut candidate = state.clone();
                candidate.push(Setting {
                    name: name.to_owned(),
                    value: value.clone(),
                });
                next.push(candidate);
            }
        }
        states = next;
    }
    let stem = file_stem(fixture_path);
    states
        .into_iter()
        .map(|overrides| {
            let variant = overrides
                .iter()
                .map(|setting| format!("{}={}", setting.name, setting.value))
                .collect::<Vec<_>>()
                .join(",");
            Configuration {
                runner_name: format!("{stem}({variant})"),
                variant,
                overrides,
            }
        })
        .collect()
}

fn merge_settings(settings: &[Setting], overrides: &[Setting]) -> Vec<Setting> {
    let mut merged = settings.to_vec();
    for override_ in overrides {
        set_ordered(&mut merged, override_.name.clone(), override_.value.clone());
    }
    merged
}

fn setting<'a>(settings: &'a [Setting], name: &str) -> Option<&'a str> {
    settings
        .iter()
        .find(|setting| setting.name == name)
        .map(|setting| setting.value.as_str())
}

fn file_stem(path: &str) -> &str {
    Path::new(path)
        .file_stem()
        .and_then(|name| name.to_str())
        .expect("fixture stem")
}

fn change_extension(path: &str, extension: &str) -> String {
    let stem = path.rsplit_once('.').map_or(path, |(stem, _)| stem);
    format!("{stem}{extension}")
}

fn assert_ordered_subset(values: &[String], order: &[&str]) {
    let expected = order
        .iter()
        .filter(|candidate| values.iter().any(|value| value == **candidate))
        .copied()
        .collect::<Vec<_>>();
    assert!(values.iter().map(String::as_str).eq(expected));
    assert_eq!(values.len(), values.iter().collect::<BTreeSet<_>>().len());
}

fn assert_relative_path(path: &str) {
    assert!(!path.is_empty());
    assert!(!path.starts_with('/'));
    assert!(!path
        .split('/')
        .any(|component| component.is_empty() || component == "." || component == ".."));
}

fn assert_lower_hex(value: &str, length: usize) {
    assert_eq!(value.len(), length);
    assert!(value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

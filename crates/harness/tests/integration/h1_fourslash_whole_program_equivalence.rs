use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

const RECORDED: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/typescript-6.0.3/fourslash-whole-program-equivalence.v1.json"
));
const PROJECTION: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/typescript-6.0.3/fourslash-emit-projection.v1.json"
));
const MANIFEST_SHA256: &str = "88cf5d26976061700c1417be71fae4fee7d5c52ef61d7c2f6df064db168d5837";
const GENERATOR_PATH: &str = "crates/oracle/h1-fourslash-equivalence.mjs";
const GENERATOR_SHA256: &str = "4928e17a87c6af722e2f7a39136278251b7aa2b7161d8077317592100b60a2d7";
const CONTRACT_PATH: &str = ".github/ci/contracts/h1-fourslash-equivalence.schema.json";
const CONTRACT_SHA256: &str = "17f545e8244b11d2ead9e2e387c32c11d6a71d6c8d025e8bd50c82a78b452522";
const SUITE_PIN_PATH: &str = "vendor/typescript-6.0.3/test-suites-pin.v3.json";
const SUITE_PIN_SHA256: &str = "5f7aee7d434066017c5cd115fb2195ff4959e5203eddc7ed9dafaf705cb38b34";
const PROJECTION_PATH: &str = "vendor/typescript-6.0.3/fourslash-emit-projection.v1.json";
const PROJECTION_SHA256: &str = "d652d0e0ad1a6195cb3d74e97cb241f3da6a55b6811bd4770fb1ec56a2843c46";
const PROFILE_PATH: &str = "ratchets/h1-emit-profile.v1.json";
const PROFILE_SHA256: &str = "501c363f2ea6c626d46b195daab949886cc9bacb1314f3c6584a1f82bd76ef8f";
const TYPESCRIPT_BUNDLE_PATH: &str = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_BUNDLE_SHA256: &str =
    "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39";
const SOURCE_ROOT: &str = "ts-tests/tests/cases/fourslash";
const REFERENCE_BASELINE_STATE: &str = "content-not-vendored-or-compared";
const VIRTUAL_BASE_PATH: &str = "/tests/cases/fourslash";

const IMPLEMENTATION_SOURCES: [(&str, &str); 2] = [
    (
        "src/harness/fourslashImpl.ts",
        "dc6341ef018f79e5d55a1d59aeafaeae2932c3d6",
    ),
    (
        "src/harness/fourslashInterfaceImpl.ts",
        "6178b2723f13e86f78261d848f48af8f4a998e18",
    ),
];

const BUNDLE_DECLARATIONS: [(&str, usize, usize, usize, usize, &str); 2] = [
    (
        "getFileEmitOutput",
        6_118_484,
        6_118_964,
        130_904,
        130_911,
        "b8dc847c489382197ad6b85f3a109cdb1d54a050d805ff46c986bd601e7ab421",
    ),
    (
        "getEmitOutput",
        7_102_303,
        7_102_668,
        154_020,
        154_025,
        "ea72a701544cbbf9389898d750a700a7e532f21df4d75dd85f45921d526ac9bd",
    ),
];

const OPERATION_METHODS: [&str; 4] = [
    "baselineGetEmitOutput",
    "getEmitOutput",
    "verifyGetEmitOutputForCurrentFile",
    "verifyGetEmitOutputContentsForCurrentFile",
];

static PARSED: OnceLock<Manifest> = OnceLock::new();
static PARSED_PROJECTION: OnceLock<ProjectionEnvelope> = OnceLock::new();

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
struct BundleDeclaration {
    name: String,
    start_offset: usize,
    end_offset: usize,
    start_line: usize,
    end_line: usize,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Inputs {
    suite_pin_v3: PathHash,
    emit_projection: PathHash,
    h1_profile: PathHash,
    typescript_bundle: PathHash,
    implementation_sources: Vec<GitSourceIdentity>,
    bundle_declarations: Vec<BundleDeclaration>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassificationContract {
    fourslash_defaults: String,
    operation_route: String,
    h1_request: String,
    promotion_rule: String,
    required_options: Vec<String>,
    execution_state: String,
    reference_baseline_state: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum Origin {
    FourslashDefault,
    GlobalSetting,
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct Setting {
    name: String,
    value: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct VirtualFile {
    path: String,
    emit_this_file: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureIdentity {
    path: String,
    bytes: u64,
    sha256: String,
    git_blob_sha1: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Operation {
    method: String,
    line: usize,
    language_service_method: String,
    selection: String,
    selected_files: Vec<String>,
    targeted_program_emit_calls: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceAnalysis {
    state: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
enum ExpectedObservation {
    BaselinePathPinnedContentNotVendoredOrCompared { baseline_path: String },
    InlineExpectationNotExecuted,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    case: usize,
    id: String,
    fixture: FixtureIdentity,
    test_type: String,
    global_settings: Vec<Setting>,
    virtual_files: Vec<VirtualFile>,
    config: Option<String>,
    config_roots: Vec<String>,
    config_diagnostic_codes: Vec<u32>,
    operation: Operation,
    effective_profile: EffectiveProfile,
    source_analysis: SourceAnalysis,
    expected_observation: ExpectedObservation,
    equivalence_decisive_blocker: String,
    equivalence_blockers: Vec<String>,
    whole_program_equivalence: String,
    promotion_state: String,
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
    fixture_bytes: u64,
    native_cases: u64,
    server_cases: u64,
    config_cases: u64,
    config_diagnostic_cases: u64,
    virtual_files: u64,
    emit_this_file_true: u64,
    emit_this_file_false: u64,
    targeted_program_emit_calls: u64,
    operation_methods: Vec<ValueCount>,
    selection_modes: Vec<ValueCount>,
    target_states: Vec<ValueCount>,
    module_states: Vec<ValueCount>,
    cases_with_rejected_effective_options: u64,
    rejected_option_cases: Vec<OptionCount>,
    baseline_path_observations: u64,
    inline_expectations: u64,
    targeted_api_blocked_cases: u64,
    required_target_module_matches: u64,
    promotion_candidates: u64,
    promoted_controls: u64,
    deferred_controls: u64,
    not_run_cases: u64,
    reference_baselines_compared: u64,
}

#[derive(Debug, Deserialize)]
struct ProjectionEnvelope {
    fixtures: Vec<ProjectionFixture>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectionFixture {
    path: String,
    git_blob_sha1: String,
    bytes: u64,
    operation: ProjectionOperation,
    emit_this_file_directives: Vec<EmitThisFileDirective>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectionOperation {
    method: String,
    line: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct EmitThisFileDirective {
    line: usize,
    value: bool,
}

#[derive(Debug)]
struct ParsedFixture {
    global_settings: Vec<Setting>,
    files: Vec<ParsedVirtualFile>,
}

#[derive(Debug)]
struct ParsedVirtualFile {
    path: String,
    emit_this_file: Option<bool>,
    content: String,
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
            .expect("H1 FourSlash equivalence classification must be strict, valid JSON")
    })
}

fn projection() -> &'static ProjectionEnvelope {
    PARSED_PROJECTION.get_or_init(|| {
        serde_json::from_slice(PROJECTION).expect("FourSlash projection must be valid JSON")
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn git_blob_sha1(bytes: &[u8]) -> String {
    let mut child = Command::new("git")
        .args(["hash-object", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to start git hash-object");
    child
        .stdin
        .take()
        .expect("git hash-object stdin")
        .write_all(bytes)
        .expect("failed to send bytes to git hash-object");
    let output = child
        .wait_with_output()
        .expect("failed to wait for git hash-object");
    assert!(
        output.status.success(),
        "git hash-object failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git hash-object output must be UTF-8")
        .trim()
        .to_owned()
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
fn recorded_classification_is_bound_to_the_exact_routes_projection_and_profile() {
    assert_eq!(sha256_hex(RECORDED), MANIFEST_SHA256, "manifest hash");
    assert_eq!(sha256_hex(PROJECTION), PROJECTION_SHA256, "projection hash");

    let manifest = parsed();
    let workspace = workspace();
    assert_eq!(manifest.schema, 1);
    assert_eq!(manifest.status, "classified-not-run");
    assert_eq!(manifest.phase, "H1.0a-fourslash-whole-program-equivalence");
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
        &manifest.inputs.suite_pin_v3,
        SUITE_PIN_PATH,
        SUITE_PIN_SHA256,
    );
    verify_path_hash(
        &workspace,
        &manifest.inputs.emit_projection,
        PROJECTION_PATH,
        PROJECTION_SHA256,
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

    verify_bundle_route(&workspace, &manifest.inputs.bundle_declarations);

    let schema: Value = serde_json::from_slice(
        &fs::read(workspace.join(CONTRACT_PATH)).expect("failed to read equivalence schema"),
    )
    .expect("equivalence schema must be valid JSON");
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(schema["$defs"]["case"]["additionalProperties"], false);
    assert_eq!(schema["$defs"]["summary"]["additionalProperties"], false);

    let contract = &manifest.classification_contract;
    assert_eq!(
        contract.fourslash_defaults,
        "getDefaultCompilerOptions, jsx cleared, CRLF newline, then global settings and skipDefaultLibCheck=true; virtual config options are parsed with existing options winning"
    );
    assert_eq!(
        contract.operation_route,
        "every selected operation calls LanguageService.getEmitOutput(fileName), which calls getFileEmitOutput and Program.emit(sourceFile)"
    );
    assert_eq!(
        contract.h1_request,
        "ProgramSession::emit is a whole-Program request with no target source"
    );
    assert_eq!(
        contract.promotion_rule,
        "a control requires exact targeted-versus-whole-Program observation equivalence and the frozen H1 profile before promotion"
    );
    assert_eq!(
        contract.required_options,
        ["target=ESNext(99)", "module=Preserve(200)"]
    );
    assert_eq!(contract.execution_state, "not-run");
    assert_eq!(contract.reference_baseline_state, REFERENCE_BASELINE_STATE);
}

fn verify_bundle_route(workspace: &Path, declarations: &[BundleDeclaration]) {
    assert_eq!(declarations.len(), BUNDLE_DECLARATIONS.len());
    let bytes = fs::read(workspace.join(TYPESCRIPT_BUNDLE_PATH))
        .expect("failed to read vendored TypeScript bundle");
    let text = std::str::from_utf8(&bytes).expect("TypeScript bundle must be UTF-8");
    for (actual, expected) in declarations.iter().zip(BUNDLE_DECLARATIONS) {
        assert_eq!(actual.name, expected.0);
        assert_eq!(actual.start_offset, expected.1);
        assert_eq!(actual.end_offset, expected.2);
        assert_eq!(actual.start_line, expected.3);
        assert_eq!(actual.end_line, expected.4);
        assert_eq!(actual.sha256, expected.5);
        assert!(text.is_char_boundary(actual.start_offset));
        assert!(text.is_char_boundary(actual.end_offset));
        let declaration = &text[actual.start_offset..actual.end_offset];
        assert_eq!(sha256_hex(declaration.as_bytes()), actual.sha256);
        assert_eq!(source_line(text, actual.start_offset), actual.start_line);
        assert_eq!(source_line(text, actual.end_offset), actual.end_line);
    }
    let file_emit = &text[declarations[0].start_offset..declarations[0].end_offset];
    let service_emit = &text[declarations[1].start_offset..declarations[1].end_offset];
    assert!(file_emit.contains("program.emit(sourceFile"));
    assert!(service_emit.contains("getFileEmitOutput(program, sourceFile"));
}

fn source_line(text: &str, offset: usize) -> usize {
    text.as_bytes()[..offset]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1
}

#[test]
fn every_projected_operation_has_an_independent_zero_promotion_disposition() {
    let manifest = parsed();
    let projection = projection();
    let workspace = workspace();
    let rejected_options = profile_rejected_options(&workspace);
    assert_eq!(manifest.cases.len(), 38);
    assert_eq!(manifest.cases.len(), projection.fixtures.len());

    let mut paths = BTreeSet::new();
    for (index, (case, projected)) in manifest.cases.iter().zip(&projection.fixtures).enumerate() {
        assert_eq!(case.case, index);
        assert_eq!(case.fixture.path, projected.path);
        assert!(paths.insert(case.fixture.path.as_str()));
        assert_eq!(
            case.id,
            format!("typescript-6.0.3/fourslash/{}", projected.path)
        );
        assert_eq!(
            case.test_type,
            if projected.path.starts_with("server/") {
                "server"
            } else {
                "native"
            }
        );

        let raw = fs::read(workspace.join(SOURCE_ROOT).join(&projected.path))
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", projected.path));
        assert_eq!(case.fixture.bytes, raw.len() as u64);
        assert_eq!(case.fixture.bytes, projected.bytes);
        assert_eq!(case.fixture.sha256, sha256_hex(&raw));
        assert_eq!(case.fixture.git_blob_sha1, git_blob_sha1(&raw));
        assert_eq!(case.fixture.git_blob_sha1, projected.git_blob_sha1);
        let text = std::str::from_utf8(&raw)
            .unwrap_or_else(|error| panic!("{} is not UTF-8: {error}", projected.path));

        let (operations, directives) = fourslash_observations(text);
        assert_eq!(
            operations,
            [(projected.operation.method.clone(), projected.operation.line)],
            "{} operation",
            projected.path
        );
        assert_eq!(
            directives, projected.emit_this_file_directives,
            "{} emitThisFile directives",
            projected.path
        );
        assert_eq!(case.operation.method, projected.operation.method);
        assert_eq!(case.operation.line, projected.operation.line);
        assert_eq!(
            case.operation.language_service_method,
            "getEmitOutput(fileName)"
        );

        let parsed_fixture = parse_fixture(text, &projected.path);
        assert_eq!(case.global_settings, parsed_fixture.global_settings);
        let observed_virtual_files = parsed_fixture
            .files
            .iter()
            .map(|file| VirtualFile {
                path: file.path.clone(),
                emit_this_file: file.emit_this_file,
            })
            .collect::<Vec<_>>();
        assert_eq!(case.virtual_files, observed_virtual_files);
        assert_eq!(
            case.virtual_files
                .iter()
                .filter_map(|file| file.emit_this_file)
                .collect::<Vec<_>>(),
            directives
                .iter()
                .map(|directive| directive.value)
                .collect::<Vec<_>>()
        );
        verify_operation_selection(case, &parsed_fixture, text);
        verify_config_observation(case);
        verify_expected_observation(case);

        verify_enum_projection(&case.effective_profile.target, canonical_target_name);
        verify_enum_projection(&case.effective_profile.module, canonical_module_name);
        assert_eq!(enum_value(&case.effective_profile.target), Some(12));
        assert_eq!(
            enum_origin(&case.effective_profile.target),
            Some(Origin::FourslashDefault)
        );
        assert_eq!(
            boolean_value(&case.effective_profile.use_define_for_class_fields),
            None
        );
        assert_eq!(boolean_value(&case.effective_profile.no_emit), None);
        let expected_blockers = derive_blockers(&case.effective_profile, &rejected_options);
        assert_eq!(case.equivalence_blockers, expected_blockers);
        assert_eq!(case.equivalence_decisive_blocker, expected_blockers[0]);
        assert_eq!(
            case.source_analysis.state,
            "not-required-effective-options-and-api-route"
        );
        assert_eq!(case.whole_program_equivalence, "deferred");
        assert_eq!(case.promotion_state, "not-promoted");
        assert_eq!(case.execution_state, "not-run");
        assert_eq!(case.reference_baseline_state, REFERENCE_BASELINE_STATE);
    }
    assert_eq!(paths.len(), 38);
    assert!(manifest
        .cases
        .windows(2)
        .all(|pair| pair[0].fixture.path < pair[1].fixture.path));

    let derived = derive_summary(manifest, &rejected_options);
    assert_eq!(manifest.summary, derived);
    assert_frozen_summary(&manifest.summary);
}

fn parse_fixture(text: &str, fixture_path: &str) -> ParsedFixture {
    let mut global_settings = Vec::new();
    let mut global_names = BTreeSet::new();
    let mut files = Vec::new();
    let mut current_path = fixture_path.to_owned();
    let mut current_emit = None;
    let mut current_content: Option<String> = None;

    let next_file = |files: &mut Vec<ParsedVirtualFile>,
                     current_path: &str,
                     current_emit: Option<bool>,
                     current_content: &mut Option<String>| {
        let Some(content) = current_content.take() else {
            return;
        };
        files.push(ParsedVirtualFile {
            path: normalize_virtual_path(current_path),
            emit_this_file: current_emit,
            content,
        });
    };

    for raw_line in text.split('\n') {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if let Some(content) = line.strip_prefix("////") {
            match &mut current_content {
                Some(existing) => {
                    existing.push('\n');
                    existing.push_str(content);
                }
                None => current_content = Some(content.to_owned()),
            }
            continue;
        }
        if let Some((name, value)) = metadata(line) {
            match name.as_str() {
                "filename" => {
                    next_file(
                        &mut files,
                        &current_path,
                        current_emit,
                        &mut current_content,
                    );
                    current_path = value;
                    current_emit = None;
                }
                "emitthisfile" => {
                    current_emit = Some(match value.as_str() {
                        "true" => true,
                        "false" => false,
                        other => panic!("invalid emitThisFile value {other}"),
                    });
                }
                "resolvereference" | "symlink" => {}
                _ => {
                    assert!(
                        global_names.insert(name.clone()),
                        "duplicate global setting {name}"
                    );
                    global_settings.push(Setting { name, value });
                }
            }
            continue;
        }
        if !line.is_empty() {
            next_file(
                &mut files,
                &current_path,
                current_emit,
                &mut current_content,
            );
            current_path = fixture_path.to_owned();
            current_emit = None;
        }
    }
    next_file(
        &mut files,
        &current_path,
        current_emit,
        &mut current_content,
    );
    assert!(!files.is_empty(), "{fixture_path} has no virtual files");
    ParsedFixture {
        global_settings,
        files,
    }
}

fn metadata(line: &str) -> Option<(String, String)> {
    let metadata = line.strip_prefix("//")?.trim();
    let metadata = metadata.strip_prefix('@')?;
    let (name, value) = metadata.split_once(':')?;
    let name = name.trim();
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return None;
    }
    Some((name.to_ascii_lowercase(), value.trim().to_owned()))
}

fn normalize_virtual_path(file_name: &str) -> String {
    let normalized = file_name.replace('\\', "/");
    let absolute = if normalized.starts_with('/') {
        normalized
    } else {
        format!("{VIRTUAL_BASE_PATH}/{normalized}")
    };
    let mut components = Vec::new();
    for component in absolute.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                components.pop().expect("virtual path must remain absolute");
            }
            component => components.push(component),
        }
    }
    format!("/{}", components.join("/"))
}

fn fourslash_observations(text: &str) -> (Vec<(String, usize)>, Vec<EmitThisFileDirective>) {
    let mut operations = Vec::new();
    let mut directives = Vec::new();
    for (index, line) in text.split('\n').enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim_start();
        for method in OPERATION_METHODS {
            let prefix = format!("verify.{method}");
            if trimmed
                .strip_prefix(&prefix)
                .is_some_and(|rest| rest.trim_start().starts_with('('))
            {
                operations.push((method.to_owned(), line_number));
            }
        }
        if let Some((name, value)) = metadata(trimmed) {
            if name == "emitthisfile" {
                directives.push(EmitThisFileDirective {
                    line: line_number,
                    value: match value.as_str() {
                        "true" => true,
                        "false" => false,
                        other => panic!("invalid emitThisFile value {other}"),
                    },
                });
            }
        }
    }
    (operations, directives)
}

fn verify_operation_selection(case: &Case, parsed: &ParsedFixture, text: &str) {
    let virtual_paths = parsed
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        case.operation.selected_files.len() as u64,
        case.operation.targeted_program_emit_calls
    );
    assert_eq!(
        case.operation
            .selected_files
            .iter()
            .collect::<BTreeSet<_>>()
            .len(),
        case.operation.selected_files.len()
    );
    assert!(case
        .operation
        .selected_files
        .iter()
        .all(|selected| virtual_paths.contains(selected.as_str())));

    if case.operation.selection == "emit-this-file-true" {
        assert!(!case.operation.method.starts_with("verifyGetEmitOutput"));
        assert_eq!(
            case.operation.selected_files,
            parsed
                .files
                .iter()
                .filter(|file| file.emit_this_file == Some(true))
                .map(|file| file.path.clone())
                .collect::<Vec<_>>()
        );
        return;
    }

    assert_eq!(case.operation.selection, "active-file");
    let prefix = text
        .split_inclusive('\n')
        .take(case.operation.line.saturating_sub(1))
        .collect::<String>();
    let marker = last_marker_name(&prefix).expect("active-file operation must have a marker");
    let owners = parsed
        .files
        .iter()
        .filter(|file| file.content.contains(&format!("/*{marker}*/")))
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    assert_eq!(case.operation.selected_files, owners);
    let expected = match case.fixture.path.as_str() {
        "constEnumsEmitOutputInMultipleFiles.ts" => "/tests/cases/fourslash/b.ts",
        "diagnosticsJsFileCompilationDuplicateFunctionImplementation.ts" => {
            "/tests/cases/fourslash/a.ts"
        }
        other => panic!("unexpected active-file fixture {other}"),
    };
    assert_eq!(case.operation.selected_files, [expected]);
}

fn last_marker_name(text: &str) -> Option<String> {
    let mut remaining = text;
    let mut marker = None;
    while let Some(offset) = remaining.find("goTo.marker(") {
        remaining = &remaining[offset + "goTo.marker(".len()..];
        let argument = remaining.trim_start();
        let quote = argument.chars().next()?;
        if quote != '\'' && quote != '"' {
            continue;
        }
        let value = &argument[quote.len_utf8()..];
        let end = value.find(quote)?;
        marker = Some(value[..end].to_owned());
        remaining = &value[end + quote.len_utf8()..];
    }
    marker
}

fn verify_config_observation(case: &Case) {
    let virtual_paths = case
        .virtual_files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<BTreeSet<_>>();
    match &case.config {
        Some(config) => {
            assert_eq!(case.test_type, "server");
            assert!(virtual_paths.contains(config.as_str()));
            assert_eq!(case.config_roots.len(), 1);
            assert!(case
                .config_roots
                .iter()
                .all(|root| virtual_paths.contains(root.as_str())));
        }
        None => assert!(case.config_roots.is_empty()),
    }
    assert!(case.config_diagnostic_codes.is_empty());
}

fn verify_expected_observation(case: &Case) {
    match &case.expected_observation {
        ExpectedObservation::BaselinePathPinnedContentNotVendoredOrCompared { baseline_path } => {
            assert_eq!(case.operation.method, "baselineGetEmitOutput");
            let baseline = case
                .global_settings
                .iter()
                .find(|setting| setting.name == "baselinefile")
                .map(|setting| setting.value.clone())
                .unwrap_or_else(|| {
                    format!(
                        "{}.baseline",
                        case.fixture
                            .path
                            .strip_suffix(".ts")
                            .expect("FourSlash fixture must have a .ts suffix")
                    )
                });
            assert_eq!(
                baseline_path,
                &format!("tests/baselines/reference/fourslash/{baseline}")
            );
        }
        ExpectedObservation::InlineExpectationNotExecuted => {
            assert_ne!(case.operation.method, "baselineGetEmitOutput");
        }
    }
}

fn enum_value(projection: &EnumProjection) -> Option<i32> {
    match projection {
        EnumProjection::Absent => None,
        EnumProjection::Set { value, .. } => Some(*value),
    }
}

fn enum_origin(projection: &EnumProjection) -> Option<Origin> {
    match projection {
        EnumProjection::Absent => None,
        EnumProjection::Set { origin, .. } => Some(*origin),
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
            assert!(matches!(
                origin,
                Origin::FourslashDefault | Origin::GlobalSetting | Origin::VirtualConfig
            ));
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
        assert_eq!(Some(name.as_str()), canonical_name(*value));
        assert!(matches!(
            origin,
            Origin::FourslashDefault | Origin::GlobalSetting | Origin::VirtualConfig
        ));
    }
}

fn derive_blockers(profile: &EffectiveProfile, rejected_options: &[String]) -> Vec<String> {
    let mut blockers = vec!["api:language-service-targeted-emit".to_owned()];
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
            assert!(previous < index, "rejected option order changed");
        }
        previous = Some(index);
        assert!(!matches!(rejected.value, Value::Null | Value::Bool(false)));
        assert!(matches!(
            rejected.origin,
            Origin::FourslashDefault | Origin::GlobalSetting | Origin::VirtualConfig
        ));
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
        fixtures: cases.len() as u64,
        fixture_bytes: cases.iter().map(|case| case.fixture.bytes).sum(),
        native_cases: cases
            .iter()
            .filter(|case| case.test_type == "native")
            .count() as u64,
        server_cases: cases
            .iter()
            .filter(|case| case.test_type == "server")
            .count() as u64,
        config_cases: cases.iter().filter(|case| case.config.is_some()).count() as u64,
        config_diagnostic_cases: cases
            .iter()
            .filter(|case| !case.config_diagnostic_codes.is_empty())
            .count() as u64,
        virtual_files: cases
            .iter()
            .map(|case| case.virtual_files.len() as u64)
            .sum(),
        emit_this_file_true: cases
            .iter()
            .flat_map(|case| &case.virtual_files)
            .filter(|file| file.emit_this_file == Some(true))
            .count() as u64,
        emit_this_file_false: cases
            .iter()
            .flat_map(|case| &case.virtual_files)
            .filter(|file| file.emit_this_file == Some(false))
            .count() as u64,
        targeted_program_emit_calls: cases
            .iter()
            .map(|case| case.operation.targeted_program_emit_calls)
            .sum(),
        operation_methods: count_values(cases.iter().map(|case| case.operation.method.clone())),
        selection_modes: count_values(cases.iter().map(|case| case.operation.selection.clone())),
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
        baseline_path_observations: cases
            .iter()
            .filter(|case| {
                matches!(
                    case.expected_observation,
                    ExpectedObservation::BaselinePathPinnedContentNotVendoredOrCompared { .. }
                )
            })
            .count() as u64,
        inline_expectations: cases
            .iter()
            .filter(|case| {
                matches!(
                    case.expected_observation,
                    ExpectedObservation::InlineExpectationNotExecuted
                )
            })
            .count() as u64,
        targeted_api_blocked_cases: cases
            .iter()
            .filter(|case| {
                case.equivalence_blockers
                    .iter()
                    .any(|blocker| blocker == "api:language-service-targeted-emit")
            })
            .count() as u64,
        required_target_module_matches: cases
            .iter()
            .filter(|case| {
                enum_value(&case.effective_profile.target) == Some(99)
                    && enum_value(&case.effective_profile.module) == Some(200)
            })
            .count() as u64,
        promotion_candidates: cases
            .iter()
            .filter(|case| case.whole_program_equivalence == "candidate-not-run")
            .count() as u64,
        promoted_controls: cases
            .iter()
            .filter(|case| case.whole_program_equivalence == "proven-equivalent")
            .count() as u64,
        deferred_controls: cases
            .iter()
            .filter(|case| case.whole_program_equivalence == "deferred")
            .count() as u64,
        not_run_cases: cases
            .iter()
            .filter(|case| case.execution_state == "not-run")
            .count() as u64,
        reference_baselines_compared: 0,
    }
}

fn assert_frozen_summary(summary: &Summary) {
    assert_eq!(summary.fixtures, 38);
    assert_eq!(summary.fixture_bytes, 31_051);
    assert_eq!(summary.native_cases, 33);
    assert_eq!(summary.server_cases, 5);
    assert_eq!(summary.config_cases, 5);
    assert_eq!(summary.config_diagnostic_cases, 0);
    assert_eq!(summary.virtual_files, 94);
    assert_eq!(summary.emit_this_file_true, 47);
    assert_eq!(summary.emit_this_file_false, 2);
    assert_eq!(summary.targeted_program_emit_calls, 47);
    assert_eq!(summary.cases_with_rejected_effective_options, 29);
    assert_eq!(summary.baseline_path_observations, 31);
    assert_eq!(summary.inline_expectations, 7);
    assert_eq!(summary.targeted_api_blocked_cases, 38);
    assert_eq!(summary.required_target_module_matches, 0);
    assert_eq!(summary.promotion_candidates, 0);
    assert_eq!(summary.promoted_controls, 0);
    assert_eq!(summary.deferred_controls, 38);
    assert_eq!(summary.not_run_cases, 38);
    assert_eq!(summary.reference_baselines_compared, 0);
    assert_eq!(
        summary
            .operation_methods
            .iter()
            .map(|row| (row.value.as_str(), row.cases))
            .collect::<Vec<_>>(),
        [
            ("baselineGetEmitOutput", 31),
            ("getEmitOutput", 5),
            ("verifyGetEmitOutputContentsForCurrentFile", 1),
            ("verifyGetEmitOutputForCurrentFile", 1),
        ]
    );
    assert_eq!(
        summary
            .selection_modes
            .iter()
            .map(|row| (row.value.as_str(), row.cases))
            .collect::<Vec<_>>(),
        [("emit-this-file-true", 36), ("active-file", 2)]
    );
    assert_eq!(
        summary
            .target_states
            .iter()
            .map(|row| (row.value.as_str(), row.cases))
            .collect::<Vec<_>>(),
        [("ES2025(12)", 38)]
    );
    assert_eq!(
        summary
            .module_states
            .iter()
            .map(|row| (row.value.as_str(), row.cases))
            .collect::<Vec<_>>(),
        [("absent", 30), ("CommonJS(1)", 7), ("AMD(2)", 1)]
    );
    assert_eq!(
        summary
            .rejected_option_cases
            .iter()
            .map(|row| (row.name.as_str(), row.cases))
            .collect::<Vec<_>>(),
        [
            ("allowImportingTsExtensions", 0),
            ("allowJs", 2),
            ("composite", 0),
            ("declaration", 17),
            ("declarationDir", 0),
            ("declarationMap", 5),
            ("emitDeclarationOnly", 0),
            ("experimentalDecorators", 0),
            ("importHelpers", 0),
            ("incremental", 0),
            ("inlineSourceMap", 2),
            ("isolatedModules", 0),
            ("jsx", 2),
            ("noCheck", 0),
            ("noEmitHelpers", 0),
            ("outDir", 7),
            ("outFile", 12),
            ("rewriteRelativeImportExtensions", 0),
            ("resolveJsonModule", 0),
            ("sourceMap", 8),
            ("tsBuildInfoFile", 0),
            ("verbatimModuleSyntax", 0),
        ]
    );
}

#[test]
fn node_reconstruction_matches_every_recorded_equivalence_row() {
    let output = Command::new("node")
        .arg(GENERATOR_PATH)
        .arg("--check")
        .current_dir(workspace())
        .output()
        .expect("failed to run H1 FourSlash equivalence generator");
    assert!(
        output.status.success(),
        "Node equivalence check failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("cases=38 promoted=0 deferred=38 status=not-run"));
}

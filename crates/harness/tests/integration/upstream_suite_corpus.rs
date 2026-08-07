use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Deserialize;
use sha2::{Digest, Sha256};

const BASE_PIN: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/typescript-6.0.3/test-suites-pin.v1.json"
));
const PIN_V2: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/typescript-6.0.3/test-suites-pin.v2.json"
));
const PIN_V3: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/typescript-6.0.3/test-suites-pin.v3.json"
));
const FOURSLASH_PROJECTION_MANIFEST: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/typescript-6.0.3/fourslash-emit-projection.v1.json"
));
const BASE_PIN_RELATIVE_PATH: &str = "vendor/typescript-6.0.3/test-suites-pin.v1.json";
const BASE_PIN_SHA256: &str = "f231d984c31d5d16a6fb845e66a25bc9601ffd23212d548cb337149e40397da9";
const PIN_V2_RELATIVE_PATH: &str = "vendor/typescript-6.0.3/test-suites-pin.v2.json";
const PIN_V2_SHA256: &str = "83f8edbb6f4535a19e61cf872532a46722f8cedbd2d746a0922dc507addc0879";
const PIN_V3_SHA256: &str = "5f7aee7d434066017c5cd115fb2195ff4959e5203eddc7ed9dafaf705cb38b34";
const FOURSLASH_PROJECTION_MANIFEST_RELATIVE_PATH: &str =
    "vendor/typescript-6.0.3/fourslash-emit-projection.v1.json";
const FOURSLASH_PROJECTION_MANIFEST_SHA256: &str =
    "d652d0e0ad1a6195cb3d74e97cb241f3da6a55b6811bd4770fb1ec56a2843c46";
const FOURSLASH_PROJECTION_GENERATOR_RELATIVE_PATH: &str =
    "crates/oracle/fourslash-emit-projection.mjs";
const FOURSLASH_PROJECTION_GENERATOR_SHA256: &str =
    "0211bc1500582945457d12e523363084b131bbad0075e4d24a8d84a3447a1f85";
const TYPESCRIPT_VERSION: &str = "6.0.3";
const SOURCE_REPOSITORY: &str = "https://github.com/microsoft/TypeScript.git";
const SOURCE_COMMIT: &str = "050880ce59e30b356b686bd3144efe24f875ebc8";
const IMPLEMENTATION_SOURCES_V2: [(&str, &str); 1] = [(
    "src/testRunner/transpileRunner.ts",
    "3926aa9b7d88e953163ed1fee843d273783be131",
)];
const IMPLEMENTATION_SOURCES_V3: [(&str, &str); 3] = [
    IMPLEMENTATION_SOURCES_V2[0],
    (
        "src/harness/fourslashImpl.ts",
        "dc6341ef018f79e5d55a1d59aeafaeae2932c3d6",
    ),
    (
        "src/harness/fourslashInterfaceImpl.ts",
        "6178b2723f13e86f78261d848f48af8f4a998e18",
    ),
];
const FOURSLASH_OPERATION_METHODS: [&str; 4] = [
    "baselineGetEmitOutput",
    "getEmitOutput",
    "verifyGetEmitOutputContentsForCurrentFile",
    "verifyGetEmitOutputForCurrentFile",
];
const SUITES: [(&str, &str, &str); 4] = [
    (
        "compiler",
        "tests/cases/compiler",
        "ts-tests/tests/cases/compiler",
    ),
    (
        "project",
        "tests/cases/project",
        "ts-tests/tests/cases/project",
    ),
    (
        "projects",
        "tests/cases/projects",
        "ts-tests/tests/cases/projects",
    ),
    (
        "transpile",
        "tests/cases/transpile",
        "ts-tests/tests/cases/transpile",
    ),
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TestSuitesPin {
    schema: u32,
    typescript_version: String,
    source_repository: String,
    source_commit: String,
    base_pin: BasePinIdentity,
    suites: Vec<SuitePin>,
    implementation_sources: Vec<ImplementationSourcePin>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectedTestSuitesPin {
    schema: u32,
    typescript_version: String,
    source_repository: String,
    source_commit: String,
    base_pin: BasePinIdentity,
    suites: Vec<SuitePin>,
    implementation_sources: Vec<ImplementationSourcePin>,
    projections: Vec<ProjectionPin>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyTestSuitesPin {
    schema: u32,
    typescript_version: String,
    source_repository: String,
    source_commit: String,
    suites: Vec<SuitePin>,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct BasePinIdentity {
    path: String,
    sha256: String,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct SuitePin {
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

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct ImplementationSourcePin {
    source_path: String,
    git_blob_sha1: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectionPin {
    name: String,
    source_path: String,
    vendored_path: String,
    source_git_tree_sha1: String,
    source_blob_inventory_sha256: String,
    source_files: u64,
    source_bytes: u64,
    source_unique_blobs: usize,
    manifest: BasePinIdentity,
    projected_git_tree_sha1: String,
    projected_blob_inventory_sha256: String,
    projected_files: u64,
    projected_bytes: u64,
    projected_unique_blobs: usize,
    projected_executable_paths: Vec<String>,
    execution_state: String,
    expansion_rows: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FourSlashProjectionManifest {
    schema: u32,
    kind: String,
    status: String,
    typescript_version: String,
    source_repository: String,
    source_commit: String,
    generator: BasePinIdentity,
    source_tree: SourceTreeIdentity,
    selector: FourSlashSelector,
    projection: FourSlashProjectionIdentity,
    fixtures: Vec<FourSlashFixture>,
    qualification: FourSlashQualification,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceTreeIdentity {
    path: String,
    git_tree_sha1: String,
    blob_inventory_sha256: String,
    files: u64,
    bytes: u64,
    unique_blobs: usize,
    executable_paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FourSlashSelector {
    operation_call_pattern: String,
    operation_call_flags: String,
    operation_methods: Vec<String>,
    metadata_pattern: String,
    metadata_flags: String,
    broad_mentions: u64,
    selected_operation_files: u64,
    false_positive_controls: Vec<FalsePositiveControl>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FalsePositiveControl {
    path: String,
    git_blob_sha1: String,
    bytes: u64,
    mention_lines: Vec<usize>,
    disposition: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FourSlashProjectionIdentity {
    vendored_path: String,
    git_tree_sha1: String,
    blob_inventory_sha256: String,
    files: u64,
    bytes: u64,
    unique_blobs: usize,
    executable_paths: Vec<String>,
    summary: FourSlashSummary,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FourSlashSummary {
    fixture_files: u64,
    fixture_bytes: u64,
    unique_blobs: usize,
    operation_calls: u64,
    operation_counts: BTreeMap<String, u64>,
    fixtures_with_emit_this_file: u64,
    emit_this_file_directives: u64,
    emit_this_file_true: u64,
    emit_this_file_false: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FourSlashFixture {
    path: String,
    git_blob_sha1: String,
    bytes: u64,
    operation: FourSlashOperation,
    emit_this_file_directives: Vec<EmitThisFileDirective>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FourSlashOperation {
    method: String,
    line: usize,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct EmitThisFileDirective {
    line: usize,
    value: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FourSlashQualification {
    expansion_rows: u64,
    executed_rows: u64,
    passing_rows: u64,
    source_inventory_integrity: bool,
    fourslash_pass_rate: bool,
    language_service_compatibility: bool,
    whole_program_emit_equivalence: bool,
}

#[derive(Debug, Eq, PartialEq)]
struct BlobIdentity {
    mode: String,
    git_blob_sha1: String,
    bytes: u64,
}

#[derive(Debug, Default)]
struct SuiteStats {
    files: u64,
    bytes: u64,
    executable_paths: BTreeSet<String>,
}

static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

struct ScratchDir {
    root: PathBuf,
    global_config: PathBuf,
}

impl ScratchDir {
    fn new(suite: &str) -> Self {
        let serial = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "tsc-rs-upstream-suite-{suite}-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap_or_else(|error| {
            panic!(
                "failed to create scratch directory {}: {error}",
                path.display()
            )
        });
        let global_config = path.join("empty-global-config");
        fs::write(&global_config, []).unwrap_or_else(|error| {
            panic!(
                "failed to create isolated Git config {}: {error}",
                global_config.display()
            )
        });
        Self {
            root: path,
            global_config,
        }
    }

    fn isolate_git(&self, command: &mut Command) {
        command
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", &self.global_config)
            .env("GIT_ATTR_NOSYSTEM", "1");
        for name in [
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_COMMON_DIR",
            "GIT_CONFIG",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_PARAMETERS",
            "GIT_CONFIG_SYSTEM",
            "GIT_DIR",
            "GIT_INDEX_FILE",
            "GIT_NAMESPACE",
            "GIT_OBJECT_DIRECTORY",
            "GIT_QUARANTINE_PATH",
            "GIT_SHALLOW_FILE",
            "GIT_WORK_TREE",
        ] {
            command.env_remove(name);
        }
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn vendored_upstream_test_suites_match_exact_git_trees() {
    assert_eq!(
        format!("{:x}", Sha256::digest(BASE_PIN)),
        BASE_PIN_SHA256,
        "legacy test suite pin hash"
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(PIN_V2)),
        PIN_V2_SHA256,
        "H1 schema-2 test suite pin hash"
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(PIN_V3)),
        PIN_V3_SHA256,
        "H1 schema-3 test suite pin hash"
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(FOURSLASH_PROJECTION_MANIFEST)),
        FOURSLASH_PROJECTION_MANIFEST_SHA256,
        "FourSlash emit projection manifest hash"
    );
    let legacy: LegacyTestSuitesPin =
        serde_json::from_slice(BASE_PIN).expect("legacy test suite pin must be valid JSON");
    let pin_v2: TestSuitesPin =
        serde_json::from_slice(PIN_V2).expect("H1 schema-2 test suite pin must be valid JSON");
    let pin_v3: ProjectedTestSuitesPin =
        serde_json::from_slice(PIN_V3).expect("H1 schema-3 test suite pin must be valid JSON");
    let fourslash: FourSlashProjectionManifest =
        serde_json::from_slice(FOURSLASH_PROJECTION_MANIFEST)
            .expect("FourSlash emit projection manifest must be valid JSON");
    assert_eq!(legacy.schema, 1, "unsupported legacy test suite pin schema");
    assert_eq!(pin_v2.schema, 2, "unsupported H1 schema-2 pin");
    assert_eq!(pin_v3.schema, 3, "unsupported H1 schema-3 pin");
    assert_eq!(pin_v2.base_pin.path, BASE_PIN_RELATIVE_PATH);
    assert_eq!(pin_v2.base_pin.sha256, BASE_PIN_SHA256);
    assert_eq!(pin_v3.base_pin.path, PIN_V2_RELATIVE_PATH);
    assert_eq!(pin_v3.base_pin.sha256, PIN_V2_SHA256);
    assert_eq!(pin_v2.typescript_version, TYPESCRIPT_VERSION);
    assert_eq!(pin_v2.source_repository, SOURCE_REPOSITORY);
    assert_eq!(pin_v2.source_commit, SOURCE_COMMIT);
    assert_eq!(pin_v3.typescript_version, pin_v2.typescript_version);
    assert_eq!(pin_v3.source_repository, pin_v2.source_repository);
    assert_eq!(pin_v3.source_commit, pin_v2.source_commit);
    assert_eq!(legacy.typescript_version, pin_v2.typescript_version);
    assert_eq!(legacy.source_repository, pin_v2.source_repository);
    assert_eq!(legacy.source_commit, pin_v2.source_commit);
    assert_eq!(
        legacy.suites.len(),
        SUITES.len() - 1,
        "schema 1 must remain the exact three-suite base"
    );
    assert_eq!(
        pin_v2.suites.len(),
        SUITES.len(),
        "the pin must contain all and only compiler/project/projects/transpile"
    );
    assert_eq!(
        &pin_v2.suites[..legacy.suites.len()],
        legacy.suites.as_slice(),
        "schema 2 must preserve the complete schema-1 suite prefix"
    );
    assert_eq!(
        pin_v3.suites, pin_v2.suites,
        "schema 3 must preserve every schema-2 full-suite identity"
    );
    assert_eq!(
        pin_v2.implementation_sources.len(),
        IMPLEMENTATION_SOURCES_V2.len()
    );
    assert_eq!(
        pin_v3.implementation_sources.len(),
        IMPLEMENTATION_SOURCES_V3.len()
    );
    assert_eq!(
        &pin_v3.implementation_sources[..pin_v2.implementation_sources.len()],
        pin_v2.implementation_sources.as_slice(),
        "schema 3 must preserve the schema-2 implementation-source prefix"
    );
    for (source, (path, git_blob_sha1)) in pin_v3
        .implementation_sources
        .iter()
        .zip(IMPLEMENTATION_SOURCES_V3)
    {
        assert_eq!(source.source_path, path);
        assert_eq!(source.git_blob_sha1, git_blob_sha1);
        assert_hex(&source.git_blob_sha1, 40, "implementation Git blob SHA-1");
    }

    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("harness crate must be inside the workspace");

    for (suite, (name, source_path, vendored_path)) in pin_v2.suites.iter().zip(SUITES) {
        assert_eq!(suite.name, name);
        assert_eq!(suite.source_path, source_path);
        assert_eq!(suite.vendored_path, vendored_path);
        assert_hex(&suite.git_tree_sha1, 40, "Git tree SHA-1");
        assert_hex(&suite.blob_inventory_sha256, 64, "blob inventory SHA-256");

        let suite_root = workspace.join(&suite.vendored_path);
        let stats = collect_suite_stats(&suite_root);
        assert_eq!(stats.files, suite.files, "{} file count", suite.name);
        assert_eq!(stats.bytes, suite.bytes, "{} byte count", suite.name);

        let expected_executable_paths = suite
            .executable_paths
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        assert_eq!(
            expected_executable_paths.len(),
            suite.executable_paths.len(),
            "{} executable paths must be unique",
            suite.name
        );
        if executable_modes_are_available() {
            assert_eq!(
                stats.executable_paths, expected_executable_paths,
                "{} executable modes",
                suite.name
            );
        }

        verify_git_identities(&suite_root, suite, &expected_executable_paths);
    }

    assert_eq!(
        pin_v3.projections.len(),
        1,
        "exactly one projection is pinned"
    );
    verify_fourslash_projection(workspace, &pin_v3.projections[0], &fourslash);
}

fn verify_fourslash_projection(
    workspace: &Path,
    pin: &ProjectionPin,
    manifest: &FourSlashProjectionManifest,
) {
    assert_eq!(pin.name, "fourslash-batch-emit");
    assert_eq!(pin.source_path, "tests/cases/fourslash");
    assert_eq!(pin.vendored_path, "ts-tests/tests/cases/fourslash");
    assert_eq!(
        pin.manifest.path,
        FOURSLASH_PROJECTION_MANIFEST_RELATIVE_PATH
    );
    assert_eq!(pin.manifest.sha256, FOURSLASH_PROJECTION_MANIFEST_SHA256);
    assert_eq!(pin.execution_state, "not-run");
    assert_eq!(pin.expansion_rows, 0);
    assert!(pin.projected_executable_paths.is_empty());
    assert_hex(
        &pin.source_git_tree_sha1,
        40,
        "FourSlash source Git tree SHA-1",
    );
    assert_hex(
        &pin.source_blob_inventory_sha256,
        64,
        "FourSlash source inventory SHA-256",
    );
    assert_hex(
        &pin.projected_git_tree_sha1,
        40,
        "FourSlash projection Git tree SHA-1",
    );
    assert_hex(
        &pin.projected_blob_inventory_sha256,
        64,
        "FourSlash projection inventory SHA-256",
    );

    assert_eq!(manifest.schema, 1);
    assert_eq!(manifest.kind, "typescript-fourslash-batch-emit-projection");
    assert_eq!(manifest.status, "inventory-only-not-run");
    assert_eq!(manifest.typescript_version, TYPESCRIPT_VERSION);
    assert_eq!(manifest.source_repository, SOURCE_REPOSITORY);
    assert_eq!(manifest.source_commit, SOURCE_COMMIT);
    assert_eq!(
        manifest.generator.path,
        FOURSLASH_PROJECTION_GENERATOR_RELATIVE_PATH
    );
    assert_eq!(
        manifest.generator.sha256,
        FOURSLASH_PROJECTION_GENERATOR_SHA256
    );
    assert_eq!(
        format!(
            "{:x}",
            Sha256::digest(
                fs::read(workspace.join(&manifest.generator.path))
                    .expect("FourSlash projection generator must exist")
            )
        ),
        FOURSLASH_PROJECTION_GENERATOR_SHA256,
        "FourSlash projection generator hash"
    );

    assert_eq!(manifest.source_tree.path, pin.source_path);
    assert_eq!(manifest.source_tree.git_tree_sha1, pin.source_git_tree_sha1);
    assert_eq!(
        manifest.source_tree.blob_inventory_sha256,
        pin.source_blob_inventory_sha256
    );
    assert_eq!(manifest.source_tree.files, pin.source_files);
    assert_eq!(manifest.source_tree.bytes, pin.source_bytes);
    assert_eq!(manifest.source_tree.unique_blobs, pin.source_unique_blobs);
    assert!(manifest.source_tree.executable_paths.is_empty());

    assert_eq!(
        manifest.selector.operation_call_pattern,
        r"^[ \t]*verify\.(baselineGetEmitOutput|getEmitOutput|verifyGetEmitOutputForCurrentFile|verifyGetEmitOutputContentsForCurrentFile)[ \t]*\("
    );
    assert_eq!(manifest.selector.operation_call_flags, "gm");
    assert!(manifest
        .selector
        .operation_methods
        .iter()
        .map(String::as_str)
        .eq(FOURSLASH_OPERATION_METHODS));
    assert_eq!(
        manifest.selector.metadata_pattern,
        r"^[ \t]*//[ \t]*@emitThisFile[ \t]*:[ \t]*(true|false)[ \t]*\r?$"
    );
    assert_eq!(manifest.selector.metadata_flags, "gm");
    assert_eq!(manifest.selector.broad_mentions, 40);
    assert_eq!(manifest.selector.selected_operation_files, 38);
    verify_false_positive_controls(&manifest.selector.false_positive_controls);

    assert_eq!(manifest.projection.vendored_path, pin.vendored_path);
    assert_eq!(
        manifest.projection.git_tree_sha1,
        pin.projected_git_tree_sha1
    );
    assert_eq!(
        manifest.projection.blob_inventory_sha256,
        pin.projected_blob_inventory_sha256
    );
    assert_eq!(manifest.projection.files, pin.projected_files);
    assert_eq!(manifest.projection.bytes, pin.projected_bytes);
    assert_eq!(manifest.projection.unique_blobs, pin.projected_unique_blobs);
    assert!(manifest.projection.executable_paths.is_empty());

    let projected_suite = SuitePin {
        name: pin.name.clone(),
        source_path: pin.source_path.clone(),
        vendored_path: pin.vendored_path.clone(),
        git_tree_sha1: pin.projected_git_tree_sha1.clone(),
        blob_inventory_sha256: pin.projected_blob_inventory_sha256.clone(),
        files: pin.projected_files,
        bytes: pin.projected_bytes,
        unique_blobs: pin.projected_unique_blobs,
        executable_paths: pin.projected_executable_paths.clone(),
    };
    let projection_root = workspace.join(&pin.vendored_path);
    let stats = collect_suite_stats(&projection_root);
    assert_eq!(stats.files, pin.projected_files);
    assert_eq!(stats.bytes, pin.projected_bytes);
    if executable_modes_are_available() {
        assert!(stats.executable_paths.is_empty());
    }
    let inventory = verify_git_identities(&projection_root, &projected_suite, &BTreeSet::new());
    verify_fourslash_fixtures(&projection_root, manifest, &inventory);

    let qualification = &manifest.qualification;
    assert_eq!(qualification.expansion_rows, 0);
    assert_eq!(qualification.executed_rows, 0);
    assert_eq!(qualification.passing_rows, 0);
    assert!(qualification.source_inventory_integrity);
    assert!(!qualification.fourslash_pass_rate);
    assert!(!qualification.language_service_compatibility);
    assert!(!qualification.whole_program_emit_equivalence);
}

fn verify_false_positive_controls(controls: &[FalsePositiveControl]) {
    let expected = [
        (
            "fourslash.ts",
            "c53af3e78a24efd8352825f050ecfbbb7f00fbd0",
            44_800,
            &[330, 331, 360, 363][..],
            "interface-declarations-only",
        ),
        (
            "incrementalParsing1.ts",
            "e09e96c9e0d2d5d9a83d940d22c019d22c58cb7b",
            540,
            &[16][..],
            "comment-only",
        ),
    ];
    assert_eq!(controls.len(), expected.len());
    for (control, (path, blob, bytes, lines, disposition)) in controls.iter().zip(expected) {
        assert_eq!(control.path, path);
        assert_eq!(control.git_blob_sha1, blob);
        assert_eq!(control.bytes, bytes);
        assert_eq!(control.mention_lines, lines);
        assert_eq!(control.disposition, disposition);
        assert_hex(&control.git_blob_sha1, 40, "false-positive Git blob SHA-1");
    }
}

fn verify_fourslash_fixtures(
    root: &Path,
    manifest: &FourSlashProjectionManifest,
    inventory: &BTreeMap<String, BlobIdentity>,
) {
    assert_eq!(manifest.fixtures.len(), inventory.len());
    let fixture_paths = manifest
        .fixtures
        .iter()
        .map(|fixture| fixture.path.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(fixture_paths.len(), manifest.fixtures.len());
    assert!(fixture_paths.iter().eq(inventory.keys()));
    assert!(
        manifest
            .fixtures
            .windows(2)
            .all(|pair| pair[0].path < pair[1].path),
        "FourSlash fixtures must be byte-path sorted"
    );

    let mut operation_counts = BTreeMap::<String, u64>::new();
    let mut fixture_bytes = 0_u64;
    let mut fixtures_with_directives = 0_u64;
    let mut directive_count = 0_u64;
    let mut true_count = 0_u64;
    let mut false_count = 0_u64;
    let mut unique_blobs = BTreeSet::new();

    for fixture in &manifest.fixtures {
        let blob = inventory
            .get(&fixture.path)
            .unwrap_or_else(|| panic!("missing projection inventory row {}", fixture.path));
        assert_eq!(blob.mode, "100644");
        assert_eq!(fixture.git_blob_sha1, blob.git_blob_sha1);
        assert_eq!(fixture.bytes, blob.bytes);
        assert_hex(&fixture.git_blob_sha1, 40, "fixture Git blob SHA-1");
        assert!(FOURSLASH_OPERATION_METHODS.contains(&fixture.operation.method.as_str()));

        let raw = fs::read(root.join(&fixture.path))
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", fixture.path));
        assert_eq!(raw.len() as u64, fixture.bytes);
        let text = std::str::from_utf8(&raw)
            .unwrap_or_else(|error| panic!("{} is not UTF-8: {error}", fixture.path));
        let (operations, directives) = fourslash_observations(text);
        assert_eq!(
            operations,
            vec![(fixture.operation.method.clone(), fixture.operation.line)],
            "{} selected operation",
            fixture.path
        );
        assert_eq!(
            directives, fixture.emit_this_file_directives,
            "{} emitThisFile directives",
            fixture.path
        );

        *operation_counts
            .entry(fixture.operation.method.clone())
            .or_default() += 1;
        fixture_bytes += fixture.bytes;
        unique_blobs.insert(fixture.git_blob_sha1.clone());
        if !directives.is_empty() {
            fixtures_with_directives += 1;
        }
        directive_count += directives.len() as u64;
        true_count += directives
            .iter()
            .filter(|directive| directive.value)
            .count() as u64;
        false_count += directives
            .iter()
            .filter(|directive| !directive.value)
            .count() as u64;
    }

    let summary = &manifest.projection.summary;
    assert_eq!(summary.fixture_files, manifest.fixtures.len() as u64);
    assert_eq!(summary.fixture_bytes, fixture_bytes);
    assert_eq!(summary.unique_blobs, unique_blobs.len());
    assert_eq!(summary.operation_calls, manifest.fixtures.len() as u64);
    assert_eq!(summary.operation_counts, operation_counts);
    assert_eq!(
        summary.fixtures_with_emit_this_file,
        fixtures_with_directives
    );
    assert_eq!(summary.emit_this_file_directives, directive_count);
    assert_eq!(summary.emit_this_file_true, true_count);
    assert_eq!(summary.emit_this_file_false, false_count);
    assert_eq!(summary.fixture_files, 38);
    assert_eq!(summary.fixture_bytes, 31_051);
    assert_eq!(summary.unique_blobs, 38);
    assert_eq!(summary.operation_counts["baselineGetEmitOutput"], 31);
    assert_eq!(summary.operation_counts["getEmitOutput"], 5);
    assert_eq!(
        summary.operation_counts["verifyGetEmitOutputContentsForCurrentFile"],
        1
    );
    assert_eq!(
        summary.operation_counts["verifyGetEmitOutputForCurrentFile"],
        1
    );
    assert_eq!(summary.fixtures_with_emit_this_file, 37);
    assert_eq!(summary.emit_this_file_directives, 49);
    assert_eq!(summary.emit_this_file_true, 47);
    assert_eq!(summary.emit_this_file_false, 2);
}

fn fourslash_observations(text: &str) -> (Vec<(String, usize)>, Vec<EmitThisFileDirective>) {
    let mut operations = Vec::new();
    let mut directives = Vec::new();
    for (index, line) in text.split('\n').enumerate() {
        let line_number = index + 1;
        let trimmed_start = line.trim_start();
        for method in FOURSLASH_OPERATION_METHODS {
            let prefix = format!("verify.{method}");
            if trimmed_start
                .strip_prefix(&prefix)
                .is_some_and(|rest| rest.trim_start().starts_with('('))
            {
                operations.push((method.to_owned(), line_number));
            }
        }

        let Some(metadata) = trimmed_start.strip_prefix("//") else {
            continue;
        };
        let Some(value) = metadata
            .trim_start()
            .strip_prefix("@emitThisFile")
            .and_then(|rest| rest.trim_start().strip_prefix(':'))
            .map(str::trim)
        else {
            continue;
        };
        let value = match value {
            "true" => true,
            "false" => false,
            _ => continue,
        };
        directives.push(EmitThisFileDirective {
            line: line_number,
            value,
        });
    }
    (operations, directives)
}

fn collect_suite_stats(root: &Path) -> SuiteStats {
    assert!(root.is_dir(), "missing vendored suite {}", root.display());
    let mut stats = SuiteStats::default();
    visit_directory(root, root, &mut stats);
    stats
}

fn visit_directory(root: &Path, directory: &Path, stats: &mut SuiteStats) {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()))
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|error| panic!("failed to enumerate {}: {error}", directory.display()));
    assert!(
        !entries.is_empty(),
        "vendored Git trees must not contain empty directory {}",
        directory.display()
    );

    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .unwrap_or_else(|error| panic!("failed to inspect {}: {error}", path.display()));
        assert!(
            !file_type.is_symlink(),
            "vendored suite contains an unexpected symlink {}",
            path.display()
        );
        if file_type.is_dir() {
            visit_directory(root, &path, stats);
            continue;
        }
        assert!(
            file_type.is_file(),
            "vendored suite contains an unsupported entry {}",
            path.display()
        );

        let metadata = entry
            .metadata()
            .unwrap_or_else(|error| panic!("failed to stat {}: {error}", path.display()));
        stats.files += 1;
        stats.bytes += metadata.len();
        if is_executable(&metadata) {
            stats.executable_paths.insert(relative_path(root, &path));
        }
    }
}

fn verify_git_identities(
    suite_root: &Path,
    suite: &SuitePin,
    expected_executable_paths: &BTreeSet<String>,
) -> BTreeMap<String, BlobIdentity> {
    let scratch = ScratchDir::new(&suite.name);
    let git_dir = scratch.root.join("objects.git");

    let mut init = Command::new("git");
    scratch.isolate_git(&mut init);
    init.args([
        "init",
        "--bare",
        "--quiet",
        "--object-format=sha1",
        "--template=",
    ])
    .arg(&git_dir);
    successful_output(&mut init, "initialize temporary Git object database");

    let mut add = suite_git_command(&scratch, &git_dir, suite_root);
    add.args([
        "-c",
        "core.autocrlf=false",
        "-c",
        "core.filemode=true",
        "add",
        "--force",
        "--all",
        "--",
        ".",
    ]);
    successful_output(&mut add, "hash every vendored suite file");

    if !expected_executable_paths.is_empty() {
        let mut chmod = suite_git_command(&scratch, &git_dir, suite_root);
        chmod
            .args(["update-index", "--chmod=+x", "--"])
            .args(expected_executable_paths);
        successful_output(&mut chmod, "apply pinned executable modes");
    }

    let mut write_tree = suite_git_command(&scratch, &git_dir, suite_root);
    write_tree.arg("write-tree");
    let tree_output = successful_output(&mut write_tree, "write reconstructed Git tree");
    let tree = String::from_utf8(tree_output.stdout)
        .expect("git write-tree output must be UTF-8")
        .trim()
        .to_owned();
    assert_eq!(tree, suite.git_tree_sha1, "{} Git tree", suite.name);

    let mut ls_tree = suite_git_command(&scratch, &git_dir, suite_root);
    ls_tree.args([
        "ls-tree",
        "-r",
        "-z",
        "--format=%(objectmode) %(objecttype) %(objectname) %(objectsize)%x09%(path)",
        &tree,
    ]);
    let inventory = successful_output(&mut ls_tree, "read reconstructed blob inventory").stdout;
    assert_eq!(
        format!("{:x}", Sha256::digest(&inventory)),
        suite.blob_inventory_sha256,
        "{} blob inventory digest",
        suite.name
    );

    let (files, bytes, unique_blobs, executable_paths, blobs_by_path) =
        parse_blob_inventory(&inventory);
    assert_eq!(files, suite.files, "{} Git blob count", suite.name);
    assert_eq!(bytes, suite.bytes, "{} Git blob bytes", suite.name);
    assert_eq!(
        unique_blobs, suite.unique_blobs,
        "{} unique Git blobs",
        suite.name
    );
    assert_eq!(
        executable_paths, *expected_executable_paths,
        "{} pinned Git executable modes",
        suite.name
    );
    blobs_by_path
}

fn parse_blob_inventory(
    bytes: &[u8],
) -> (
    u64,
    u64,
    usize,
    BTreeSet<String>,
    BTreeMap<String, BlobIdentity>,
) {
    assert_eq!(
        bytes.last(),
        Some(&0),
        "Git inventory must be NUL-terminated"
    );
    let mut files = 0_u64;
    let mut total_bytes = 0_u64;
    let mut blobs = BTreeSet::new();
    let mut executable_paths = BTreeSet::new();
    let mut blobs_by_path = BTreeMap::new();

    for record in bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let record = std::str::from_utf8(record).expect("Git inventory paths must be UTF-8");
        let (metadata, path) = record
            .split_once('\t')
            .expect("Git inventory record must contain a path");
        let mut fields = metadata.split(' ');
        let mode = fields.next().expect("Git inventory mode");
        assert_eq!(fields.next(), Some("blob"), "Git inventory object type");
        let blob = fields.next().expect("Git inventory blob ID");
        let size = fields
            .next()
            .expect("Git inventory blob size")
            .parse::<u64>()
            .expect("Git inventory blob size must be an integer");
        assert_eq!(fields.next(), None, "unexpected Git inventory metadata");
        assert_hex(blob, 40, "Git blob SHA-1");
        assert!(!path.is_empty(), "Git inventory path must not be empty");
        blobs.insert(blob.to_owned());
        if mode == "100755" {
            executable_paths.insert(path.to_owned());
        } else {
            assert_eq!(mode, "100644", "unsupported Git blob mode for {path}");
        }
        assert!(
            blobs_by_path
                .insert(
                    path.to_owned(),
                    BlobIdentity {
                        mode: mode.to_owned(),
                        git_blob_sha1: blob.to_owned(),
                        bytes: size,
                    },
                )
                .is_none(),
            "duplicate Git inventory path {path}"
        );
        files += 1;
        total_bytes += size;
    }

    (
        files,
        total_bytes,
        blobs.len(),
        executable_paths,
        blobs_by_path,
    )
}

fn suite_git_command(scratch: &ScratchDir, git_dir: &Path, work_tree: &Path) -> Command {
    let mut command = Command::new("git");
    scratch.isolate_git(&mut command);
    command
        .env("GIT_DIR", git_dir)
        .env("GIT_WORK_TREE", work_tree)
        .current_dir(work_tree);
    command
}

fn successful_output(command: &mut Command, description: &str) -> Output {
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("failed to {description}: {error}"));
    assert!(
        output.status.success(),
        "failed to {description}: status={} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .expect("suite entry must be below its root")
        .to_str()
        .expect("vendored suite paths must be UTF-8")
        .replace('\\', "/")
}

fn assert_hex(value: &str, length: usize, description: &str) {
    assert_eq!(value.len(), length, "{description} length");
    assert!(
        value.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "{description} must be hexadecimal"
    );
}

const fn executable_modes_are_available() -> bool {
    cfg!(unix)
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;

    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_metadata: &fs::Metadata) -> bool {
    false
}

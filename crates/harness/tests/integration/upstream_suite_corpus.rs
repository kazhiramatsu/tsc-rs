use std::collections::BTreeSet;
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
const PIN: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/typescript-6.0.3/test-suites-pin.v2.json"
));
const BASE_PIN_RELATIVE_PATH: &str = "vendor/typescript-6.0.3/test-suites-pin.v1.json";
const BASE_PIN_SHA256: &str = "f231d984c31d5d16a6fb845e66a25bc9601ffd23212d548cb337149e40397da9";
const PIN_SHA256: &str = "83f8edbb6f4535a19e61cf872532a46722f8cedbd2d746a0922dc507addc0879";
const TYPESCRIPT_VERSION: &str = "6.0.3";
const SOURCE_REPOSITORY: &str = "https://github.com/microsoft/TypeScript.git";
const SOURCE_COMMIT: &str = "050880ce59e30b356b686bd3144efe24f875ebc8";
const IMPLEMENTATION_SOURCES: [(&str, &str); 1] = [(
    "src/testRunner/transpileRunner.ts",
    "3926aa9b7d88e953163ed1fee843d273783be131",
)];
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
struct LegacyTestSuitesPin {
    schema: u32,
    typescript_version: String,
    source_repository: String,
    source_commit: String,
    suites: Vec<SuitePin>,
}

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImplementationSourcePin {
    source_path: String,
    git_blob_sha1: String,
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
        format!("{:x}", Sha256::digest(PIN)),
        PIN_SHA256,
        "H1 test suite pin hash"
    );
    let legacy: LegacyTestSuitesPin =
        serde_json::from_slice(BASE_PIN).expect("legacy test suite pin must be valid JSON");
    let pin: TestSuitesPin =
        serde_json::from_slice(PIN).expect("H1 test suite pin must be valid JSON");
    assert_eq!(legacy.schema, 1, "unsupported legacy test suite pin schema");
    assert_eq!(pin.schema, 2, "unsupported H1 test suite pin schema");
    assert_eq!(pin.base_pin.path, BASE_PIN_RELATIVE_PATH);
    assert_eq!(pin.base_pin.sha256, BASE_PIN_SHA256);
    assert_eq!(pin.typescript_version, TYPESCRIPT_VERSION);
    assert_eq!(pin.source_repository, SOURCE_REPOSITORY);
    assert_eq!(pin.source_commit, SOURCE_COMMIT);
    assert_eq!(legacy.typescript_version, pin.typescript_version);
    assert_eq!(legacy.source_repository, pin.source_repository);
    assert_eq!(legacy.source_commit, pin.source_commit);
    assert_eq!(
        legacy.suites.len(),
        SUITES.len() - 1,
        "schema 1 must remain the exact three-suite base"
    );
    assert_eq!(
        pin.suites.len(),
        SUITES.len(),
        "the pin must contain all and only compiler/project/projects/transpile"
    );
    assert_eq!(
        &pin.suites[..legacy.suites.len()],
        legacy.suites.as_slice(),
        "schema 2 must preserve the complete schema-1 suite prefix"
    );
    assert_eq!(
        pin.implementation_sources.len(),
        IMPLEMENTATION_SOURCES.len()
    );
    for (source, (path, git_blob_sha1)) in pin
        .implementation_sources
        .iter()
        .zip(IMPLEMENTATION_SOURCES)
    {
        assert_eq!(source.source_path, path);
        assert_eq!(source.git_blob_sha1, git_blob_sha1);
        assert_hex(&source.git_blob_sha1, 40, "implementation Git blob SHA-1");
    }

    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("harness crate must be inside the workspace");

    for (suite, (name, source_path, vendored_path)) in pin.suites.iter().zip(SUITES) {
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
) {
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

    let (files, bytes, unique_blobs, executable_paths) = parse_blob_inventory(&inventory);
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
}

fn parse_blob_inventory(bytes: &[u8]) -> (u64, u64, usize, BTreeSet<String>) {
    assert_eq!(
        bytes.last(),
        Some(&0),
        "Git inventory must be NUL-terminated"
    );
    let mut files = 0_u64;
    let mut total_bytes = 0_u64;
    let mut blobs = BTreeSet::new();
    let mut executable_paths = BTreeSet::new();

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
        files += 1;
        total_bytes += size;
    }

    (files, total_bytes, blobs.len(), executable_paths)
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

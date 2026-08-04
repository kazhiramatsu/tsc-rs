use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tsc_host::{CompilerHost, HostError, MemoryCompilerHost};
use tsc_program::{
    parse_config_root_plan, ConfigHostError, ConfigParseHost, ConfigRootPlanRequest,
    HostResolvedModule, ModuleResolver, ResolutionMode, ResolutionOutcome,
};

const ARTIFACT_PATH: &str = "vendor/typescript-6.0.3/compiler-module-suffixes.v1.json";
const MANIFEST_PATH: &str = "vendor/typescript-6.0.3/test-suite-expansion.v1.json";
const TYPESCRIPT_BUNDLE_PATH: &str = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_VERSION: &str = "6.0.3";
const TYPESCRIPT_SOURCE_COMMIT: &str = "050880ce59e30b356b686bd3144efe24f875ebc8";
const NODE_VERSION: &str = "25.2.1";
const MANIFEST_SHA256: &str = "9c6e991103b571f7a8800dc5e1ef66088017689f3769de8fcdb408b2dc125188";
const TYPESCRIPT_BUNDLE_SHA256: &str =
    "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39";

#[derive(Default)]
struct OracleConfigHost {
    files: BTreeMap<String, String>,
}

impl OracleConfigHost {
    fn insert(&mut self, path: &str, text: &str) {
        assert!(
            self.files
                .insert(path.to_ascii_lowercase(), text.to_owned())
                .is_none(),
            "duplicate case-insensitive oracle unit {path:?}"
        );
    }
}

impl ConfigParseHost for OracleConfigHost {
    fn use_case_sensitive_file_names(&self) -> bool {
        false
    }

    fn file_exists(&self, path: &str) -> Result<bool, ConfigHostError> {
        Ok(self.files.contains_key(&path.to_ascii_lowercase()))
    }

    fn read_file(&self, path: &str) -> Result<Option<String>, ConfigHostError> {
        Ok(self.files.get(&path.to_ascii_lowercase()).cloned())
    }

    fn read_directory(
        &self,
        _directory: &str,
        extensions: &[&str],
        _excludes: Option<&[String]>,
        _includes: Option<&[String]>,
        _depth: Option<usize>,
    ) -> Result<Vec<String>, ConfigHostError> {
        Ok(self
            .files
            .keys()
            .filter(|path| extensions.iter().any(|extension| path.ends_with(extension)))
            .cloned()
            .collect())
    }
}

struct RecordingCompilerHost {
    inner: MemoryCompilerHost,
    file_probes: RefCell<Vec<Value>>,
}

impl CompilerHost for RecordingCompilerHost {
    fn current_directory(&self) -> Result<PathBuf, HostError> {
        self.inner.current_directory()
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.inner.use_case_sensitive_file_names()
    }

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError> {
        self.inner.read_file(path)
    }

    fn file_exists(&self, path: &Path) -> Result<bool, HostError> {
        let result = self.inner.file_exists(path)?;
        self.file_probes.borrow_mut().push(json!({
            "path": path.to_string_lossy(),
            "result": result,
        }));
        Ok(result)
    }

    fn directory_exists(&self, path: &Path) -> Result<bool, HostError> {
        self.inner.directory_exists(path)
    }

    fn read_directory(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        self.inner.read_directory(path)
    }

    fn realpath(&self, path: &Path) -> Result<Option<PathBuf>, HostError> {
        self.inner.realpath(path)
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn array<'a>(value: &'a Value, fixture_id: &str, field: &str) -> &'a [Value] {
    value
        .as_array()
        .unwrap_or_else(|| panic!("{fixture_id}: {field} is an array"))
}

fn string<'a>(value: &'a Value, fixture_id: &str, field: &str) -> &'a str {
    value
        .as_str()
        .unwrap_or_else(|| panic!("{fixture_id}: {field} is a string"))
}

fn u64_value(value: &Value, fixture_id: &str, field: &str) -> u64 {
    value
        .as_u64()
        .unwrap_or_else(|| panic!("{fixture_id}: {field} is an unsigned integer"))
}

fn resolution_record(outcome: ResolutionOutcome<HostResolvedModule>) -> Value {
    let ResolutionOutcome::Resolved(resolved) = outcome else {
        return json!({ "state": "not_found" });
    };
    let package_id = resolved.package_id().map(|package_id| {
        json!({
            "name": package_id.name(),
            "sub_module_name": package_id.submodule_name(),
            "version": package_id.version(),
            "peer_dependencies": package_id.peer_dependencies(),
        })
    });
    json!({
        "state": "resolved",
        "resolved_file_name": resolved.resolved_file().display().to_string_lossy(),
        "original_path": resolved
            .original_path()
            .map(|path| path.display().to_string_lossy().into_owned()),
        "extension": resolved.extension().as_str(),
        "is_external_library_import": resolved.is_external_library_import(),
        "package_id": package_id,
    })
}

#[test]
fn official_module_suffix_fixtures_match_the_frozen_typescript_oracle() {
    let workspace = workspace_root();
    let artifact_bytes = fs::read(workspace.join(ARTIFACT_PATH)).expect("read suffix oracle");
    let artifact: Value =
        serde_json::from_slice(&artifact_bytes).expect("suffix oracle is valid JSON");
    assert_eq!(artifact["schema"], 1);
    assert_eq!(artifact["typescript_version"], TYPESCRIPT_VERSION);
    assert_eq!(artifact["source_commit"], TYPESCRIPT_SOURCE_COMMIT);
    assert_eq!(artifact["node_version"], NODE_VERSION);
    assert_eq!(artifact["producer"]["path"], TYPESCRIPT_BUNDLE_PATH);
    assert_eq!(artifact["producer"]["sha256"], TYPESCRIPT_BUNDLE_SHA256);
    assert_eq!(artifact["manifest"]["path"], MANIFEST_PATH);
    assert_eq!(artifact["manifest"]["sha256"], MANIFEST_SHA256);
    assert_eq!(
        artifact["fixture_range"],
        json!({ "first": 4293, "last": 4308 })
    );
    assert_eq!(
        artifact["summary"],
        json!({
            "fixture_total": 16,
            "request_total": 18,
            "resolved_total": 16,
            "unresolved_total": 2,
            "file_probe_total": 78,
            "failed_lookup_total": 95,
        })
    );

    for (relative, expected_hash) in [
        (MANIFEST_PATH, MANIFEST_SHA256),
        (TYPESCRIPT_BUNDLE_PATH, TYPESCRIPT_BUNDLE_SHA256),
    ] {
        let bytes = fs::read(workspace.join(relative))
            .unwrap_or_else(|error| panic!("read pinned oracle input {relative}: {error}"));
        assert_eq!(sha256(&bytes), expected_hash, "pinned input {relative}");
    }
    let manifest_bytes = fs::read(workspace.join(MANIFEST_PATH)).expect("read expansion manifest");
    let manifest: Value =
        serde_json::from_slice(&manifest_bytes).expect("expansion manifest is JSON");
    let manifest_cases = array(&manifest["cases"], "manifest", "cases");
    let manifest_sources = array(&manifest["sources"], "manifest", "sources");
    let manifest_fixtures = array(
        &manifest["compiler_fixtures"],
        "manifest",
        "compiler_fixtures",
    );

    let fixtures = array(&artifact["fixtures"], "oracle", "fixtures");
    assert_eq!(fixtures.len(), 16);
    let mut seen_cases = BTreeSet::new();
    for (offset, fixture) in fixtures.iter().enumerate() {
        let fixture_id = string(&fixture["case_id"], "oracle", "case_id");
        assert!(
            seen_cases.insert(fixture_id),
            "duplicate oracle case {fixture_id}"
        );
        let fixture_index = u64_value(&fixture["fixture_index"], fixture_id, "fixture_index");
        assert_eq!(fixture_index, 4293 + offset as u64, "{fixture_id}");
        assert_eq!(fixture["source"]["index"], fixture_index, "{fixture_id}");
        let source_index = usize::try_from(fixture_index).expect("fixture index fits usize");
        let manifest_source = manifest_sources
            .get(source_index)
            .unwrap_or_else(|| panic!("{fixture_id}: source inventory row is present"));
        assert_eq!(manifest_source["suite"], "compiler", "{fixture_id}");
        for field in ["path", "bytes", "sha256", "git_blob_sha1"] {
            assert_eq!(
                fixture["source"][field], manifest_source[field],
                "{fixture_id}: source {field} matches the manifest inventory"
            );
        }
        let manifest_fixture = manifest_fixtures
            .get(source_index)
            .unwrap_or_else(|| panic!("{fixture_id}: compiler fixture row is present"));
        assert_eq!(manifest_fixture["source"], fixture_index, "{fixture_id}");
        assert_eq!(
            fixture["source"]["decoded_sha256"], manifest_fixture["decoded_sha256"],
            "{fixture_id}: decoded source identity matches the compiler fixture row"
        );

        let manifest_case = manifest_cases
            .iter()
            .find(|case| case["id"] == fixture_id)
            .unwrap_or_else(|| panic!("{fixture_id}: absent from expansion manifest"));
        assert_eq!(manifest_case["source"], fixture_index, "{fixture_id}");
        assert_eq!(
            manifest_case["initial_execution_state"], "not-run",
            "{fixture_id}: resolver evidence must not claim compiler-suite execution"
        );

        let source_relative = string(&fixture["source"]["path"], fixture_id, "source path");
        let source_bytes = fs::read(
            workspace
                .join("ts-tests/tests/cases/compiler")
                .join(source_relative),
        )
        .unwrap_or_else(|error| panic!("{fixture_id}: read pinned fixture: {error}"));
        assert_eq!(
            source_bytes.len() as u64,
            u64_value(&fixture["source"]["bytes"], fixture_id, "source bytes"),
            "{fixture_id}"
        );
        assert_eq!(
            sha256(&source_bytes),
            string(&fixture["source"]["sha256"], fixture_id, "source sha256"),
            "{fixture_id}"
        );
        assert_eq!(
            string(
                &fixture["source"]["git_blob_sha1"],
                fixture_id,
                "source git blob",
            )
            .len(),
            40,
            "{fixture_id}: source blob identity is retained"
        );

        let config = &fixture["config_unit"];
        let config_name = string(&config["name"], fixture_id, "config name");
        let config_text = string(&config["text"], fixture_id, "config text");
        assert_eq!(
            config_text.len() as u64,
            u64_value(&config["utf8_bytes"], fixture_id, "config bytes"),
            "{fixture_id}"
        );
        assert_eq!(
            sha256(config_text.as_bytes()),
            string(&config["sha256"], fixture_id, "config sha256"),
            "{fixture_id}"
        );

        let units = array(&fixture["units"], fixture_id, "units");
        let mut config_host = OracleConfigHost::default();
        config_host.insert(config_name, config_text);
        let mut compiler_builder = MemoryCompilerHost::builder("/").case_sensitive(false);
        for unit in units {
            let name = string(&unit["name"], fixture_id, "unit name");
            let text = string(&unit["text"], fixture_id, "unit text");
            assert_eq!(
                text.len() as u64,
                u64_value(&unit["utf8_bytes"], fixture_id, "unit bytes"),
                "{fixture_id}: {name}"
            );
            assert_eq!(
                sha256(text.as_bytes()),
                string(&unit["sha256"], fixture_id, "unit sha256"),
                "{fixture_id}: {name}"
            );
            config_host.insert(name, text);
            compiler_builder = compiler_builder.file(name, text.as_bytes().to_vec());
        }
        let compiler_host = compiler_builder
            .build()
            .unwrap_or_else(|error| panic!("{fixture_id}: build compiler host: {error}"));
        let root_plan = parse_config_root_plan(
            &config_host,
            ConfigRootPlanRequest {
                file_name: config_name.to_owned(),
                text: config_text.to_owned(),
                base_path: "/".to_owned(),
            },
        )
        .unwrap_or_else(|error| panic!("{fixture_id}: parse config root plan: {error}"));
        assert!(
            root_plan.errors().is_empty(),
            "{fixture_id}: official config must parse without diagnostics: {:?}",
            root_plan.errors()
        );
        let projected = root_plan.module_resolution_options();

        for request in array(&fixture["requests"], fixture_id, "requests") {
            let containing_file = string(
                &request["containing_file"],
                fixture_id,
                "request containing file",
            );
            let specifier = string(&request["specifier"], fixture_id, "request specifier");
            let host = RecordingCompilerHost {
                inner: compiler_host.clone(),
                file_probes: RefCell::new(Vec::new()),
            };
            let actual = ModuleResolver::new_with_program_options(
                &host,
                projected.compiler_options(),
                projected.program_options(),
            )
            .unwrap_or_else(|error| panic!("{fixture_id}: construct resolver: {error}"))
            .resolve(
                Path::new(containing_file),
                specifier,
                ResolutionMode::Unspecified,
            )
            .unwrap_or_else(|error| {
                panic!("{fixture_id}: resolve {specifier:?} from {containing_file}: {error}")
            });
            assert_eq!(
                resolution_record(actual),
                request["resolution"],
                "{fixture_id}: resolution of {specifier:?} from {containing_file}"
            );
            assert_eq!(
                host.file_probes.into_inner(),
                array(
                    request.get("file_probes").unwrap(),
                    fixture_id,
                    "file probes"
                ),
                "{fixture_id}: file-probe order for {specifier:?} from {containing_file}"
            );
        }
    }
    assert_eq!(seen_cases.len(), 16);
}

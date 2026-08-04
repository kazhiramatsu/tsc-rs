use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use serde_json::{json, Value};
use tsc_diagnostics::{Diagnostic, DiagnosticCategory};
use tsc_program::{
    parse_config_root_plan, ConfigHostError, ConfigHostOperation, ConfigOptionValueState,
    ConfigParseHost, ConfigRootPlanRequest,
};

const ARTIFACT_PATH: &str = "vendor/typescript-6.0.3/compiler-config-diagnostics.v1.json";
const TYPESCRIPT_VERSION: &str = "6.0.3";
const TYPESCRIPT_SOURCE_COMMIT: &str = "050880ce59e30b356b686bd3144efe24f875ebc8";
const NODE_VERSION: &str = "25.2.1";
const TYPESCRIPT_BUNDLE_SHA256: &str =
    "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39";
const TYPESCRIPT_SOURCE_SHA256: &str =
    "1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3";

#[derive(Clone, Debug)]
enum ReadOutcome {
    Text(String),
    Error(String),
}

#[derive(Clone, Debug)]
struct HostFile {
    exists: bool,
    read: ReadOutcome,
}

struct OracleConfigHost {
    case_sensitive: bool,
    files: BTreeMap<String, HostFile>,
    read_directory_result: Vec<String>,
    log: RefCell<Vec<Value>>,
}

impl OracleConfigHost {
    fn from_input(input: &Value, fixture_id: &str) -> Self {
        let case_sensitive = input["use_case_sensitive_file_names"]
            .as_bool()
            .unwrap_or_else(|| panic!("{fixture_id}: case-sensitivity input is a boolean"));
        let mut files = BTreeMap::new();
        for file in array(&input["host_files"], fixture_id, "host_files") {
            let path = string(&file["path"], fixture_id, "host file path").to_owned();
            let exists = file["file_exists"]
                .as_bool()
                .unwrap_or_else(|| panic!("{fixture_id}: host file existence is a boolean"));
            let state = string(&file["read"]["state"], fixture_id, "host read state");
            let read = match state {
                "text" => ReadOutcome::Text(
                    string(
                        &file["read"]["source"]["text"],
                        fixture_id,
                        "host source text",
                    )
                    .to_owned(),
                ),
                "error" => ReadOutcome::Error(
                    string(
                        &file["read"]["detail"],
                        fixture_id,
                        "host read error detail",
                    )
                    .to_owned(),
                ),
                other => panic!("{fixture_id}: unsupported host read state {other:?}"),
            };
            assert!(
                files
                    .insert(path.clone(), HostFile { exists, read })
                    .is_none(),
                "{fixture_id}: duplicate host file {path:?}"
            );
        }
        Self {
            case_sensitive,
            files,
            read_directory_result: string_array(
                &input["read_directory_result"],
                fixture_id,
                "read_directory_result",
            ),
            log: RefCell::new(Vec::new()),
        }
    }

    fn file(&self, path: &str) -> Option<&HostFile> {
        if self.case_sensitive {
            self.files.get(path)
        } else {
            self.files
                .iter()
                .find_map(|(name, file)| name.eq_ignore_ascii_case(path).then_some(file))
        }
    }
}

impl ConfigParseHost for OracleConfigHost {
    fn use_case_sensitive_file_names(&self) -> bool {
        self.case_sensitive
    }

    fn file_exists(&self, path: &str) -> Result<bool, ConfigHostError> {
        let result = self.file(path).is_some_and(|file| file.exists);
        self.log.borrow_mut().push(json!({
            "operation": "file_exists",
            "path": path,
            "result": result,
        }));
        Ok(result)
    }

    fn read_file(&self, path: &str) -> Result<Option<String>, ConfigHostError> {
        let Some(file) = self.file(path) else {
            self.log.borrow_mut().push(json!({
                "operation": "read_file",
                "path": path,
                "result": "missing",
            }));
            return Ok(None);
        };
        match &file.read {
            ReadOutcome::Text(text) => {
                self.log.borrow_mut().push(json!({
                    "operation": "read_file",
                    "path": path,
                    "result": "text",
                }));
                Ok(Some(text.clone()))
            }
            ReadOutcome::Error(detail) => {
                self.log.borrow_mut().push(json!({
                    "operation": "read_file",
                    "path": path,
                    "result": "error",
                }));
                Err(ConfigHostError::new(
                    ConfigHostOperation::ReadFile,
                    path,
                    detail,
                ))
            }
        }
    }

    fn read_directory(
        &self,
        directory: &str,
        extensions: &[&str],
        excludes: Option<&[String]>,
        includes: Option<&[String]>,
        depth: Option<usize>,
    ) -> Result<Vec<String>, ConfigHostError> {
        self.log.borrow_mut().push(json!({
            "operation": "read_directory",
            "directory": directory,
            "extensions": extensions,
            "excludes": excludes,
            "includes": includes,
            "depth": depth,
            "result": self.read_directory_result,
        }));
        Ok(self.read_directory_result.clone())
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
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

fn string_array(value: &Value, fixture_id: &str, field: &str) -> Vec<String> {
    array(value, fixture_id, field)
        .iter()
        .map(|entry| string(entry, fixture_id, field).to_owned())
        .collect()
}

fn category_name(category: DiagnosticCategory) -> &'static str {
    match category {
        DiagnosticCategory::Warning => "warning",
        DiagnosticCategory::Error => "error",
        DiagnosticCategory::Suggestion => "suggestion",
        DiagnosticCategory::Message => "message",
    }
}

fn diagnostic_record(diagnostic: &Diagnostic) -> Value {
    json!({
        "code": diagnostic.code(),
        "category": category_name(diagnostic.category()),
        "file": diagnostic.file_name,
        "start": diagnostic.start,
        "length": diagnostic.length,
        "message": diagnostic.message_text(),
    })
}

fn diagnostic_records<'a>(diagnostics: impl IntoIterator<Item = &'a Diagnostic>) -> Value {
    Value::Array(diagnostics.into_iter().map(diagnostic_record).collect())
}

fn json_values_equivalent(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => left.as_f64() == right.as_f64(),
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| json_values_equivalent(left, right))
        }
        (Value::Object(left), Value::Object(right)) => {
            left.len() == right.len()
                && left.iter().all(|(key, left)| {
                    right
                        .get(key)
                        .is_some_and(|right| json_values_equivalent(left, right))
                })
        }
        _ => left == right,
    }
}

fn assert_source_record(source: &Value, fixture_id: &str, label: &str) {
    let text = string(&source["text"], fixture_id, label);
    let expected_bytes = source["utf8_bytes"]
        .as_u64()
        .unwrap_or_else(|| panic!("{fixture_id}: {label} byte count is an integer"));
    assert_eq!(
        u64::try_from(text.len()).expect("usize fits u64"),
        expected_bytes,
        "{fixture_id}: {label} byte count drifted"
    );
    let hash = string(&source["sha256"], fixture_id, label);
    assert_eq!(hash.len(), 64, "{fixture_id}: {label} hash width");
    assert!(
        hash.bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "{fixture_id}: {label} hash is lowercase hexadecimal"
    );
}

fn assert_artifact_metadata(artifact: &Value) {
    assert_eq!(artifact["schema"], 1);
    assert_eq!(artifact["typescript_version"], TYPESCRIPT_VERSION);
    assert_eq!(artifact["source_commit"], TYPESCRIPT_SOURCE_COMMIT);
    assert_eq!(artifact["node_version"], NODE_VERSION);
    assert_eq!(
        artifact["producer"]["path"],
        "vendor/typescript-6.0.3/lib/typescript.js"
    );
    assert_eq!(artifact["producer"]["sha256"], TYPESCRIPT_BUNDLE_SHA256);
    assert_eq!(
        artifact["source_reference"]["path"],
        "vendor/typescript-6.0.3/lib/_tsc.js"
    );
    assert_eq!(
        artifact["source_reference"]["sha256"],
        TYPESCRIPT_SOURCE_SHA256
    );
    assert_eq!(
        artifact["source_reference"]["spans"],
        json!([
            {"symbol": "parseJsonConfigFileContentWorker", "lines": "39004-39171"},
            {"symbol": "parseConfig", "lines": "39272-39330"},
            {"symbol": "getExtendsConfigPathOrArray", "lines": "39342-39378"},
            {"symbol": "getExtendsConfigPath", "lines": "39436-39459"},
            {"symbol": "getExtendedConfig", "lines": "39460-39499"},
            {"symbol": "validateSpecs", "lines": "39697-39710"},
            {"symbol": "specToDiagnostic", "lines": "39711-39718"},
        ])
    );
    assert_eq!(artifact["summary"]["fixture_total"], 39);
    assert_eq!(artifact["summary"]["root_parse_diagnostic_total"], 4);
    assert_eq!(artifact["summary"]["parsed_error_total"], 66);
    assert_eq!(artifact["summary"]["config_diagnostic_total"], 70);
    assert_eq!(artifact["summary"]["located_config_diagnostic_total"], 63);
    assert_eq!(artifact["summary"]["file_name_total"], 36);
    assert_eq!(artifact["summary"]["extended_source_total"], 16);
    assert_eq!(artifact["summary"]["extended_source_text_total"], 14);
    assert_eq!(artifact["summary"]["host_call_total"], 46);
    assert_eq!(artifact["summary"]["host_calls"]["file_exists"], 26);
    assert_eq!(artifact["summary"]["host_calls"]["read_file"], 18);
    assert_eq!(artifact["summary"]["host_calls"]["read_directory"], 2);
}

#[test]
fn config_diagnostic_plans_match_the_frozen_typescript_oracle() {
    let bytes = fs::read(workspace_root().join(ARTIFACT_PATH))
        .expect("read compiler config diagnostic oracle");
    let artifact: Value =
        serde_json::from_slice(&bytes).expect("compiler config diagnostic oracle is valid JSON");
    assert_artifact_metadata(&artifact);

    let fixtures = artifact["fixtures"]
        .as_array()
        .expect("oracle fixtures is an array");
    let expected_ids = [
        "unknown-invalid-options",
        "mixed-extends-two-phase",
        "missing-explicit-and-implicit",
        "read-file-error-continues-sibling",
        "cycle-continues-sibling",
        "root-syntax-partial",
        "extended-syntax-atomic-sibling",
        "mixed-invalid-spec-elements",
        "empty-source-no-input",
        "empty-files",
        "empty-include-no-input",
        "duplicate-compiler-options",
        "duplicate-extends-last-effective",
        "omitted-files-element",
        "misplaced-root-compiler-option",
        "typed-file-option-normalization",
        "prototype-compiler-options-own-undefined-placement",
        "prototype-files-own-undefined-inheritance",
        "missing-command-line-option-value",
        "extended-truthy-scalar-files",
        "extended-null-file-element",
        "unquoted-config-properties",
        "single-quoted-config-properties",
        "identifier-compiler-option-value",
        "array-compiler-options",
        "invalid-module-option",
        "unquoted-keyword-option-name",
        "typed-rooted-path-options",
        "quoted-array-root-cycle-suffix-order",
        "file-url-parent-normalization",
        "file-url-parent-boundary",
        "keyword-compiler-option-value",
        "path-option-normalization-boundaries",
        "array-compiler-option-value",
        "extended-self-cycle-conversion-replay",
        "unknown-option-single-quoted-value",
        "command-line-only-string-value",
        "object-compiler-option-value",
        "file-spec-diagnostic-owner-order",
    ];
    assert_eq!(fixtures.len(), expected_ids.len());
    let mut seen_ids = BTreeSet::new();

    for (fixture, expected_id) in fixtures.iter().zip(expected_ids) {
        let fixture_id = string(&fixture["id"], "artifact", "fixture id");
        assert_eq!(fixture_id, expected_id, "fixture order drifted");
        assert!(
            seen_ids.insert(fixture_id),
            "duplicate fixture {fixture_id}"
        );
        let input = &fixture["input"];
        let root = &input["root"];
        assert_source_record(root, fixture_id, "root source");
        for host_file in array(&input["host_files"], fixture_id, "host_files") {
            if host_file["read"]["state"] == "text" {
                assert_eq!(
                    host_file["path"], host_file["read"]["source"]["file_name"],
                    "{fixture_id}: host source identity drifted"
                );
                assert_source_record(&host_file["read"]["source"], fixture_id, "host source");
            }
        }

        let root_parse = array(
            &fixture["root_parse_diagnostics"],
            fixture_id,
            "root_parse_diagnostics",
        );
        let parsed_errors = array(&fixture["parsed_errors"], fixture_id, "parsed_errors");
        let expected_combined = root_parse
            .iter()
            .chain(parsed_errors)
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            fixture["config_diagnostics"],
            Value::Array(expected_combined),
            "{fixture_id}: diagnostic partition drifted"
        );

        let host = OracleConfigHost::from_input(input, fixture_id);
        let request = ConfigRootPlanRequest {
            file_name: string(&root["file_name"], fixture_id, "root file name").to_owned(),
            text: string(&root["text"], fixture_id, "root text").to_owned(),
            base_path: string(&input["base_path"], fixture_id, "base path").to_owned(),
        };
        let plan = parse_config_root_plan(&host, request)
            .unwrap_or_else(|error| panic!("{fixture_id}: partial config plan failed: {error}"));

        assert!(
            json_values_equivalent(plan.raw(), &fixture["plan"]["raw"]),
            "{fixture_id}: raw config drifted: Rust={:?}, TypeScript={:?}",
            plan.raw(),
            fixture["plan"]["raw"]
        );
        let raw_probes = array(
            &fixture["plan"]["raw_own_property_probes"],
            fixture_id,
            "raw own-property probes",
        );
        assert_eq!(
            string_array(
                &input["raw_own_property_probe_keys"],
                fixture_id,
                "raw own-property probe keys",
            ),
            raw_probes
                .iter()
                .map(|probe| string(&probe["name"], fixture_id, "raw probe name").to_owned())
                .collect::<Vec<_>>(),
            "{fixture_id}: raw own-property probe inputs drifted"
        );
        let raw_object = plan
            .raw()
            .as_object()
            .unwrap_or_else(|| panic!("{fixture_id}: public raw config is an object"));
        for probe in raw_probes {
            let name = string(&probe["name"], fixture_id, "raw probe name");
            match string(&probe["state"], fixture_id, "raw probe state") {
                "absent" | "undefined" => assert!(
                    raw_object.get(name).is_none(),
                    "{fixture_id}: raw property {name:?} must be absent from the JSON projection"
                ),
                "value" => {
                    let actual = raw_object
                        .get(name)
                        .unwrap_or_else(|| panic!("{fixture_id}: raw property {name:?} is absent"));
                    assert!(
                        json_values_equivalent(actual, &probe["value"]),
                        "{fixture_id}: raw property {name:?} value drifted"
                    );
                }
                state => panic!("{fixture_id}: unsupported raw probe state {state:?}"),
            }
        }
        assert_eq!(
            json!(plan.file_names()),
            fixture["plan"]["file_names"],
            "{fixture_id}: file names drifted"
        );
        assert_eq!(
            json!(plan.extended_source_files()),
            fixture["plan"]["extended_source_files"],
            "{fixture_id}: extended source order drifted"
        );
        let expected_extended_sources = array(
            &fixture["plan"]["extended_sources"],
            fixture_id,
            "extended sources",
        );
        for source in expected_extended_sources {
            assert_source_record(source, fixture_id, "extended source");
        }
        assert_eq!(
            plan.extended_sources()
                .iter()
                .map(|source| json!({
                    "file_name": source.file_name,
                    "text": source.text,
                }))
                .collect::<Vec<_>>(),
            expected_extended_sources
                .iter()
                .map(|source| json!({
                    "file_name": source["file_name"],
                    "text": source["text"],
                }))
                .collect::<Vec<_>>(),
            "{fixture_id}: available extended source texts drifted"
        );
        assert_eq!(
            diagnostic_records(plan.root_parse_diagnostics()),
            fixture["root_parse_diagnostics"],
            "{fixture_id}: root parse diagnostics drifted"
        );
        assert_eq!(
            diagnostic_records(plan.errors()),
            fixture["parsed_errors"],
            "{fixture_id}: parsed errors drifted"
        );
        assert_eq!(
            diagnostic_records(plan.diagnostics()),
            fixture["config_diagnostics"],
            "{fixture_id}: compiler-visible diagnostic order drifted"
        );

        let option_probes = array(
            &fixture["plan"]["option_probes"],
            fixture_id,
            "option probes",
        );
        assert_eq!(
            string_array(&input["option_probe_keys"], fixture_id, "option probe keys",),
            option_probes
                .iter()
                .map(|probe| string(&probe["name"], fixture_id, "option probe name").to_owned())
                .collect::<Vec<_>>(),
            "{fixture_id}: option probe inputs drifted"
        );
        for probe in option_probes {
            let name = string(&probe["name"], fixture_id, "option probe name");
            let expected_state = string(&probe["state"], fixture_id, "option probe state");
            match (
                expected_state,
                plan.options().typed_value_state(name),
            ) {
                ("absent", ConfigOptionValueState::Absent)
                | ("undefined", ConfigOptionValueState::Undefined) => {}
                ("value", ConfigOptionValueState::Value(actual)) => assert_eq!(
                    actual, &probe["value"],
                    "{fixture_id}: option {name:?} value drifted"
                ),
                ("absent" | "undefined" | "value", actual) => panic!(
                    "{fixture_id}: option {name:?} state drifted: expected {expected_state:?}, got {actual:?}"
                ),
                (state, _) => {
                    panic!("{fixture_id}: unsupported option probe state {state:?}")
                }
            }
        }

        assert_eq!(
            Value::Array(host.log.into_inner()),
            fixture["host_log"],
            "{fixture_id}: host observation order drifted"
        );
    }
}

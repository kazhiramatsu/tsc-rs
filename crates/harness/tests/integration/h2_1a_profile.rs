use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

const RECORDED: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h2-1a-profile.v1.json"
));
const CONTRACT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../.github/ci/contracts/h2-1a-profile.schema.json"
));
const RUNTIME_INPUTS: [&str; 14] = [
    "crates/compiler/src/cli.rs",
    "crates/compiler/src/lib.rs",
    "crates/compiler/tests/integration/emit_session_contract.rs",
    "crates/emitter/src/activity.rs",
    "crates/emitter/src/builtins.rs",
    "crates/emitter/src/execute.rs",
    "crates/emitter/src/host.rs",
    "crates/emitter/src/printer.rs",
    "crates/emitter/src/transform.rs",
    "crates/emitter/tests/integration/output_plan_contract.rs",
    "crates/harness/src/upstream_suites/execution.rs",
    "crates/xtask/src/h2_1a_acceptance.rs",
    "crates/xtask/src/main.rs",
    "crates/xtask/tests/unit/h2_1a_acceptance/tests.rs",
];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonical workspace")
}

fn sha256(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}

fn object<'a>(value: &'a Value, label: &str) -> &'a Map<String, Value> {
    value
        .as_object()
        .unwrap_or_else(|| panic!("{label} must be an object"))
}

fn array<'a>(value: &'a Value, label: &str) -> &'a [Value] {
    value
        .as_array()
        .unwrap_or_else(|| panic!("{label} must be an array"))
}

fn string<'a>(value: &'a Value, label: &str) -> &'a str {
    value
        .as_str()
        .unwrap_or_else(|| panic!("{label} must be a string"))
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => serde_json::to_string(value).unwrap(),
        Value::Array(values) => format!(
            "[{}]",
            values.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        Value::Object(values) => {
            let mut keys = values.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            format!(
                "{{{}}}",
                keys.into_iter()
                    .map(|key| format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap(),
                        canonical(&values[key])
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
    }
}

fn assert_current_path_hash(workspace: &Path, record: &Value, expected_path: &str) {
    assert_eq!(record["path"], expected_path);
    assert_eq!(
        string(&record["sha256"], "path hash"),
        sha256(fs::read(workspace.join(expected_path)).expect("read content-addressed input"))
    );
}

fn assert_strict_object_schemas(value: &Value, path: &str) {
    match value {
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                assert_strict_object_schemas(value, &format!("{path}[{index}]"));
            }
        }
        Value::Object(values) => {
            if values.get("type").and_then(Value::as_str) == Some("object") {
                assert_eq!(
                    values.get("additionalProperties"),
                    Some(&Value::Bool(false)),
                    "object schema {path} is not strict"
                );
            }
            for (key, value) in values {
                assert_strict_object_schemas(value, &format!("{path}.{key}"));
            }
        }
        _ => {}
    }
}

#[test]
fn h2_1a_profile_is_content_addressed_and_closes_the_transition() {
    let workspace = workspace();
    let artifact: Value = serde_json::from_slice(RECORDED).expect("H2.1a profile JSON");
    assert_eq!(
        sha256(RECORDED),
        "f6114e8bc606cd6f2f361186c0f84554120ddb795cb0542a273e0cbf60e1385c"
    );
    assert_eq!(artifact["schema"], 1);
    assert_eq!(artifact["kind"], "h2-runtime-profile");
    assert_eq!(artifact["status"], "qualified");
    assert_eq!(artifact["phase"], "H2.1a");
    assert_eq!(
        artifact["origin"]["trusted_h2_0b_merge"],
        "b22491e86da731e4657fb8ec2c31c19291099b4c"
    );
    assert_eq!(artifact["transition"]["completed_slice"], "H2.1a");
    assert_eq!(artifact["transition"]["next_slice"], "H2.1b");
    assert_eq!(artifact["admitted_profile"]["exact_cases"], 241);
    assert_eq!(
        artifact["admitted_profile"]["exact_reported_diagnostics"],
        499
    );
    assert_eq!(artifact["admitted_profile"]["exact_writes"], 251);
    assert_eq!(
        artifact["admitted_profile"]["diagnostic_deferred_output_controls"],
        5
    );
    assert_eq!(artifact["admitted_profile"]["source_deferred_cases"], 49);
    assert_eq!(artifact["summary"]["executed_candidates"], 295);
    assert_eq!(artifact["summary"]["historical_artifacts_reinterpreted"], 0);

    assert_current_path_hash(
        &workspace,
        &artifact["generator"],
        "crates/oracle/h2-1a-profile.mjs",
    );
    assert_current_path_hash(
        &workspace,
        &artifact["contract"],
        ".github/ci/contracts/h2-1a-profile.schema.json",
    );
    assert_current_path_hash(
        &workspace,
        &artifact["qualification"],
        "ratchets/h2-1a-qualification.v1.json",
    );
    let inputs = array(&artifact["runtime_inputs"], "runtime inputs");
    assert_eq!(inputs.len(), RUNTIME_INPUTS.len());
    for (record, expected) in inputs.iter().zip(RUNTIME_INPUTS) {
        assert_eq!(record["path"], expected);
        assert_eq!(
            string(&record["sha256"], "historical runtime hash").len(),
            64
        );
    }

    for (field, expected_path, expected_hash) in [
        (
            "profile_transition",
            "ratchets/h2-profile-transition.v1.json",
            "a743f9489c13a6a6d717ce9a6eff48dbb304e6afb959c5d76055e4a884adff60",
        ),
        (
            "runtime_baseline",
            "ratchets/h2-runtime-baseline.v1.json",
            "634492148d44c374c922ed6bd0545c43cdcabe913c78dbffd9d2f940c4ac7cd9",
        ),
    ] {
        let record = &artifact["origin"]["historical"][field];
        assert_eq!(record["path"], expected_path);
        assert_eq!(record["sha256"], expected_hash);
    }

    let mut semantic = artifact.clone();
    let fingerprint = semantic
        .as_object_mut()
        .unwrap()
        .remove("profile_fingerprint_sha256")
        .expect("profile fingerprint");
    assert_eq!(
        string(&fingerprint, "profile fingerprint"),
        sha256(canonical(&semantic))
    );
}

#[test]
fn h2_1a_profile_schema_is_strict_at_every_object_boundary() {
    let schema: Value = serde_json::from_slice(CONTRACT).expect("H2.1a profile schema JSON");
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(schema["additionalProperties"], false);
    let required = array(&schema["required"], "schema required")
        .iter()
        .map(|value| string(value, "required field"))
        .collect::<BTreeSet<_>>();
    let artifact: Value = serde_json::from_slice(RECORDED).expect("H2.1a profile JSON");
    let artifact_keys = object(&artifact, "artifact")
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(required, artifact_keys);
    assert_strict_object_schemas(&schema, "$schema");
}

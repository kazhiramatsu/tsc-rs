use std::collections::BTreeSet;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

const RECORDED: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h2-1b-profile.v1.json"
));
const CONTRACT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../.github/ci/contracts/h2-1b-profile.schema.json"
));
const RUNTIME_INPUTS: [&str; 17] = [
    "crates/checker/src/emit.rs",
    "crates/checker/src/modules.rs",
    "crates/compiler/src/lib.rs",
    "crates/compiler/tests/integration/emit_session_contract.rs",
    "crates/diagnostics/src/gen.rs",
    "crates/emitter/src/activity.rs",
    "crates/emitter/src/builtins.rs",
    "crates/emitter/src/execute.rs",
    "crates/emitter/src/factory.rs",
    "crates/emitter/src/host.rs",
    "crates/emitter/src/printer.rs",
    "crates/emitter/src/resolver.rs",
    "crates/emitter/src/transform.rs",
    "crates/harness/src/upstream_suites/execution.rs",
    "crates/xtask/src/h2_1b_acceptance.rs",
    "crates/xtask/src/main.rs",
    "crates/xtask/tests/unit/h2_1b_acceptance/tests.rs",
];

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

fn assert_recorded_path_hash(record: &Value, expected_path: &str) {
    assert_eq!(record["path"], expected_path);
    assert_eq!(string(&record["sha256"], "path hash").len(), 64);
}

fn assert_recorded_exact(record: &Value, expected_path: &str, expected_hash: &str) {
    assert_recorded_path_hash(record, expected_path);
    assert_eq!(record["sha256"], expected_hash);
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
fn h2_1b_profile_is_content_addressed_and_closes_the_transition() {
    let artifact: Value = serde_json::from_slice(RECORDED).expect("H2.1b profile JSON");
    assert_eq!(
        sha256(RECORDED),
        "e8da0de3ddc8ac456d798b149c47b172b995a1fe5ab57e86f72fbf880dd372e3"
    );
    assert_eq!(artifact["schema"], 1);
    assert_eq!(artifact["kind"], "h2-runtime-profile");
    assert_eq!(artifact["status"], "qualified");
    assert_eq!(artifact["phase"], "H2.1b");
    assert_eq!(
        artifact["origin"]["trusted_h2_1a_merge"],
        "49a8a87c443972e3dc2a7a57d6f2e45b8581a601"
    );
    assert_eq!(artifact["transition"]["completed_slice"], "H2.1b");
    assert_eq!(artifact["transition"]["next_slice"], "H2.1c");
    assert_eq!(
        artifact["transition"]["active_runtime_slices"],
        serde_json::json!(["H2.1a", "H2.1b"])
    );
    assert_eq!(artifact["admitted_profile"]["exact_cases"], 251);
    assert_eq!(artifact["admitted_profile"]["h2_1b_exact_cases"], 10);
    assert_eq!(
        artifact["admitted_profile"]["exact_reported_diagnostics"],
        501
    );
    assert_eq!(artifact["admitted_profile"]["exact_writes"], 266);
    assert_eq!(artifact["admitted_profile"]["source_deferred_cases"], 39);
    assert_eq!(artifact["summary"]["h2_1b_executed_candidates"], 15);
    assert_eq!(artifact["summary"]["historical_artifacts_reinterpreted"], 0);

    assert_recorded_exact(
        &artifact["generator"],
        "crates/oracle/h2-1b-profile.mjs",
        "8cf4be2a99a077f633cec9983f407d9538b7bb1e9b43a2285c4d5cb576a41eff",
    );
    assert_recorded_exact(
        &artifact["contract"],
        ".github/ci/contracts/h2-1b-profile.schema.json",
        "4881d93dd418fd23cc0af5ad092fb979fb74211821dbebc29618f834d450792d",
    );
    assert_recorded_exact(
        &artifact["qualification"],
        "ratchets/h2-1b-qualification.v1.json",
        "98f0df7740c85f18bde2adea8a80f9aa511102388c369bcb405b36396d72bdb1",
    );
    let inputs = array(&artifact["runtime_inputs"], "runtime inputs");
    assert_eq!(inputs.len(), RUNTIME_INPUTS.len());
    for (record, expected) in inputs.iter().zip(RUNTIME_INPUTS) {
        assert_recorded_path_hash(record, expected);
    }

    for (field, expected_path, expected_hash) in [
        (
            "profile",
            "ratchets/h2-1a-profile.v1.json",
            "7e838cf92ddea5c49a36d42a25471552a3d103a617fd428511ab5b92b0f0d1c6",
        ),
        (
            "qualification",
            "ratchets/h2-1a-qualification.v1.json",
            "d8fa008746f9c509d9dfc1253a986e8b6437b4f0199903506161a75da9bd7d76",
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
fn h2_1b_profile_schema_is_strict_at_every_object_boundary() {
    let schema: Value = serde_json::from_slice(CONTRACT).expect("H2.1b profile schema JSON");
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(schema["additionalProperties"], false);
    let required = array(&schema["required"], "schema required")
        .iter()
        .map(|value| string(value, "required field"))
        .collect::<BTreeSet<_>>();
    let artifact: Value = serde_json::from_slice(RECORDED).expect("H2.1b profile JSON");
    let artifact_keys = object(&artifact, "artifact")
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(required, artifact_keys);
    assert_strict_object_schemas(&schema, "$schema");
}

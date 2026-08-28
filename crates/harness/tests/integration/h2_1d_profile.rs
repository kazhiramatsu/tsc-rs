use std::collections::BTreeSet;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

const RECORDED: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h2-1d-profile.v1.json"
));
const CONTRACT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../.github/ci/contracts/h2-1d-profile.schema.json"
));
const RUNTIME_INPUTS: [&str; 29] = [
    "crates/checker/src/emit.rs",
    "crates/checker/src/modules.rs",
    "crates/compiler/src/lib.rs",
    "crates/compiler/tests/integration/emit_session_contract.rs",
    "crates/compiler/tests/integration/program_session_contract.rs",
    "crates/compiler/tests/integration/upstream_no_emit_harness_contract.rs",
    "crates/diagnostics/src/gen.rs",
    "crates/emitter/src/activity.rs",
    "crates/emitter/src/builtins.rs",
    "crates/emitter/src/builtins/system.rs",
    "crates/emitter/src/execute.rs",
    "crates/emitter/src/factory.rs",
    "crates/emitter/src/host.rs",
    "crates/emitter/src/printer.rs",
    "crates/emitter/src/resolver.rs",
    "crates/emitter/src/transform.rs",
    "crates/harness/src/upstream_suites/execution.rs",
    "crates/harness/src/upstream_suites/execution/project.rs",
    "crates/program/src/prepared.rs",
    "crates/syntax/src/incremental.rs",
    "crates/syntax/src/lib.rs",
    "crates/syntax/src/parser.rs",
    "crates/syntax/tests/unit/incremental/tests.rs",
    "crates/syntax/tests/unit/parser/tests.rs",
    "crates/xtask/src/h2_1c_acceptance.rs",
    "crates/xtask/src/h2_1d_acceptance.rs",
    "crates/xtask/src/main.rs",
    "crates/xtask/tests/unit/h2_1c_acceptance/tests.rs",
    "crates/xtask/tests/unit/h2_1d_acceptance/tests.rs",
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
fn h2_1d_profile_is_content_addressed_and_closes_the_transition() {
    let artifact: Value = serde_json::from_slice(RECORDED).expect("H2.1d profile JSON");
    assert_eq!(
        sha256(RECORDED),
        "7c76251016ee984c875892c9b4c5bb0b52c3bb2d7503cf8573c51a98044808f2"
    );
    assert_eq!(artifact["schema"], 1);
    assert_eq!(artifact["kind"], "h2-runtime-profile");
    assert_eq!(artifact["status"], "qualified");
    assert_eq!(artifact["phase"], "H2.1d");
    assert_eq!(
        artifact["origin"]["trusted_h2_1c_merge"],
        "533caca4df1ebcf9e9f2ec5fd13b9c73a3ee2786"
    );
    assert_eq!(artifact["transition"]["completed_slice"], "H2.1d");
    assert_eq!(artifact["transition"]["next_slice"], "H2.1e");
    assert_eq!(
        artifact["transition"]["active_runtime_slices"],
        serde_json::json!(["H2.1a", "H2.1b", "H2.1c", "H2.1d"])
    );
    assert_eq!(artifact["admitted_profile"]["exact_cases"], 262);
    assert_eq!(artifact["admitted_profile"]["h2_1d_exact_cases"], 5);
    assert_eq!(
        artifact["admitted_profile"]["exact_reported_diagnostics"],
        512
    );
    assert_eq!(artifact["admitted_profile"]["exact_writes"], 289);
    assert_eq!(artifact["admitted_profile"]["source_deferred_cases"], 28);
    assert_eq!(artifact["summary"]["h2_1d_executed_candidates"], 6);
    assert_eq!(artifact["summary"]["historical_artifacts_reinterpreted"], 0);

    assert_recorded_exact(
        &artifact["generator"],
        "crates/oracle/h2-1d-profile.mjs",
        "828684b6b4d4c431255af4770c2d0c195ed9ff99044270c662386d4b08dd9bdb",
    );
    assert_recorded_exact(
        &artifact["contract"],
        ".github/ci/contracts/h2-1d-profile.schema.json",
        "bb34e87b872b5f35f223ec424f4dcd621f63d1b8cd224d5ac5230eac7d966310",
    );
    assert_recorded_exact(
        &artifact["qualification"],
        "ratchets/h2-1d-qualification.v1.json",
        "e933279cf2ed6f5cd02ccbf3542885629e1fa7bb4868bc5805b6b796b4bd2980",
    );
    assert_recorded_exact(
        &artifact["evidence"]["owner_controls"]["artifact"],
        "ratchets/h2-1d-owner-controls.v1.json",
        "aba2b26cbd92fab4bd6525274b5d4561bf168e45353a3c3ab6f8dea070054916",
    );
    assert_recorded_exact(
        &artifact["evidence"]["owner_controls"]["generator"],
        "crates/oracle/h2-1d-owner-controls.mjs",
        "1b2e2abdd44dc4cf394a55a3b90c28deae27d801576bf5597de71485fe31437b",
    );
    assert_recorded_exact(
        &artifact["evidence"]["owner_controls"]["contract"],
        ".github/ci/contracts/h2-1d-owner-controls.schema.json",
        "cc203640d70cc4427cd3dc2a9491a91b53a5c898308b61c4101d87d32d8c095d",
    );
    let inputs = array(&artifact["runtime_inputs"], "runtime inputs");
    assert_eq!(inputs.len(), RUNTIME_INPUTS.len());
    for (record, expected) in inputs.iter().zip(RUNTIME_INPUTS) {
        assert_recorded_path_hash(record, expected);
    }

    for (field, expected_path, expected_hash) in [
        (
            "profile",
            "ratchets/h2-1c-profile.v1.json",
            "c81fb6ce66ff6d24febf68bf27cdb4f1366da3ceec5301a0f99375930bcdd3b9",
        ),
        (
            "qualification",
            "ratchets/h2-1c-qualification.v1.json",
            "204341c987aec65331bfe9c45b28eb0d4f3aa61f87c9b14d01f9c2fa41172503",
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
fn h2_1d_profile_schema_is_strict_at_every_object_boundary() {
    let schema: Value = serde_json::from_slice(CONTRACT).expect("H2.1d profile schema JSON");
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(schema["additionalProperties"], false);
    let required = array(&schema["required"], "schema required")
        .iter()
        .map(|value| string(value, "required field"))
        .collect::<BTreeSet<_>>();
    let artifact: Value = serde_json::from_slice(RECORDED).expect("H2.1d profile JSON");
    let artifact_keys = object(&artifact, "artifact")
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(required, artifact_keys);
    assert_strict_object_schemas(&schema, "$schema");
}

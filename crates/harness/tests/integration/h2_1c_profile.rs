use std::collections::BTreeSet;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

const RECORDED: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h2-1c-profile.v1.json"
));
const CONTRACT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../.github/ci/contracts/h2-1c-profile.schema.json"
));
const RUNTIME_INPUTS: [&str; 26] = [
    "crates/checker/src/emit.rs",
    "crates/checker/src/modules.rs",
    "crates/compiler/src/lib.rs",
    "crates/compiler/tests/integration/emit_session_contract.rs",
    "crates/compiler/tests/integration/program_session_contract.rs",
    "crates/compiler/tests/integration/upstream_no_emit_harness_contract.rs",
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
    "crates/harness/src/upstream_suites/execution/project.rs",
    "crates/program/src/prepared.rs",
    "crates/syntax/src/incremental.rs",
    "crates/syntax/src/lib.rs",
    "crates/syntax/src/parser.rs",
    "crates/syntax/tests/unit/incremental/tests.rs",
    "crates/syntax/tests/unit/parser/tests.rs",
    "crates/xtask/src/h2_1c_acceptance.rs",
    "crates/xtask/src/main.rs",
    "crates/xtask/tests/unit/h2_1c_acceptance/tests.rs",
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
fn h2_1c_profile_is_content_addressed_and_closes_the_transition() {
    let artifact: Value = serde_json::from_slice(RECORDED).expect("H2.1c profile JSON");
    assert_eq!(
        sha256(RECORDED),
        "d00c03bb117af8e4869a4d14ef5aac9f19ae24219bd5710403df0204316da788"
    );
    assert_eq!(artifact["schema"], 1);
    assert_eq!(artifact["kind"], "h2-runtime-profile");
    assert_eq!(artifact["status"], "qualified");
    assert_eq!(artifact["phase"], "H2.1c");
    assert_eq!(
        artifact["origin"]["trusted_h2_1b_merge"],
        "53a5509cc6a3f295744a7286a0bbc4b7c6096fcb"
    );
    assert_eq!(artifact["transition"]["completed_slice"], "H2.1c");
    assert_eq!(artifact["transition"]["next_slice"], "H2.1d");
    assert_eq!(
        artifact["transition"]["active_runtime_slices"],
        serde_json::json!(["H2.1a", "H2.1b", "H2.1c"])
    );
    assert_eq!(artifact["admitted_profile"]["exact_cases"], 257);
    assert_eq!(artifact["admitted_profile"]["h2_1c_exact_cases"], 6);
    assert_eq!(
        artifact["admitted_profile"]["exact_reported_diagnostics"],
        507
    );
    assert_eq!(artifact["admitted_profile"]["exact_writes"], 278);
    assert_eq!(artifact["admitted_profile"]["source_deferred_cases"], 33);
    assert_eq!(artifact["summary"]["h2_1c_executed_candidates"], 8);
    assert_eq!(artifact["summary"]["historical_artifacts_reinterpreted"], 0);

    assert_recorded_exact(
        &artifact["generator"],
        "crates/oracle/h2-1c-profile.mjs",
        "2e23691e00491ef33b2d8e482960569baee94a0c18f9ebc46ee2cf7cda4d6fde",
    );
    assert_recorded_exact(
        &artifact["contract"],
        ".github/ci/contracts/h2-1c-profile.schema.json",
        "23fad428ca467c2af1d821081dd58e26d3d637125e5d9a1295af3e7b9f6f4cdd",
    );
    assert_recorded_exact(
        &artifact["qualification"],
        "ratchets/h2-1c-qualification.v1.json",
        "dd53c4fdf439c92be63819171dfb0d79669d57e6bbb2cedd3da5897a84311865",
    );
    assert_recorded_exact(
        &artifact["evidence"]["owner_controls"]["artifact"],
        "ratchets/h2-1c-owner-controls.v1.json",
        "6aa502734a38a0905d5e996dd3d1a61ce20617c9d1ffa4f7b72d689fd95596de",
    );
    assert_recorded_exact(
        &artifact["evidence"]["owner_controls"]["generator"],
        "crates/oracle/h2-1c-owner-controls.mjs",
        "d6e7a70b8ec85a82b0f68c7884bdf9c2b15b0f41d0dfffa82cfca2f6a0ed0362",
    );
    assert_recorded_exact(
        &artifact["evidence"]["owner_controls"]["contract"],
        ".github/ci/contracts/h2-1c-owner-controls.schema.json",
        "9c25aade0699776dcf7d9d1d7ad42389e6c8df20d978a219247e6ebda01425bc",
    );
    let inputs = array(&artifact["runtime_inputs"], "runtime inputs");
    assert_eq!(inputs.len(), RUNTIME_INPUTS.len());
    for (record, expected) in inputs.iter().zip(RUNTIME_INPUTS) {
        assert_recorded_path_hash(record, expected);
    }

    for (field, expected_path, expected_hash) in [
        (
            "profile",
            "ratchets/h2-1b-profile.v1.json",
            "6c9e2556ba12731c3a9e5071ac70a5592564d859727a923b90d99a7064d2b47d",
        ),
        (
            "qualification",
            "ratchets/h2-1b-qualification.v1.json",
            "b6aa708bc6eb622d852d0c22cffc15b18e673bc88d1a0fbe2f639ed125c7ae7b",
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
fn h2_1c_profile_schema_is_strict_at_every_object_boundary() {
    let schema: Value = serde_json::from_slice(CONTRACT).expect("H2.1c profile schema JSON");
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(schema["additionalProperties"], false);
    let required = array(&schema["required"], "schema required")
        .iter()
        .map(|value| string(value, "required field"))
        .collect::<BTreeSet<_>>();
    let artifact: Value = serde_json::from_slice(RECORDED).expect("H2.1c profile JSON");
    let artifact_keys = object(&artifact, "artifact")
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(required, artifact_keys);
    assert_strict_object_schemas(&schema, "$schema");
}

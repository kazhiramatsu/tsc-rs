use std::collections::BTreeSet;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

const RECORDED: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h2-2c-profile.v1.json"
));
const CONTRACT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../.github/ci/contracts/h2-2c-profile.schema.json"
));
const RUNTIME_INPUTS: [&str; 41] = [
    "crates/checker/src/emit.rs",
    "crates/checker/src/evaluate.rs",
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
    "crates/emitter/src/lib.rs",
    "crates/emitter/src/metadata.rs",
    "crates/emitter/src/printer.rs",
    "crates/emitter/src/resolver.rs",
    "crates/emitter/src/transform.rs",
    "crates/harness/src/upstream_suites/execution.rs",
    "crates/harness/src/upstream_suites/execution/project.rs",
    "crates/program/src/prepared.rs",
    "crates/program/src/module_requests.rs",
    "crates/program/tests/integration/module_request_contract.rs",
    "crates/syntax/src/incremental.rs",
    "crates/syntax/src/lib.rs",
    "crates/syntax/src/parser.rs",
    "crates/syntax/tests/unit/incremental/tests.rs",
    "crates/syntax/tests/unit/parser/tests.rs",
    "crates/xtask/src/h2_1a_acceptance.rs",
    "crates/xtask/src/h2_1d_acceptance.rs",
    "crates/xtask/src/h2_1e_acceptance.rs",
    "crates/xtask/src/h2_2a_acceptance.rs",
    "crates/xtask/src/h2_2b_acceptance.rs",
    "crates/xtask/src/h2_2c_acceptance.rs",
    "crates/xtask/src/main.rs",
    "crates/xtask/tests/unit/h2_1d_acceptance/tests.rs",
    "crates/xtask/tests/unit/h2_1e_acceptance/tests.rs",
    "crates/xtask/tests/unit/h2_2a_acceptance/tests.rs",
    "crates/xtask/tests/unit/h2_2b_acceptance/tests.rs",
    "crates/xtask/tests/unit/h2_2c_acceptance/tests.rs",
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
fn h2_2c_profile_is_content_addressed_and_closes_the_transition() {
    let artifact: Value = serde_json::from_slice(RECORDED).expect("H2.2c profile JSON");
    assert_eq!(
        sha256(RECORDED),
        "b7b71765721d92f47478e86da5ae0f7317252f85e2dc9038d9ef694dc20f86b5"
    );
    assert_eq!(artifact["schema"], 1);
    assert_eq!(artifact["kind"], "h2-runtime-profile");
    assert_eq!(artifact["status"], "qualified");
    assert_eq!(artifact["phase"], "H2.2c");
    assert_eq!(
        artifact["origin"]["trusted_h2_2b_merge"],
        "cb450995d1d0e72f34b774dc3feb9a32384e6068"
    );
    assert_eq!(artifact["transition"]["completed_slice"], "H2.2c");
    assert_eq!(artifact["transition"]["next_slice"], "H2.2d");
    assert_eq!(
        artifact["transition"]["active_runtime_slices"],
        serde_json::json!(["H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a", "H2.2b", "H2.2c"])
    );
    assert_eq!(artifact["admitted_profile"]["exact_cases"], 293);
    assert_eq!(artifact["admitted_profile"]["h2_2c_exact_cases"], 6);
    assert_eq!(
        artifact["admitted_profile"]["exact_reported_diagnostics"],
        597
    );
    assert_eq!(artifact["admitted_profile"]["exact_writes"], 384);
    assert_eq!(artifact["admitted_profile"]["source_deferred_cases"], 0);
    assert_eq!(artifact["summary"]["h2_2c_executed_candidates"], 6);
    assert_eq!(artifact["summary"]["historical_artifacts_reinterpreted"], 0);

    assert_recorded_exact(
        &artifact["generator"],
        "crates/oracle/h2-2c-profile.mjs",
        "524e77499bacb2b84f0c033d5cf723b6fb1d213a5c723062fb51011972ecfe0e",
    );
    assert_recorded_exact(
        &artifact["contract"],
        ".github/ci/contracts/h2-2c-profile.schema.json",
        "e66208499e81d6004ff43232138a2989a72d55cc6284a5d2a795b475523dd333",
    );
    assert_recorded_exact(
        &artifact["qualification"],
        "ratchets/h2-2c-qualification.v1.json",
        "00a6ebb1291507cdf79eb2114869618540834701cd62e6011e069a4df3f5d13a",
    );
    let inputs = array(&artifact["runtime_inputs"], "runtime inputs");
    assert_eq!(inputs.len(), RUNTIME_INPUTS.len());
    for (record, expected) in inputs.iter().zip(RUNTIME_INPUTS) {
        assert_recorded_path_hash(record, expected);
    }

    for (field, expected_path, expected_hash) in [
        (
            "profile",
            "ratchets/h2-2b-profile.v1.json",
            "edcad179754ddec8329ac7bffd9c2cfb16f82d264c116e134427cd7c735a8a65",
        ),
        (
            "qualification",
            "ratchets/h2-2b-qualification.v1.json",
            "f2e815e6fbad3300e4db6feb9d07235c8886cd46d4528dc51306b58b0bae25a3",
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
fn h2_2c_profile_schema_is_strict_at_every_object_boundary() {
    let schema: Value = serde_json::from_slice(CONTRACT).expect("H2.2c profile schema JSON");
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(schema["additionalProperties"], false);
    let required = array(&schema["required"], "schema required")
        .iter()
        .map(|value| string(value, "required field"))
        .collect::<BTreeSet<_>>();
    let artifact: Value = serde_json::from_slice(RECORDED).expect("H2.2c profile JSON");
    let artifact_keys = object(&artifact, "artifact")
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(required, artifact_keys);
    assert_strict_object_schemas(&schema, "$schema");
}

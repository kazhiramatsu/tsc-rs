use std::collections::BTreeSet;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

const RECORDED: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h2-2a-profile.v1.json"
));
const CONTRACT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../.github/ci/contracts/h2-2a-profile.schema.json"
));
const RUNTIME_INPUTS: [&str; 37] = [
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
    "crates/xtask/src/main.rs",
    "crates/xtask/tests/unit/h2_1d_acceptance/tests.rs",
    "crates/xtask/tests/unit/h2_1e_acceptance/tests.rs",
    "crates/xtask/tests/unit/h2_2a_acceptance/tests.rs",
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
fn h2_2a_profile_is_content_addressed_and_closes_the_transition() {
    let artifact: Value = serde_json::from_slice(RECORDED).expect("H2.2a profile JSON");
    assert_eq!(
        sha256(RECORDED),
        "7e68362d9a3810af320feb825f6c4054a58e9b2c8b67d8faaa1ed92e7af77031"
    );
    assert_eq!(artifact["schema"], 1);
    assert_eq!(artifact["kind"], "h2-runtime-profile");
    assert_eq!(artifact["status"], "qualified");
    assert_eq!(artifact["phase"], "H2.2a");
    assert_eq!(
        artifact["origin"]["trusted_h2_1e_merge"],
        "ba45ad089d903e632676950be9fcea3ab56fbb37"
    );
    assert_eq!(artifact["transition"]["completed_slice"], "H2.2a");
    assert_eq!(artifact["transition"]["next_slice"], "H2.2b");
    assert_eq!(
        artifact["transition"]["active_runtime_slices"],
        serde_json::json!(["H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a"])
    );
    assert_eq!(artifact["admitted_profile"]["exact_cases"], 272);
    assert_eq!(artifact["admitted_profile"]["h2_2a_exact_cases"], 6);
    assert_eq!(
        artifact["admitted_profile"]["exact_reported_diagnostics"],
        526
    );
    assert_eq!(artifact["admitted_profile"]["exact_writes"], 306);
    assert_eq!(artifact["admitted_profile"]["source_deferred_cases"], 18);
    assert_eq!(artifact["summary"]["h2_2a_executed_candidates"], 11);
    assert_eq!(artifact["summary"]["historical_artifacts_reinterpreted"], 0);

    assert_recorded_exact(
        &artifact["generator"],
        "crates/oracle/h2-2a-profile.mjs",
        "78a2a9d84ffbc5125918d783d3bf847a3b43e05d86d2e3f64539604bc645380d",
    );
    assert_recorded_exact(
        &artifact["contract"],
        ".github/ci/contracts/h2-2a-profile.schema.json",
        "1d4e4442d206d85793aa7987a8e67b4a13afa5b7cb678a1a786afae2f2c00ca7",
    );
    assert_recorded_exact(
        &artifact["qualification"],
        "ratchets/h2-2a-qualification.v1.json",
        "23920a4e6ced2e26929be5403c50ed26938cacd360e658c647dbef336d5d02dc",
    );
    let inputs = array(&artifact["runtime_inputs"], "runtime inputs");
    assert_eq!(inputs.len(), RUNTIME_INPUTS.len());
    for (record, expected) in inputs.iter().zip(RUNTIME_INPUTS) {
        assert_recorded_path_hash(record, expected);
    }

    for (field, expected_path, expected_hash) in [
        (
            "profile",
            "ratchets/h2-1e-profile.v1.json",
            "4aa24e50dfab90fb89f457a398eb2cd297e261b670cc73228efbc933e7fde2b1",
        ),
        (
            "qualification",
            "ratchets/h2-1e-qualification.v1.json",
            "493f24fae3b791f6f9c15a301d60299e3471ed812f9fb9def36002096a1cc2fc",
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
fn h2_2a_profile_schema_is_strict_at_every_object_boundary() {
    let schema: Value = serde_json::from_slice(CONTRACT).expect("H2.2a profile schema JSON");
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(schema["additionalProperties"], false);
    let required = array(&schema["required"], "schema required")
        .iter()
        .map(|value| string(value, "required field"))
        .collect::<BTreeSet<_>>();
    let artifact: Value = serde_json::from_slice(RECORDED).expect("H2.2a profile JSON");
    let artifact_keys = object(&artifact, "artifact")
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(required, artifact_keys);
    assert_strict_object_schemas(&schema, "$schema");
}

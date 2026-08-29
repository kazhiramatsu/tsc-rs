use std::collections::BTreeSet;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

const RECORDED: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h2-3c-profile.v1.json"
));
const CONTRACT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../.github/ci/contracts/h2-3c-profile.schema.json"
));
const RUNTIME_INPUTS: [&str; 60] = [
    "crates/checker/src/emit.rs",
    "crates/checker/src/evaluate.rs",
    "crates/checker/src/lib.rs",
    "crates/checker/src/modules.rs",
    "crates/compiler/src/lib.rs",
    "crates/compiler/tests/integration/emit_session_contract.rs",
    "crates/compiler/tests/integration/h1_emit_qualification_contract.rs",
    "crates/compiler/tests/integration/program_session_contract.rs",
    "crates/compiler/tests/integration/upstream_no_emit_harness_contract.rs",
    "crates/diagnostics/src/gen.rs",
    "crates/emitter/src/activity.rs",
    "crates/emitter/src/builtins.rs",
    "crates/emitter/src/builtins/jsx.rs",
    "crates/emitter/src/builtins/system.rs",
    "crates/emitter/src/execute.rs",
    "crates/emitter/src/factory.rs",
    "crates/emitter/src/host.rs",
    "crates/emitter/src/lib.rs",
    "crates/emitter/src/metadata.rs",
    "crates/emitter/src/plan.rs",
    "crates/emitter/src/printer.rs",
    "crates/emitter/src/resolver.rs",
    "crates/emitter/src/transform.rs",
    "crates/emitter/tests/integration/active_transform_contract.rs",
    "crates/emitter/tests/integration/output_plan_contract.rs",
    "crates/emitter/tests/unit/activity/tests.rs",
    "crates/harness/src/upstream_suites/execution.rs",
    "crates/harness/src/upstream_suites/execution/project.rs",
    "crates/program/src/prepared.rs",
    "crates/program/src/loader.rs",
    "crates/program/src/module_requests.rs",
    "crates/program/tests/integration/module_request_contract.rs",
    "crates/syntax/src/incremental.rs",
    "crates/syntax/src/lib.rs",
    "crates/syntax/src/parser.rs",
    "crates/syntax/tests/unit/incremental/tests.rs",
    "crates/syntax/tests/unit/parser/tests.rs",
    "crates/xtask/src/h2_1a_acceptance.rs",
    "crates/xtask/src/h2_1b_acceptance.rs",
    "crates/xtask/src/h2_1c_acceptance.rs",
    "crates/xtask/src/h2_1d_acceptance.rs",
    "crates/xtask/src/h2_1e_acceptance.rs",
    "crates/xtask/src/h2_2a_acceptance.rs",
    "crates/xtask/src/h2_2b_acceptance.rs",
    "crates/xtask/src/h2_2c_acceptance.rs",
    "crates/xtask/src/h2_2d_acceptance.rs",
    "crates/xtask/src/h2_3a_acceptance.rs",
    "crates/xtask/src/h2_3b_acceptance.rs",
    "crates/xtask/src/h2_3c_acceptance.rs",
    "crates/xtask/src/main.rs",
    "crates/xtask/tests/unit/h2_1d_acceptance/tests.rs",
    "crates/xtask/tests/unit/h2_1e_acceptance/tests.rs",
    "crates/xtask/tests/unit/h2_2a_acceptance/tests.rs",
    "crates/xtask/tests/unit/h2_2b_acceptance/tests.rs",
    "crates/xtask/tests/unit/h2_2c_acceptance/tests.rs",
    "crates/xtask/tests/unit/h2_2d_acceptance/tests.rs",
    "crates/xtask/tests/unit/h2_3a_acceptance/tests.rs",
    "crates/xtask/tests/unit/h2_3b_acceptance/tests.rs",
    "crates/xtask/tests/unit/h2_3c_acceptance/tests.rs",
    "crates/types/src/options.rs",
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
fn h2_3c_profile_is_content_addressed_and_closes_the_transition() {
    let artifact: Value = serde_json::from_slice(RECORDED).expect("H2.3c profile JSON");
    assert_eq!(
        sha256(RECORDED),
        "1340065eb924a28b503d33634509fe9ffdf8db19c23346aee5e90912e90ef490"
    );
    assert_eq!(artifact["schema"], 1);
    assert_eq!(artifact["kind"], "h2-runtime-profile");
    assert_eq!(artifact["status"], "qualified");
    assert_eq!(artifact["phase"], "H2.3c");
    assert_eq!(
        artifact["origin"]["trusted_h2_3b_merge"],
        "7aaaa414133d630180931dd79cd9169d43e54121"
    );
    assert_eq!(artifact["transition"]["completed_slice"], "H2.3c");
    assert_eq!(artifact["transition"]["next_slice"], "H2.3d");
    assert_eq!(
        artifact["transition"]["active_runtime_slices"],
        serde_json::json!([
            "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a", "H2.2b", "H2.2c", "H2.2d",
            "H2.3a", "H2.3b", "H2.3c"
        ])
    );
    assert_eq!(artifact["admitted_profile"]["exact_cases"], 309);
    assert_eq!(artifact["admitted_profile"]["h2_3c_exact_cases"], 4);
    assert_eq!(
        artifact["admitted_profile"]["exact_reported_diagnostics"],
        680
    );
    assert_eq!(artifact["admitted_profile"]["exact_writes"], 404);
    assert_eq!(artifact["admitted_profile"]["source_deferred_cases"], 0);
    assert_eq!(artifact["admitted_profile"]["candidate_denominator"], 302);
    assert_eq!(artifact["summary"]["h2_3c_executed_candidates"], 4);
    assert_eq!(artifact["summary"]["historical_artifacts_reinterpreted"], 0);

    for (record, expected_path, expected_hash) in [
        (
            &artifact["generator"],
            "crates/oracle/h2-3c-profile.mjs",
            "2bbf020e6046bdf78f80307e5cff18879b8645edc5eaae5547d44c8d55dbf1c0",
        ),
        (
            &artifact["contract"],
            ".github/ci/contracts/h2-3c-profile.schema.json",
            "aa9b14c064e5be1ee77c3fd43b81994b81bcce0f930582730e24e2a3fe70ef70",
        ),
        (
            &artifact["qualification"],
            "ratchets/h2-3c-qualification.v1.json",
            "cb7fe27c9688357a9caf74ec284709e3cdbe8b6249e7783284c281b897567cfd",
        ),
        (
            &artifact["evidence"]["owner_controls"]["artifact"],
            "ratchets/h2-3c-owner-controls.v1.json",
            "7e158b0311f5c3a6b3fcb60dcfbbebb5cd418d603502022382473ac3ac7b916b",
        ),
        (
            &artifact["evidence"]["owner_controls"]["generator"],
            "crates/oracle/h2-3c-owner-controls.mjs",
            "351b828df9d01025702d15cc7ea9e368bf6fd9218b6eedaccb7fcfbe7be49def",
        ),
        (
            &artifact["evidence"]["owner_controls"]["contract"],
            ".github/ci/contracts/h2-3c-owner-controls.schema.json",
            "a0a0d54df71862647cfd21d58ea960ddfdded1d8c4df8b552e943297e5579ed1",
        ),
    ] {
        assert_recorded_exact(record, expected_path, expected_hash);
    }

    let inputs = array(&artifact["runtime_inputs"], "runtime inputs");
    assert_eq!(inputs.len(), RUNTIME_INPUTS.len());
    for (record, expected) in inputs.iter().zip(RUNTIME_INPUTS) {
        assert_recorded_path_hash(record, expected);
    }

    for (field, expected_path, expected_hash) in [
        (
            "profile",
            "ratchets/h2-3b-profile.v1.json",
            "7cc64c48c58d2e80066ee212d513534ee8d3d7e24b7f3f2a627ad9fa68632ad2",
        ),
        (
            "qualification",
            "ratchets/h2-3b-qualification.v1.json",
            "217fc5df10a398c0e8ae6fa35ce2a0c29375ccfc9ef3923841cd5ecf7f6664ac",
        ),
        (
            "owner_controls",
            "ratchets/h2-3b-owner-controls.v1.json",
            "7cb7058d0b2130a196ba0352093c531fc7ea059a69bd7438585936d04e2e7e72",
        ),
    ] {
        assert_recorded_exact(
            &artifact["origin"]["historical"][field],
            expected_path,
            expected_hash,
        );
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
fn h2_3c_profile_schema_is_strict_at_every_object_boundary() {
    let schema: Value = serde_json::from_slice(CONTRACT).expect("H2.3c profile schema JSON");
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(schema["additionalProperties"], false);
    let required = array(&schema["required"], "schema required")
        .iter()
        .map(|value| string(value, "required field"))
        .collect::<BTreeSet<_>>();
    let artifact: Value = serde_json::from_slice(RECORDED).expect("H2.3c profile JSON");
    let artifact_keys = object(&artifact, "artifact")
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(required, artifact_keys);
    assert_strict_object_schemas(&schema, "$schema");
}

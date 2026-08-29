use std::collections::BTreeSet;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

const RECORDED: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h2-3b-profile.v1.json"
));
const CONTRACT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../.github/ci/contracts/h2-3b-profile.schema.json"
));
const RUNTIME_INPUTS: [&str; 58] = [
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
    "crates/xtask/src/main.rs",
    "crates/xtask/tests/unit/h2_1d_acceptance/tests.rs",
    "crates/xtask/tests/unit/h2_1e_acceptance/tests.rs",
    "crates/xtask/tests/unit/h2_2a_acceptance/tests.rs",
    "crates/xtask/tests/unit/h2_2b_acceptance/tests.rs",
    "crates/xtask/tests/unit/h2_2c_acceptance/tests.rs",
    "crates/xtask/tests/unit/h2_2d_acceptance/tests.rs",
    "crates/xtask/tests/unit/h2_3a_acceptance/tests.rs",
    "crates/xtask/tests/unit/h2_3b_acceptance/tests.rs",
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
fn h2_3b_profile_is_content_addressed_and_closes_the_transition() {
    let artifact: Value = serde_json::from_slice(RECORDED).expect("H2.3b profile JSON");
    assert_eq!(
        sha256(RECORDED),
        "b3b58853309ffd5ed1bcf2727c296357b3b6c5e13523063801c871dbbdedb705"
    );
    assert_eq!(artifact["schema"], 1);
    assert_eq!(artifact["kind"], "h2-runtime-profile");
    assert_eq!(artifact["status"], "qualified");
    assert_eq!(artifact["phase"], "H2.3b");
    assert_eq!(
        artifact["origin"]["trusted_h2_3a_merge"],
        "3ce56e1fdb1c3841bc27f37b33488d3dd25b65a0"
    );
    assert_eq!(artifact["transition"]["completed_slice"], "H2.3b");
    assert_eq!(artifact["transition"]["next_slice"], "H2.3c");
    assert_eq!(
        artifact["transition"]["active_runtime_slices"],
        serde_json::json!([
            "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a", "H2.2b", "H2.2c", "H2.2d",
            "H2.3a", "H2.3b"
        ])
    );
    assert_eq!(artifact["admitted_profile"]["exact_cases"], 305);
    assert_eq!(artifact["admitted_profile"]["h2_3b_exact_cases"], 2);
    assert_eq!(
        artifact["admitted_profile"]["exact_reported_diagnostics"],
        638
    );
    assert_eq!(artifact["admitted_profile"]["exact_writes"], 400);
    assert_eq!(artifact["admitted_profile"]["source_deferred_cases"], 4);
    assert_eq!(artifact["admitted_profile"]["candidate_denominator"], 302);
    assert_eq!(artifact["summary"]["h2_3b_executed_candidates"], 6);
    assert_eq!(artifact["summary"]["historical_artifacts_reinterpreted"], 0);

    for (record, expected_path, expected_hash) in [
        (
            &artifact["generator"],
            "crates/oracle/h2-3b-profile.mjs",
            "af474d1a1489e1b67638daa242bb38eecd7645b3b0fb8308c7447daf4745e0bc",
        ),
        (
            &artifact["contract"],
            ".github/ci/contracts/h2-3b-profile.schema.json",
            "d21a658c8fc4d004130074deb38b60b7d133e3ace6b7b497a1a54cc2beb48f04",
        ),
        (
            &artifact["qualification"],
            "ratchets/h2-3b-qualification.v1.json",
            "4ae20ded498223f4bcfd9e3911753948bc850763899a9629f9b2d1760a3b5a36",
        ),
        (
            &artifact["evidence"]["owner_controls"]["artifact"],
            "ratchets/h2-3b-owner-controls.v1.json",
            "7cb7058d0b2130a196ba0352093c531fc7ea059a69bd7438585936d04e2e7e72",
        ),
        (
            &artifact["evidence"]["owner_controls"]["generator"],
            "crates/oracle/h2-3b-owner-controls.mjs",
            "d2bee6c116d2f98a31e4bf67b0601d80384236f9db8e851a82d72fc8bec283de",
        ),
        (
            &artifact["evidence"]["owner_controls"]["contract"],
            ".github/ci/contracts/h2-3b-owner-controls.schema.json",
            "fb1f709a9c0b468b1c1849f2a6c4268e94729869d7bc5a91f742101c486b3f12",
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
            "ratchets/h2-3a-profile.v1.json",
            "daf09a76edfb2ee9c4a9f0a307d3d6cf188eadb660468c61ad382606ec94b1a5",
        ),
        (
            "qualification",
            "ratchets/h2-3a-qualification.v1.json",
            "3da6d5af73829102993ba2b752d5bfc5af1c3ac6805920bfc5be0849f666a2e4",
        ),
        (
            "owner_controls",
            "ratchets/h2-3a-owner-controls.v1.json",
            "76ac4421892d434f9b6ee5113994776644e506263d9fc94c2cca79670cadb622",
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
fn h2_3b_profile_schema_is_strict_at_every_object_boundary() {
    let schema: Value = serde_json::from_slice(CONTRACT).expect("H2.3b profile schema JSON");
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(schema["additionalProperties"], false);
    let required = array(&schema["required"], "schema required")
        .iter()
        .map(|value| string(value, "required field"))
        .collect::<BTreeSet<_>>();
    let artifact: Value = serde_json::from_slice(RECORDED).expect("H2.3b profile JSON");
    let artifact_keys = object(&artifact, "artifact")
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(required, artifact_keys);
    assert_strict_object_schemas(&schema, "$schema");
}

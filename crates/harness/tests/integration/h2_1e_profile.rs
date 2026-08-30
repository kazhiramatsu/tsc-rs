use std::collections::BTreeSet;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

const RECORDED: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h2-1e-profile.v1.json"
));
const CONTRACT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../.github/ci/contracts/h2-1e-profile.schema.json"
));
const RUNTIME_INPUTS: [&str; 31] = [
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
    "crates/program/src/module_requests.rs",
    "crates/program/tests/integration/module_request_contract.rs",
    "crates/syntax/src/incremental.rs",
    "crates/syntax/src/lib.rs",
    "crates/syntax/src/parser.rs",
    "crates/syntax/tests/unit/incremental/tests.rs",
    "crates/syntax/tests/unit/parser/tests.rs",
    "crates/xtask/src/h2_1d_acceptance.rs",
    "crates/xtask/src/h2_1e_acceptance.rs",
    "crates/xtask/src/main.rs",
    "crates/xtask/tests/unit/h2_1d_acceptance/tests.rs",
    "crates/xtask/tests/unit/h2_1e_acceptance/tests.rs",
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
fn h2_1e_profile_is_content_addressed_and_closes_the_transition() {
    let artifact: Value = serde_json::from_slice(RECORDED).expect("H2.1e profile JSON");
    assert_eq!(
        sha256(RECORDED),
        "1a8f3a0ff29eff6221c4e2a8c0a23cb272045010126d856b8b1661dafac33e53"
    );
    assert_eq!(artifact["schema"], 1);
    assert_eq!(artifact["kind"], "h2-runtime-profile");
    assert_eq!(artifact["status"], "qualified");
    assert_eq!(artifact["phase"], "H2.1e");
    assert_eq!(
        artifact["origin"]["trusted_h2_1d_merge"],
        "3cfa24fd7ef3bdd8dab97d4adf860306fac75782"
    );
    assert_eq!(artifact["transition"]["completed_slice"], "H2.1e");
    assert_eq!(artifact["transition"]["next_slice"], "H2.2a");
    assert_eq!(
        artifact["transition"]["active_runtime_slices"],
        serde_json::json!(["H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e"])
    );
    assert_eq!(artifact["admitted_profile"]["exact_cases"], 266);
    assert_eq!(artifact["admitted_profile"]["h2_1e_exact_cases"], 4);
    assert_eq!(
        artifact["admitted_profile"]["exact_reported_diagnostics"],
        518
    );
    assert_eq!(artifact["admitted_profile"]["exact_writes"], 297);
    assert_eq!(artifact["admitted_profile"]["source_deferred_cases"], 24);
    assert_eq!(artifact["summary"]["h2_1e_executed_candidates"], 6);
    assert_eq!(artifact["summary"]["historical_artifacts_reinterpreted"], 0);

    assert_recorded_exact(
        &artifact["generator"],
        "crates/oracle/h2-1e-profile.mjs",
        "27f2e3a98a0b2e03643c495e990b4012f423c50edee0ee93c436dd90757819ea",
    );
    assert_recorded_exact(
        &artifact["contract"],
        ".github/ci/contracts/h2-1e-profile.schema.json",
        "1f174584838885f3e26edb5bb8425056194bc285289820201237c1fc5660a305",
    );
    assert_recorded_exact(
        &artifact["qualification"],
        "ratchets/h2-1e-qualification.v1.json",
        "5f571156030f12a5ada71fc535b0f253ec6da1eb3c2eef90b030601c62a692b8",
    );
    assert_recorded_exact(
        &artifact["evidence"]["owner_controls"]["artifact"],
        "ratchets/h2-1e-owner-controls.v1.json",
        "259edadf2d0db814353b5de2060bb1ea9164e2c4f2a10a990ddc1594d7e9f6f4",
    );
    assert_recorded_exact(
        &artifact["evidence"]["owner_controls"]["generator"],
        "crates/oracle/h2-1e-owner-controls.mjs",
        "e3feec8f379215a3beb47342e57cf8b3bd6e5481cc83070b732358fde3bb57f1",
    );
    assert_recorded_exact(
        &artifact["evidence"]["owner_controls"]["contract"],
        ".github/ci/contracts/h2-1e-owner-controls.schema.json",
        "3ffaff1f62f94a036225dfe1e9cdb382a42968ea5aa98f4828e3bce1f8ac7956",
    );
    let inputs = array(&artifact["runtime_inputs"], "runtime inputs");
    assert_eq!(inputs.len(), RUNTIME_INPUTS.len());
    for (record, expected) in inputs.iter().zip(RUNTIME_INPUTS) {
        assert_recorded_path_hash(record, expected);
    }

    for (field, expected_path, expected_hash) in [
        (
            "profile",
            "ratchets/h2-1d-profile.v1.json",
            "7739e35f298c80230869f0c1144199be3d9871307300f78e70432521a28a8b5f",
        ),
        (
            "qualification",
            "ratchets/h2-1d-qualification.v1.json",
            "f84fbe7d5dfb87595286583723227e1ab6718d541a488cbc36045073789b1b3b",
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
fn h2_1e_profile_schema_is_strict_at_every_object_boundary() {
    let schema: Value = serde_json::from_slice(CONTRACT).expect("H2.1e profile schema JSON");
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(schema["additionalProperties"], false);
    let required = array(&schema["required"], "schema required")
        .iter()
        .map(|value| string(value, "required field"))
        .collect::<BTreeSet<_>>();
    let artifact: Value = serde_json::from_slice(RECORDED).expect("H2.1e profile JSON");
    let artifact_keys = object(&artifact, "artifact")
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(required, artifact_keys);
    assert_strict_object_schemas(&schema, "$schema");
}

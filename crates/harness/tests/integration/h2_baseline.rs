use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

const RECORDED: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h2-runtime-baseline.v1.json"
));
const CONTRACT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../.github/ci/contracts/h2-runtime-baseline.schema.json"
));
const GENERATOR: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../oracle/h2-baseline.mjs"
));
const GENERATOR_PATH: &str = "crates/oracle/h2-baseline.mjs";
const BASE_COMMIT: &str = "5d50819f39c8c36f9b8b3e420d5e96c779737578";
const CANDIDATE_COMMIT: &str = "2894d167b336c6c8039f23f71d31bef223c40ef5";
const AUTHORITIES: [(&str, &str); 9] = [
    (
        "ratchets/h2-owner-inventory.v1.json",
        "1f3d666d107247bef7b5e18d6e9506ff51d281277c136bf70112a863b6dfa98d",
    ),
    (
        "ratchets/h2-candidate-dispositions.v1.json",
        "6930d377041d755579e6dbcc1f5551ba84b8ebac29535e4b05e46b386e29b53a",
    ),
    (
        "ratchets/h2-profile-transition.v1.json",
        "c7e02004a8ca337b2c9a2abd1784a2d1098b00f1660166c04233c41cccf7eb5a",
    ),
    (
        "ratchets/h1-emit-qualification.v1.json",
        "4a9a36b3b35acd9c22bf22fc88ba2c463bc6a16a18f61d2ee38c528d4aaa42ef",
    ),
    (
        "ratchets/h1-noemit-performance.v1.json",
        "452d2125fae0c386a7ced5fdcdb0ac91269bd29111013eb134e247ca6516303e",
    ),
    (
        "ratchets/h1-emit-performance.v1.json",
        "33fa3f9710733c937d7b66327d5957575a5b94424df47a69036c1a6ee9fc0754",
    ),
    (
        "ratchets/l1-incremental-parser-performance.v1.json",
        "05b4fdc0a7e50bfe05c722165d71ff47a5ba3e74f10647c0f4c9b5492d77ac5c",
    ),
    (
        "ratchets/l0-fixtures.v1.json",
        "365bb5c697a16713926345936ce553cb9a8d93b65aa34bcfa6398334c26e5d47",
    ),
    (
        "crates/compiler/examples/h2_baseline_qualification.rs",
        "2f4f5db638421b9097b0483bbd58ef36b78ea5a6acd7190379902a3b5d755c90",
    ),
];
const RUNTIME_SLICES: [&str; 37] = [
    "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a", "H2.2b", "H2.2c", "H2.2d", "H2.3a",
    "H2.3b", "H2.3c", "H2.3d", "H2.4a", "H2.4b", "H2.5a", "H2.5b", "H2.5c", "H2.5d", "H2.5e",
    "H2.5f", "H2.5g", "H2.5h", "H2.6a", "H2.6b", "H2.6c", "H2.7a", "H2.7b", "H2.7c", "H2.7d",
    "H2.7e", "H2.8a", "H2.8b", "H2.8c", "H2.8d", "H2.8e", "H2.9",
];
const POSITIVE_FIELDS: [&str; 12] = [
    "emit_session_constructions",
    "output_plan_constructions",
    "emit_resolver_borrows",
    "script_transformer_list_constructions",
    "transform_typescript_constructions",
    "transform_class_fields_constructions",
    "transform_ecmascript_module_constructions",
    "transform_context_constructions",
    "printer_constructions",
    "javascript_artifact_creations",
    "output_sink_write_attempts",
    "output_sink_failures",
];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
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

fn integer(value: &Value, label: &str) -> u64 {
    value
        .as_u64()
        .unwrap_or_else(|| panic!("{label} must be a nonnegative integer"))
}

fn exact_keys(value: &Value, expected: &[&str], label: &str) {
    let actual = object(value, label)
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(actual, expected, "{label} field set drifted");
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

fn assert_path_hash(workspace: &Path, record: &Value, expected_path: &str) {
    exact_keys(record, &["path", "sha256"], expected_path);
    assert_eq!(string(&record["path"], "path"), expected_path);
    assert_eq!(
        string(&record["sha256"], "sha256"),
        sha256(fs::read(workspace.join(expected_path)).unwrap())
    );
}

fn assert_recorded_path_hash(record: &Value, expected_path: &str, expected_sha256: &str) {
    exact_keys(record, &["path", "sha256"], expected_path);
    assert_eq!(string(&record["path"], "path"), expected_path);
    assert_eq!(string(&record["sha256"], "sha256"), expected_sha256);
}

fn assert_pairs(value: &Value, label: &str) {
    let pairs = array(value, label);
    assert_eq!(pairs.len(), 8, "{label}");
    for (ordinal, pair) in pairs.iter().enumerate() {
        exact_keys(pair, &["ordinal", "order", "base", "candidate"], label);
        assert_eq!(integer(&pair["ordinal"], label), ordinal as u64, "{label}");
        assert_eq!(
            string(&pair["order"], label),
            if ordinal % 2 == 0 { "ab" } else { "ba" },
            "{label}"
        );
    }
}

fn assert_activity(value: &Value, expected_positive: [u64; 12], label: &str) {
    exact_keys(value, &["positive", "runtime_slices"], label);
    exact_keys(&value["positive"], &POSITIVE_FIELDS, label);
    for (field, expected) in POSITIVE_FIELDS.into_iter().zip(expected_positive) {
        assert_eq!(
            integer(&value["positive"][field], field),
            expected,
            "{label}.{field}"
        );
    }
    exact_keys(&value["runtime_slices"], &RUNTIME_SLICES, label);
    for slice in RUNTIME_SLICES {
        assert_eq!(
            integer(&value["runtime_slices"][slice], slice),
            0,
            "{label}.{slice}"
        );
    }
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
fn h2_baseline_is_immutable_content_addressed_and_fingerprinted() {
    let workspace = workspace();
    let artifact: Value = serde_json::from_slice(RECORDED).expect("H2 baseline is JSON");
    exact_keys(
        &artifact,
        &[
            "schema",
            "kind",
            "status",
            "phase",
            "typescript",
            "generator",
            "contract",
            "authorities",
            "historical_lineage",
            "base",
            "candidate",
            "measured_at_utc",
            "runner",
            "toolchain",
            "policy",
            "sampling",
            "no_emit",
            "h1_emit",
            "l1_edit",
            "binary_startup",
            "canaries",
            "summary",
            "qualified",
            "evidence_fingerprint_sha256",
        ],
        "artifact",
    );
    assert_eq!(artifact["schema"], 1);
    assert_eq!(artifact["kind"], "h2-pre-runtime-baseline");
    assert_eq!(artifact["status"], "qualified");
    assert_eq!(artifact["phase"], "H2.0b");
    assert_eq!(artifact["qualified"], true);
    assert_eq!(artifact["base"]["commit"], BASE_COMMIT);
    assert_eq!(artifact["candidate"]["commit"], CANDIDATE_COMMIT);
    assert_path_hash(&workspace, &artifact["generator"], GENERATOR_PATH);
    assert_eq!(sha256(GENERATOR), artifact["generator"]["sha256"]);
    assert_path_hash(
        &workspace,
        &artifact["contract"],
        ".github/ci/contracts/h2-runtime-baseline.schema.json",
    );
    assert_eq!(sha256(CONTRACT), artifact["contract"]["sha256"]);

    let authorities = array(&artifact["authorities"], "authorities");
    assert_eq!(authorities.len(), AUTHORITIES.len());
    for (record, (expected_path, expected_sha256)) in authorities.iter().zip(AUTHORITIES) {
        assert_recorded_path_hash(record, expected_path, expected_sha256);
    }
    for (field, path, expected_sha256) in [
        (
            "h1_no_emit",
            "ratchets/h1-noemit-performance.v1.json",
            "452d2125fae0c386a7ced5fdcdb0ac91269bd29111013eb134e247ca6516303e",
        ),
        (
            "h1_emit",
            "ratchets/h1-emit-performance.v1.json",
            "33fa3f9710733c937d7b66327d5957575a5b94424df47a69036c1a6ee9fc0754",
        ),
        (
            "l1_edit",
            "ratchets/l1-incremental-parser-performance.v1.json",
            "05b4fdc0a7e50bfe05c722165d71ff47a5ba3e74f10647c0f4c9b5492d77ac5c",
        ),
    ] {
        assert_recorded_path_hash(
            &artifact["historical_lineage"][field],
            path,
            expected_sha256,
        );
    }
    assert_eq!(
        artifact["historical_lineage"]["interpretation"],
        "immutable-historical-lineage; current runtime ownership transfers to H2"
    );

    let mut semantic = artifact.clone();
    let recorded = semantic
        .as_object_mut()
        .unwrap()
        .remove("evidence_fingerprint_sha256")
        .unwrap();
    assert_eq!(
        string(&recorded, "fingerprint"),
        sha256(canonical(&semantic))
    );
}

#[test]
fn h2_baseline_freezes_every_cross_track_pair_and_resource_boundary() {
    let artifact: Value = serde_json::from_slice(RECORDED).unwrap();
    assert_eq!(artifact["sampling"]["pair_count"], 8);
    assert_eq!(artifact["sampling"]["warm_pair_count"], 7);
    assert_eq!(artifact["sampling"]["cold_pair_ordinal"], 0);
    assert_eq!(artifact["sampling"]["order"], "alternating-ab-ba");

    let no_emit = array(&artifact["no_emit"], "no_emit");
    assert_eq!(no_emit.len(), 3);
    assert_eq!(
        no_emit
            .iter()
            .map(|workload| string(&workload["id"], "workload id"))
            .collect::<Vec<_>>(),
        ["explicit-root", "project", "scale"]
    );
    for workload in no_emit {
        assert_pairs(&workload["pairs"], string(&workload["id"], "workload id"));
        assert_eq!(workload["qualified"], true);
        assert_eq!(workload["base_summary"]["max_output_writes"], 0);
        assert_eq!(workload["candidate_summary"]["max_output_writes"], 0);
    }
    assert_pairs(&artifact["h1_emit"]["pairs"], "h1_emit");
    assert_pairs(&artifact["l1_edit"]["pairs"], "l1_edit");
    assert_eq!(artifact["h1_emit"]["qualified"], true);
    assert_eq!(artifact["l1_edit"]["qualified"], true);
    assert_eq!(artifact["binary_startup"]["qualified"], true);
    assert_eq!(artifact["h1_emit"]["workload"]["source_files"], 3);
    assert_eq!(artifact["h1_emit"]["workload"]["expected_exit_code"], 2);
    assert_eq!(
        artifact["h1_emit"]["workload"]["expected_output"]["utf8_sha256"],
        "7325b73d3ff4bdb8012618dc4431a79661b9b42c99af4b788fadb05867a2eaef"
    );
    assert_eq!(
        artifact["l1_edit"]["workload"]["edit"]["after_sha256"],
        "f74be9b7a09832b9f247faf4e5f158fbca6914656e68641e9f67b8cc360c29a5"
    );

    exact_keys(
        &artifact["summary"],
        &[
            "no_emit_workloads",
            "h1_emit_cases",
            "l1_edit_workloads",
            "output_fault_observations",
            "h2_runtime_slices",
            "h2_runtime_activity",
            "runtime_admissions",
            "all_qualified",
        ],
        "summary",
    );
    assert_eq!(artifact["summary"]["no_emit_workloads"], 3);
    assert_eq!(artifact["summary"]["h1_emit_cases"], 1);
    assert_eq!(artifact["summary"]["l1_edit_workloads"], 1);
    assert_eq!(artifact["summary"]["output_fault_observations"], 2);
    assert_eq!(artifact["summary"]["h2_runtime_slices"], 37);
    assert_eq!(artifact["summary"]["h2_runtime_activity"], 0);
    assert_eq!(artifact["summary"]["runtime_admissions"], 0);
    assert_eq!(artifact["summary"]["all_qualified"], true);
}

#[test]
fn h2_activity_canaries_have_positive_h1_controls_and_zero_h2_admissions() {
    let artifact: Value = serde_json::from_slice(RECORDED).unwrap();
    let canaries = &artifact["canaries"];
    assert_eq!(
        array(&canaries["runtime_slice_order"], "runtime_slice_order")
            .iter()
            .map(|value| string(value, "runtime slice"))
            .collect::<Vec<_>>(),
        RUNTIME_SLICES
    );
    assert_eq!(canaries["runtime_activity_sum"], 0);
    assert_eq!(canaries["positive_controls_observed"], true);

    let no_emit = array(&canaries["no_emit"], "no-emit canaries");
    assert_eq!(no_emit.len(), 3);
    for canary in no_emit {
        assert_eq!(canary["exit_code"], 0);
        assert_eq!(canary["output_writes"], 0);
        assert_activity(&canary["h2_activity"], [0; 12], "no-emit canary");
    }
    assert_activity(
        &canaries["h1_emit"]["h2_activity"],
        [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0],
        "H1 emit canary",
    );
    assert_eq!(canaries["h1_emit"]["output_writes"], 1);
    assert_eq!(canaries["h1_emit"]["output"]["bytes"], 161);

    let faults = array(&canaries["output_faults"], "output faults");
    assert_eq!(faults.len(), 2);
    for (failed_index, fault) in faults.iter().enumerate() {
        assert_eq!(fault["failed_index"], failed_index as u64);
        assert_eq!(fault["diagnostics"][0]["code"], 5033);
        assert_eq!(fault["diagnostics"][0]["category"], "error");
        assert_eq!(array(&fault["emitted_files"], "emitted files").len(), 2);
        assert_eq!(
            array(&fault["filesystem_attempts"], "filesystem attempts").len(),
            3
        );
        assert_eq!(
            array(&fault["successful_files"], "successful files").len(),
            1
        );
        assert_activity(
            &fault["h2_activity"],
            [1, 1, 1, 2, 2, 2, 2, 2, 1, 2, 2, 1],
            "fault canary",
        );
    }
}

#[test]
fn h2_baseline_schema_is_strict_at_every_object_boundary() {
    let schema: Value = serde_json::from_slice(CONTRACT).expect("H2 baseline schema is JSON");
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(
        schema["$id"],
        "https://github.com/kazhiramatsu/tsc-rs/.github/ci/contracts/h2-runtime-baseline.schema.json"
    );
    assert_eq!(schema["additionalProperties"], false);
    let required = array(&schema["required"], "schema required")
        .iter()
        .map(|value| string(value, "required field"))
        .collect::<BTreeSet<_>>();
    let artifact: Value = serde_json::from_slice(RECORDED).unwrap();
    let artifact_keys = object(&artifact, "artifact")
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(required, artifact_keys);
    assert_strict_object_schemas(&schema, "$schema");
}

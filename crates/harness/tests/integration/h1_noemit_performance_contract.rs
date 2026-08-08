use std::collections::BTreeSet;

use serde_json::Value;
use sha2::{Digest, Sha256};

const RECORDED: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h1-noemit-performance.v1.json"
));
const CONTRACT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../.github/ci/contracts/h1-noemit-performance.schema.json"
));
const GENERATOR: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../oracle/h1-noemit-performance.mjs"
));
const TRUSTED_PRE_H1_COMMIT: &str = "c0951bf15cdec74223de29e06cd908b0899712f6";
const ACTIVITY_FIELDS: [&str; 8] = [
    "emit_resolver_constructions",
    "transformer_initializations",
    "transform_context_constructions",
    "emit_side_table_allocations",
    "printer_writer_constructions",
    "output_plan_constructions",
    "emit_artifact_creations",
    "output_sink_writes",
];

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn object<'a>(value: &'a Value, label: &str) -> &'a serde_json::Map<String, Value> {
    value
        .as_object()
        .unwrap_or_else(|| panic!("{label} must be an object"))
}

fn exact_keys(value: &Value, expected: &[&str], label: &str) {
    let actual = object(value, label)
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(actual, expected, "{label} field set drifted");
}

fn field<'a>(value: &'a Value, name: &str) -> &'a Value {
    value
        .get(name)
        .unwrap_or_else(|| panic!("missing field {name}"))
}

#[test]
fn frozen_h1_noemit_evidence_covers_every_workload_and_zero_canary() {
    let artifact: Value = serde_json::from_slice(RECORDED).expect("H1 no-emit artifact is JSON");
    exact_keys(
        &artifact,
        &[
            "schema",
            "kind",
            "status",
            "phase",
            "typescript_version",
            "base",
            "candidate",
            "generator",
            "contract",
            "fixture_manifest",
            "measured_at_utc",
            "runner",
            "toolchain",
            "sampling",
            "observed_variance",
            "trusted_runtime_parent",
            "absolute_h0_policy",
            "relative_regression_policy",
            "binary_size",
            "workloads",
        ],
        "artifact",
    );
    assert_eq!(field(&artifact, "schema"), 1);
    assert_eq!(field(&artifact, "kind"), "h1-noemit-performance");
    assert_eq!(field(&artifact, "status"), "qualified");
    assert_eq!(field(&artifact, "phase"), "H1.0b");
    assert_eq!(
        field(field(&artifact, "base"), "commit"),
        TRUSTED_PRE_H1_COMMIT
    );

    let generator = field(&artifact, "generator");
    assert_eq!(
        field(generator, "path"),
        "crates/oracle/h1-noemit-performance.mjs"
    );
    let generator_sha256 = sha256(GENERATOR);
    assert_eq!(
        field(generator, "sha256").as_str(),
        Some(generator_sha256.as_str())
    );
    let contract = field(&artifact, "contract");
    assert_eq!(
        field(contract, "path"),
        ".github/ci/contracts/h1-noemit-performance.schema.json"
    );
    let contract_sha256 = sha256(CONTRACT);
    assert_eq!(
        field(contract, "sha256").as_str(),
        Some(contract_sha256.as_str())
    );

    let sampling = field(&artifact, "sampling");
    assert_eq!(field(sampling, "pair_count"), 8);
    assert_eq!(field(sampling, "warm_pair_count"), 7);
    assert_eq!(field(sampling, "cold_pair_ordinal"), 0);
    assert_eq!(field(sampling, "order"), "alternating-ab-ba");
    assert_eq!(field(field(&artifact, "binary_size"), "qualified"), true);

    let workloads = field(&artifact, "workloads")
        .as_array()
        .expect("workloads are an array");
    assert_eq!(workloads.len(), 3);
    assert_eq!(
        workloads
            .iter()
            .map(|workload| field(workload, "id").as_str().expect("workload id"))
            .collect::<Vec<_>>(),
        ["explicit-root", "project", "scale"]
    );
    for workload in workloads {
        let id = field(workload, "id").as_str().expect("workload id");
        assert_eq!(field(workload, "qualified"), true, "{id}");
        let pairs = field(workload, "pairs")
            .as_array()
            .expect("pairs are an array");
        assert_eq!(pairs.len(), 8, "{id}");
        for (ordinal, pair) in pairs.iter().enumerate() {
            assert_eq!(field(pair, "ordinal"), ordinal, "{id}");
            assert_eq!(
                field(pair, "order"),
                if ordinal % 2 == 0 { "ab" } else { "ba" },
                "{id}"
            );
            let base = field(pair, "base");
            let candidate = field(pair, "candidate");
            assert_eq!(field(base, "output_writes"), 0, "{id}");
            assert!(field(base, "h1_no_emit").is_null(), "{id}");
            assert_eq!(field(candidate, "output_writes"), 0, "{id}");
            let activity = field(candidate, "h1_no_emit");
            exact_keys(activity, &ACTIVITY_FIELDS, "candidate activity");
            for activity_field in ACTIVITY_FIELDS {
                assert_eq!(field(activity, activity_field), 0, "{id}.{activity_field}");
            }
        }
        assert_eq!(
            field(field(workload, "candidate_summary"), "max_output_writes"),
            0,
            "{id}"
        );
    }
}

#[test]
fn historical_schema_and_generator_hashes_remain_frozen() {
    let schema: Value = serde_json::from_slice(CONTRACT).expect("H1 no-emit schema is JSON");
    assert_eq!(
        field(&schema, "$schema"),
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(field(&schema, "additionalProperties"), false);
    assert_eq!(
        field(&schema, "$id"),
        "https://github.com/kazhiramatsu/tsc-rs/.github/ci/contracts/h1-noemit-performance.schema.json"
    );
}

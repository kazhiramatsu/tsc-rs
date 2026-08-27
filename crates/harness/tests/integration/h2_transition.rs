use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

const OWNER_PATH: &str = "ratchets/h2-owner-inventory.v1.json";
const CANDIDATE_PATH: &str = "ratchets/h2-candidate-dispositions.v1.json";
const PROFILE_PATH: &str = "ratchets/h2-profile-transition.v1.json";

const H1_FROZEN: [(&str, &str); 5] = [
    (
        "ratchets/h1-owner-inventory.v1.json",
        "6148160678bf0b34a8310551eac8c9ab3f2afb1cd9260fa8eaa59efadc71abb5",
    ),
    (
        "ratchets/h1-rust-omissions.v1.json",
        "0412e9987343d7c2488081b95fd2a40ebf23e0fe1bce451be23215ca3bf6b12f",
    ),
    (
        "ratchets/h1-emit-profile.v1.json",
        "d7a7d212780ef94cb9675c104ec8d2ca28af95764fa78f8aeb8c7c25885fa7db",
    ),
    (
        "ratchets/h1-emit-oracle.v1.json",
        "c0c06a1472c2f49d9d90b733f3d594e737d62d350da9e4c8317d7e2331c0056d",
    ),
    (
        "ratchets/h1-emit-qualification.v1.json",
        "1467752c4a295e51380b9f6c974861ca689abea426312a1ef3702d7e44de4a13",
    ),
];

const SLICE_ORDER: [&str; 39] = [
    "H2.0a", "H2.0b", "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a", "H2.2b", "H2.2c",
    "H2.2d", "H2.3a", "H2.3b", "H2.3c", "H2.3d", "H2.4a", "H2.4b", "H2.5a", "H2.5b", "H2.5c",
    "H2.5d", "H2.5e", "H2.5f", "H2.5g", "H2.5h", "H2.6a", "H2.6b", "H2.6c", "H2.7a", "H2.7b",
    "H2.7c", "H2.7d", "H2.7e", "H2.8a", "H2.8b", "H2.8c", "H2.8d", "H2.8e", "H2.9",
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

fn file_sha256(workspace: &Path, relative: &str) -> String {
    sha256(fs::read(workspace.join(relative)).unwrap())
}

fn read_json(workspace: &Path, relative: &str) -> Value {
    serde_json::from_slice(&fs::read(workspace.join(relative)).unwrap()).unwrap()
}

fn object(value: &Value) -> &Map<String, Value> {
    value.as_object().unwrap()
}

fn array(value: &Value) -> &[Value] {
    value.as_array().unwrap()
}

fn string(value: &Value) -> &str {
    value.as_str().unwrap()
}

fn integer(value: &Value) -> u64 {
    value.as_u64().unwrap()
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
            let sorted = values.iter().collect::<BTreeMap<_, _>>();
            format!(
                "{{{}}}",
                sorted
                    .into_iter()
                    .map(|(key, value)| format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap(),
                        canonical(value)
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
    }
}

fn assert_fingerprint(manifest: &Value, field: &str) {
    let mut semantic = manifest.clone();
    let expected = semantic
        .as_object_mut()
        .unwrap()
        .remove(field)
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(expected, sha256(canonical(&semantic)));
}

fn assert_path_hash(workspace: &Path, record: &Value) {
    let record = object(record);
    assert_eq!(record.len(), 2);
    let relative = string(&record["path"]);
    assert_eq!(string(&record["sha256"]), file_sha256(workspace, relative));
}

#[test]
fn h2_transition_is_fresh_and_preserves_every_historical_h1_input() {
    let workspace = workspace();

    let owner = read_json(&workspace, OWNER_PATH);
    let candidates = read_json(&workspace, CANDIDATE_PATH);
    let profile = read_json(&workspace, PROFILE_PATH);
    assert_fingerprint(&owner, "inventory_fingerprint_sha256");
    assert_fingerprint(&candidates, "inventory_fingerprint_sha256");
    assert_fingerprint(&profile, "profile_fingerprint_sha256");

    for manifest in [&owner, &candidates, &profile] {
        assert_path_hash(&workspace, &manifest["generator"]);
        assert_path_hash(&workspace, &manifest["contract"]);
    }
    let h1_frozen_inputs = array(&profile["h1_frozen_inputs"]);
    assert_eq!(h1_frozen_inputs.len(), H1_FROZEN.len());
    for record in h1_frozen_inputs {
        let record = object(record);
        assert_eq!(record.len(), 2);
        let relative = string(&record["path"]);
        let expected = H1_FROZEN
            .iter()
            .find_map(|(path, sha256)| (*path == relative).then_some(*sha256))
            .unwrap_or_else(|| panic!("unexpected historical H1 input: {relative}"));
        assert_eq!(string(&record["sha256"]), expected, "{relative}");
    }
    assert_path_hash(&workspace, &profile["h2_inputs"]["owner_inventory"]);
    assert_path_hash(&workspace, &profile["h2_inputs"]["candidate_dispositions"]);
    assert_path_hash(
        &workspace,
        &profile["oracle_contracts"]["source_reachability"],
    );
    assert_path_hash(&workspace, &profile["oracle_contracts"]["emit_observation"]);
    assert_path_hash(&workspace, &profile["oracle_contracts"]["runtime_baseline"]);
}

#[test]
fn frozen_h2_owner_and_rust_converse_have_no_unassigned_rows() {
    let workspace = workspace();
    let manifest = read_json(&workspace, OWNER_PATH);
    assert_eq!(manifest["schema"], 1);
    assert_eq!(manifest["status"], "frozen");
    assert_eq!(manifest["phase"], "H2.0a-owner-converse-inventory");

    let summary = object(&manifest["summary"]);
    assert_eq!(integer(&summary["owner_roots"]), 50);
    assert_eq!(integer(&summary["closed_h1_roots"]), 1);
    assert_eq!(integer(&summary["partial_h1_roots"]), 22);
    assert_eq!(integer(&summary["deferred_h2_roots"]), 27);
    assert_eq!(integer(&summary["dependency_edges"]), 46);
    assert_eq!(integer(&summary["rust_converse_rows"]), 14);
    assert_eq!(integer(&summary["undispositioned_owners"]), 0);
    assert_eq!(integer(&summary["unmapped_rust_converse_rows"]), 0);

    let mut owner_keys = BTreeSet::new();
    let mut declaration_ids = BTreeSet::new();
    for owner in array(&manifest["owners"]) {
        let owner = object(owner);
        assert_eq!(owner.len(), 6);
        assert!(owner_keys.insert(string(&owner["key"]).to_owned()));
        assert!(matches!(
            string(&owner["disposition"]),
            "closed-h1" | "partial-h1-residual" | "deferred-h2"
        ));
        let declaration = object(&owner["declaration"]);
        assert!(declaration_ids.insert(string(&declaration["id"]).to_owned()));
        assert_eq!(string(&declaration["declaration_sha256"]).len(), 64);
        assert_eq!(string(&declaration["body_sha256"]).len(), 64);
        assert_eq!(string(&declaration["ledger_slice_sha256"]).len(), 64);
    }
    assert_eq!(owner_keys.len(), 50);

    let mut dependency_pairs = BTreeSet::new();
    for dependency in array(&manifest["dependencies"]) {
        let dependency = object(dependency);
        let from = string(&dependency["from"]);
        let to = string(&dependency["to"]);
        assert_ne!(from, to);
        assert!(owner_keys.contains(from));
        assert!(owner_keys.contains(to));
        assert!(dependency_pairs.insert((from.to_owned(), to.to_owned())));
        assert!(!array(&dependency["usage_kinds"]).is_empty());
        assert!(!array(&dependency["sites"]).is_empty());
    }
    assert_eq!(dependency_pairs.len(), 46);

    for row in array(&manifest["rust_converse"]) {
        let row = object(row);
        assert!(!array(&row["upstream_owners"]).is_empty());
        for upstream in array(&row["upstream_owners"]) {
            assert!(owner_keys.contains(string(upstream)));
        }
        let anchor = object(&row["anchor"]);
        assert!(workspace.join(string(&anchor["path"])).is_file());
        assert_eq!(string(&anchor["file_sha256"]).len(), 64);
        assert_eq!(string(&anchor["text_sha256"]).len(), 64);
        assert!(integer(&anchor["line"]) > 0);
    }
}

#[test]
fn every_h2_runner_case_has_an_exact_monotonic_disposition() {
    let workspace = workspace();
    let manifest = read_json(&workspace, CANDIDATE_PATH);
    assert_eq!(manifest["schema"], 1);
    assert_eq!(manifest["status"], "frozen");
    assert_eq!(manifest["phase"], "H2.0a-runner-candidate-dispositions");

    let summary = object(&manifest["summary"]);
    assert_eq!(integer(&summary["cases"]), 15_642);
    assert_eq!(integer(&summary["closed_h1_cases"]), 1);
    assert_eq!(integer(&summary["module_only_candidates"]), 295);
    assert_eq!(integer(&summary["module_only_compiler_candidates"]), 94);
    assert_eq!(integer(&summary["module_only_conformance_candidates"]), 201);
    assert_eq!(integer(&summary["not_run_cases"]), 15_641);
    assert_eq!(integer(&summary["undispositioned_cases"]), 0);

    let ranks = SLICE_ORDER
        .iter()
        .enumerate()
        .map(|(index, slice)| (*slice, index))
        .collect::<BTreeMap<_, _>>();
    let mut identities = BTreeSet::new();
    let mut suites = BTreeMap::<String, usize>::new();
    let mut module_only = BTreeMap::<String, usize>::new();
    let mut closed = Vec::new();
    for case in array(&manifest["cases"]) {
        let case = object(case);
        let suite = string(&case["suite"]);
        let id = string(&case["id"]);
        assert!(identities.insert((suite.to_owned(), id.to_owned())));
        *suites.entry(suite.to_owned()).or_default() += 1;

        let required = array(&case["required_slices"]);
        let mut previous = None;
        let mut unique = BTreeSet::new();
        for slice in required {
            let slice = string(slice);
            let rank = ranks[slice];
            assert!(unique.insert(slice));
            assert!(previous.is_none_or(|previous| previous < rank));
            previous = Some(rank);
        }
        if string(&case["disposition"]) == "closed-h1-exact" {
            assert!(required.is_empty());
            assert!(case["next_slice"].is_null());
            assert_eq!(string(&case["execution_state"]), "executed-h1-exact");
            closed.push(id.to_owned());
        } else {
            assert!(!required.is_empty());
            assert_eq!(string(&case["next_slice"]), string(&required[0]));
            assert_eq!(string(&case["execution_state"]), "not-run");
            assert_eq!(string(&case["reference_baseline_state"]), "not-compared");
        }

        if string(&case["candidate_class"]) == "h2.1a-module-only" {
            assert!(matches!(suite, "compiler" | "conformance"));
            let blockers = array(&case["profile_blockers"]);
            assert_eq!(blockers.len(), 1);
            let blocker = string(&blockers[0]);
            assert!(matches!(
                blocker,
                "required-option:module=absent" | "required-option:module=ESNext(99)"
            ));
            assert_eq!(string(&case["disposition"]), "pending-source-analysis");
            assert_eq!(
                string(&case["source_analysis_state"]),
                "pending-owning-slice"
            );
            assert_eq!(string(&case["next_slice"]), "H2.1a");
            *module_only.entry(suite.to_owned()).or_default() += 1;
        }
    }
    assert_eq!(identities.len(), 15_642);
    assert_eq!(
        suites,
        BTreeMap::from([
            ("compiler".to_owned(), 7_276),
            ("conformance".to_owned(), 7_697),
            ("project".to_owned(), 632),
            ("transpile".to_owned(), 37),
        ])
    );
    assert_eq!(
        module_only,
        BTreeMap::from([("compiler".to_owned(), 94), ("conformance".to_owned(), 201),])
    );
    assert_eq!(
        closed,
        ["typescript-6.0.3/compiler/esmNoSynthesizedDefault.ts#module%3Dpreserve"]
    );
}

#[test]
fn profile_transition_closes_h2_0b_and_selects_h2_1a() {
    let workspace = workspace();
    let manifest = read_json(&workspace, PROFILE_PATH);
    assert_eq!(manifest["schema"], 1);
    assert_eq!(manifest["status"], "frozen-pre-runtime-baseline");
    assert_eq!(manifest["phase"], "H2.0b-baseline-transition");
    assert_eq!(manifest["summary"]["transition_rows"], 39);
    assert_eq!(manifest["summary"]["completed_rows"], 2);
    assert_eq!(manifest["summary"]["next_rows"], 1);
    assert_eq!(manifest["summary"]["planned_rows"], 36);
    assert_eq!(manifest["summary"]["runtime_admissions"], 0);

    let transitions = array(&manifest["transitions"]);
    assert_eq!(
        transitions
            .iter()
            .map(|row| string(&row["slice"]))
            .collect::<Vec<_>>(),
        SLICE_ORDER
    );
    assert_eq!(transitions[0]["state"], "complete-evidence-only");
    assert_eq!(transitions[1]["state"], "complete-evidence-only");
    assert_eq!(transitions[2]["state"], "next");
    assert!(transitions[3..].iter().all(|row| row["state"] == "planned"));
    assert_eq!(
        manifest["first_runtime_candidate"]["status"],
        "not-admitted"
    );
    assert_eq!(manifest["first_runtime_candidate"]["candidate_cases"], 295);
    assert_eq!(
        manifest["admission_contract"]["h1_evidence_reuse"],
        "forbidden-outside-exact-h1-profile"
    );
}

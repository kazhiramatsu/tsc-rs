//! Original D283 + E-only8 and the 23 H2.8a directory comparisons over one frozen TS join.
//! Focused controls and the compiler test's CLI checks are not hosted entries.
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::path::Path;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

#[path = "../../compiler/tests/integration/h2_7d_original_corpus_shared.rs"]
mod h2_7d_original_corpus_shared;
#[path = "../../compiler/tests/integration/h2_7e_original_corpus_shared.rs"]
mod h2_7e_original_corpus_shared;

const QUALIFICATION: &str = "ratchets/h2-7de-qualification.v1.json";
const CENSUS: &str = "ratchets/h2-7de-candidates.v1.json";
const INPUTS: &str = "ratchets/h2-7de-candidate-inputs.v1.json";
const OBSERVATIONS: &str = "ratchets/h2-7de-observations.v1.json";

pub fn run_h2_7de(workspace: &Path) -> Result<(), Box<dyn Error>> {
    if workspace.canonicalize()?
        != Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()?
    {
        return Err("H2.7d/e runner must use the compiled workspace".into());
    }
    let artifact: Value = serde_json::from_slice(&std::fs::read(workspace.join(QUALIFICATION))?)?;
    let (expected_d, expected_e) = validate_artifact(workspace, &artifact)?;
    // Each comparator constructs fresh Programs twice. A failed D rung must not
    // prevent the independent E rung from reporting its own original failures.
    let d = std::panic::catch_unwind(|| {
        h2_7d_original_corpus_shared::assert_original_corpus(workspace)
    });
    let e = std::panic::catch_unwind(|| {
        h2_7e_original_corpus_shared::assert_original_corpus(workspace)
    });
    let directories = std::panic::catch_unwind(|| {
        h2_7d_original_corpus_shared::assert_output_directory_references(workspace)
    });
    let d = checked(d, "D283")?;
    let e = checked(e, "E-only8")?;
    let directories = checked(directories, "H2.8a directories")?;
    if d != expected_d || e != expected_e || !d.is_disjoint(&e) || d.union(&e).count() != 291 {
        return Err("H2.7d/e production coverage differs from the qualified original union".into());
    }
    if directories.len() != 23 || !directories.is_disjoint(&d) || !directories.is_disjoint(&e) {
        return Err("H2.8a directory supplement differs from its 23 original IDs".into());
    }
    println!("H2.7d/e original corpus with H2.8a: D283 + E-only8 + 23 directory migrations = 314 exact IDs, twice; 11 later references, including 2 unobserved transpile APIs; historical 291-row qualification unchanged; no inherited successes");
    Ok(())
}

fn checked(
    result: std::thread::Result<BTreeSet<String>>,
    band: &str,
) -> Result<BTreeSet<String>, Box<dyn Error>> {
    result.map_err(|error| {
        let detail = error
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| error.downcast_ref::<&str>().copied())
            .unwrap_or("non-string panic");
        format!("H2.7d/e {band} acceptance failed: {detail}").into()
    })
}

fn digest(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}

fn referenced_row<'a>(
    id: &str,
    reference: &Value,
    artifact: &'a Value,
    label: &str,
) -> Result<&'a Value, Box<dyn Error>> {
    let index = usize::try_from(reference["index"].as_u64().ok_or("missing row index")?)?;
    let row = artifact["cases"]
        .as_array()
        .and_then(|rows| rows.get(index))
        .ok_or("qualification row index is out of range")?;
    // The existing program/checker/harness dependencies enable serde_json's
    // preserve_order feature, as required by the C qualification fingerprint.
    if row["case_id"] != id || reference["sha256"] != digest(serde_json::to_vec(row)?) {
        return Err(format!("H2.7d/e {id}: {label} identity changed").into());
    }
    Ok(row)
}

fn validate_artifact(
    workspace: &Path,
    artifact: &Value,
) -> Result<(BTreeSet<String>, BTreeSet<String>), Box<dyn Error>> {
    if artifact["schema"] != 1
        || artifact["kind"] != "h2-7de-qualification"
        || artifact["status"] != "qualified-typescript-oracle"
        || artifact["typescript"] != "6.0.3"
        || artifact["source_commit"] != "050880ce59e30b356b686bd3144efe24f875ebc8"
        || artifact["repetitions"] != 2
    {
        return Err("invalid H2.7d/e qualification identity".into());
    }
    let mut payload = artifact.clone();
    let fingerprint = payload
        .as_object_mut()
        .ok_or("qualification must be an object")?
        .remove("qualification_fingerprint_sha256")
        .ok_or("qualification fingerprint missing")?;
    if fingerprint != digest(serde_json::to_vec(&payload)?) {
        return Err("H2.7d/e qualification fingerprint mismatch".into());
    }
    if artifact["summary"]
        != json!({
            "union":{"candidates":325,"eligible":291,"deferred":34},
            "bands":{"H2.7d":{"candidates":315,"eligible":283,"deferred":32},
                "H2.7e":{"candidates":13,"eligible":11,"deferred":2}},
            "intersection":{"candidates":3,"eligible":3,"deferred":0},
            "exclusive_groups":{"d_only":{"candidates":312,"eligible":280,"deferred":32},
                "e_only":{"candidates":10,"eligible":8,"deferred":2},
                "d_and_e":{"candidates":3,"eligible":3,"deferred":0}},
            "observed_whole_program":323,"observed_eligible":291,"observed_deferred":32,
            "unobserved_transpile_references":2,
            "later_by_owner":{"H2.8a":23,"H2.8b":5,"H2.8c":2,"H2.9":4}
        })
    {
        return Err("H2.7d/e qualification denominator changed".into());
    }
    if artifact["generator"]["path"] != "crates/oracle/h2-7de-qualification.mjs"
        || artifact["contract"]["path"] != ".github/ci/contracts/h2-7de-qualification.schema.json"
    {
        return Err("H2.7d/e qualification producer path changed".into());
    }
    let inputs = artifact["inputs"]
        .as_array()
        .ok_or("qualification inputs missing")?;
    let expected_paths = BTreeSet::from([
        CENSUS,
        INPUTS,
        OBSERVATIONS,
        "crates/oracle/h2-7de-candidates.mjs",
        "crates/oracle/h2-7de-observations.mjs",
        "crates/oracle/vfs-directory-overlay.mjs",
        "vendor/typescript-6.0.3/lib/typescript.js",
        "vendor/typescript-6.0.3/lib/_tsc.js",
        ".node-version",
    ]);
    let actual_paths = inputs
        .iter()
        .map(|pin| pin["path"].as_str().ok_or("input path missing"))
        .collect::<Result<BTreeSet<_>, _>>()?;
    if inputs.len() != 9 || actual_paths != expected_paths {
        return Err("H2.7d/e qualification input set changed".into());
    }
    for pin in inputs
        .iter()
        .chain([&artifact["generator"], &artifact["contract"]])
    {
        let name = pin["path"].as_str().ok_or("input identity path missing")?;
        if pin["sha256"] != digest(std::fs::read(workspace.join(name))?) {
            return Err(format!("H2.7d/e input changed: {name}").into());
        }
    }
    let mut frozen = BTreeMap::new();
    for (name, hash) in [
        (
            CENSUS,
            "1af6d75acf8212135a0850c5ff09487a5589de4d0f825ff1f0e9bc8e3f0f141d",
        ),
        (
            INPUTS,
            "f2e078a6b6d10cd3c6df833584924c18e8f78fe98c1e41621c70e10e215a073a",
        ),
        (
            OBSERVATIONS,
            "1a1681b2375d27d9012b06e29808aca72aa3e39d1dbc1536b80ba2aadf9e8ce2",
        ),
    ] {
        let bytes = std::fs::read(workspace.join(name))?;
        if digest(&bytes) != hash {
            return Err(format!("H2.7d/e frozen source changed: {name}").into());
        }
        frozen.insert(name, serde_json::from_slice::<Value>(&bytes)?);
    }
    let cases = artifact["cases"]
        .as_array()
        .ok_or("qualification cases missing")?;
    if cases.len() != 325 {
        return Err("H2.7d/e union length changed".into());
    }
    let (mut seen, mut d, mut e) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    let (mut deferred, mut observed, mut unobserved) = (0, 0, 0);
    for case in cases {
        let id = case["case_id"].as_str().ok_or("case ID missing")?;
        if !seen.insert(id) {
            return Err(format!("duplicate H2.7d/e case {id}").into());
        }
        let candidate = referenced_row(id, &case["candidate"], &frozen[CENSUS], "candidate")?;
        let input = referenced_row(id, &case["input"], &frozen[INPUTS], "input")?;
        let required = candidate["required_slices"]
            .as_array()
            .ok_or("required owners missing")?;
        let bands = ["H2.7d", "H2.7e"]
            .into_iter()
            .filter(|band| required.contains(&json!(band)))
            .collect::<Vec<_>>();
        let remaining = required
            .iter()
            .filter(|owner| **owner != "H2.7d" && **owner != "H2.7e")
            .cloned()
            .collect::<Vec<_>>();
        let whole = input["input"]["route"] == "whole-program";
        let eligible = whole && remaining.is_empty();
        if case["suite"] != candidate["suite"]
            || case["source"] != candidate["source"]
            || case["source"] != input["source"]
            || case["required_slices"] != candidate["required_slices"]
            || case["bands"] != json!(bands)
            || case["remaining_slices"] != json!(remaining)
            || case["input_route"] != input["input"]["route"]
            || case["input"]["sha256"] != candidate["input_sha256"]
            || case["disposition"]
                != if eligible {
                    "eligible-for-rust-comparison"
                } else {
                    "deferred"
                }
        {
            return Err(format!("H2.7d/e {id}: membership or input projection changed").into());
        }
        if whole {
            let observation = referenced_row(
                id,
                &case["observation"],
                &frozen[OBSERVATIONS],
                "observation",
            )?;
            if observation["input_sha256"] != candidate["input_sha256"]
                || observation["required_slices"] != candidate["required_slices"]
                || observation["repetitions"] != 2
                || case["observation"]["repetitions"] != 2
                || case["observation"]["typescript_observation_sha256"]
                    != digest(serde_json::to_vec(&observation["typescript_observation"])?)
            {
                return Err(format!("H2.7d/e {id}: complete observation changed").into());
            }
            observed += 1;
        } else {
            if input["input"]["route"] != "transpile-api"
                || !case["observation"].is_null()
                || case["remaining_slices"] != json!(["H2.8c"])
            {
                return Err(format!("H2.7d/e {id}: unobserved transpile boundary changed").into());
            }
            unobserved += 1;
        }
        if eligible {
            if bands.contains(&"H2.7d") {
                d.insert(id.to_owned());
            } else if bands == ["H2.7e"] {
                e.insert(id.to_owned());
            } else {
                return Err(format!("H2.7d/e {id}: unknown eligible band").into());
            }
        } else {
            deferred += 1;
        }
    }
    if (d.len(), e.len(), deferred, observed, unobserved) != (283, 8, 34, 323, 2) {
        return Err("H2.7d/e joined coverage changed".into());
    }
    Ok((d, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qualification() -> (std::path::PathBuf, Value) {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let artifact = serde_json::from_slice(
            &std::fs::read(workspace.join(QUALIFICATION)).expect("frozen qualification"),
        )
        .expect("qualification JSON");
        (workspace, artifact)
    }

    fn repin(artifact: &mut Value) {
        artifact
            .as_object_mut()
            .unwrap()
            .remove("qualification_fingerprint_sha256");
        artifact["qualification_fingerprint_sha256"] =
            json!(digest(serde_json::to_vec(artifact).unwrap()));
    }

    #[test]
    fn frozen_qualification_selects_the_original_291_ids() {
        let (workspace, artifact) = qualification();
        let (d, e) = validate_artifact(&workspace, &artifact).unwrap();
        assert_eq!((d.len(), e.len()), (283, 8));
        assert!(d.is_disjoint(&e));
        assert_eq!(d.union(&e).count(), 291);
        assert!(
            e.contains("typescript-6.0.3/compiler/declarationMapsWithoutDeclaration.ts#default")
        );
    }

    #[test]
    fn qualification_rejects_fingerprint_and_repinned_identity_changes() {
        let (workspace, original) = qualification();
        let eligible = original["cases"]
            .as_array()
            .unwrap()
            .iter()
            .position(|case| case["disposition"] == "eligible-for-rust-comparison")
            .unwrap();
        let transpile = original["cases"]
            .as_array()
            .unwrap()
            .iter()
            .position(|case| case["input_route"] == "transpile-api")
            .unwrap();

        let mut fingerprint = original.clone();
        fingerprint["qualification_fingerprint_sha256"] = json!("0".repeat(64));
        assert!(validate_artifact(&workspace, &fingerprint)
            .unwrap_err()
            .to_string()
            .contains("fingerprint mismatch"));

        let mut input = original.clone();
        input["cases"][eligible]["input"]["sha256"] = json!("0".repeat(64));
        let mut tuple = original.clone();
        tuple["cases"][eligible]["observation"]["typescript_observation_sha256"] =
            json!("0".repeat(64));
        let mut promoted = original.clone();
        promoted["cases"][transpile]["disposition"] = json!("eligible-for-rust-comparison");
        promoted["cases"][transpile]["remaining_slices"] = json!([]);
        let mut denominator = original.clone();
        denominator["summary"]["union"]["eligible"] = json!(292);
        for (mut artifact, reason) in [
            (input, "input identity changed"),
            (tuple, "complete observation changed"),
            (promoted, "membership or input projection changed"),
            (denominator, "denominator changed"),
        ] {
            repin(&mut artifact);
            let error = validate_artifact(&workspace, &artifact).unwrap_err();
            assert!(error.to_string().contains(reason), "{reason}: {error}");
        }
    }
}

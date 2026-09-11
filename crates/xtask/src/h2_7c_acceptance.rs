//! H2.7c uses the same production-entry comparators as the compiler contracts.
//! Keep these in the existing xtask binary so hosted acceptance needs no second
//! Cargo build or test profile. Original corpus and focused controls are counted
//! separately; later-slice refusals never count as exact comparisons.

use std::error::Error;
use std::path::Path;

use serde_json::Value;
use sha2::{Digest, Sha256};

// The historical comparator also contains earlier packet controls. Only its
// observation comparison is shared by this acceptance rung.
#[allow(dead_code)]
#[path = "../../compiler/tests/integration/h2_7b_w4a_controls.rs"]
mod h2_7b_w4a_controls;
#[path = "../../compiler/tests/integration/h2_7c_corpus.rs"]
mod h2_7c_corpus;
#[path = "../../compiler/tests/integration/h2_7c_declaration_blocking.rs"]
mod h2_7c_declaration_blocking;
#[path = "../../compiler/tests/integration/h2_7c_declaration_getters.rs"]
mod h2_7c_declaration_getters;
#[path = "../../compiler/tests/integration/h2_7c_forced_declarations.rs"]
mod h2_7c_forced_declarations;
#[path = "../../compiler/tests/integration/h2_7c_strip_internal.rs"]
mod h2_7c_strip_internal;

pub fn run(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let artifact: Value = serde_json::from_slice(&std::fs::read(
        workspace.join("ratchets/h2-7c-qualification.v1.json"),
    )?)?;
    validate_artifact(workspace, &artifact)?;
    check_result(std::panic::catch_unwind(|| {
        h2_7c_corpus::assert_corpus(&artifact)
    }))?;
    println!("H2.7c corpus: 42 candidates, 32 exact (1 H2.8a rootDir migration), 10 deferred, repetitions=2");
    Ok(())
}

pub fn run_owner_controls(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let compared = std::panic::catch_unwind(|| {
        h2_7c_strip_internal::assert_strip_internal();
        for (name, count) in [
            ("declaration-blocking", 22),
            ("isolated-declaration-inference", 12),
            ("isolated-declaration-parameters", 21),
            ("isolated-declaration-accessors", 18),
            ("isolated-declaration-enums", 21),
            ("isolated-declaration-expando-augmentation", 27),
            ("isolated-declaration-private-types", 18),
            ("declaration-dir", 34),
        ] {
            let fixture: Value = serde_json::from_slice(
                &std::fs::read(
                    workspace.join(format!("crates/compiler/tests/fixtures/{name}.json")),
                )
                .expect("H2.7c focused observations"),
            )
            .expect("H2.7c focused JSON");
            assert_eq!(fixture["typescript"], "6.0.3");
            assert_eq!(fixture["repetitions"], 2);
            assert_eq!(fixture["cases"].as_array().unwrap().len(), count);
            h2_7c_declaration_blocking::assert_cases(&fixture);
            eprintln!("H2.7c {name}: {count} cases x 2 PASS");
        }
        h2_7c_declaration_getters::assert_declaration_getters();
        use h2_7c_forced_declarations::SourceFamily;
        for family in [
            SourceFamily::TypeScriptAndJavaScript,
            SourceFamily::Json,
            SourceFamily::Empty,
        ] {
            h2_7c_forced_declarations::assert_cases(family);
        }
    });
    check_result(compared)?;
    println!("H2.7c focused: 244 cases, 244 exact (3 H2.8a outDir migrations), repetitions=2; 37 ordinary API references remain H2.8d");
    Ok(())
}

fn check_result(compared: std::thread::Result<()>) -> Result<(), Box<dyn Error>> {
    if let Err(error) = compared {
        let detail = error
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| error.downcast_ref::<&str>().copied())
            .unwrap_or("non-string panic");
        return Err(format!("H2.7c acceptance failed: {detail}").into());
    }
    Ok(())
}

fn validate_artifact(workspace: &Path, artifact: &Value) -> Result<(), Box<dyn Error>> {
    if artifact["kind"] != "h2-7c-qualification"
        || artifact["status"] != "qualified-typescript-oracle"
        || artifact["repetitions"] != 2
    {
        return Err("invalid H2.7c qualification identity".into());
    }
    let mut payload = artifact.clone();
    let expected = payload
        .as_object_mut()
        .ok_or("H2.7c artifact must be an object")?
        .remove("qualification_fingerprint_sha256")
        .ok_or("missing H2.7c fingerprint")?;
    let actual = format!("{:x}", Sha256::digest(serde_json::to_vec(&payload)?));
    if expected != actual {
        return Err("H2.7c qualification fingerprint mismatch".into());
    }
    let mut identities = vec![&artifact["generator"], &artifact["contract"]];
    identities.extend(artifact["inputs"].as_array().ok_or("H2.7c inputs")?);
    for focused in artifact["focused"]
        .as_array()
        .ok_or("H2.7c focused manifest")?
    {
        identities.extend([&focused["observer"], &focused["fixture"]]);
    }
    for identity in identities {
        let path = identity["path"].as_str().ok_or("H2.7c input path")?;
        let actual = format!("{:x}", Sha256::digest(std::fs::read(workspace.join(path))?));
        if identity["sha256"] != actual {
            return Err(format!("H2.7c input changed: {path}").into());
        }
    }
    Ok(())
}

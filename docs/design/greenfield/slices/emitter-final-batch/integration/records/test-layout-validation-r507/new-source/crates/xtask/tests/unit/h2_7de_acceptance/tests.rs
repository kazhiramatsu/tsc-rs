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
    assert!(e.contains("typescript-6.0.3/compiler/declarationMapsWithoutDeclaration.ts#default"));
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

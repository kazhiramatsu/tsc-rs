//! Registry mutations exercise all current exact originals. Historical refusal
//! records stay archived and cannot authorize current refusal or vector changes.
use super::*;
use h2_6c_refusal_migrations::CaseOutcome;

const RETIRED_HOST: &str =
    "typescript-6.0.3/compiler/sourceMapWithNonCaseSensitiveFileNames.ts#default";
const RETIRED_ISOLATED: &str = "typescript-6.0.3/compiler/isolatedModulesSourceMap.ts#default";

struct Records {
    workspace: PathBuf,
    cases: Vec<Value>,
    listed: H2LoadedVectorManifest,
    inputs: H2_6cExecutionInputs,
}

impl Records {
    fn load() -> Self {
        assert!(h2_6c_de_promotions::promoted_count() > 0);
        assert!(h2_6c_output_promotions::promoted_count() > 0);
        // Explicit current-state guard, not a conditional skip of old tests.
        assert_eq!(h2_6c_refusal_migrations::count(), 0);
        assert!(h2_6c_de_promotions::find(RETIRED_HOST).is_some());
        assert!(h2_6c_output_promotions::find(RETIRED_ISOLATED).is_some());
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let artifact: Value = serde_json::from_slice(
            &fs::read(workspace.join(H2_6C_QUALIFICATION_RELATIVE_PATH)).unwrap(),
        )
        .unwrap();
        let cases = validate_h2_6c_qualification(&artifact).unwrap().to_vec();
        let listed = load_h2_6c_divergence_manifest_state(&workspace, false).unwrap();
        assert!(listed.entries.is_empty());
        let inputs = H2_6cExecutionInputs::load(&workspace).unwrap();
        h2_6c_de_promotions::validate(&workspace, &cases, &inputs.h2_7b_expected_members).unwrap();
        h2_6c_output_promotions::validate(&cases, &inputs.h2_7b_expected_members).unwrap();
        h2_6c_refusal_migrations::validate(&workspace, &cases, &listed).unwrap();
        Self {
            workspace,
            cases,
            listed,
            inputs,
        }
    }

    fn results(&self) -> Vec<Result<CaseOutcome, String>> {
        self.cases
            .iter()
            .map(|case| {
                let id = case["case_id"].as_str().unwrap();
                let promotion = h2_6c_de_promotions::find(id);
                let output_promotion = h2_6c_output_promotions::find(id);
                Ok(h2_6c_refusal_migrations::compared(H2VectorCaseOutcome {
                    case_id: id.to_owned(),
                    deferred: case["disposition"] == "deferred-to-slices",
                    h2_7b_activity: promotion
                        .map(|row| row.declaration_members)
                        .or_else(|| output_promotion.map(|row| row.declaration_members))
                        .unwrap_or_else(|| {
                            self.inputs
                                .h2_7b_expected_members
                                .get(id)
                                .copied()
                                .unwrap_or(0)
                        }),
                    divergence: H2VectorDivergence::default(),
                }))
            })
            .collect()
    }

    fn promotion_id(&self) -> &str {
        self.cases
            .iter()
            .map(|case| case["case_id"].as_str().unwrap())
            .find(|id| h2_6c_de_promotions::find(id).is_some())
            .unwrap()
    }
}

#[test]
fn populated_registry_identity_and_retired_refusals_reject_mutations() {
    let records = Records::load();
    for id in [RETIRED_HOST, RETIRED_ISOLATED, records.promotion_id()] {
        assert!(!h2_6c_refusal_migrations::registered(id));
        for field in ["observation_input_sha256", "case_fingerprint_sha256"] {
            let mut cases = records.cases.clone();
            cases.iter_mut().find(|case| case["case_id"] == id).unwrap()[field] = json!("changed");
            let rejected = if id == RETIRED_ISOLATED {
                h2_6c_output_promotions::validate(&cases, &records.inputs.h2_7b_expected_members)
                    .is_err()
            } else {
                h2_6c_de_promotions::validate(
                    &records.workspace,
                    &cases,
                    &records.inputs.h2_7b_expected_members,
                )
                .is_err()
            };
            assert!(rejected, "{id}: {field} mutation must fail");
        }
    }
    let (ordinary, migrations) =
        h2_6c_refusal_migrations::partition(&records.cases, records.results()).unwrap();
    assert!(migrations.is_empty());
    for (id, old_option) in [
        (RETIRED_HOST, "useCaseSensitiveFileNames"),
        (RETIRED_ISOLATED, "isolatedModules"),
    ] {
        let mut changed = ordinary.clone();
        let row = changed
            .iter_mut()
            .find(|result| result.as_ref().unwrap().case_id == id)
            .unwrap()
            .as_mut()
            .unwrap();
        row.divergence = vectorize_refusal(H2MismatchProfile::H2_6c, old_option);
        assert!(h2_6c_refusal_migrations::validate_ordinary_results(
            &changed,
            &records.listed.entries
        )
        .is_err());
        if id == RETIRED_HOST {
            assert!(
                h2_6c_de_promotions::validate_results(&changed, &records.listed.entries).is_err()
            );
        } else {
            assert!(
                h2_6c_output_promotions::validate_results(&changed, &records.listed.entries)
                    .is_err()
            );
        }
    }
}

#[test]
fn populated_partition_rejects_missing_duplicate_and_unknown_originals() {
    let records = Records::load();
    assert!(h2_6c_refusal_migrations::partition(&records.cases, records.results()).is_ok());
    for id in [RETIRED_HOST, records.promotion_id()] {
        let index = records
            .cases
            .iter()
            .position(|case| case["case_id"] == id)
            .unwrap();
        let mut missing = records.results();
        missing.remove(index);
        assert!(h2_6c_refusal_migrations::partition(&records.cases, missing).is_err());
        let mut duplicate = records.results();
        duplicate.push(records.results().remove(index));
        assert!(h2_6c_refusal_migrations::partition(&records.cases, duplicate).is_err());
    }
    let mut unknown = records.results();
    let CaseOutcome::Compared(row) = unknown[0].as_mut().unwrap() else {
        unreachable!()
    };
    row.case_id = "unknown-exact-original".to_owned();
    assert!(h2_6c_refusal_migrations::partition(&records.cases, unknown).is_err());
    let mut duplicate_cases = records.cases.clone();
    duplicate_cases.push(records.cases[0].clone());
    assert!(h2_6c_refusal_migrations::partition(&duplicate_cases, records.results()).is_err());
}

#[test]
fn populated_exact_partition_has_no_refusals_and_keeps_manifest_absent() {
    let records = Records::load();
    let (ordinary, migrations) =
        h2_6c_refusal_migrations::partition(&records.cases, records.results()).unwrap();
    assert!(migrations.is_empty());
    let listed = h2_6c_refusal_migrations::ordinary_manifest(&records.listed);
    h2_6c_refusal_migrations::validate_ordinary_results(&ordinary, &listed).unwrap();
    h2_6c_de_promotions::validate_results(&ordinary, &listed).unwrap();
    h2_6c_output_promotions::validate_results(&ordinary, &listed).unwrap();
    assert!(h2_6c_refused_option_totals(&ordinary, &migrations)
        .unwrap()
        .is_empty());
    let (exact, deferred, diverging) =
        h2_vector_ratchet_join("H2.6c", ordinary, &listed, true).unwrap();
    let retained =
        h2_6c_refusal_migrations::retained_manifest(&records.listed, &diverging).unwrap();
    assert!(retained.is_empty());
    assert_eq!(exact + deferred, records.cases.len() as u64);
    report_h2_6c_suite_outcomes(&records.cases, &diverging, &migrations).unwrap();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "tsrs-registry-roundtrip-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(directory.join("ratchets")).unwrap();
    write_h2_6c_divergence_manifest(&directory, &retained).unwrap();
    assert!(!directory
        .join(H2_6C_KNOWN_DIVERGENCES_RELATIVE_PATH)
        .exists());
    assert!(load_h2_6c_divergence_manifest_state(&directory, false)
        .unwrap()
        .entries
        .is_empty());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn populated_exact_registry_rejects_new_vectors_deferred_and_duplicate_results() {
    let records = Records::load();
    let (ordinary, _) =
        h2_6c_refusal_migrations::partition(&records.cases, records.results()).unwrap();
    let listed = h2_6c_refusal_migrations::ordinary_manifest(&records.listed);
    h2_6c_de_promotions::validate_results(&ordinary, &listed).unwrap();
    h2_6c_output_promotions::validate_results(&ordinary, &listed).unwrap();
    let id = records.promotion_id();
    let index = ordinary
        .iter()
        .position(|result| result.as_ref().unwrap().case_id == id)
        .unwrap();
    let mut missing = ordinary.clone();
    let _ = missing.remove(index);
    assert!(h2_6c_de_promotions::validate_results(&missing, &listed).is_err());
    let mut duplicate = ordinary.clone();
    duplicate.push(ordinary[index].clone());
    assert!(h2_6c_de_promotions::validate_results(&duplicate, &listed).is_err());
    let mut deferred = ordinary.clone();
    deferred[index].as_mut().unwrap().deferred = true;
    assert!(h2_6c_de_promotions::validate_results(&deferred, &listed).is_err());
    let mut refused = ordinary.clone();
    refused[index].as_mut().unwrap().divergence =
        vectorize_refusal(H2MismatchProfile::H2_6c, "outDir");
    assert!(h2_6c_de_promotions::validate_results(&refused, &listed).is_err());
    let mut replaced = listed.clone();
    replaced.insert(
        id.to_owned(),
        vectorize_refusal(H2MismatchProfile::H2_6c, "outDir"),
    );
    assert!(
        h2_6c_de_promotions::validate_results(&ordinary, &replaced).is_err(),
        "cannot invent a different previous vector"
    );
    let mut changed = ordinary.clone();
    let other = changed
        .iter()
        .position(|result| {
            let row = result.as_ref().unwrap();
            !row.deferred && h2_6c_de_promotions::find(&row.case_id).is_none()
        })
        .unwrap();
    changed[other].as_mut().unwrap().divergence =
        vectorize_refusal(H2MismatchProfile::H2_6c, "unregistered-new-refusal");
    assert!(h2_6c_refusal_migrations::validate_ordinary_results(&changed, &listed).is_err());
    assert!(
        h2_6c_de_promotions::validate_results(&changed, &listed).is_err(),
        "write mode has no exemption for a changed residual"
    );
    assert!(!h2_6c_de_promotions::pinned_request(
        "unknown-exact-original",
        H2RuntimeSlice::H2_7d
    ));
}

#[test]
fn original_output_registry_rejects_identity_activity_and_result_mutations() {
    let records = Records::load();
    let (ordinary, _) =
        h2_6c_refusal_migrations::partition(&records.cases, records.results()).unwrap();
    let listed = h2_6c_refusal_migrations::ordinary_manifest(&records.listed);
    h2_6c_output_promotions::validate_results(&ordinary, &listed).unwrap();
    let index = ordinary
        .iter()
        .position(|result| {
            h2_6c_output_promotions::find(&result.as_ref().unwrap().case_id).is_some()
        })
        .unwrap();
    let id = &ordinary[index].as_ref().unwrap().case_id;
    let mut cases = records.cases.clone();
    cases
        .iter_mut()
        .find(|case| case["case_id"] == *id)
        .unwrap()["observation_input_sha256"] = json!("changed");
    assert!(
        h2_6c_output_promotions::validate(&cases, &records.inputs.h2_7b_expected_members).is_err()
    );
    let mut changed = ordinary.clone();
    changed[index].as_mut().unwrap().h2_7b_activity += 1;
    assert!(h2_6c_output_promotions::validate_results(&changed, &listed).is_err());
    let mut changed = ordinary.clone();
    changed[index].as_mut().unwrap().divergence =
        vectorize_refusal(H2MismatchProfile::H2_6c, "outDir");
    assert!(h2_6c_output_promotions::validate_results(&changed, &listed).is_err());
    let mut missing = ordinary.clone();
    let _ = missing.remove(index);
    assert!(h2_6c_output_promotions::validate_results(&missing, &listed).is_err());
    let mut duplicate = ordinary.clone();
    duplicate.push(ordinary[index].clone());
    assert!(h2_6c_output_promotions::validate_results(&duplicate, &listed).is_err());
    let mut replaced = listed.clone();
    replaced.insert(
        id.clone(),
        vectorize_refusal(H2MismatchProfile::H2_6c, "unregistered"),
    );
    assert!(h2_6c_output_promotions::validate_results(&ordinary, &replaced).is_err());
}

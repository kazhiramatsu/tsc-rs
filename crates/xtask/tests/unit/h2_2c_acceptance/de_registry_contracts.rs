//! Registry contract mutations over frozen identities. These tests do not
//! manufacture runtime success evidence or replace the repeated emit collector.
use super::*;
use h2_6c_refusal_migrations::{CaseOutcome, ObservedRefusal};

struct MigrationInput {
    options: CompilerOptions,
    case_sensitive: bool,
    current_option: &'static str,
}

struct Records {
    workspace: PathBuf,
    cases: Vec<Value>,
    listed: H2LoadedVectorManifest,
    inputs: H2_6cExecutionInputs,
    migrations: BTreeMap<String, MigrationInput>,
}

impl Records {
    fn load() -> Self {
        // Require population; silently returning on empty registries would
        // turn the mutation tests into vacuous passing tests.
        assert!(
            h2_6c_de_promotions::promoted_count() > 0,
            "populate measured exact registry first"
        );
        assert!(
            h2_6c_refusal_migrations::count() > 0,
            "populate measured migration registry first"
        );
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
        let inputs = H2_6cExecutionInputs::load(&workspace).unwrap();
        h2_6c_de_promotions::validate(&workspace, &cases, &inputs.h2_7b_expected_members).unwrap();
        h2_6c_output_promotions::validate(&cases, &inputs.h2_7b_expected_members).unwrap();
        h2_6c_refusal_migrations::validate(&workspace, &cases, &listed).unwrap();
        let migrations = cases
            .iter()
            .filter(|case| h2_6c_refusal_migrations::registered(case["case_id"].as_str().unwrap()))
            .map(|case| {
                // Prepare only: no checker/emit result is simulated by this step.
                let program = prepare_h2_6c_case(&workspace, case, &inputs).unwrap();
                let options = program.compiler_options().clone();
                let case_sensitive = program.path_context().use_case_sensitive_file_names();
                let current_option = if !case_sensitive {
                    "useCaseSensitiveFileNames"
                } else {
                    assert!(
                        options.out_dir.is_some(),
                        "review a newly introduced later-owner boundary"
                    );
                    "outDir"
                };
                (
                    case["case_id"].as_str().unwrap().to_owned(),
                    MigrationInput {
                        options,
                        case_sensitive,
                        current_option,
                    },
                )
            })
            .collect();
        Self {
            workspace,
            cases,
            listed,
            inputs,
            migrations,
        }
    }

    fn observed(&self, id: &str) -> ObservedRefusal {
        let input = &self.migrations[id];
        h2_6c_refusal_migrations::observe(
            id,
            &input.options,
            input.case_sensitive,
            input.current_option,
            [0, 0],
        )
        .unwrap()
        .unwrap()
    }

    fn results(&self) -> Vec<Result<CaseOutcome, String>> {
        self.cases
            .iter()
            .map(|case| {
                let id = case["case_id"].as_str().unwrap();
                if self.migrations.contains_key(id) {
                    return Ok(h2_6c_refusal_migrations::migrated(self.observed(id)));
                }
                let promotion = h2_6c_de_promotions::find(id);
                let output_promotion = h2_6c_output_promotions::find(id);
                let divergence = if promotion.is_some() || output_promotion.is_some() {
                    H2VectorDivergence::default()
                } else {
                    self.listed.entries.get(id).cloned().unwrap_or_default()
                };
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
                    divergence,
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
fn populated_registry_identity_and_error_fingerprint_reject_mutations() {
    let records = Records::load();
    let id = records.migrations.keys().next().unwrap();
    let input = &records.migrations[id];
    let observed = records.observed(id);
    assert_eq!(&observed.case_id, id);
    assert_eq!(observed.current_option, input.current_option);

    let mut changed = input.options.clone();
    changed.out_file = Some(format!("{}-changed", changed.out_file.as_deref().unwrap()));
    let error = h2_6c_refusal_migrations::observe(
        id,
        &changed,
        input.case_sensitive,
        input.current_option,
        [0, 0],
    )
    .err()
    .expect("same active boundary with changed actual input must fail");
    assert!(error.to_string().contains("fingerprint differs"), "{error}");
    for writes in [[1, 0], [0, 1]] {
        assert!(h2_6c_refusal_migrations::observe(
            id,
            &input.options,
            input.case_sensitive,
            input.current_option,
            writes
        )
        .is_err());
    }
    assert!(
        h2_6c_refusal_migrations::observe(
            id,
            &input.options,
            input.case_sensitive,
            "outFile",
            [0, 0]
        )
        .is_err(),
        "old-profile refusal cannot satisfy the current observation"
    );
    assert!(h2_6c_refusal_migrations::observe(
        "unknown-id",
        &input.options,
        input.case_sensitive,
        input.current_option,
        [0, 0]
    )
    .unwrap()
    .is_none());

    for (id, is_migration) in [(id.as_str(), true), (records.promotion_id(), false)] {
        let mut cases = records.cases.clone();
        cases.iter_mut().find(|case| case["case_id"] == id).unwrap()["observation_input_sha256"] =
            json!("changed");
        let rejected = if is_migration {
            h2_6c_refusal_migrations::validate(&records.workspace, &cases, &records.listed).is_err()
        } else {
            h2_6c_de_promotions::validate(
                &records.workspace,
                &cases,
                &records.inputs.h2_7b_expected_members,
            )
            .is_err()
        };
        assert!(rejected, "{id}: identity mutation must fail");
    }
    let mut listed = H2LoadedVectorManifest {
        entries: records.listed.entries.clone(),
        vectors_populated: true,
    };
    listed.entries.remove(id);
    assert!(
        h2_6c_refusal_migrations::validate(&records.workspace, &records.cases, &listed).is_err(),
        "a migrated historical row cannot be removed"
    );
    listed.entries.insert(
        id.clone(),
        vectorize_refusal(H2MismatchProfile::H2_6c, input.current_option),
    );
    assert!(
        h2_6c_refusal_migrations::validate(&records.workspace, &records.cases, &listed).is_err(),
        "current refusal cannot replace the historical vector"
    );
}

#[test]
fn populated_partition_rejects_missing_duplicate_unknown_and_success_substitution() {
    let records = Records::load();
    let ordinary_index = records
        .cases
        .iter()
        .position(|case| {
            !records
                .migrations
                .contains_key(case["case_id"].as_str().unwrap())
        })
        .unwrap();
    let migration_index = records
        .cases
        .iter()
        .position(|case| {
            records
                .migrations
                .contains_key(case["case_id"].as_str().unwrap())
        })
        .unwrap();
    assert!(h2_6c_refusal_migrations::partition(&records.cases, records.results()).is_ok());
    for index in [ordinary_index, migration_index] {
        let mut results = records.results();
        let _ = results.remove(index);
        assert!(
            h2_6c_refusal_migrations::partition(&records.cases, results).is_err(),
            "missing original at {index}"
        );
    }
    let mut duplicate = records.results();
    duplicate.push(records.results().remove(ordinary_index));
    assert!(h2_6c_refusal_migrations::partition(&records.cases, duplicate).is_err());
    let mut duplicate = records.results();
    duplicate.push(records.results().remove(migration_index));
    assert!(h2_6c_refusal_migrations::partition(&records.cases, duplicate).is_err());
    let mut unknown = records.results();
    let CaseOutcome::Compared(outcome) = unknown[ordinary_index].as_mut().unwrap() else {
        unreachable!()
    };
    outcome.case_id = "unknown-exact-original".to_owned();
    outcome.divergence = H2VectorDivergence::default();
    assert!(
        h2_6c_refusal_migrations::partition(&records.cases, unknown).is_err(),
        "unknown exact row must not balance a missing original"
    );
    let mut substituted = records.results();
    substituted[migration_index] = Ok(h2_6c_refusal_migrations::compared(H2VectorCaseOutcome {
        case_id: records.cases[migration_index]["case_id"]
            .as_str()
            .unwrap()
            .to_owned(),
        deferred: false,
        h2_7b_activity: 0,
        divergence: H2VectorDivergence::default(),
    }));
    assert!(
        h2_6c_refusal_migrations::partition(&records.cases, substituted).is_err(),
        "a migrated refusal becoming success needs a separate reviewed promotion"
    );
    let mut duplicate_cases = records.cases.clone();
    duplicate_cases.push(records.cases[0].clone());
    assert!(h2_6c_refusal_migrations::partition(&duplicate_cases, records.results()).is_err());
}

fn histogram(rows: &[(String, H2VectorDivergence)]) -> BTreeMap<String, u64> {
    let mut counts = BTreeMap::new();
    for (_, row) in rows {
        if let Some(option) = &row.refused_option {
            *counts.entry(option.clone()).or_default() += 1;
        }
    }
    counts
}

#[test]
fn populated_partition_preserves_history_and_reports_current_refusals_separately() {
    let records = Records::load();
    let (ordinary, migrations) =
        h2_6c_refusal_migrations::partition(&records.cases, records.results()).unwrap();
    let listed = h2_6c_refusal_migrations::ordinary_manifest(&records.listed);
    h2_6c_refusal_migrations::validate_ordinary_results(&ordinary, &listed).unwrap();
    h2_6c_de_promotions::validate_results(&ordinary, &listed).unwrap();
    h2_6c_output_promotions::validate_results(&ordinary, &listed).unwrap();
    let actual_counts = h2_6c_refused_option_totals(&ordinary, &migrations).unwrap();
    let (exact, deferred, diverging) =
        h2_vector_ratchet_join("H2.6c", ordinary, &listed, true).unwrap();
    let retained =
        h2_6c_refusal_migrations::retained_manifest(&records.listed, &diverging).unwrap();
    let expected_retained = records
        .listed
        .entries
        .iter()
        .filter(|(id, _)| {
            h2_6c_de_promotions::find(id).is_none() && h2_6c_output_promotions::find(id).is_none()
        })
        .map(|(id, row)| (id.clone(), row.clone()))
        .collect::<HashMap<_, _>>();
    assert_eq!(
        retained.iter().cloned().collect::<HashMap<_, _>>(),
        expected_retained
    );
    assert_eq!(
        retained.len(),
        expected_retained.len(),
        "no duplicated historical rows"
    );
    assert_eq!(
        exact + deferred + diverging.len() as u64 + migrations.len() as u64,
        records.cases.len() as u64
    );
    let mut projected_current = histogram(&retained);
    for migrated in &migrations {
        let historical = &records.listed.entries[&migrated.case_id];
        assert_eq!(expected_retained[&migrated.case_id], *historical);
        assert_ne!(
            historical.refused_option.as_deref(),
            Some(migrated.current_option.as_str())
        );
        *projected_current
            .get_mut(historical.refused_option.as_ref().unwrap())
            .unwrap() -= 1;
        *projected_current
            .entry(migrated.current_option.clone())
            .or_default() += 1;
    }
    projected_current.retain(|_, count| *count != 0);
    assert_eq!(actual_counts, projected_current);
    report_h2_6c_suite_outcomes(&records.cases, &diverging, &migrations).unwrap();

    // Exercise the real writer/loader against a temporary workspace, never the
    // actual known-divergence file. Every retained vector must round-trip.
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
    let reloaded = load_h2_6c_divergence_manifest_state(&directory, false).unwrap();
    assert_eq!(reloaded.entries, expected_retained);
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

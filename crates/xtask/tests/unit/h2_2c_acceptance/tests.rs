use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::{fs, thread};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonical workspace")
}

#[test]
fn all_h2_2c_candidate_dispositions_execute() {
    super::run(&workspace()).expect("H2.2c acceptance");
}

#[test]
fn all_h2_4a_candidate_dispositions_execute_or_are_source_deferred() {
    super::run_h2_4a(&workspace()).expect("H2.4a acceptance");
}

#[test]
fn all_h2_4b_candidate_dispositions_execute_or_are_source_deferred() {
    super::run_h2_4b(&workspace()).expect("H2.4b acceptance");
}

#[test]
fn h2_2c_two_worker_execution_is_exact_and_isolated() {
    const CASE_IDS: [&str; 2] = [
        "typescript-6.0.3/conformance/classes/members/instanceAndStaticMembers/typeOfThisInStaticMembers5.ts#target%3Desnext",
        "typescript-6.0.3/conformance/classes/propertyMemberDeclarations/initializationOrdering1.ts#target%3Desnext%2Cusedefineforclassfields%3Dtrue",
    ];
    let workspace = workspace();
    let artifact: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.join(super::QUALIFICATION_RELATIVE_PATH))
            .expect("read H2.2c qualification"),
    )
    .expect("parse H2.2c qualification");
    let cases = CASE_IDS.map(|case_id| {
        artifact["cases"]
            .as_array()
            .and_then(|cases| cases.iter().find(|case| case["case_id"] == case_id))
            .cloned()
            .unwrap_or_else(|| panic!("missing two-worker case {case_id}"))
    });

    let results = thread::scope(|scope| {
        let workspace = &workspace;
        cases
            .into_iter()
            .map(|case| {
                scope.spawn(move || {
                    super::execute_observed(workspace, &case)
                        .unwrap_or_else(|error| panic!("two-worker H2.2c emit failed: {error}"))
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|worker| worker.join().expect("H2.2c worker panicked"))
            .collect::<Vec<_>>()
    });
    assert_eq!(results[0], (1, 0));
    assert_eq!(results[1], (1, 1));
}

#[test]
fn h2_2c_parameter_property_outputs_are_exact() {
    const CASE_IDS: [&str; 2] = [
        "typescript-6.0.3/conformance/classes/propertyMemberDeclarations/assignParameterPropertyToPropertyDeclarationESNext.ts#default",
        "typescript-6.0.3/conformance/classes/propertyMemberDeclarations/initializationOrdering1.ts#target%3Desnext%2Cusedefineforclassfields%3Dtrue",
    ];
    let workspace = workspace();
    let artifact: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.join(super::QUALIFICATION_RELATIVE_PATH))
            .expect("read H2.2c qualification"),
    )
    .expect("parse H2.2c qualification");
    for case_id in CASE_IDS {
        let case = artifact["cases"]
            .as_array()
            .and_then(|cases| cases.iter().find(|case| case["case_id"] == case_id))
            .unwrap_or_else(|| panic!("missing parameter-property case {case_id}"));
        super::execute_observed(&workspace, case)
            .unwrap_or_else(|error| panic!("{case_id}: {error}"));
        assert!(
            case["files"]
                .as_array()
                .expect("case files")
                .iter()
                .any(|file| {
                    file["feature_roots"].as_array().is_some_and(|roots| {
                        roots
                            .iter()
                            .any(|root| root["feature"] == "parameter-properties")
                    })
                }),
            "{case_id}: parameter-property owner source"
        );
    }
}

#[test]
fn h2_5g_transform_activity_uses_complete_unique_write_provenance() {
    let multi_output = serde_json::json!({
        "writes": [
            { "source_files": ["/.src/a.ts"] },
            { "source_files": ["/.src/a.ts"] },
            { "source_files": ["/.src/a.ts", "/.src/b.ts"] }
        ]
    });
    assert_eq!(
        super::transform_source_paths(&multi_output)
            .expect("complete write provenance")
            .into_iter()
            .collect::<Vec<_>>(),
        ["/.src/a.ts", "/.src/b.ts"],
    );

    let no_writes = serde_json::json!({ "writes": [] });
    assert!(super::transform_source_paths(&no_writes)
        .expect("noEmit/noEmitOnError has complete empty provenance")
        .is_empty());

    let missing_provenance = serde_json::json!({ "writes": [{}] });
    assert!(
        super::transform_source_paths(&missing_provenance).is_err(),
        "a write without source_files must not broaden transformer activity"
    );
}

#[test]
fn h2_5g_qualification_validator_owns_the_closed_acceptance_scope() {
    let mut artifact = serde_json::json!({
        "schema": 1,
        "status": "qualified-typescript-oracle",
        "phase": "H2.5g-es2016-target",
        "selection_contract": {
            "global_h2_5g_rows": 11_910,
            "global_candidate_denominator": 9_027,
            "candidate_denominator": 9_027,
            "future_deferred_rows": 2_883,
        },
        "summary": {
            "candidates": 9_027,
            "compiler_candidates": 4_712,
            "conformance_candidates": 4_315,
            "recorded_compiler_plan_cases": 4_712,
            "qualified_vfs_cases": 4_315,
            "virtual_config_cases": 56,
            "vfs_symlink_cases": 3,
            "vfs_symlink_paths": 4,
            "admitted_cases": 8_511,
            "deferred_cases": 516,
            "source_deferred_cases": 516,
            "no_emit_control_cases": 59,
            "typescript_runs": 18_054,
            "deterministic_typescript_cases": 9_027,
            "admitted_typescript_writes": 9_466,
            "admitted_typescript_diagnostics": 26_815,
            "unexecuted_candidates": 0,
            "undispositioned_candidates": 0,
        },
        "owner_closure": [{ "key": "transform-es2016" }],
        "cases": Vec::<serde_json::Value>::new(),
    });
    artifact["cases"] = serde_json::Value::Array(vec![serde_json::Value::Null; 9_027]);

    assert_eq!(
        super::validate_h2_5g_qualification(&artifact)
            .expect("closed H2.5g qualification")
            .len(),
        9_027,
    );

    artifact["phase"] = serde_json::json!("H2.5f-es2017-target");
    assert!(super::validate_h2_5g_qualification(&artifact).is_err());
    artifact["phase"] = serde_json::json!("H2.5g-es2016-target");

    artifact["selection_contract"]["future_deferred_rows"] = serde_json::json!(2_882);
    assert!(super::validate_h2_5g_qualification(&artifact).is_err());
    artifact["selection_contract"]["future_deferred_rows"] = serde_json::json!(2_883);

    artifact["summary"]["admitted_cases"] = serde_json::json!(8_510);
    assert!(super::validate_h2_5g_qualification(&artifact).is_err());
    artifact["summary"]["admitted_cases"] = serde_json::json!(8_511);

    artifact["owner_closure"][0]["key"] = serde_json::json!("transform-es2017");
    assert!(super::validate_h2_5g_qualification(&artifact).is_err());
}

#[test]
fn h2_5g_case_disposition_uses_the_first_typed_deferred_owner() {
    let admitted = serde_json::json!({
        "case_id": "admitted",
        "disposition": "admitted-for-execution",
    });
    assert_eq!(
        super::classify_h2_5g_case(&admitted).expect("admitted disposition"),
        super::H2_5gCaseDisposition::Admitted,
    );

    let deferred = |required_slices: serde_json::Value| {
        serde_json::json!({
            "case_id": "deferred",
            "disposition": "deferred-to-slices",
            "diagnostic_disposition": {
                "state": "not-observed-source-deferred",
            },
            "rust_expectation": "typed-failure-before-first-sink-write",
            "required_slices": required_slices,
        })
    };
    assert_eq!(
        super::classify_h2_5g_case(&deferred(serde_json::json!(["H2.8a"])))
            .expect("H2.8a deferred disposition"),
        super::H2_5gCaseDisposition::Deferred(super::H2_5gDeferredOwner::H2_8a),
    );
    assert_eq!(
        super::classify_h2_5g_case(&deferred(serde_json::json!(["H2.8a", "H2.9"])))
            .expect("multi-owner H2.8a deferred disposition"),
        super::H2_5gCaseDisposition::Deferred(super::H2_5gDeferredOwner::H2_8a),
    );
    assert_eq!(
        super::classify_h2_5g_case(&deferred(serde_json::json!(["H2.9"])))
            .expect("H2.9 deferred disposition"),
        super::H2_5gCaseDisposition::Deferred(super::H2_5gDeferredOwner::H2_9),
    );
    assert!(super::classify_h2_5g_case(&deferred(serde_json::json!([]))).is_err());
    assert!(super::classify_h2_5g_case(&deferred(serde_json::json!(["H2.10"]))).is_err());

    let mut invalid_state = deferred(serde_json::json!(["H2.9"]));
    invalid_state["diagnostic_disposition"]["state"] = serde_json::json!("exact-required");
    assert!(super::classify_h2_5g_case(&invalid_state).is_err());
}

#[test]
fn h2_5g_worker_selection_is_bounded_and_local_default_is_serial() {
    assert_eq!(super::select_h2_5g_workers(None, 8).unwrap(), 1);
    assert_eq!(super::select_h2_5g_workers(Some("2"), 8).unwrap(), 2);
    assert_eq!(super::select_h2_5g_workers(Some("2"), 1).unwrap(), 1);
    assert!(super::select_h2_5g_workers(Some("0"), 8).is_err());
    assert!(super::select_h2_5g_workers(Some("3"), 8).is_err());
    assert!(super::select_h2_5g_workers(Some("wat"), 8).is_err());
}

#[test]
fn h2_5g_duplicate_output_block_routes_sources_without_transforming_them() {
    const CASE_ID: &str = "typescript-6.0.3/compiler/filesEmittingIntoSameOutput.ts#default";
    let workspace = workspace();
    let artifact: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.join(super::H2_5G_QUALIFICATION_RELATIVE_PATH))
            .expect("read H2.5g qualification"),
    )
    .expect("parse H2.5g qualification");
    let case = artifact["cases"]
        .as_array()
        .and_then(|cases| cases.iter().find(|case| case["case_id"] == CASE_ID))
        .unwrap_or_else(|| panic!("missing duplicate-output case {CASE_ID}"));
    let inputs = super::H2_5gExecutionInputs::load(&workspace).expect("load H2.5g inputs");

    assert_eq!(
        super::execute_h2_5g_observed(&workspace, case, &inputs)
            .unwrap_or_else(|error| panic!("{CASE_ID}: {error}")),
        (0, 1),
    );
}

#[test]
fn h2_5g_activity_projection_uses_prepared_typed_sources_and_global_module_route() {
    let workspace = workspace();
    let artifact: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.join(super::H2_5G_QUALIFICATION_RELATIVE_PATH))
            .expect("read H2.5g qualification"),
    )
    .expect("parse H2.5g qualification");
    let cases = artifact["cases"].as_array().expect("H2.5g cases");
    let inputs = super::H2_5gExecutionInputs::load(&workspace).expect("load H2.5g inputs");

    for (index, expected_routed_sources, expected_jsx_sources, expected_automatic_jsx_sources) in [
        (2691, 1, 1, 1),
        (2693, 1, 1, 1),
        (2703, 5, 4, 2),
        (2704, 5, 4, 2),
        (2705, 5, 4, 2),
        (2706, 5, 4, 2),
    ] {
        let case = &cases[index];
        let case_id = super::string(case, "case_id").expect("case id");
        let observation =
            super::compact_typescript_observation(case).expect("compact TypeScript observation");
        let transform_sources =
            super::transform_source_paths(observation).expect("transform source provenance");
        let program = inputs
            .prepare(&workspace, case)
            .unwrap_or_else(|error| panic!("{case_id}: prepare failed: {error}"));
        let activity = super::expected_typed_activity(&program, &transform_sources);
        assert_eq!(
            activity.automatic_jsx_sources, expected_automatic_jsx_sources,
            "{case_id}"
        );
        assert_eq!(
            activity.routed_sources, expected_routed_sources,
            "{case_id}"
        );
        assert_eq!(activity.jsx_sources, expected_jsx_sources, "{case_id}");
        assert_eq!(activity.javascript_sources, 0, "{case_id}");
        assert_eq!(activity.json_sources, 0, "{case_id}");
    }

    let decorator_case = &cases[5167];
    let decorator_observation = super::compact_typescript_observation(decorator_case)
        .expect("decorator TypeScript observation");
    let decorator_transform_sources = super::transform_source_paths(decorator_observation)
        .expect("decorator transform source provenance");
    let decorator_program = inputs
        .prepare(&workspace, decorator_case)
        .expect("prepare decorator recovery case");
    let decorator_activity =
        super::expected_typed_activity(&decorator_program, &decorator_transform_sources);
    assert_eq!(decorator_activity.decorator_sources, 1);
    assert_eq!(decorator_activity.h2_4a_sources, 1);
    assert_eq!(decorator_activity.h2_4b_sources, 1);
    assert!(decorator_case["files"]
        .as_array()
        .expect("decorator files")
        .iter()
        .all(|file| file["feature_roots"]
            .as_array()
            .is_none_or(|roots| { roots.iter().all(|root| root["feature"] != "decorators") })));

    for index in [7323, 7325] {
        let case = &cases[index];
        let case_id = super::string(case, "case_id").expect("case id");
        let observation =
            super::compact_typescript_observation(case).expect("compact TypeScript observation");
        let transform_sources =
            super::transform_source_paths(observation).expect("transform source provenance");
        let program = inputs
            .prepare(&workspace, case)
            .unwrap_or_else(|error| panic!("{case_id}: prepare failed: {error}"));
        let activity = super::expected_typed_activity(&program, &transform_sources);
        assert_eq!(activity.transformed_sources, 3, "{case_id}");
        assert_eq!(activity.preserve_sources, 3, "{case_id}");
        assert_eq!(activity.node_format_sources, 2, "{case_id}");
        assert_eq!(activity.h2_1a_sources, 0, "{case_id}");
        assert_eq!(activity.h2_1b_sources, 0, "{case_id}");
        assert_eq!(activity.h2_1c_sources, 0, "{case_id}");
        assert_eq!(activity.h2_1d_sources, 0, "{case_id}");
    }
}

/// Local-only probe for finding a later H2.5g parity failure without changing
/// the fixed, unsplit acceptance entrypoint. The half-open range is selected
/// with `TSRS_H2_5G_PROBE_START` and `TSRS_H2_5G_PROBE_END` and every admitted
/// row still goes through the exact two-run acceptance comparison.
#[test]
#[ignore = "local H2.5g diagnostic range probe"]
fn probe_h2_5g_exact_range_locally() {
    let workspace = workspace();
    let artifact: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.join(super::H2_5G_QUALIFICATION_RELATIVE_PATH))
            .expect("read H2.5g qualification"),
    )
    .expect("parse H2.5g qualification");
    let cases = artifact["cases"].as_array().expect("H2.5g cases");
    let start = std::env::var("TSRS_H2_5G_PROBE_START")
        .ok()
        .map(|value| value.parse::<usize>().expect("valid H2.5g probe start"))
        .unwrap_or(1_000);
    let end = std::env::var("TSRS_H2_5G_PROBE_END")
        .ok()
        .map(|value| value.parse::<usize>().expect("valid H2.5g probe end"))
        .unwrap_or_else(|| (start + 500).min(cases.len()));
    assert!(
        start < end && end <= cases.len(),
        "invalid H2.5g probe range {start}..{end} for {} cases",
        cases.len(),
    );

    let inputs = super::H2_5gExecutionInputs::load(&workspace).expect("load H2.5g inputs");
    let mut admitted = 0usize;
    for (index, case) in cases[start..end].iter().enumerate() {
        let absolute_index = start + index;
        let case_id = super::string(case, "case_id").expect("H2.5g case id");
        super::compact_typescript_observation(case)
            .unwrap_or_else(|error| panic!("{absolute_index} {case_id}: {error}"));
        match super::string(case, "disposition").expect("H2.5g disposition") {
            "admitted-for-execution" => {
                super::execute_h2_5g_observed(&workspace, case, &inputs)
                    .unwrap_or_else(|error| panic!("{absolute_index} {case_id}: {error}"));
                admitted += 1;
            }
            "deferred-to-slices" => {}
            disposition => {
                panic!("{absolute_index} {case_id}: unknown H2.5g disposition {disposition}")
            }
        }
        if absolute_index.is_multiple_of(100) {
            eprintln!("H2.5g exact probe reached case {absolute_index}: {case_id}");
        }
    }
    eprintln!("H2.5g exact probe passed {start}..{end}: admitted={admitted}");
}

/// Local inventory companion to `probe_h2_5g_exact_range_locally`.
///
/// This is deliberately ignored and has no acceptance meaning: it continues
/// after exact mismatches so a whole range can be designed as semantic
/// clusters before implementation. The caller must provide an explicit JSONL
/// path with `TSRS_H2_5G_DIFF_REPORT`; every admitted row still executes the
/// production two-run exact comparator.
#[test]
#[ignore = "local H2.5g exact-difference inventory"]
fn collect_h2_5g_exact_differences_locally() {
    let workspace = workspace();
    let artifact: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.join(super::H2_5G_QUALIFICATION_RELATIVE_PATH))
            .expect("read H2.5g qualification"),
    )
    .expect("parse H2.5g qualification");
    let cases = artifact["cases"].as_array().expect("H2.5g cases");
    let start = std::env::var("TSRS_H2_5G_PROBE_START")
        .ok()
        .map(|value| value.parse::<usize>().expect("valid H2.5g probe start"))
        .unwrap_or(0);
    let end = std::env::var("TSRS_H2_5G_PROBE_END")
        .ok()
        .map(|value| value.parse::<usize>().expect("valid H2.5g probe end"))
        .unwrap_or(cases.len());
    assert!(
        start < end && end <= cases.len(),
        "invalid H2.5g inventory range {start}..{end} for {} cases",
        cases.len(),
    );
    let report_path = std::env::var_os("TSRS_H2_5G_DIFF_REPORT")
        .map(PathBuf::from)
        .expect("TSRS_H2_5G_DIFF_REPORT must name an explicit local JSONL path");
    let mut report = BufWriter::new(fs::File::create(&report_path).unwrap_or_else(|error| {
        panic!(
            "create H2.5g difference report {}: {error}",
            report_path.display()
        )
    }));
    let inputs = super::H2_5gExecutionInputs::load(&workspace).expect("load H2.5g inputs");
    let mut admitted = 0usize;
    let mut differences = 0usize;

    for (offset, case) in cases[start..end].iter().enumerate() {
        let index = start + offset;
        let case_id = super::string(case, "case_id").expect("H2.5g case id");
        let result = (|| {
            super::compact_typescript_observation(case)?;
            match super::string(case, "disposition")? {
                "admitted-for-execution" => {
                    admitted += 1;
                    super::execute_h2_5g_observed(&workspace, case, &inputs)?;
                }
                "deferred-to-slices" => {}
                disposition => {
                    return Err(super::failure(format!(
                        "{index} {case_id}: unknown H2.5g disposition {disposition}"
                    )));
                }
            }
            Ok::<_, Box<dyn std::error::Error>>(())
        })();
        if let Err(error) = result {
            serde_json::to_writer(
                &mut report,
                &serde_json::json!({
                    "index": index,
                    "case_id": case_id,
                    "error": error.to_string(),
                }),
            )
            .expect("write H2.5g difference row");
            writeln!(report).expect("terminate H2.5g difference row");
            differences += 1;
        }
        if index.is_multiple_of(100) {
            eprintln!(
                "H2.5g inventory reached case {index}: admitted={admitted} differences={differences}"
            );
        }
    }
    report.flush().expect("flush H2.5g difference report");
    eprintln!(
        "H2.5g inventory completed {start}..{end}: admitted={admitted} differences={differences} report={}",
        report_path.display()
    );
}

#[test]
fn h2_6c_qualification_validator_pins_census_identity_and_counts() {
    let workspace = workspace();
    let mut artifact: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.join(super::H2_6C_QUALIFICATION_RELATIVE_PATH))
            .expect("read H2.6c qualification"),
    )
    .expect("parse H2.6c qualification");

    assert_eq!(
        super::validate_h2_6c_qualification(&artifact)
            .expect("closed H2.6c qualification")
            .len(),
        643,
    );

    artifact["phase"] = serde_json::json!("H2.6b-inline-and-roots");
    assert!(super::validate_h2_6c_qualification(&artifact).is_err());
    artifact["phase"] = serde_json::json!("H2.6c-map-observation");

    artifact["origin"]["census"]["fingerprint_sha256"] = serde_json::json!("changed");
    assert!(super::validate_h2_6c_qualification(&artifact).is_err());
    artifact["origin"]["census"]["fingerprint_sha256"] =
        serde_json::json!(super::H2_6C_CENSUS_FINGERPRINT_SHA256);

    artifact["summary"]["admitted_cases"] = serde_json::json!(638);
    assert!(super::validate_h2_6c_qualification(&artifact).is_err());
    artifact["summary"]["admitted_cases"] = serde_json::json!(639);

    artifact["summary"]["deferred_cases"] = serde_json::json!(3);
    assert!(super::validate_h2_6c_qualification(&artifact).is_err());
    artifact["summary"]["deferred_cases"] = serde_json::json!(4);

    artifact["cases"].as_array_mut().expect("H2.6c cases").pop();
    assert!(super::validate_h2_6c_qualification(&artifact).is_err());
}

#[test]
fn h2_6c_divergence_manifest_requires_absence_named_owner_and_nonempty_facets() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "tsc-rs-h2-6c-manifest-{}-{nonce}",
        std::process::id()
    ));

    assert!(super::load_h2_6c_divergence_manifest(&root)
        .expect("absent H2.6c manifest")
        .is_empty());

    let path = root.join(super::H2_6C_KNOWN_DIVERGENCES_RELATIVE_PATH);
    fs::create_dir_all(path.parent().expect("manifest parent")).expect("create ratchets dir");
    let store = |body: serde_json::Value| {
        fs::write(
            &path,
            format!(
                "{}\n",
                serde_json::to_string_pretty(&body).expect("render manifest")
            ),
        )
        .expect("write manifest");
    };

    store(serde_json::json!({ "schema": 1, "cases": [] }));
    assert!(super::load_h2_6c_divergence_manifest(&root).is_err());

    let entry = serde_json::json!({
        "case_id": "case-a",
        "owner": super::H2_6C_DIVERGENCE_OWNER,
        "writes_diverging": 2,
        "diagnostics_diverging": false,
        "emit_result_diverging": true,
        "emit_refused": false,
    });
    store(serde_json::json!({ "schema": 1, "cases": [entry.clone()] }));
    let listed = super::load_h2_6c_divergence_manifest(&root).expect("valid H2.6c manifest");
    assert_eq!(
        listed.get("case-a"),
        Some(&super::H2_5hDivergence {
            writes_diverging: 2,
            diagnostics_diverging: false,
            emit_result_diverging: true,
            emit_refused: false,
        })
    );

    let mut wrong_owner = entry.clone();
    wrong_owner["owner"] = serde_json::json!("h2-6c-unnamed");
    store(serde_json::json!({ "schema": 1, "cases": [wrong_owner] }));
    assert!(super::load_h2_6c_divergence_manifest(&root).is_err());

    let mut exact = entry.clone();
    exact["writes_diverging"] = serde_json::json!(0);
    exact["emit_result_diverging"] = serde_json::json!(false);
    store(serde_json::json!({ "schema": 1, "cases": [exact] }));
    assert!(super::load_h2_6c_divergence_manifest(&root).is_err());

    store(serde_json::json!({ "schema": 1, "cases": [entry.clone(), entry] }));
    assert!(super::load_h2_6c_divergence_manifest(&root).is_err());

    fs::remove_dir_all(&root).expect("remove H2.6c manifest test directory");
}

// H2.5h CA-4: the divergence ratchet's four outcomes (packet §4).
mod h2_5h_ratchet {
    use std::collections::HashMap;

    use super::super::{
        h2_5h_ratchet_join as join, H2_5hCaseOutcome as Outcome, H2_5hDivergence as Divergence,
    };

    fn outcome(case_id: &str, divergence: Divergence) -> Result<Outcome, String> {
        Ok(Outcome {
            case_id: case_id.to_owned(),
            deferred: false,
            divergence,
        })
    }

    fn diverging() -> Divergence {
        Divergence {
            writes_diverging: 1,
            ..Divergence::default()
        }
    }

    #[test]
    fn exact_unlisted_passes_and_diverging_listed_passes_facet_exact() {
        let listed = HashMap::from([("b".to_owned(), diverging())]);
        let (exact, deferred, known) = join(
            vec![
                outcome("a", Divergence::default()),
                outcome("b", diverging()),
            ],
            &listed,
            false,
        )
        .expect("both lanes pass");
        assert_eq!((exact, deferred, known.len()), (1, 0, 1));
    }

    #[test]
    fn diverging_unlisted_fails() {
        let error = join(vec![outcome("a", diverging())], &HashMap::new(), false)
            .expect_err("a new divergence must fail");
        assert!(error.to_string().contains("NEW divergence"), "{error}");
    }

    #[test]
    fn exact_listed_fails_stale() {
        let listed = HashMap::from([("a".to_owned(), diverging())]);
        let error = join(vec![outcome("a", Divergence::default())], &listed, false)
            .expect_err("a stale entry must fail");
        assert!(error.to_string().contains("stale"), "{error}");
    }

    #[test]
    fn facet_mismatch_fails() {
        let listed = HashMap::from([(
            "a".to_owned(),
            Divergence {
                diagnostics_diverging: true,
                ..Divergence::default()
            },
        )]);
        let error = join(vec![outcome("a", diverging())], &listed, false)
            .expect_err("facet drift must fail");
        assert!(error.to_string().contains("facets differ"), "{error}");
    }
}

use std::path::{Path, PathBuf};
use std::{fs, thread};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonical workspace")
}

#[test]
fn all_h2_1a_candidate_dispositions_execute() {
    super::run(&workspace()).expect("H2.1a acceptance");
}

#[test]
fn h2_1a_two_worker_execution_is_exact_and_isolated() {
    const CASE_IDS: [&str; 2] = [
        "typescript-6.0.3/compiler/acceptSymbolAsWeakType.ts#default",
        "typescript-6.0.3/conformance/async/es6/functionDeclarations/asyncOrYieldAsBindingIdentifier1.ts#default",
    ];
    let workspace = workspace();
    let artifact: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.join(super::QUALIFICATION_RELATIVE_PATH))
            .expect("read H2.1a qualification"),
    )
    .expect("parse H2.1a qualification");
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
                    super::execute_observed(workspace, &case, super::DiagnosticExpectation::Exact)
                        .unwrap_or_else(|error| panic!("two-worker H2.1a emit failed: {error}"))
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|worker| worker.join().expect("H2.1a worker panicked"))
            .collect::<Vec<_>>()
    });
    assert!(results.iter().all(|(writes, _)| *writes == 1));
}

#[test]
fn historical_diagnostic_controls_are_currently_exact() {
    let workspace = workspace();
    let artifact: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.join(super::QUALIFICATION_RELATIVE_PATH))
            .expect("read H2.1a qualification"),
    )
    .expect("parse H2.1a qualification");
    let cases = artifact["cases"]
        .as_array()
        .expect("H2.1a qualification cases");
    let historical_controls = cases
        .iter()
        .filter(|case| case["disposition"] == "diagnostic-deferred-output-control")
        .collect::<Vec<_>>();
    assert_eq!(historical_controls.len(), 5);
    assert_eq!(
        historical_controls.len(),
        super::CURRENT_EXACT_DIAGNOSTIC_PROMOTIONS.len(),
    );

    let mut exact_writes = 0;
    let mut exact_diagnostics = 0;
    for promotion in super::CURRENT_EXACT_DIAGNOSTIC_PROMOTIONS {
        let case = cases
            .iter()
            .find(|case| case["case_id"] == promotion.case_id)
            .unwrap_or_else(|| panic!("missing promoted H2.1a case {}", promotion.case_id));
        assert_eq!(
            case["case_fingerprint_sha256"],
            promotion.case_fingerprint_sha256,
        );
        assert_eq!(case["disposition"], "diagnostic-deferred-output-control");
        assert_eq!(case["diagnostic_disposition"]["state"], "deferred-to-H2.9");
        assert!(case["typescript_runs"].as_array().is_some_and(|runs| {
            runs.len() == 2
                && runs.iter().all(|run| {
                    run["reported_diagnostics"]
                        .as_array()
                        .is_some_and(|diagnostics| {
                            diagnostics.len() == promotion.expected_reported_diagnostics
                        })
                        && run["writes"]
                            .as_array()
                            .is_some_and(|writes| writes.len() == promotion.expected_writes)
                })
        }));
        assert_eq!(
            super::current_exact_diagnostic_promotion(case)
                .expect("validate current exact promotion"),
            Some(promotion),
        );
        let (case_writes, case_diagnostics) = super::execute_observed(
            &workspace,
            case,
            super::DiagnosticExpectation::CurrentExactPromotion,
        )
        .unwrap_or_else(|error| panic!("{} exact promotion failed: {error}", promotion.case_id));
        exact_writes += case_writes;
        exact_diagnostics += case_diagnostics;
    }
    assert_eq!(exact_writes, 5);
    assert_eq!(exact_diagnostics, 205);
    assert!(historical_controls.iter().all(|case| {
        super::current_exact_diagnostic_promotion(case)
            .expect("validate complete current promotion list")
            .is_some()
    }));
}

#[test]
fn current_source_promotions_compare_original_observations_and_pin_their_owner() {
    let workspace = workspace();
    let artifact: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.join(super::QUALIFICATION_RELATIVE_PATH))
            .expect("read H2.1a qualification"),
    )
    .expect("parse H2.1a qualification");
    let cases = artifact["cases"].as_array().expect("qualification cases");
    assert_eq!(super::CURRENT_EXACT_SOURCE_PROMOTIONS.len(), 4);
    let mut writes = 0;
    let mut diagnostics = 0;
    for (case_id, _, required_slice) in super::CURRENT_EXACT_SOURCE_PROMOTIONS {
        let case = cases
            .iter()
            .find(|case| case["case_id"] == *case_id)
            .expect("recorded source promotion");
        let (case_writes, case_diagnostics) = super::execute_observed(
            &workspace,
            case,
            super::DiagnosticExpectation::CurrentExactSourcePromotion,
        )
        .unwrap_or_else(|error| panic!("{case_id}: {error}"));
        writes += case_writes;
        diagnostics += case_diagnostics;

        let mut wrong_owner = case.clone();
        wrong_owner["required_slices"] = serde_json::json!([if *required_slice == "H2.9" {
            "H2.8a"
        } else {
            "H2.9"
        }]);
        assert!(super::current_exact_source_promotion(&wrong_owner).is_err());
        let mut wrong_identity = case.clone();
        wrong_identity["case_fingerprint_sha256"] = serde_json::json!("changed");
        assert!(super::current_exact_source_promotion(&wrong_identity).is_err());
    }
    assert_eq!((writes, diagnostics), (4, 10));
}

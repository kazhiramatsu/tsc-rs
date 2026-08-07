use super::*;

#[test]
fn runner_contract_keeps_all_six_observations_not_run() {
    let contract = expected_runner_contract();
    assert_eq!(contract.vary_by.len(), 77);
    assert_eq!(contract.variation_limit, 25);
    assert_eq!(contract.observations.len(), OBSERVATION_INDEXES.len());
    assert!(contract
        .observations
        .iter()
        .all(|observation| observation.initial_execution_state == ExecutionState::NotRun));
    assert!(contract.observations.iter().all(|observation| {
        observation.reference_baseline_state == ReferenceBaselineState::ContentNotVendoredOrCompared
    }));
}

#[test]
fn runner_enumeration_does_not_promote_the_pinned_javascript_control() {
    assert!(is_runner_fixture("nested/example.ts"));
    assert!(is_runner_fixture("nested/example.tsx"));
    assert!(!is_runner_fixture(NOT_ENUMERATED_JS_PATH));
    assert!(!is_runner_fixture("nested/example.jsx"));
    assert!(!is_runner_fixture("nested/example.d.ts.map"));
}

#[test]
fn conformance_case_ids_preserve_paths_and_escape_matrix_punctuation() {
    assert_eq!(
        case_id("a b/example.ts", "target=esnext,module=preserve"),
        "typescript-6.0.3/conformance/a%20b/example.ts#target%3Desnext%2Cmodule%3Dpreserve"
    );
}

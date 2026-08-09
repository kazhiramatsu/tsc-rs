use std::path::{Path, PathBuf};
use std::{fs, thread};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonical workspace")
}

#[test]
fn all_h2_2b_candidate_dispositions_execute() {
    super::run(&workspace()).expect("H2.2b acceptance");
}

#[test]
fn h2_2b_two_worker_execution_is_exact_and_isolated() {
    const CASE_IDS: [&str; 2] = [
        "typescript-6.0.3/compiler/exportDeclarationForModuleOrEnumWithMemberOfSameName.ts#module%3Dcommonjs",
        "typescript-6.0.3/compiler/typeNamedUndefined1.ts#default",
    ];
    let workspace = workspace();
    let artifact: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.join(super::QUALIFICATION_RELATIVE_PATH))
            .expect("read H2.2b qualification"),
    )
    .expect("parse H2.2b qualification");
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
                        .unwrap_or_else(|error| panic!("two-worker H2.2b emit failed: {error}"))
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|worker| worker.join().expect("H2.2b worker panicked"))
            .collect::<Vec<_>>()
    });
    assert_eq!(results[0], (1, 0));
    assert_eq!(results[1], (1, 2));
}

#[test]
fn h2_2b_runtime_namespace_outputs_are_exact() {
    const CASE_IDS: [&str; 2] = [
        "typescript-6.0.3/compiler/exportDeclarationForModuleOrEnumWithMemberOfSameName.ts#module%3Dsystem",
        "typescript-6.0.3/compiler/typeGuardNarrowsIndexedAccessOfKnownProperty7.ts#default",
    ];
    let workspace = workspace();
    let artifact: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.join(super::QUALIFICATION_RELATIVE_PATH))
            .expect("read H2.2b qualification"),
    )
    .expect("parse H2.2b qualification");
    for case_id in CASE_IDS {
        let case = artifact["cases"]
            .as_array()
            .and_then(|cases| cases.iter().find(|case| case["case_id"] == case_id))
            .unwrap_or_else(|| panic!("missing namespace case {case_id}"));
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
                            .any(|root| root["feature"] == "runtime-namespaces")
                    })
                }),
            "{case_id}: runtime-namespace owner source"
        );
    }
}

#[test]
fn h2_2b_later_owner_controls_are_joined_to_h2_2d() {
    let workspace = workspace();
    let artifact: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.join(super::QUALIFICATION_RELATIVE_PATH))
            .expect("read H2.2b qualification"),
    )
    .expect("parse H2.2b qualification");
    let deferred = artifact["cases"]
        .as_array()
        .expect("qualification cases")
        .iter()
        .filter(|case| case["disposition"] == "deferred-to-slices")
        .collect::<Vec<_>>();
    let h2_2d_artifact: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.join(super::H2_2D_QUALIFICATION_RELATIVE_PATH))
            .expect("read H2.2d qualification"),
    )
    .expect("parse H2.2d qualification");
    let h2_2d_cases = h2_2d_artifact["cases"]
        .as_array()
        .expect("H2.2d qualification cases");
    assert_eq!(deferred.len(), 3);
    for case in deferred {
        assert_eq!(case["required_slices"], serde_json::json!(["H2.2d"]));
        assert!(
            crate::h2_2d_acceptance::promotes_historical_case(case, h2_2d_cases)
                .unwrap_or_else(|error| panic!("{}: {error}", case["case_id"])),
            "{}: missing H2.2d exact promotion",
            case["case_id"]
        );
    }
}

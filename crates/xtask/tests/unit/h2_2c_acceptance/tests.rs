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

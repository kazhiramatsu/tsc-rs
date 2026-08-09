use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::{fs, thread};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonical workspace")
}

#[test]
fn all_h2_2d_candidate_dispositions_execute() {
    super::run(&workspace()).expect("H2.2d acceptance");
}

#[test]
fn h2_2d_two_worker_execution_is_exact_and_isolated() {
    const CASE_IDS: [&str; 2] = [
        "typescript-6.0.3/compiler/moduleNodeImportRequireEmit.ts#target%3Desnext",
        "typescript-6.0.3/conformance/dynamicImport/importCallExpressionInExportEqualsAMD.ts#default",
    ];
    let workspace = workspace();
    let artifact: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.join(super::QUALIFICATION_RELATIVE_PATH))
            .expect("read H2.2d qualification"),
    )
    .expect("parse H2.2d qualification");
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
                        .unwrap_or_else(|error| panic!("two-worker H2.2d emit failed: {error}"))
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|worker| worker.join().expect("H2.2d worker panicked"))
            .collect::<Vec<_>>()
    });
    assert_eq!(results[0], (1, 0));
    assert_eq!(results[1], (2, 1));
}

#[test]
fn h2_2d_import_export_equals_outputs_are_exact() {
    const CASE_IDS: [&str; 2] = [
        "typescript-6.0.3/conformance/externalModules/topLevelAwait.2.ts#module%3Desnext",
        "typescript-6.0.3/conformance/dynamicImport/importCallExpressionInExportEqualsCJS.ts#default",
    ];
    let workspace = workspace();
    let artifact: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.join(super::QUALIFICATION_RELATIVE_PATH))
            .expect("read H2.2d qualification"),
    )
    .expect("parse H2.2d qualification");
    for case_id in CASE_IDS {
        let case = artifact["cases"]
            .as_array()
            .and_then(|cases| cases.iter().find(|case| case["case_id"] == case_id))
            .unwrap_or_else(|| panic!("missing import/export-equals case {case_id}"));
        super::execute_observed(&workspace, case)
            .unwrap_or_else(|error| panic!("{case_id}: {error}"));
        assert!(
            case["files"]
                .as_array()
                .expect("case files")
                .iter()
                .any(|file| {
                    file["feature_roots"].as_array().is_some_and(|roots| {
                        roots.iter().any(|root| {
                            matches!(
                                root["feature"].as_str(),
                                Some("import-equals" | "export-equals")
                            )
                        })
                    })
                }),
            "{case_id}: import/export-equals owner source"
        );
    }
}

#[test]
fn h2_2d_historical_source_deferred_rows_are_exactly_promoted() {
    const HISTORICAL_QUALIFICATIONS: [&str; 4] = [
        "ratchets/h2-1b-qualification.v1.json",
        "ratchets/h2-1c-qualification.v1.json",
        "ratchets/h2-1e-qualification.v1.json",
        "ratchets/h2-2b-qualification.v1.json",
    ];

    let workspace = workspace();
    let artifact: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.join(super::QUALIFICATION_RELATIVE_PATH))
            .expect("read H2.2d qualification"),
    )
    .expect("parse H2.2d qualification");
    let h2_2d_cases = artifact["cases"]
        .as_array()
        .expect("H2.2d qualification cases");
    let exact_ids = h2_2d_cases
        .iter()
        .map(|case| case["case_id"].as_str().expect("H2.2d case id").to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(exact_ids.len(), 9);

    let mut promoted_ids = BTreeSet::new();
    for relative_path in HISTORICAL_QUALIFICATIONS {
        let historical: serde_json::Value = serde_json::from_slice(
            &fs::read(workspace.join(relative_path)).expect("read historical qualification"),
        )
        .expect("parse historical qualification");
        for case in historical["cases"]
            .as_array()
            .expect("historical qualification cases")
        {
            let Some(case_id) = case["case_id"].as_str() else {
                continue;
            };
            if !exact_ids.contains(case_id) {
                continue;
            }
            assert_eq!(case["disposition"], "deferred-to-slices");
            assert_eq!(
                case["diagnostic_disposition"]["state"],
                "not-observed-source-deferred"
            );
            assert!(
                super::promotes_historical_case(case, h2_2d_cases)
                    .unwrap_or_else(|error| panic!("{case_id}: {error}")),
                "{case_id}: missing exact H2.2d promotion"
            );
            promoted_ids.insert(case_id.to_owned());
        }
    }
    assert_eq!(promoted_ids, exact_ids);
}

use super::*;

fn paths(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn documentation_framework_and_new_ci_edits_select_no_acceptance_slice() {
    let plan = plan_from_paths(&paths(&[
        "README.md",
        "docs/design/greenfield/foo.md",
        "new-ci/src/lib.rs",
    ]));
    assert_eq!(plan.mode, "none");
    assert!(plan.selected.is_empty());
    assert!(!plan.skipped.is_empty());
}

#[test]
fn shared_compiler_edits_select_every_slice() {
    let plan = plan_from_paths(&paths(&["crates/checker/src/lib.rs"]));
    assert_eq!(plan.mode, "all");
    assert_eq!(plan.selected.len(), SLICE_IDS.len());
    assert_eq!(plan.selected.first().unwrap().id, "conformance");
    assert_eq!(plan.selected.last().unwrap().id, "h2-5g");
}

#[test]
fn individual_acceptance_module_selects_only_its_slice() {
    let plan = plan_from_paths(&paths(&["crates/xtask/src/h2_1b_acceptance.rs"]));
    assert_eq!(plan.mode, "affected");
    assert_eq!(
        plan.selected
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["h2-1b"]
    );
}

#[test]
fn shared_h2_2c_module_selects_all_of_its_callers() {
    let plan = plan_from_paths(&paths(&["crates/xtask/src/h2_2c_acceptance.rs"]));
    assert_eq!(plan.mode, "affected");
    assert_eq!(
        plan.selected
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        [
            "h2-2c", "h2-4a", "h2-4b", "h2-5a", "h2-5b", "h2-5c", "h2-5d", "h2-5e", "h2-5f",
            "h2-5g"
        ]
    );
}

#[test]
fn shared_h2_2d_module_selects_its_historical_promoters() {
    let plan = plan_from_paths(&paths(&["crates/xtask/src/h2_2d_acceptance.rs"]));
    assert_eq!(plan.mode, "affected");
    assert_eq!(
        plan.selected
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["h2-1a", "h2-1b", "h2-1c", "h2-1e", "h2-2a", "h2-2b", "h2-2d"]
    );
}

#[test]
fn shared_h2_3c_module_selects_its_historical_promoter() {
    let plan = plan_from_paths(&paths(&["crates/xtask/src/h2_3c_acceptance.rs"]));
    assert_eq!(plan.mode, "affected");
    assert_eq!(
        plan.selected
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["h2-3b", "h2-3c"]
    );
}

#[test]
fn unknown_paths_fail_closed_to_all() {
    let plan = plan_from_paths(&paths(&["new-tooling/unknown.toml"]));
    assert_eq!(plan.mode, "all");
    assert_eq!(plan.selected.len(), SLICE_IDS.len());
}

#[test]
fn duplicate_and_empty_paths_are_canonicalized() {
    let plan = plan_from_paths(&paths(&[
        "",
        "crates/xtask/src/h1_emit_acceptance.rs",
        "crates/xtask/src/h1_emit_acceptance.rs",
    ]));
    assert_eq!(
        plan.changed_paths,
        ["crates/xtask/src/h1_emit_acceptance.rs"]
    );
    assert_eq!(plan.selected.len(), 1);
}

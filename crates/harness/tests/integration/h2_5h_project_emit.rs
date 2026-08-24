//! H2.5h / CA-4 focused loader-shape checks for `load_project_emit`: the
//! CA-3 §5.3a option floor over the shared project mount, one explicit-root
//! descriptor and one config-arm descriptor. The band-wide byte gate is
//! `cargo xtask h2-5h-acceptance` (`run_h2_5h`); these tests pin the loader
//! contract the packet's §5.1 specifies.

use std::path::{Path, PathBuf};

use tsc_harness::upstream_suites::execution::{
    load_project_emit, load_recorded_execution_plans, UpstreamExecutionInput,
};
use tsc_program::ProgramLoadLimits;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonical workspace")
}

fn limits() -> ProgramLoadLimits {
    ProgramLoadLimits::new(256, 2_048, 64, 16 * 1_024 * 1_024, 128 * 1_024 * 1_024)
}

fn project_plan(
    corpus: &tsc_harness::upstream_suites::execution::UpstreamExecutionCorpus,
    case_id: &str,
) -> tsc_harness::upstream_suites::execution::ProjectExecutionPlan {
    corpus
        .plans
        .iter()
        .find_map(|recorded| match &recorded.input {
            UpstreamExecutionInput::Project(plan)
                if recorded.provenance.case_id.as_ref() == case_id =>
            {
                Some(plan.clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing project plan {case_id}"))
}

#[test]
fn explicit_root_descriptor_loads_with_the_observation_floor() {
    let workspace = workspace_root();
    let corpus = load_recorded_execution_plans(&workspace).expect("recorded plans");
    let plan = project_plan(
        &corpus,
        "typescript-6.0.3/project/baseline.json#module%3Damd",
    );
    let loaded = load_project_emit(&workspace, &plan, limits()).expect("emit load");
    let options = &loaded.effective_compiler_options;
    assert_eq!(options.module, Some(2), "amd variant");
    assert_eq!(options.module_resolution, Some(1), "Classic floor");
    assert_eq!(options.new_line, Some(0), "CRLF pin");
    assert_eq!(options.no_emit, None, "no forced noEmit");
    assert_eq!(options.no_error_truncation, Some(false));
    assert_eq!(options.skip_default_lib_check, Some(false));
    assert_eq!(
        loaded.root_names.as_ref(),
        [PathBuf::from("/.src/tests/cases/projects/baseline/emit.ts")],
        "every requested root, normalized against the project cwd"
    );
}

#[test]
fn map_root_descriptor_applies_the_emit_options_instead_of_rejecting() {
    let workspace = workspace_root();
    let corpus = load_recorded_execution_plans(&workspace).expect("recorded plans");
    let plan = project_plan(
        &corpus,
        "typescript-6.0.3/project/mapRootWithNoSourceMapOption.json#module%3Dcommonjs",
    );
    let loaded = load_project_emit(&workspace, &plan, limits())
        .expect("the emit lane applies mapRoot as an ordinary option");
    assert_eq!(
        loaded.effective_compiler_options.map_root.as_deref(),
        Some("../mapFiles"),
        "the H0 adapter's rejection is not the emit-lane record"
    );
    assert_eq!(loaded.effective_compiler_options.module, Some(1));
}

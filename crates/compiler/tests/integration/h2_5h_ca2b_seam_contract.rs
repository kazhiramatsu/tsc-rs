//! H2.5h / CA-2b focused replays for the checker/harness report-parity
//! cluster. Every expectation below is copied from the frozen
//! `ratchets/h2-5h-qualification.v1.json` observations (never
//! hand-authored); the recorded-plan route drives the same harness
//! loaders the acceptance uses, so the `noEmitOnError` settings mapping
//! is exercised end-to-end. The blocked-emit BEHAVIOR itself is
//! production-owned and already upstream-exact (CA-2b review probe);
//! its band-wide machine gate arrives with CA-4's `run_h2_5h`.

use std::path::PathBuf;

use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_harness::upstream_suites::execution::{
    load_compiler_emit, load_recorded_execution_plans, CompilerExecutionPlan,
    UpstreamExecutionInput, UpstreamExecutionPlan,
};
use tsc_program::ProgramLoadLimits;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn limits() -> ProgramLoadLimits {
    ProgramLoadLimits::new(128, 1_024, 32, 8 * 1_024 * 1_024, 64 * 1_024 * 1_024)
}

fn compiler_plan<'a>(
    plans: &'a [UpstreamExecutionPlan],
    case_id: &str,
) -> &'a CompilerExecutionPlan {
    let plan = plans
        .iter()
        .find(|plan| plan.provenance.case_id.as_ref() == case_id)
        .unwrap_or_else(|| panic!("missing upstream compiler case {case_id}"));
    match &plan.input {
        UpstreamExecutionInput::Compiler(plan) => plan,
        UpstreamExecutionInput::Project(_) => panic!("expected compiler case {case_id}"),
    }
}

fn reported_rows(case_id: &str) -> Vec<(u32, Option<u32>, Option<u32>)> {
    let workspace = workspace_root();
    let corpus = load_recorded_execution_plans(&workspace)
        .unwrap_or_else(|error| panic!("failed to load upstream plans: {error}"));
    let plan = compiler_plan(&corpus.plans, case_id);
    let prepared = load_compiler_emit(&workspace, plan, limits())
        .unwrap_or_else(|error| panic!("{case_id}: prepare failed: {error}"));
    let mut sink = MemoryOutputSink::new();
    let (_, reported) = ProgramSession::new(prepared)
        .emit_with_reported_diagnostics_for_harness(&mut sink)
        .unwrap_or_else(|error| panic!("{case_id}: emit failed: {error}"));
    reported
        .iter()
        .map(|diagnostic| (diagnostic.code(), diagnostic.start, diagnostic.length))
        .collect()
}

/// tsc reports TS2396 on the whole parameter node
/// (`errorSkippedOn("noEmit", p, ...)`, `_tsc.js:83234-83236`). The frozen
/// observation for this admitted H2.5h band row records exactly the two
/// parameter-spanned rows below; the pre-CA-2b port reported the name
/// identifier spans (24,9)/(112,9) instead.
#[test]
fn collision_arguments_reports_2396_on_the_parameter_node() {
    let rows = reported_rows(
        "typescript-6.0.3/compiler/collisionArgumentsArrowFunctions.ts#alwaysstrict%3Dfalse%2Ctarget%3Des5",
    );
    assert_eq!(
        rows,
        vec![(2396, Some(21), Some(12)), (2396, Some(112), Some(17))],
        "frozen h2-5h-qualification observation for the case",
    );
}

/// `getIgnoreDeprecationsVersion` (`_tsc.js:125052-125061`) accepts exactly
/// "5.0" and "6.0"; the invalid "5.1" adds the memoized 5103 row
/// (`reportInvalidIgnoreDeprecations`, `_tsc.js:122639`) while the
/// deprecation rows still fire. The frozen observation records ten rows
/// with 5103 at the config value span (364,5).
#[test]
fn deprecated_compiler_options6_reports_the_invalid_ignore_deprecations_row() {
    let rows = reported_rows("typescript-6.0.3/compiler/deprecatedCompilerOptions6.ts#default");
    assert_eq!(rows.len(), 10, "frozen observation row count");
    assert!(
        rows.contains(&(5103, Some(364), Some(5))),
        "the invalid-ignoreDeprecations row at the config value span: {rows:?}",
    );
}

/// Polarity guard: a VALID `"5.0"` must not gain a 5103 row (the green
/// `deprecatedCompilerOptions2` replay would regress if the accepted set
/// shrank to "6.0" alone).
#[test]
fn deprecated_compiler_options2_accepts_five_zero_without_5103() {
    let rows = reported_rows("typescript-6.0.3/compiler/deprecatedCompilerOptions2.ts#default");
    assert!(
        rows.iter().all(|row| row.0 != 5103),
        "no invalid-value row for the accepted \"5.0\": {rows:?}",
    );
}

/// The harness settings mapper must carry `@noEmitOnError` into the
/// prepared options (it sat in the silent-ignore arm before CA-2b, so no
/// harness execution was ever emit-blocked and the blocked-row
/// observations could not be reproduced).
#[test]
fn no_emit_on_error_setting_is_mapped_into_the_prepared_options() {
    let workspace = workspace_root();
    let corpus = load_recorded_execution_plans(&workspace)
        .unwrap_or_else(|error| panic!("failed to load upstream plans: {error}"));
    let plan = compiler_plan(
        &corpus.plans,
        "typescript-6.0.3/compiler/noEmitOnError.ts#default",
    );
    let prepared = load_compiler_emit(&workspace, plan, limits())
        .unwrap_or_else(|error| panic!("noEmitOnError.ts: prepare failed: {error}"));
    assert_eq!(
        prepared.compiler_options().no_emit_on_error,
        Some(true),
        "the harness mapper must not drop @noEmitOnError",
    );
}

//! H2.5h / CA-2a focused replay for the opened promote lanes: a decorated
//! ES5 class-wrapper case whose production emit must reproduce the frozen
//! `ratchets/h2-5h-qualification.v1.json` oracle write byte-for-byte
//! (asserted via the recorded callback sha256 — never hand-authored).
//! The unit driver cannot host this lane (no typescript/class-fields
//! passes), so the recorded-plan route is the guard.

use std::path::PathBuf;

use sha2::{Digest, Sha256};
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

/// The frozen H2.5h observation for the decorated metadata case: exactly one
/// JavaScript write at `/.src/emitDecoratorMetadata_object.js`, callback sha256
/// `4282db8b...` (1,253 bytes). The decorated promote lane produces the
/// wrapper + `__decorate` composition through the full production pipeline.
#[test]
fn decorated_class_wrapper_reproduces_the_frozen_oracle_write() {
    let case_id = "typescript-6.0.3/compiler/emitDecoratorMetadata_object.ts#target%3Des5";
    let workspace = workspace_root();
    let corpus = load_recorded_execution_plans(&workspace)
        .unwrap_or_else(|error| panic!("failed to load upstream plans: {error}"));
    let plan = compiler_plan(&corpus.plans, case_id);
    let prepared = load_compiler_emit(&workspace, plan, limits())
        .unwrap_or_else(|error| panic!("{case_id}: prepare failed: {error}"));
    let mut sink = MemoryOutputSink::new();
    ProgramSession::new(prepared)
        .emit_with_reported_diagnostics_for_harness(&mut sink)
        .unwrap_or_else(|error| panic!("{case_id}: emit failed: {error}"));
    let writes = sink.writes();
    assert_eq!(writes.len(), 1, "expected exactly one write");
    let write = &writes[0];
    assert_eq!(
        write.path().to_string_lossy(),
        "/.src/emitDecoratorMetadata_object.js"
    );
    let digest = Sha256::digest(write.callback_text().as_bytes());
    assert_eq!(
        format!("{digest:x}"),
        "4282db8b7fc0c6e9472e9a67b3526715c5698e785b980935d6b364b8fafed3cc",
        "callback bytes diverged from the frozen oracle observation"
    );
}

use std::path::PathBuf;

use tsc_compiler::ProgramSession;
use tsc_harness::upstream_suites::execution::{
    load_compiler_no_emit, load_recorded_execution_plans, CompilerExecutionPlan,
    UpstreamExecutionInput, UpstreamExecutionPlan,
};
use tsc_program::ProgramLoadLimits;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn plan<'a>(plans: &'a [UpstreamExecutionPlan], case_id: &str) -> &'a CompilerExecutionPlan {
    let plan = plans
        .iter()
        .find(|plan| plan.provenance.case_id.as_ref() == case_id)
        .unwrap_or_else(|| panic!("missing upstream compiler case {case_id}"));
    match &plan.input {
        UpstreamExecutionInput::Compiler(plan) => plan,
        UpstreamExecutionInput::Project(_) => panic!("expected compiler case {case_id}"),
    }
}

fn limits() -> ProgramLoadLimits {
    ProgramLoadLimits::new(128, 1_024, 32, 8 * 1_024 * 1_024, 64 * 1_024 * 1_024)
}

#[test]
fn compiler_session_runs_recorded_no_emit_programs() {
    let workspace = workspace_root();
    let corpus = load_recorded_execution_plans(&workspace)
        .unwrap_or_else(|error| panic!("failed to load upstream plans: {error}"));
    for case_id in [
        "typescript-6.0.3/compiler/2dArrays.ts#default",
        "typescript-6.0.3/compiler/augmentExportEquals2.ts#default",
        "typescript-6.0.3/compiler/typeReferenceDirectives3.ts#default",
        "typescript-6.0.3/compiler/configFileExtendsAsList.ts#default",
        "typescript-6.0.3/compiler/moduleResolutionWithSymlinks_preserveSymlinks.ts#default",
        "typescript-6.0.3/compiler/arrayIterationLibES5TargetDifferent.ts#nolib%3Dtrue%2Ctarget%3Des5",
        "typescript-6.0.3/compiler/declarationEmitForGlobalishSpecifierSymlink.ts#default",
        "typescript-6.0.3/compiler/duplicatePackage.ts#default",
        "typescript-6.0.3/compiler/duplicatePackage_globalMerge.ts#default",
        "typescript-6.0.3/compiler/duplicatePackage_packageIdIncludesSubModule.ts#default",
        "typescript-6.0.3/compiler/duplicatePackage_referenceTypes.ts#default",
        "typescript-6.0.3/compiler/duplicatePackage_relativeImportWithinPackage.ts#default",
        "typescript-6.0.3/compiler/duplicatePackage_relativeImportWithinPackage_scoped.ts#default",
        "typescript-6.0.3/compiler/duplicatePackage_subModule.ts#default",
        "typescript-6.0.3/compiler/duplicatePackage_withErrors.ts#default",
        "typescript-6.0.3/compiler/moduleResolutionPackageIdWithRelativeAndAbsolutePath.ts#default",
        "typescript-6.0.3/compiler/pathMappingBasedModuleResolution_rootImport_aliasWithRoot_differentRootTypes.ts#default",
    ] {
        let prepared = load_compiler_no_emit(&workspace, plan(&corpus.plans, case_id), limits())
            .unwrap_or_else(|error| panic!("failed to load {case_id}: {error}"));
        let outcome = ProgramSession::new(prepared)
            .run()
            .unwrap_or_else(|error| panic!("failed to execute {case_id}: {error:?}"));
        assert!(outcome.config_diagnostics().is_empty(), "{case_id}");
    }
}

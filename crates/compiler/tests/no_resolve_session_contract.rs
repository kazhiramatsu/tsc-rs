use std::path::PathBuf;

use tsc_compiler::ProgramSession;
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_no_lib_program, plan_source_requests, CompilerOptions, ProgramLoadLimits, ProgramOptions,
    ResolutionOutcome, ResolvedModuleTarget, UnloadedModuleReason,
};

const LIMITS: ProgramLoadLimits = ProgramLoadLimits::new(64, 128, 16, 1 << 20, 1 << 22);

#[test]
fn no_resolve_module_rows_reach_the_authoritative_checker_seam() {
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/main.ts",
            b"import { value } from './dependency';\nexport { value };\n".to_vec(),
        )
        .file("/work/dependency.ts", b"export const value = 1;\n".to_vec())
        .build()
        .expect("build noResolve session host");
    let prepared = load_no_lib_program(
        &host,
        &[PathBuf::from("/work/main.ts")],
        CompilerOptions {
            no_emit: Some(true),
            no_resolve: Some(true),
            module: Some(1),
            module_resolution: Some(2),
            ..CompilerOptions::default()
        },
        ProgramOptions::default()
            .with_no_lib(true)
            .with_types(Vec::new()),
        LIMITS,
    )
    .expect("load noResolve prepared program");

    assert_eq!(prepared.source_files().len(), 1);
    let plan = plan_source_requests(&prepared.source_files()[0], prepared.compiler_options())
        .expect("plan noResolve module request");
    let key = plan
        .module_requests()
        .iter()
        .find(|key| key.specifier() == "./dependency")
        .expect("planned noResolve module request");
    let resolution = prepared
        .resolutions()
        .require_module(key)
        .expect("noResolve module row");
    let ResolutionOutcome::Resolved(module) = resolution.outcome() else {
        panic!("noResolve module request must resolve");
    };
    assert!(matches!(
        module.target(),
        ResolvedModuleTarget::Unloaded {
            reason: UnloadedModuleReason::NoResolve,
            ..
        }
    ));

    ProgramSession::new(prepared)
        .run()
        .expect("noResolve row must be consumable by the checker");
}

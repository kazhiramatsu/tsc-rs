//! Read-ahead cache consolidation preserves the command session's products.
use std::{num::NonZeroUsize, path::PathBuf};
use tsc_compiler::{CheckerBudget, ProgramSession};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    CompilerOptions, LibraryCatalog, ProgramLoadLimits, ProgramOptions, WorkerBudget,
};

#[test]
fn imported_roots_preserve_command_products_after_resolution_cache_consolidation() {
    let mut host = MemoryCompilerHost::builder("/p")
        .file("/p/globals.d.ts", b"interface IArguments {} interface Array<T> { length: number; [n: number]: T; } interface Object {} interface Function {} interface CallableFunction {} interface NewableFunction {} interface String {} interface Number {} interface Boolean {} interface RegExp {}".to_vec())
        .file("/p/shared.ts", b"export const value = 1;".to_vec());
    let mut roots = vec![PathBuf::from("/p/globals.d.ts")];
    for index in 0..270 {
        let name = format!("/p/d{index:03}/main.ts");
        roots.push(PathBuf::from(&name));
        host = host.file(
            &name,
            b"import { value } from '../shared'; export const result: string = value;".to_vec(),
        );
    }
    let host = host.build().unwrap();
    for (list_only, no_check, incremental) in [
        (false, false, false),
        (false, true, false),
        (false, false, true),
        (false, true, true),
        (true, false, false),
        (true, false, true),
    ] {
        let mut expected = None;
        for workers in [1, 4, 16] {
            let budget = WorkerBudget::new(NonZeroUsize::new(workers).unwrap());
            let prepared = tsc_program::load_program(
                &host,
                &roots,
                CompilerOptions {
                    no_emit: Some(true),
                    no_check: Some(no_check),
                    list_files_only: Some(list_only),
                    incremental: Some(incremental),
                    ts_build_info_file: incremental.then(|| "/p/state.tsbuildinfo".into()),
                    ..Default::default()
                },
                ProgramOptions::default()
                    .with_no_lib(true)
                    .with_types(Vec::new())
                    .with_identity_domain(tsc_types::IdentityDomain::new())
                    .with_source_binding(),
                &LibraryCatalog::typescript_7_1("bundled:///libs"),
                ProgramLoadLimits::new(1000, 1000, 100, 1 << 20, 4 << 20).with_workers(budget),
            )
            .unwrap();
            assert_eq!(prepared.source_files().len(), roots.len() + 1);
            let outcome = ProgramSession::new(prepared)
                .with_worker_budget(budget)
                .with_checker_budget(CheckerBudget::new(
                    NonZeroUsize::new(workers.min(4)).unwrap(),
                ))
                .with_list_files_only(list_only)
                .with_testing(true)
                .run_no_emit_command()
                .unwrap();
            assert_eq!(
                outcome.semantic_diagnostics().len(),
                if no_check || list_only { 0 } else { 270 }
            );
            if incremental {
                assert_eq!(outcome.program_report().unwrap().len(), roots.len() + 1);
            }
            if incremental && !list_only {
                assert!(outcome.build_info().is_some());
            }
            let actual = (
                outcome.diagnostics().cloned().collect::<Vec<_>>(),
                outcome.build_info().cloned(),
                outcome.program_report().map(<[_]>::to_vec),
            );
            if let Some(expected) = &expected {
                assert_eq!(&actual, expected, "workers={workers}, list={list_only}, noCheck={no_check}, incremental={incremental}");
            } else {
                expected = Some(actual);
            }
        }
    }
}

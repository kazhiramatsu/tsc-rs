//! Mixed ready binds and overflow preserve the command and emit products.
use std::{num::NonZeroUsize, path::PathBuf};
use tsc_compiler::{CheckerBudget, MemoryOutputSink, ProgramSession};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_emitting_program, CompilerOptions, LibraryCatalog, ProgramLoadLimits, ProgramOptions,
    WorkerBudget,
};

#[test]
fn ready_bind_command_sessions_preserve_incremental_facts_and_mode_demand() {
    let mut host = MemoryCompilerHost::builder("/p").file(
        "/p/globals.d.ts",
        b"interface IArguments {} interface Array<T> { length: number; [n: number]: T; } interface Object {} interface Function {} interface CallableFunction {} interface NewableFunction {} interface String {} interface Number {} interface Boolean {} interface RegExp {}".to_vec(),
    );
    host = host.file("/p/shared.ts", b"export const shared = 1;".to_vec());
    let overflow = format!(
        "{}function f(x) {{ return x; }}\nexport class Overflow {{}}",
        "/** @overload\n * @param {string} x\n * @returns {string}\n */\n".repeat(32)
    );
    host = host.file("/p/overflow.js", overflow.into_bytes());
    let mut roots = vec![
        PathBuf::from("/p/globals.d.ts"),
        PathBuf::from("/p/overflow.js"),
    ];
    for index in 0..260 {
        let name = format!("/p/source{index:03}.ts");
        roots.push(PathBuf::from(&name));
        host = host.file(
            &name,
            format!("import {{ shared }} from './shared'; export const value{index}: number = 'error'; export const copy = shared;").into_bytes(),
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
            for demand in [false, true] {
                let budget = WorkerBudget::new(NonZeroUsize::new(workers).unwrap());
                let mut options = ProgramOptions::default()
                    .with_no_lib(true)
                    .with_types(Vec::new())
                    .with_identity_domain(tsc_types::IdentityDomain::new());
                if demand {
                    options = options.with_source_binding();
                }
                let prepared = tsc_program::load_program(
                    &host,
                    &roots,
                    CompilerOptions {
                        no_emit: Some(true),
                        allow_js: true,
                        no_check: Some(no_check),
                        list_files_only: Some(list_only),
                        incremental: Some(incremental),
                        ts_build_info_file: incremental.then(|| "/p/state.tsbuildinfo".into()),
                        ..Default::default()
                    },
                    options,
                    &LibraryCatalog::typescript_7_1("bundled:///libs"),
                    ProgramLoadLimits::new(1000, 1000, 100, 1 << 20, 4 << 20).with_workers(budget),
                )
                .unwrap();
                // This entry requests command build information just as the
                // CLI does; run() alone does not exercise incremental demand.
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
                if list_only && !incremental {
                    assert_eq!(outcome.work_counters().bound_documents(), 0);
                } else {
                    assert!(outcome.work_counters().bound_documents() > 0);
                }
                if incremental {
                    assert_eq!(outcome.program_report().unwrap().len(), roots.len() + 1);
                }
                if incremental && !list_only {
                    assert!(outcome.build_info().is_some());
                }
                if no_check || list_only {
                    assert!(outcome.semantic_diagnostics().is_empty());
                } else {
                    assert_eq!(outcome.semantic_diagnostics().len(), 260);
                }
                let actual = (
                    outcome.diagnostics().cloned().collect::<Vec<_>>(),
                    outcome.build_info().cloned(),
                    outcome.program_report().map(<[_]>::to_vec),
                );
                if let Some(expected) = &expected {
                    assert_eq!(&actual, expected, "workers={workers}, demand={demand}, list={list_only}, noCheck={no_check}, incremental={incremental}");
                } else {
                    expected = Some(actual);
                }
            }
        }
    }
}

#[test]
fn small_command_programs_preserve_emit_at_every_worker_budget() {
    let sources = [
        ("/p/globals.d.ts", "interface IArguments {} interface Array<T> { length: number; [n: number]: T; } interface Object {} interface Function {} interface CallableFunction {} interface NewableFunction {} interface String {} interface Number {} interface Boolean {} interface RegExp {}"),
        ("/p/a.ts", "export class A { #value = 1; get value() { return this.#value; } }"),
        ("/p/b.ts", "import { A } from './a.js'; export const result = new A().value;"),
    ];
    let mut host = MemoryCompilerHost::builder("/p");
    for (path, text) in sources {
        host = host.file(path, text.as_bytes().to_vec());
    }
    let host = host.build().unwrap();
    let roots: Vec<_> = sources
        .iter()
        .map(|(path, _)| PathBuf::from(path))
        .collect();
    let mut expected = None;
    for workers in [1, 4, 16] {
        for no_check in [false, true] {
            let budget = WorkerBudget::new(NonZeroUsize::new(workers).unwrap());
            let prepared = load_emitting_program(
                &host,
                &roots,
                CompilerOptions {
                    no_emit: Some(false),
                    no_check: Some(no_check),
                    target: Some(99),
                    module: Some(199),
                    declaration: Some(true),
                    declaration_map: Some(true),
                    source_map: Some(true),
                    out_dir: Some("/out".into()),
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
            let mut sink = MemoryOutputSink::default();
            let outcome = ProgramSession::new(prepared)
                .with_worker_budget(budget)
                .with_checker_budget(CheckerBudget::new(
                    NonZeroUsize::new(workers.min(4)).unwrap(),
                ))
                .emit(&mut sink)
                .unwrap();
            let actual = (outcome, sink.into_writes());
            assert!(!actual.1.is_empty());
            if let Some(expected) = &expected {
                assert_eq!(&actual, expected, "workers={workers}, noCheck={no_check}");
            } else {
                expected = Some(actual);
            }
        }
    }
}

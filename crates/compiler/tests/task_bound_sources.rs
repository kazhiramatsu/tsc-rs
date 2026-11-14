//! Finite task binds preserve diagnostics, JavaScript, declarations and maps.
use std::{num::NonZeroUsize, path::PathBuf};
use tsc_compiler::{CheckerBudget, MemoryOutputSink, ProgramSession};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_emitting_program, CompilerOptions, LibraryCatalog, ProgramLoadLimits, ProgramOptions,
    WorkerBudget,
};

#[test]
fn task_bound_roots_emit_the_same_products_at_every_budget() {
    let sources = [
        ("/p/globals.d.ts", "interface IArguments { length: number; callee: Function; } interface Array<T> { length: number; [n: number]: T; } interface Object {} interface Function {} interface CallableFunction extends Function {} interface NewableFunction extends Function {} interface String {} interface Number {} interface Boolean {} interface RegExp {}"),
        ("/p/main.ts", "export declare const key: unique symbol; export interface Record { [key]: number; } export class A { #value = 1; read(other: A) { return this.#value + other.#value; } }"),
        ("/p/common.cts", "export class C { #value = 2; read() { return this.#value; } }"),
        ("/p/module.mts", "import { A } from './main.js'; export const a = new A();"),
        ("/p/expando.js", "/** @param {number} n */ export function f(n) { return n; } f.a = 1; f.b = 'b'; f.c = true; Object.defineProperty(f, 'd', { value: 4 });"),
        ("/p/legacy.cjs", "function legacy() {} module.exports = legacy; legacy.value = 1;"),
        ("/p/view.tsx", "export const view = <div />;"),
        ("/p/data.json", "{\"value\": 1}"),
    ];
    let mut roots: Vec<PathBuf> = sources
        .iter()
        .map(|(name, _)| PathBuf::from(name))
        .collect();
    let mut host =
        MemoryCompilerHost::builder("/p").file("/p/package.json", br#"{"type":"module"}"#.to_vec());
    for (name, text) in sources {
        host = host.file(name, text.as_bytes().to_vec());
    }
    // The small-input path keeps root binding in the checker. Exercise the
    // ordered-load crew with declaration roots that add no emitted products.
    for index in 0..260 {
        let name = format!("/p/helper{index:03}.d.ts");
        roots.push(PathBuf::from(&name));
        host = host.file(
            &name,
            format!("declare const helper{index}: number;").into_bytes(),
        );
    }
    let host = host.build().unwrap();
    let mut expected = None;
    for workers in [1, 4, 16] {
        for demand in [false, true] {
            let budget = WorkerBudget::new(NonZeroUsize::new(workers).unwrap());
            let mut program_options = ProgramOptions::default()
                .with_no_lib(true)
                .with_types(Vec::new())
                .with_identity_domain(tsc_types::IdentityDomain::new());
            if demand {
                program_options = program_options.with_source_binding();
            }
            let prepared = load_emitting_program(
                &host,
                &roots,
                CompilerOptions {
                    no_emit: Some(false),
                    target: Some(99),
                    module: Some(199),
                    module_resolution: Some(99),
                    allow_js: true,
                    check_js: Some(true),
                    resolve_json_module: Some(true),
                    jsx: Some(4),
                    declaration: Some(true),
                    declaration_map: Some(true),
                    source_map: Some(true),
                    out_dir: Some("/out".into()),
                    ..Default::default()
                },
                program_options,
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
                assert_eq!(&actual, expected, "workers={workers}, demand={demand}");
            } else {
                expected = Some(actual);
            }
        }
    }
}

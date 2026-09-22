//! Loader-parse adoption contract: a program session adopts the loader's
//! request-planning parse of each source (relocating it into its identity
//! domain) instead of parsing again, reports that work honestly through the
//! `adopted_documents` counter, and falls back to parsing, with identical
//! diagnostics, when the slot was already consumed or the session would have
//! parsed with different options. Every session is also run under the serial
//! and a four-worker `WorkerBudget`, which must publish identical outcomes.
//!
//! Expected diagnostics are the complete fresh TypeScript 6.0.3 records for
//! exactly this fixture (roots `/work/globals.d.ts`, `/work/a.ts`,
//! `/work/b.ts`; `noEmit`, `strict`, `noLib`, `types: []`), observed through
//! the vendored compiler's public getters:
//! `target/benchmarks/fable51-parity-20260922/review/adoption-oracle-20260922/stdout.json`
//! (integrator run, 2026-09-22). Source order there is globals, dep, a, b;
//! `getSemanticDiagnostics()` returns a.ts, b.ts, dep.ts; one Suggestion
//! (TS6133, reportsUnnecessary) on a.ts; no syntactic, options or global
//! rows, so the command-line semantic gate is open. A CLI run of the same
//! files (`tsc --noEmit --strict --noLib --pretty false a.ts b.ts lib.d.ts`,
//! exit 2) reports the same three errors.

use std::num::NonZeroUsize;
use std::path::PathBuf;

use tsc_compiler::{JSDocParsingMode, NoEmitOutcome, ProgramSession, SourceApiFacts, WorkerBudget};
use tsc_diagnostics::{Diagnostic, DiagnosticCategory};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_no_lib_program, CompilerOptions, PreparedProgram, ProgramLoadLimits, ProgramOptions,
};

const GENEROUS_LIMIT: usize = 1_024 * 1_024;

/// Byte-identical to `with-globals.json`'s `/work/globals.d.ts`.
const MINIMAL_GLOBALS: &str = r#"
interface IArguments { length: number; callee: Function; }
interface Array<T> { length: number; [index: number]: T; }
interface Object {}
interface Function {}
interface CallableFunction extends Function {}
interface NewableFunction extends Function {}
interface String {}
interface Number {}
interface Boolean {}
interface RegExp {}
"#;

const A: &str =
    "import { dep } from './dep';\n/** doc */\nexport const a: number = dep;\nlet unused = 1;\n";
const DEP: &str = "export const dep: string = 1;\n";
const B: &str = "export const b = 'text' as number;\n";

/// One diagnostic record in the shape of the fresh TypeScript observation:
/// (file, start, length, code, category, message, reportsUnnecessary).
type Record = (
    Option<String>,
    Option<u32>,
    Option<u32>,
    u32,
    DiagnosticCategory,
    String,
    bool,
);

fn record(diagnostic: &Diagnostic) -> Record {
    assert!(
        diagnostic.related.is_empty(),
        "the fixture has no related information: {diagnostic:?}"
    );
    (
        diagnostic
            .file_name
            .as_ref()
            .map(|name| name.as_str().expect("scalar file name").to_owned()),
        diagnostic.start,
        diagnostic.length,
        diagnostic.code(),
        diagnostic.category(),
        diagnostic
            .message_text()
            .as_str()
            .expect("scalar message")
            .to_owned(),
        diagnostic.reports_unnecessary.unwrap_or(false),
    )
}

fn records(diagnostics: &[Diagnostic]) -> Vec<Record> {
    diagnostics.iter().map(record).collect()
}

fn error(file: &str, start: u32, length: u32, code: u32, message: &str) -> Record {
    (
        Some(file.to_owned()),
        Some(start),
        Some(length),
        code,
        DiagnosticCategory::Error,
        message.to_owned(),
        false,
    )
}

/// `getSemanticDiagnostics()` of the fresh observation, in its returned order.
fn expected_semantic() -> Vec<Record> {
    vec![
        error(
            "/work/a.ts",
            53,
            1,
            2322,
            "Type 'string' is not assignable to type 'number'.",
        ),
        error(
            "/work/b.ts",
            17,
            16,
            2352,
            "Conversion of type 'string' to type 'number' may be a mistake because neither type sufficiently overlaps with the other. If this was intentional, convert the expression to 'unknown' first.",
        ),
        error(
            "/work/dep.ts",
            13,
            3,
            2322,
            "Type 'number' is not assignable to type 'string'.",
        ),
    ]
}

/// The single `getSuggestionDiagnostics` row of the fresh observation.
fn expected_suggestion() -> Record {
    (
        Some("/work/a.ts".to_owned()),
        Some(74),
        Some(6),
        6133,
        DiagnosticCategory::Suggestion,
        "'unused' is declared but its value is never read.".to_owned(),
        true,
    )
}

fn host() -> MemoryCompilerHost {
    MemoryCompilerHost::builder("/work")
        .file("/work/globals.d.ts", MINIMAL_GLOBALS.as_bytes().to_vec())
        .file("/work/a.ts", A.as_bytes().to_vec())
        .file("/work/dep.ts", DEP.as_bytes().to_vec())
        .file("/work/b.ts", B.as_bytes().to_vec())
        .build()
        .expect("build fixture host")
}

fn four_workers() -> WorkerBudget {
    WorkerBudget::new(NonZeroUsize::new(4).expect("nonzero"))
}

fn loaded_program(host: &MemoryCompilerHost, workers: WorkerBudget) -> PreparedProgram {
    let roots = ["/work/globals.d.ts", "/work/a.ts", "/work/b.ts"]
        .iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    load_no_lib_program(
        host,
        &roots,
        CompilerOptions {
            no_emit: Some(true),
            strict: Some(true),
            ..CompilerOptions::default()
        },
        ProgramOptions::default()
            .with_no_lib(true)
            .with_types(Vec::new()),
        ProgramLoadLimits::new(
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
        )
        .with_workers(workers),
    )
    .expect("load the fixture program")
}

/// The complete observable outcome must equal the fresh TypeScript records:
/// open command gate, the three semantic errors, and the aggregate
/// conformance stream (public per-file getters, suggestions included).
fn assert_matches_fresh_observation(outcome: &NoEmitOutcome) {
    assert!(outcome.config_diagnostics().is_empty());
    assert!(outcome.syntactic_diagnostics().is_empty());
    assert!(outcome.options_diagnostics().is_empty());
    assert!(outcome.global_diagnostics().is_empty());
    assert_eq!(records(outcome.semantic_diagnostics()), expected_semantic());
    let mut expected_conformance = expected_semantic();
    expected_conformance.insert(1, expected_suggestion());
    assert_eq!(
        records(outcome.conformance_diagnostics()),
        expected_conformance
    );
}

fn all_slots_available(prepared: &PreparedProgram) -> bool {
    prepared
        .source_files()
        .iter()
        .all(|source| source.preparsed_syntax().is_available())
}

#[test]
fn first_session_adopts_every_loader_parse_and_a_repeat_session_parses_again() {
    let prepared = loaded_program(&host(), four_workers());
    assert_eq!(prepared.source_files().len(), 4);
    assert!(all_slots_available(&prepared));

    let first = ProgramSession::new(prepared.clone())
        .with_worker_budget(four_workers())
        .run()
        .expect("first session");
    let work = first.work_counters();
    assert_eq!(work.adopted_documents(), 4, "every loader parse is adopted");
    assert_eq!(work.parsed_documents(), 0, "nothing was parsed twice");
    assert_eq!(work.bound_documents(), 4);
    assert_eq!(work.full_text_copies(), 0);
    assert_matches_fresh_observation(&first);
    // Adoption consumed the shared take-once slots for every clone.
    assert!(!all_slots_available(&prepared));
    assert!(prepared
        .source_files()
        .iter()
        .all(|source| !source.preparsed_syntax().is_available()));

    let second = ProgramSession::new(prepared)
        .with_worker_budget(four_workers())
        .run()
        .expect("second session");
    let work = second.work_counters();
    assert_eq!(work.adopted_documents(), 0);
    assert_eq!(
        work.parsed_documents(),
        4,
        "the consumed slot forces a real parse of every document"
    );
    assert_eq!(work.bound_documents(), 4);
    assert_matches_fresh_observation(&second);
    assert_eq!(first, second);
}

#[test]
fn serial_and_parallel_budgets_publish_identical_outcomes() {
    // Serial loader + serial session: the exact pre-concurrency path.
    let serial_prepared = loaded_program(&host(), WorkerBudget::serial());
    assert!(all_slots_available(&serial_prepared));
    let serial = ProgramSession::new(serial_prepared.clone())
        .with_worker_budget(WorkerBudget::serial())
        .run()
        .expect("serial session");
    assert_eq!(serial.work_counters().adopted_documents(), 4);
    assert_eq!(serial.work_counters().parsed_documents(), 0);
    assert_matches_fresh_observation(&serial);

    // Parallel loader (read-ahead) + parallel session (worker binding), and
    // the mixed combinations, publish the same outcome and the same work
    // counters. Each combination loads its own fresh prepared program: the
    // take-once slots are consumed by the first session over a program, and
    // the consumed-slot fallback is covered by the repeat-session test.
    assert_eq!(loaded_program(&host(), four_workers()), serial_prepared);
    for (label, loader_workers, session_workers) in [
        ("parallel/parallel", four_workers(), four_workers()),
        ("parallel/serial", four_workers(), WorkerBudget::serial()),
        ("serial/parallel", WorkerBudget::serial(), four_workers()),
    ] {
        let prepared = loaded_program(&host(), loader_workers);
        assert!(all_slots_available(&prepared), "{label}");
        let outcome = ProgramSession::new(prepared)
            .with_worker_budget(session_workers)
            .run()
            .unwrap_or_else(|error| panic!("{label}: {error:?}"));
        assert_eq!(outcome.work_counters(), serial.work_counters(), "{label}");
        assert_matches_fresh_observation(&outcome);
        assert_eq!(outcome, serial, "{label}");
    }
}

#[test]
fn an_api_parse_option_override_falls_back_to_parsing_that_source_only() {
    let prepared = loaded_program(&host(), four_workers());
    let a = prepared
        .source_files()
        .iter()
        .position(|source| source.path().display().as_str() == Some("/work/a.ts"))
        .expect("a.ts is loaded");
    let a_id = prepared
        .source_id(prepared.source_files()[a].path().canonical())
        .expect("a.ts identity");

    let reference = ProgramSession::new(prepared.clone())
        .with_worker_budget(four_workers())
        .run()
        .expect("reference session");
    assert_matches_fresh_observation(&reference);

    let prepared = loaded_program(&host(), four_workers());
    let overridden = ProgramSession::new(prepared)
        .with_worker_budget(four_workers())
        .with_source_api_facts(
            a_id,
            SourceApiFacts {
                file_name: None,
                module_name: None,
                renamed_dependencies: Vec::new(),
                js_doc_parsing_mode: Some(JSDocParsingMode::ParseForTypeErrors),
            },
        )
        .run()
        .expect("overridden session");
    let work = overridden.work_counters();
    assert_eq!(
        work.parsed_documents(),
        1,
        "a.ts's planner parse used ParseAll, so the ParseForTypeErrors session parses it"
    );
    assert_eq!(
        work.adopted_documents(),
        3,
        "the other loader parses are still adopted"
    );
    assert_eq!(work.bound_documents(), 4);
    // The fixture's JSDoc (`/** doc */`) carries no type-affecting tag, so
    // the complete observable outcome is unchanged by the parse mode.
    assert_matches_fresh_observation(&overridden);
    assert_eq!(reference, overridden);
}

/// Deep-input control for the worker binding stack budget: a nested array
/// literal and a long binary chain bind identically under the serial and
/// four-worker budgets, with the same diagnostics. A control, not a proof of
/// a universal depth bound.
///
/// Expected records are the fresh TypeScript 6.0.3 observation of exactly
/// this fixture (roots globals, deep, b; same options as above):
/// `review/adoption-deep-oracle-20260922/stdout.json` (integrator run,
/// 2026-09-22): semantic b.ts@17+16 TS2352 and deep.ts@4177+7 TS2322 on the
/// variable name `mistake`; suggestion deep.ts@4177+7 TS6133
/// (reportsUnnecessary); no syntactic, options or global rows.
#[test]
fn deep_inputs_bind_identically_under_every_budget() {
    const NESTING: usize = 64;
    const CHAIN_TERMS: usize = 1_000;
    let mut deep = String::new();
    deep.push_str("export const nested = ");
    deep.push_str(&"[".repeat(NESTING));
    deep.push('1');
    deep.push_str(&"]".repeat(NESTING));
    deep.push_str(";\nexport const chain = ");
    deep.push_str(&"1 + ".repeat(CHAIN_TERMS - 1));
    deep.push_str("1;\n");
    let mistake_start = deep.len() + "let ".len();
    deep.push_str("let mistake: number = 'x';\n");
    assert_eq!(deep.len(), 4_200, "the observed fixture is 4,200 bytes");
    assert_eq!(mistake_start, 4_177);
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/globals.d.ts", MINIMAL_GLOBALS.as_bytes().to_vec())
        .file("/work/deep.ts", deep.into_bytes())
        .file("/work/b.ts", B.as_bytes().to_vec())
        .build()
        .expect("build deep host");
    let roots = ["/work/globals.d.ts", "/work/deep.ts", "/work/b.ts"]
        .iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let load = |workers: WorkerBudget| {
        load_no_lib_program(
            &host,
            &roots,
            CompilerOptions {
                no_emit: Some(true),
                strict: Some(true),
                ..CompilerOptions::default()
            },
            ProgramOptions::default()
                .with_no_lib(true)
                .with_types(Vec::new()),
            ProgramLoadLimits::new(
                GENEROUS_LIMIT,
                GENEROUS_LIMIT,
                GENEROUS_LIMIT,
                GENEROUS_LIMIT,
                GENEROUS_LIMIT,
            )
            .with_workers(workers),
        )
        .expect("load the deep program")
    };
    let expected = vec![
        error(
            "/work/b.ts",
            17,
            16,
            2352,
            "Conversion of type 'string' to type 'number' may be a mistake because neither type sufficiently overlaps with the other. If this was intentional, convert the expression to 'unknown' first.",
        ),
        error(
            "/work/deep.ts",
            mistake_start as u32,
            7,
            2322,
            "Type 'string' is not assignable to type 'number'.",
        ),
    ];
    let mut expected_conformance = expected.clone();
    expected_conformance.push((
        Some("/work/deep.ts".to_owned()),
        Some(mistake_start as u32),
        Some(7),
        6133,
        DiagnosticCategory::Suggestion,
        "'mistake' is declared but its value is never read.".to_owned(),
        true,
    ));
    let serial = ProgramSession::new(load(WorkerBudget::serial()))
        .with_worker_budget(WorkerBudget::serial())
        .run()
        .expect("serial deep session");
    assert!(serial.global_diagnostics().is_empty());
    assert!(serial.options_diagnostics().is_empty());
    assert!(serial.syntactic_diagnostics().is_empty());
    assert_eq!(records(serial.semantic_diagnostics()), expected);
    assert_eq!(
        records(serial.conformance_diagnostics()),
        expected_conformance
    );
    assert_eq!(serial.work_counters().adopted_documents(), 3);
    assert_eq!(serial.work_counters().parsed_documents(), 0);
    let parallel = ProgramSession::new(load(four_workers()))
        .with_worker_budget(four_workers())
        .run()
        .expect("parallel deep session");
    assert_eq!(records(parallel.semantic_diagnostics()), expected);
    assert_eq!(
        records(parallel.conformance_diagnostics()),
        expected_conformance
    );
    assert_eq!(parallel.work_counters(), serial.work_counters());
    assert_eq!(parallel, serial);
}

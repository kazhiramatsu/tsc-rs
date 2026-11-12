//! First-consumer outputs captured by the Go API oracle at aa8149273.
use serde::Deserialize;
use std::num::NonZeroUsize;
use tsc_compiler::LiveProgram;
use tsc_diagnostics::flattened_message;
use tsc_program::{PreparedProgram, WorkerBudget};

#[path = "../../program/tests/support/loader_order.rs"]
#[allow(dead_code)]
mod corpus;

#[derive(Deserialize)]
struct Expected {
    name: String,
    syntax: bool,
    first: String,
    explanation: String,
    messages: Vec<String>,
    later: String,
}

fn explain(program: &PreparedProgram) -> String {
    render(program.explain_files("/project".into()))
}

fn render(
    rows: impl IntoIterator<Item = (tsc_diagnostics::JsString, Vec<tsc_diagnostics::JsString>)>,
) -> String {
    let mut output = String::new();
    for (path, reasons) in rows {
        output.push_str(&path.to_string_lossy());
        output.push('\n');
        for reason in reasons {
            output.push_str("   ");
            output.push_str(&reason.to_string_lossy());
            output.push('\n');
        }
    }
    output
}

#[test]
fn processing_details_follow_the_first_api_or_lsp_consumer() {
    let cases: Vec<Expected> = serde_json::from_str(include_str!(
        "../../program/tests/fixtures/include-consumer-api.json"
    ))
    .unwrap();
    for case in cases {
        for workers in [
            WorkerBudget::serial(),
            WorkerBudget::new(NonZeroUsize::new(4).unwrap()),
            WorkerBudget::automatic(),
        ] {
            let mut fixture = corpus::fixture(&case.name);
            if case.syntax {
                fixture
                    .files
                    .get_mut("main.ts")
                    .unwrap()
                    .push_str("\nconst broken = ;\n");
            }
            let program = corpus::load(&fixture, workers, false);
            let mut live = LiveProgram::new(program).unwrap();
            let main = (0..live.file_count())
                .find(|&file| {
                    live.file_name(file)
                        .unwrap()
                        .to_string_lossy()
                        .ends_with("/main.ts")
                })
                .unwrap();
            match case.first.as_str() {
                "explain" => {}
                "program" => {
                    live.program_diagnostics();
                }
                "syntax" => {
                    live.syntactic_diagnostics(main);
                }
                "bind" => {
                    live.bind_diagnostics(main);
                }
                "global" => {
                    live.global_diagnostics();
                }
                "semantic" => {
                    live.semantic_diagnostics(main).unwrap();
                }
                "suggestion" => {
                    live.suggestion_diagnostics(main).unwrap();
                }
                "declaration" => {
                    live.declaration_diagnostics(&[main]).unwrap();
                }
                "include" => {
                    live.prepared().diagnostics().program();
                }
                unknown => panic!("unknown Go consumer {unknown}"),
            }
            let label = format!("{} syntax={} {workers:?}", case.first, case.syntax);
            assert_eq!(explain(live.prepared()), case.explanation, "{label}");
            let messages: Vec<_> = live
                .prepared()
                .diagnostics()
                .program()
                .iter()
                .filter(|row| matches!(row.code(), 1261 | 1149))
                .map(|row| {
                    flattened_message(&row.message, "\n", None)
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            assert_eq!(messages, case.messages, "{label}: later diagnostic");
            assert_eq!(
                explain(live.prepared()),
                case.later,
                "{label}: later explanation"
            );
            // The LSP asks for syntax and then semantics even if syntax fails.
            // Repeated getters must reuse the first rendered details as well.
            let semantic = live.semantic_diagnostics(main).unwrap();
            let messages: Vec<_> = semantic
                .iter()
                .filter(|row| matches!(row.code(), 1261 | 1149))
                .map(|row| {
                    flattened_message(&row.message, "\n", None)
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            assert_eq!(messages, case.messages, "{label}: semantic getter");
        }
    }
}

#[test]
fn scoped_diagnostic_getters_materialize_after_initialization() {
    let cases: Vec<Expected> = serde_json::from_str(include_str!(
        "../../program/tests/fixtures/include-consumer-api.json"
    ))
    .unwrap();
    for case in cases
        .into_iter()
        .filter(|case| matches!(case.first.as_str(), "program" | "explain"))
    {
        for workers in [
            WorkerBudget::serial(),
            WorkerBudget::new(NonZeroUsize::new(4).unwrap()),
            WorkerBudget::automatic(),
        ] {
            let mut fixture = corpus::fixture(&case.name);
            if case.syntax {
                fixture
                    .files
                    .get_mut("main.ts")
                    .unwrap()
                    .push_str("\nconst broken = ;\n");
            }
            let program = corpus::load(&fixture, workers, false);
            let shared = program.clone();
            tsc_compiler::ProgramSession::new(program)
                .with_worker_budget(workers)
                .with_declarations(|session| {
                    if case.first == "program" {
                        session.get_program_diagnostics()?;
                    }
                    assert_eq!(
                        explain(&shared),
                        case.explanation,
                        "scoped {} syntax={}",
                        case.first,
                        case.syntax
                    );
                    let diagnostics = session.get_program_diagnostics()?;
                    let messages: Vec<_> = diagnostics
                        .semantic()
                        .iter()
                        .filter(|row| matches!(row.code(), 1261 | 1149))
                        .map(|row| {
                            flattened_message(&row.message, "\n", None)
                                .to_string_lossy()
                                .into_owned()
                        })
                        .collect();
                    assert_eq!(messages, case.messages, "scoped later semantic getter");
                    assert_eq!(explain(&shared), case.later, "scoped later explanation");
                    Ok(())
                })
                .unwrap();
        }
    }
}

#[test]
fn a_reused_program_starts_with_unrendered_details() {
    let cases: Vec<Expected> = serde_json::from_str(include_str!(
        "../../program/tests/fixtures/include-consumer-api.json"
    ))
    .unwrap();
    let relative = cases
        .iter()
        .find(|case| case.first == "explain" && !case.syntax)
        .unwrap();
    let absolute = cases
        .iter()
        .find(|case| case.first == "program" && !case.syntax)
        .unwrap();
    let fixture = corpus::fixture(&relative.name);
    for workers in [
        WorkerBudget::serial(),
        WorkerBudget::new(NonZeroUsize::new(4).unwrap()),
        WorkerBudget::automatic(),
    ] {
        let program = corpus::load(&fixture, workers, false);
        program.diagnostics().program();
        assert_eq!(explain(&program), absolute.explanation);
        let source = program
            .source_files()
            .iter()
            .find(|source| {
                source
                    .path()
                    .display()
                    .to_string_lossy()
                    .ends_with("/Dep.ts")
            })
            .unwrap();
        let mut host = tsc_host::MemoryCompilerHost::builder("/project").case_sensitive(false);
        for (name, text) in &fixture.files {
            host = host.file(format!("/project/{name}"), text.as_bytes().to_vec());
        }
        let host = host.build().unwrap();
        let documents = tsc_compiler::DocumentRegistry::new();
        let reused = LiveProgram::reuse(
            &program,
            source.path().canonical(),
            "export {}; // replacement\n".to_owned(),
            &host,
            &documents,
        )
        .expect("the dependency's requests and diagnostic ownership are unchanged")
        .unwrap();
        // Go ReuseProgram's new Program omits includeProcessor from its
        // initializer (program.go:427-438), while retaining processedFiles.
        assert_eq!(explain(reused.prepared()), relative.explanation);
        assert_eq!(
            explain(&program),
            absolute.explanation,
            "the old Program keeps its rendering"
        );
    }
}

#[test]
fn an_emit_write_callback_can_render_before_diagnostic_getters() {
    struct InspectSink {
        program: PreparedProgram,
        first: Option<String>,
    }
    impl tsc_emitter::OutputSink for InspectSink {
        fn write(
            &mut self,
            _: tsc_emitter::EmitArtifact,
        ) -> Result<tsc_emitter::EmitWriteDisposition, tsc_emitter::EmitIoError> {
            if self.first.is_none() {
                self.first = Some(explain(&self.program));
            }
            Ok(tsc_emitter::EmitWriteDisposition::Written)
        }
    }
    let cases: Vec<Expected> = serde_json::from_str(include_str!(
        "../../program/tests/fixtures/include-consumer-emit.json"
    ))
    .unwrap();
    for case in cases {
        for workers in [
            WorkerBudget::serial(),
            WorkerBudget::new(NonZeroUsize::new(4).unwrap()),
            WorkerBudget::automatic(),
        ] {
            let mut fixture = corpus::fixture(&case.name);
            let mut config: serde_json::Value =
                serde_json::from_str(&fixture.files["tsconfig.json"]).unwrap();
            config["compilerOptions"]["noEmit"] = false.into();
            config["compilerOptions"]["outDir"] = "./out".into();
            fixture
                .files
                .insert("tsconfig.json".to_owned(), config.to_string());
            if case.syntax {
                fixture
                    .files
                    .get_mut("main.ts")
                    .unwrap()
                    .push_str("\nconst broken = ;\n");
            }
            let mut host = tsc_host::MemoryCompilerHost::builder("/project").case_sensitive(false);
            for (name, text) in &fixture.files {
                host = host.file(format!("/project/{name}"), text.as_bytes().to_vec());
            }
            let host = host.build().unwrap();
            let plan = tsc_program::parse_config_root_plan(
                &tsc_program::CompilerConfigHost::new(&host),
                tsc_program::ConfigRootPlanRequest {
                    file_name: "/project/tsconfig.json".into(),
                    text: fixture.files["tsconfig.json"].clone(),
                    base_path: "/project".into(),
                },
            )
            .unwrap();
            let program = tsc_program::load_emitting_config_program(
                &host,
                &plan,
                &tsc_program::LibraryCatalog::typescript_7_1(std::path::PathBuf::from(
                    "/project/lib",
                )),
                tsc_program::ProgramLoadLimits::new(128, 1024, 64, 1 << 20, 1 << 24)
                    .with_workers(workers),
            )
            .unwrap();
            let mut sink = InspectSink {
                program: program.clone(),
                first: None,
            };
            let result = tsc_compiler::ProgramSession::new(program)
                .with_worker_budget(workers)
                .with_checker_budget(tsc_compiler::CheckerBudget::serial())
                .emit_then_run_for_native_harness(
                    tsc_compiler::NativeHarnessCollection::default(),
                    &mut sink,
                )
                .unwrap();
            let Ok((outcome, emit)) = result else {
                panic!("the Program can emit before reporting diagnostics");
            };
            emit.expect("the Program emitted").unwrap();
            assert_eq!(
                sink.first.as_deref(),
                Some(case.explanation.as_str()),
                "emit write syntax={} {workers:?}",
                case.syntax
            );
            let messages: Vec<_> = outcome
                .native_harness_diagnostics()
                .iter()
                .filter(|row| matches!(row.code(), 1261 | 1149))
                .map(|row| {
                    flattened_message(&row.message, "\n", None)
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            assert_eq!(
                messages, case.messages,
                "later getter syntax={}",
                case.syntax
            );
            assert_eq!(explain(&sink.program), case.later);
        }
    }
}

#[test]
fn an_explanation_returned_before_a_diagnostic_getter_keeps_its_first_rendering() {
    let cases: Vec<Expected> = serde_json::from_str(include_str!(
        "../../program/tests/fixtures/include-consumer-api.json"
    ))
    .unwrap();
    let case = cases
        .iter()
        .find(|case| case.first == "explain" && !case.syntax)
        .unwrap();
    for workers in [
        WorkerBudget::serial(),
        WorkerBudget::new(NonZeroUsize::new(4).unwrap()),
        WorkerBudget::automatic(),
    ] {
        let program = corpus::load(&corpus::fixture(&case.name), workers, false);
        let first = program.explain_files("/project".into());
        program.diagnostics().program();
        assert_eq!(render(first), case.explanation);
        assert_eq!(explain(&program), case.later);
    }
}
